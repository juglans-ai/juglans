// src/builtins/system.rs
use super::Tool;
use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::services::interface::JuglansRuntime;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Parse a string into a context value, preserving large integers as strings to avoid f64 precision loss.
fn parse_context_value(value_str: &str) -> Value {
    match serde_json::from_str::<Value>(value_str) {
        Ok(Value::Number(n))
            if n.as_f64()
                .map(|f| f.abs() > 9_007_199_254_740_992.0)
                .unwrap_or(false)
                && value_str.bytes().all(|b| b.is_ascii_digit() || b == b'-') =>
        {
            // Large integer exceeding f64 precision (e.g. Google/Apple user ID), keep as string
            json!(value_str)
        }
        Ok(v) => v,
        Err(_) => json!(value_str),
    }
}

pub struct Timer;
#[async_trait]
impl Tool for Timer {
    fn name(&self) -> &str {
        "timer"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Support both 'ms' (preferred) and 'seconds' (backward compatible)
        let duration_ms: u64 = if let Some(ms) = params.get("ms") {
            ms.parse().unwrap_or(1000)
        } else if let Some(secs) = params.get("seconds") {
            secs.parse::<u64>().unwrap_or(1) * 1000
        } else {
            1000 // default 1 second
        };

        if !context.has_event_sender() {
            println!("⏳ Sleeping for {} ms...", duration_ms);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
        Ok(Some(
            json!({ "status": "finished", "duration_ms": duration_ms }),
        ))
    }
}

pub struct SetContext;
#[async_trait]
impl Tool for SetContext {
    fn name(&self) -> &str {
        "set_context"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Supports two modes:
        // 1. Legacy mode: set_context(path="key", value="val")
        // 2. Multi-field mode: set_context(key1=$input.val1, key2=$input.val2)

        let mut last_value: Option<Value> = None;

        if let (Some(path), Some(value_str)) = (params.get("path"), params.get("value")) {
            // Legacy mode
            let value = parse_context_value(value_str);
            let stripped_path = path.strip_prefix("$ctx.").unwrap_or(path).trim_matches('"');
            context.set(stripped_path.to_string(), value.clone())?;
            last_value = Some(value);
        } else {
            // Multi-field mode: set each key=value pair into ctx
            for (key, value_str) in params {
                // Skip reserved fields
                if key == "path" || key == "value" {
                    continue;
                }
                let value = parse_context_value(value_str);
                context.set(key.clone(), value.clone())?;
                last_value = Some(value);
            }
        }
        Ok(last_value)
    }
}

pub struct Notify;
#[async_trait]
impl Tool for Notify {
    fn name(&self) -> &str {
        "notify"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // If status is provided, update ctx.reply.status for transparent thinking stream
        if let Some(status) = params.get("status") {
            context.set("reply.status".to_string(), json!(status))?;
            if !context.has_event_sender() {
                println!("💡 [Status] {}", status);
            }
        }

        let msg = params.get("message").map(|s| s.as_str()).unwrap_or("");
        if !msg.is_empty() && !context.has_event_sender() {
            println!("🔔 [Notification] {}", msg);
        }

        Ok(Some(json!({ "status": "sent", "content": msg })))
    }
}

/// print(message="text") — plain output, no emoji prefix, does not modify context
/// Unlike notify, print only does println, suitable for debugging and Hello World
pub struct Print;
#[async_trait]
impl Tool for Print {
    fn name(&self) -> &str {
        "print"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let msg = params
            .get("message")
            .or_else(|| params.get("value"))
            .map(|s| s.as_str())
            .unwrap_or("");
        if !context.has_event_sender() {
            println!("{}", msg);
        }
        Ok(Some(json!(msg)))
    }
}

/// reply(message="content", state="context_visible") - return content directly without calling AI
/// Used for system event handling where fixed text is needed without going through the LLM
/// Supports state parameter for SSE/persistence control, including compound syntax input:output
pub struct Reply {
    runtime: Arc<dyn JuglansRuntime>,
}

impl Reply {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for Reply {
    fn name(&self) -> &str {
        "reply"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let message = params.get("message").map(|s| s.as_str()).unwrap_or("");

        // Support compound syntax input:output (consistent with chat())
        let state_raw = params
            .get("state")
            .map(|s| s.as_str())
            .unwrap_or("context_visible");
        let (input_state, output_state) = match state_raw.split_once(':') {
            Some((i, o)) => (i, o),
            None => (state_raw, state_raw),
        };

        // should_stream based on output_state
        let should_stream = output_state == "context_visible" || output_state == "display_only";

        // SSE output
        if should_stream {
            context.emit(WorkflowEvent::Token(message.to_string()));
        }

        // Persist reply message to jug0 (output_state controls reply persistence)
        let should_persist_reply =
            output_state == "context_visible" || output_state == "context_hidden";
        if should_persist_reply {
            if let Ok(Some(chat_id_val)) = context.resolve_path("reply.chat_id") {
                if let Some(chat_id) = chat_id_val.as_str() {
                    let _ = self
                        .runtime
                        .create_message(chat_id, "assistant", message, output_state)
                        .await;
                }
            }
        }

        // Retroactively update original user message state using input_state
        if let (Ok(Some(chat_id_val)), Ok(Some(umid_val))) = (
            context.resolve_path("reply.chat_id"),
            context.resolve_path("reply.user_message_id"),
        ) {
            if let (Some(chat_id), Some(umid)) = (chat_id_val.as_str(), umid_val.as_i64()) {
                let _ = self
                    .runtime
                    .update_message_state(chat_id, umid as i32, input_state)
                    .await;
            }
        }

        // Update reply.output (consistent with chat())
        let current = context
            .resolve_path("reply.output")
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        context.set(
            "reply.output".to_string(),
            json!(format!("{}{}", current, message)),
        )?;

        Ok(Some(json!({
            "content": message,
            "status": "sent"
        })))
    }
}

/// feishu_webhook(message="content") - Push messages to group chat via Feishu Webhook
/// Reads webhook URL from juglans.toml [bot.feishu] webhook_url
pub struct FeishuWebhook;

#[async_trait]
impl Tool for FeishuWebhook {
    fn name(&self) -> &str {
        "feishu_webhook"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let message = params
            .get("message")
            .ok_or_else(|| anyhow!("feishu_webhook() requires 'message' parameter"))?;

        // Prefer webhook_url from params, otherwise get from context (injected at bot startup)
        let webhook_url = if let Some(url) = params.get("webhook_url") {
            url.clone()
        } else if let Ok(Some(url_val)) = context.resolve_path("bot.feishu_webhook_url") {
            url_val.as_str().unwrap_or("").to_string()
        } else {
            // Try to load from config file
            match crate::services::config::JuglansConfig::load() {
                Ok(config) => config
                    .bot
                    .as_ref()
                    .and_then(|b| b.feishu.as_ref())
                    .and_then(|f| f.webhook_url.clone())
                    .ok_or_else(|| anyhow!("No webhook_url in [bot.feishu] config"))?,
                Err(_) => return Err(anyhow!("Cannot load config for feishu webhook_url")),
            }
        };

        // Call Feishu webhook API directly
        let client = reqwest::Client::new();
        let resp = client
            .post(&webhook_url)
            .json(&json!({
                "msg_type": "text",
                "content": {
                    "text": message
                }
            }))
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(json!({}));
        let ok = body["code"].as_i64() == Some(0) || status.is_success();

        if !ok {
            return Err(anyhow!("Feishu webhook error: {:?}", body));
        }

        Ok(Some(json!({
            "status": "sent",
            "content": message
        })))
    }
}

/// feishu_send(chat_id="oc_xxx", message="text", image="url_or_path")
/// Send messages (text/image) via Feishu Open API using app_id + app_secret.
pub struct FeishuSend;

#[async_trait]
impl Tool for FeishuSend {
    fn name(&self) -> &str {
        "feishu_send"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params
            .get("chat_id")
            .ok_or_else(|| anyhow!("feishu_send() requires 'chat_id' parameter"))?
            .trim_matches('"');

        let message = params
            .get("message")
            .map(|s| s.trim_matches('"').to_string());
        let image = params.get("image").map(|s| s.trim_matches('"').to_string());

        if message.is_none() && image.is_none() {
            return Err(anyhow!(
                "feishu_send() requires 'message' or 'image' parameter"
            ));
        }

        // Load config
        let config = crate::services::config::JuglansConfig::load()
            .map_err(|e| anyhow!("Cannot load config: {}", e))?;
        let feishu = config
            .bot
            .as_ref()
            .and_then(|b| b.feishu.as_ref())
            .ok_or_else(|| anyhow!("Missing [bot.feishu] config"))?;
        let app_id = feishu
            .app_id
            .as_ref()
            .ok_or_else(|| anyhow!("Missing [bot.feishu] app_id"))?;
        let app_secret = feishu
            .app_secret
            .as_ref()
            .ok_or_else(|| anyhow!("Missing [bot.feishu] app_secret"))?;
        let base_url = &feishu.base_url;

        // Get tenant access token
        let client = reqwest::Client::new();
        let token_resp = client
            .post(format!(
                "{}/open-apis/auth/v3/tenant_access_token/internal",
                base_url
            ))
            .json(&json!({
                "app_id": app_id,
                "app_secret": app_secret
            }))
            .send()
            .await?;
        let token_body: Value = token_resp.json().await?;
        let token = token_body["tenant_access_token"]
            .as_str()
            .ok_or_else(|| anyhow!("Failed to get tenant_access_token: {:?}", token_body))?
            .to_string();

        // If image provided, upload it first
        if let Some(ref image_src) = image {
            // Get image bytes
            let image_bytes =
                if image_src.starts_with("http://") || image_src.starts_with("https://") {
                    client.get(image_src).send().await?.bytes().await?.to_vec()
                } else {
                    tokio::fs::read(image_src)
                        .await
                        .map_err(|e| anyhow!("Cannot read image file '{}': {}", image_src, e))?
                };

            // Upload image to Feishu
            let form = reqwest::multipart::Form::new()
                .text("image_type", "message")
                .part(
                    "image",
                    reqwest::multipart::Part::bytes(image_bytes)
                        .file_name("image.png")
                        .mime_str("image/png")?,
                );
            let upload_resp = client
                .post(format!("{}/open-apis/im/v1/images", base_url))
                .bearer_auth(&token)
                .multipart(form)
                .send()
                .await?;
            let upload_body: Value = upload_resp.json().await?;
            let image_key = upload_body["data"]["image_key"]
                .as_str()
                .ok_or_else(|| anyhow!("Failed to upload image: {:?}", upload_body))?;

            // Send image message
            let content = json!({"image_key": image_key}).to_string();
            let send_resp = client
                .post(format!(
                    "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
                    base_url
                ))
                .bearer_auth(&token)
                .json(&json!({
                    "receive_id": chat_id,
                    "msg_type": "image",
                    "content": content
                }))
                .send()
                .await?;
            let send_body: Value = send_resp.json().await?;
            if send_body["code"].as_i64() != Some(0) {
                return Err(anyhow!("Feishu send image error: {:?}", send_body));
            }

            // If also has text message, send it separately
            if let Some(ref msg) = message {
                let text_content = json!({"text": msg}).to_string();
                client
                    .post(format!(
                        "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
                        base_url
                    ))
                    .bearer_auth(&token)
                    .json(&json!({
                        "receive_id": chat_id,
                        "msg_type": "text",
                        "content": text_content
                    }))
                    .send()
                    .await?;
            }

            return Ok(Some(json!({
                "status": "sent",
                "type": "image",
                "image_key": image_key
            })));
        }

        // Text-only message
        if let Some(ref msg) = message {
            let content = json!({"text": msg}).to_string();
            let send_resp = client
                .post(format!(
                    "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
                    base_url
                ))
                .bearer_auth(&token)
                .json(&json!({
                    "receive_id": chat_id,
                    "msg_type": "text",
                    "content": content
                }))
                .send()
                .await?;
            let send_body: Value = send_resp.json().await?;
            if send_body["code"].as_i64() != Some(0) {
                return Err(anyhow!("Feishu send error: {:?}", send_body));
            }
        }

        Ok(Some(json!({
            "status": "sent",
            "type": "text"
        })))
    }
}

/// return(value=expr) — Return the evaluated expression result as $output
/// Used in function definitions to return computed results: `[add(a, b)]: return(value=$ctx.a + $ctx.b)`
pub struct Return;
#[async_trait]
impl Tool for Return {
    fn name(&self) -> &str {
        "return"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        if let Some(value_str) = params.get("value") {
            let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
            Ok(Some(value))
        } else if let Some((_key, value_str)) = params.iter().next() {
            let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
            Ok(Some(value))
        } else {
            Ok(Some(Value::Null))
        }
    }
}

/// call(fn="function_name", ...args) — Dynamic function dispatch by string name
/// Looks up a function defined in the current workflow and executes it with the provided arguments.
/// The `fn` parameter specifies the function name; all other parameters are passed as arguments.
pub struct Call {
    builtin_registry: Option<std::sync::Weak<super::BuiltinRegistry>>,
}

impl Default for Call {
    fn default() -> Self {
        Self::new()
    }
}

impl Call {
    pub fn new() -> Self {
        Self {
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: std::sync::Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }
}

#[async_trait]
impl Tool for Call {
    fn name(&self) -> &str {
        "call"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let fn_name = params
            .get("fn")
            .ok_or_else(|| anyhow!("call() requires 'fn' parameter (function name)"))?;

        // Get executor via builtin registry
        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("call(): BuiltinRegistry not available"))?;

        let executor = registry
            .get_executor()
            .ok_or_else(|| anyhow!("call(): WorkflowExecutor not available"))?;

        // Get workflow: prefer root workflow (where functions are defined),
        // fall back to current workflow for nested contexts
        let workflow = context
            .get_root_workflow()
            .or_else(|| context.get_current_workflow())
            .ok_or_else(|| anyhow!("call(): no active workflow"))?;

        // Collect remaining params as function args (exclude "fn")
        let args: HashMap<String, Value> = params
            .iter()
            .filter(|(k, _)| k.as_str() != "fn")
            .map(|(k, v)| {
                let val = serde_json::from_str(v).unwrap_or(json!(v));
                (k.clone(), val)
            })
            .collect();

        executor
            .execute_function(fn_name.clone(), args, workflow, context)
            .await
    }
}

// Shell has been replaced by devtools::Bash (registered as "bash" + "sh" alias)
