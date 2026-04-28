// src/adapters/feishu.rs
#![cfg(not(target_arch = "wasm32"))]

use anyhow::Result;
use axum::Json;
use dashmap::DashSet;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

use super::{run_agent_for_message, Channel, MessageDispatcher, PlatformMessage, ToolExecutor};
use crate::services::config::JuglansConfig;

/// Feishu platform tool executor -- invokes bill_utils.py via Python subprocess
struct FeishuToolExecutor {
    project_root: PathBuf,
    app_id: String,
    app_secret: String,
    base_url: String,
    approvers: Vec<String>,
    platform_chat_id: String,
    platform_user_id: String,
}

impl FeishuToolExecutor {
    fn from_handler(handler: &FeishuWebhookHandler, msg: &PlatformMessage) -> Self {
        Self {
            project_root: handler.project_root.clone(),
            app_id: handler.app_id.clone(),
            app_secret: handler.app_secret.clone(),
            base_url: handler.base_url.clone(),
            approvers: handler.approvers.clone(),
            platform_chat_id: msg.platform_chat_id.clone(),
            platform_user_id: msg.platform_user_id.clone(),
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for FeishuToolExecutor {
    async fn execute(&self, tool_name: &str, args: Value) -> anyhow::Result<String> {
        // Inject system parameters into args
        let mut full_args = args.clone();
        if let Some(obj) = full_args.as_object_mut() {
            obj.insert("_app_id".into(), json!(self.app_id));
            obj.insert("_app_secret".into(), json!(self.app_secret));
            obj.insert("_base_url".into(), json!(self.base_url));
            obj.insert("_approvers".into(), json!(self.approvers));
            obj.insert("_chat_id".into(), json!(self.platform_chat_id));
            obj.insert("_user_id".into(), json!(self.platform_user_id));
        }

        let python_code = format!(
            r#"
import json, sys
sys.path.insert(0, sys.argv[1])
import bill_utils

args = json.loads(sys.argv[2])

# Extract system parameters
_sys = {{k: args.pop(k) for k in list(args) if k.startswith('_')}}

# Inject context (used by create_bill / clear_chat_history)
bill_utils._context = {{
    "user_id": _sys.get("_user_id"),
    "chat_id": _sys.get("_chat_id"),
    "app_id": _sys.get("_app_id"),
    "app_secret": _sys.get("_app_secret"),
    "base_url": _sys.get("_base_url", "https://open.larksuite.com"),
    "approvers": _sys.get("_approvers", []),
}}

func = getattr(bill_utils, "{}")
result = func(**args)

if isinstance(result, str):
    print(result)
else:
    print(json.dumps(result, ensure_ascii=False))
"#,
            tool_name
        );

        let output = tokio::process::Command::new("python3")
            .arg("-c")
            .arg(&python_code)
            .arg(self.project_root.to_str().unwrap_or("."))
            .arg(serde_json::to_string(&full_args)?)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("[FeishuToolExecutor] {} failed: {}", tool_name, stderr);
            return Err(anyhow::anyhow!(
                "Python tool {} failed: {}",
                tool_name,
                stderr
            ));
        }

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        info!(
            "[FeishuToolExecutor] {} → {}",
            tool_name,
            if result.len() > 100 {
                &result[..100]
            } else {
                &result
            }
        );
        Ok(result)
    }
}

// ======================================================================
// Public webhook handler (for web_server.rs serverless integration)
// ======================================================================

/// Standalone Feishu webhook handler, embeddable in web_server.
///
/// No need to start a separate Feishu bot service; mounts directly onto juglans web routes.
/// Suitable for serverless deployment scenarios.
pub struct FeishuWebhookHandler {
    config: JuglansConfig,
    project_root: PathBuf,
    agent_slug: String,
    app_id: String,
    app_secret: String,
    base_url: String,
    approvers: Vec<String>,
    access_token: Mutex<Option<(String, std::time::Instant)>>,
    processed_events: DashSet<String>,
}

impl FeishuWebhookHandler {
    /// Handle Feishu webhook request body, return JSON response
    /// Variant that threads a channel egress reference so workflow `reply()`
    /// calls route back through the channel system. The plain `handle_event`
    /// stays as the no-origin fallback for legacy paths.
    pub async fn handle_event_with_channel(
        &self,
        body: Value,
        channel: Arc<dyn crate::core::context::ChannelEgress>,
    ) -> Value {
        self.handle_event_inner(body, Some(channel)).await
    }

    /// No-origin fallback for callers that don't have a `Channel` reference
    /// (e.g. juglans-wallet's external orchestration). The channel-aware path
    /// goes through [`handle_event_with_channel`].
    #[allow(dead_code)]
    pub async fn handle_event(&self, body: Value) -> Value {
        self.handle_event_inner(body, None).await
    }

    async fn handle_event_inner(
        &self,
        body: Value,
        channel: Option<Arc<dyn crate::core::context::ChannelEgress>>,
    ) -> Value {
        // URL verification challenge
        if let Some(challenge) = body["challenge"].as_str() {
            info!("[Feishu Webhook] URL verification challenge");
            return json!({ "challenge": challenge });
        }

        let event_type = body
            .pointer("/header/event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let event_id = body
            .pointer("/header/event_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Event deduplication
        if !event_id.is_empty() && !self.processed_events.insert(event_id.clone()) {
            info!("[Feishu Webhook] Duplicate event {}, skipping", event_id);
            return json!({"code": 0, "msg": "duplicate"});
        }

        match event_type {
            "im.message.receive_v1" => {
                if let Some(event) = body.get("event") {
                    let event = event.clone();
                    // Reuse handle_message logic
                    if let Err(e) = self.handle_message(&event, channel.clone()).await {
                        error!("[Feishu Webhook] Message handling failed: {}", e);
                    }
                }
                json!({"code": 0, "msg": "ok"})
            }
            _ => {
                if !event_type.is_empty() {
                    warn!(
                        "[Feishu Webhook] Unhandled event type: {} (id: {})",
                        event_type, event_id
                    );
                }
                json!({"code": 0, "msg": "ok"})
            }
        }
    }

    /// Handle message event (reuses run_agent_for_message logic).
    /// `channel`, if present, becomes the run's `ChannelOrigin` so `reply()`
    /// calls inside the workflow round-trip back via Feishu OpenAPI.
    async fn handle_message(
        &self,
        event: &Value,
        channel: Option<Arc<dyn crate::core::context::ChannelEgress>>,
    ) -> Result<()> {
        let message = event
            .get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in event"))?;

        let msg_type = message["message_type"].as_str().unwrap_or("");
        if msg_type != "text" {
            info!(
                "[Feishu Webhook] Skipping non-text message (type: {})",
                msg_type
            );
            return Ok(());
        }

        let content_str = message["content"].as_str().unwrap_or("{}");
        let content: Value = serde_json::from_str(content_str).unwrap_or(json!({}));
        let raw_text = content["text"].as_str().unwrap_or("");

        let text = raw_text
            .split_whitespace()
            .filter(|s| !s.starts_with("@_user_"))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();

        if text.is_empty() {
            return Ok(());
        }

        let chat_id = message["chat_id"].as_str().unwrap_or("").to_string();
        let empty = json!({});
        let sender = event.get("sender").unwrap_or(&empty);
        let sender_id = sender["sender_id"]["open_id"]
            .as_str()
            .unwrap_or("")
            .to_string();

        info!(
            "📩 [Feishu Webhook] User {}: {}",
            sender_id,
            if text.chars().count() > 50 {
                &text[..text
                    .char_indices()
                    .nth(50)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len())]
            } else {
                &text
            }
        );

        let platform_msg = PlatformMessage {
            event_type: "message".into(),
            event_data: json!({ "text": &text }),
            platform_user_id: sender_id,
            platform_chat_id: chat_id.clone(),
            text,
            username: None,
            platform: "feishu".into(),
        };

        let origin = channel
            .as_ref()
            .map(|ch| crate::core::context::ChannelOrigin {
                channel: ch.clone(),
                conversation: chat_id.clone(),
            });
        let result = {
            let tool_executor = FeishuToolExecutor::from_handler(self, &platform_msg);
            run_agent_for_message(
                &self.config,
                &self.project_root,
                &self.agent_slug,
                &platform_msg,
                Some(&tool_executor),
                origin,
            )
            .await
        };

        match result {
            Ok(reply) => {
                if !reply.text.is_empty() && reply.text != "(No response)" {
                    let token = get_access_token(
                        &self.app_id,
                        &self.app_secret,
                        &self.base_url,
                        &self.access_token,
                    )
                    .await?;
                    send_feishu_message(&token, &chat_id, &reply.text, &self.base_url).await?;
                }
            }
            Err(e) => {
                error!("[Feishu Webhook] Agent error: {}", e);
                let token = get_access_token(
                    &self.app_id,
                    &self.app_secret,
                    &self.base_url,
                    &self.access_token,
                )
                .await?;
                send_feishu_message(&token, &chat_id, &format!("Error: {}", e), &self.base_url)
                    .await?;
            }
        }

        Ok(())
    }
}

/// Send message to Feishu group via webhook URL (custom bot)
#[allow(dead_code)] // wired up by point 9 (workflow reply(channel=...))
pub async fn send_webhook(webhook_url: &str, text: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let resp = client
        .post(webhook_url)
        .json(&json!({
            "msg_type": "text",
            "content": {
                "text": text
            }
        }))
        .send()
        .await?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));

    if body["code"].as_i64() != Some(0) && !status.is_success() {
        warn!("[Feishu Webhook] Send failed: {} {:?}", status, body);
        return Err(anyhow::anyhow!("Feishu webhook error: {:?}", body));
    }

    info!("[Feishu Webhook] Message sent successfully");
    Ok(())
}

/// Send rich text message via webhook (Markdown-style post message)
async fn get_access_token(
    app_id: &str,
    app_secret: &str,
    base_url: &str,
    cache: &Mutex<Option<(String, std::time::Instant)>>,
) -> Result<String> {
    // Check cache (token valid for 2 hours, refresh 5 minutes early)
    if let Ok(guard) = cache.lock() {
        if let Some((ref token, ref created)) = *guard {
            if created.elapsed() < std::time::Duration::from_secs(7000) {
                return Ok(token.clone());
            }
        }
    }

    let client = reqwest::Client::new();
    let resp: Value = client
        .post(format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            base_url
        ))
        .json(&json!({
            "app_id": app_id,
            "app_secret": app_secret
        }))
        .send()
        .await?
        .json()
        .await?;

    // Check API response code
    let code = resp["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        return Err(anyhow::anyhow!(
            "Feishu token API error: code={}, msg={}",
            code,
            resp["msg"]
        ));
    }

    let token = resp["tenant_access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Feishu returned empty access token: {:?}", resp))?
        .to_string();

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((token.clone(), std::time::Instant::now()));
    }

    info!("[Feishu] Access token refreshed (len={})", token.len());
    Ok(token)
}

/// Send Feishu message (event subscription mode, requires access_token)
async fn send_feishu_message(token: &str, chat_id: &str, text: &str, base_url: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/open-apis/im/v1/messages", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .query(&[("receive_id_type", "chat_id")])
        .json(&json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": serde_json::to_string(&json!({"text": text}))?
        }))
        .send()
        .await?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));
    let api_code = body["code"].as_i64().unwrap_or(-1);

    if !status.is_success() || api_code != 0 {
        warn!(
            "[Feishu] Send message failed: HTTP {} | code: {} | body: {:?}",
            status, api_code, body
        );
        return Err(anyhow::anyhow!(
            "Feishu send failed: code={}, msg={}",
            api_code,
            body["msg"]
        ));
    }

    info!("[Feishu] Message sent to chat_id: {}", chat_id);
    Ok(())
}

// ======================================================================
// Channel impls — uniform Channel API over the Feishu adapters
// ======================================================================

/// Bidirectional Feishu event-subscription channel. Mounts a POST route at
/// `/webhook/feishu/<instance_id>` that the Feishu Open Platform pushes events
/// to; sends replies via Feishu OpenAPI (`im/v1/messages`).
///
/// Backed by the existing [`FeishuWebhookHandler`] state; the channel layer
/// adds route mounting and `Channel`-trait conformance.
pub struct FeishuEventChannel {
    id: String,
    instance_id: String,
    handler: Arc<FeishuWebhookHandler>,
}

impl FeishuEventChannel {
    pub fn new(instance_id: String, handler: FeishuWebhookHandler) -> Self {
        Self {
            id: format!("feishu:{}", instance_id),
            instance_id,
            handler: Arc::new(handler),
        }
    }
}

#[async_trait::async_trait]
impl crate::core::context::ChannelEgress for FeishuEventChannel {
    async fn send(&self, conversation: &str, text: &str) -> Result<()> {
        let token = get_access_token(
            &self.handler.app_id,
            &self.handler.app_secret,
            &self.handler.base_url,
            &self.handler.access_token,
        )
        .await?;
        send_feishu_message(&token, conversation, text, &self.handler.base_url).await
    }
}

#[async_trait::async_trait]
impl Channel for FeishuEventChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "feishu"
    }

    fn install_routes(
        self: Arc<Self>,
        router: axum::Router,
        _dispatcher: Arc<dyn MessageDispatcher>,
    ) -> axum::Router {
        let path = format!("/webhook/feishu/{}", self.instance_id);
        let handler = self.handler.clone();
        // Carry self as the channel egress so messages dispatched via this
        // route get a ChannelOrigin pointing back through Feishu OpenAPI.
        let channel: Arc<dyn crate::core::context::ChannelEgress> = self.clone();
        router.route(
            &path,
            axum::routing::post(move |Json(body): Json<Value>| {
                let handler = handler.clone();
                let channel = channel.clone();
                async move { Json(handler.handle_event_with_channel(body, channel).await) }
            }),
        )
    }
}

/// Egress-only Feishu channel: pushes plain-text messages to a Feishu group via
/// an incoming-webhook URL bound to that group. No ingress; `conversation` is
/// ignored because the URL itself selects the destination.
pub struct FeishuWebhookChannel {
    id: String,
    #[allow(dead_code)] // read in `send` (currently uncallable until point 9)
    webhook_url: String,
}

impl FeishuWebhookChannel {
    pub fn new(instance_id: String, webhook_url: String) -> Self {
        Self {
            id: format!("feishu-webhook:{}", instance_id),
            webhook_url,
        }
    }
}

#[async_trait::async_trait]
impl crate::core::context::ChannelEgress for FeishuWebhookChannel {
    async fn send(&self, _conversation: &str, text: &str) -> Result<()> {
        send_webhook(&self.webhook_url, text).await
    }
}

#[async_trait::async_trait]
impl Channel for FeishuWebhookChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "feishu"
    }
}

/// Discover Feishu channels from `[channels.feishu.<id>]`. Each entry produces
/// either an event channel (when `app_id` + `app_secret` are set) or a webhook
/// channel (when `incoming_webhook_url` is set), or both if you want both
/// directions registered separately.
pub fn discover_channels(
    config: &JuglansConfig,
    project_root: &Path,
) -> Vec<(Arc<dyn Channel>, String)> {
    let mut out: Vec<(Arc<dyn Channel>, String)> = Vec::new();
    for (instance_id, cfg) in &config.channels.feishu {
        let agent = cfg.agent.clone();

        if cfg.app_id.is_some() && cfg.app_secret.is_some() {
            let handler = FeishuWebhookHandler {
                config: config.clone(),
                project_root: project_root.to_path_buf(),
                agent_slug: agent.clone(),
                app_id: cfg.app_id.clone().unwrap_or_default(),
                app_secret: cfg.app_secret.clone().unwrap_or_default(),
                base_url: cfg.base_url.clone(),
                approvers: cfg.approvers.clone(),
                access_token: Mutex::new(None),
                processed_events: DashSet::new(),
            };
            let channel: Arc<dyn Channel> =
                Arc::new(FeishuEventChannel::new(instance_id.clone(), handler));
            out.push((channel, agent.clone()));
        }

        if let Some(url) = cfg.incoming_webhook_url.clone() {
            if !url.is_empty() {
                let channel: Arc<dyn Channel> =
                    Arc::new(FeishuWebhookChannel::new(instance_id.clone(), url));
                out.push((channel, agent));
            }
        }
    }
    out
}
