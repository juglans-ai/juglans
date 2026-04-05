// src/adapters/mod.rs
#![cfg(not(target_arch = "wasm32"))]

pub mod feishu;
pub mod telegram;
pub mod wechat;

use anyhow::{anyhow, Result};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::core::executor::WorkflowExecutor;
use crate::core::parser::GraphParser;
use crate::services::config::JuglansConfig;
use crate::services::interface::JuglansRuntime;
use crate::services::jug0::Jug0Client;
use crate::services::local_runtime::LocalRuntime;
use crate::services::prompt_loader::PromptRegistry;

/// Standardized platform event envelope.
///
/// All platform events (messages, card callbacks, etc.) use a uniform format; workflow routes via $input.event_type.
pub struct PlatformMessage {
    /// Event type: "message" | "card_action" | ...
    pub event_type: String,
    /// Event-specific data (message: {"text": "..."}, card_action: {"action": "confirm", ...})
    pub event_data: Value,
    pub platform_user_id: String,
    pub platform_chat_id: String,
    /// Convenience field: message text (populated for message events, empty for others)
    pub text: String,
    pub username: Option<String>,
    /// Platform identifier: "telegram" | "feishu" | "wechat" | "web"
    pub platform: String,
}

/// Bot reply
pub struct BotReply {
    pub text: String,
}

/// Tool executor trait -- implemented by platform adapters
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, tool_name: &str, args: Value) -> Result<String>;
}

/// No-op tool executor (for adapters without platform-specific tools, e.g. Telegram)
pub struct NoopToolExecutor;

#[async_trait::async_trait]
impl ToolExecutor for NoopToolExecutor {
    async fn execute(&self, tool_name: &str, _args: Value) -> Result<String> {
        Ok(format!(
            "Tool '{}' is not available in this context",
            tool_name
        ))
    }
}

/// Chat via jug0 (SSE client mode).
///
/// Sends message to jug0 /api/chat and reads the SSE stream:
/// - content events -> concatenated into reply text
/// - tool_call events -> invoke tool_executor to run tools -> POST /tool-result
pub async fn chat_via_jug0(
    config: &JuglansConfig,
    agent_slug: &str,
    message: &PlatformMessage,
    tool_executor: &dyn ToolExecutor,
) -> Result<BotReply> {
    let jug0_base = config.jug0.base_url.trim_end_matches('/');
    let chat_url = format!("{}/api/chat", jug0_base);

    let body = json!({
        "chat_id": format!("@{}", agent_slug),
        "messages": [{"type": "text", "role": "user", "content": &message.text}],
        "variables": {
            "platform": &message.platform,
            "platform_user_id": &message.platform_user_id,
            "platform_chat_id": &message.platform_chat_id,
        },
        "stream": true,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&chat_url)
        .header("X-API-KEY", config.account.api_key.as_deref().unwrap_or(""))
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("jug0 chat request failed: {} {}", status, text));
    }

    let mut reply_text = String::new();

    // Parse SSE using eventsource-stream
    use eventsource_stream::Eventsource;
    let mut stream = resp.bytes_stream().eventsource();

    while let Some(event_result) = stream.next().await {
        let event = match event_result {
            Ok(e) => e,
            Err(e) => {
                warn!("[SSE] Parse error: {}", e);
                continue;
            }
        };

        let event_type = event.event.as_str();

        match event_type {
            "" | "message" => {
                // Default event = content token
                if let Ok(data) = serde_json::from_str::<Value>(&event.data) {
                    if let Some(text) = data["text"].as_str() {
                        reply_text.push_str(text);
                    }
                }
            }
            "tool_call" => {
                // Client tool bridge: execute tools and return results
                if let Ok(data) = serde_json::from_str::<Value>(&event.data) {
                    let call_id = data["call_id"].as_str().unwrap_or("").to_string();
                    let tools = data["tools"].as_array().cloned().unwrap_or_default();

                    info!(
                        "🔧 [Tool Bridge] Received {} tool call(s), call_id: {}",
                        tools.len(),
                        call_id
                    );

                    let mut results = Vec::new();
                    for tool in &tools {
                        let tool_name = tool["name"]
                            .as_str()
                            .or_else(|| tool["function"]["name"].as_str())
                            .unwrap_or("unknown");
                        let args_str = tool["arguments"]
                            .as_str()
                            .or_else(|| tool["function"]["arguments"].as_str())
                            .unwrap_or("{}");
                        let tool_call_id = tool["id"].as_str().unwrap_or("").to_string();
                        let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

                        info!("🔧 [Tool Bridge] Executing: {}({})", tool_name, args_str);

                        let content = match tool_executor.execute(tool_name, args).await {
                            Ok(result) => result,
                            Err(e) => {
                                error!("🔧 [Tool Bridge] {} failed: {}", tool_name, e);
                                format!("Error: {}", e)
                            }
                        };

                        results.push(json!({
                            "tool_call_id": tool_call_id,
                            "content": content,
                        }));
                    }

                    // POST /tool-result
                    let result_url = format!("{}/api/chat/tool-result", jug0_base);
                    let result_resp = client
                        .post(&result_url)
                        .header("X-API-KEY", config.account.api_key.as_deref().unwrap_or(""))
                        .json(&json!({
                            "call_id": call_id,
                            "results": results,
                        }))
                        .send()
                        .await;

                    match result_resp {
                        Ok(r) => info!("🔧 [Tool Bridge] tool-result response: {}", r.status()),
                        Err(e) => error!("🔧 [Tool Bridge] Failed to send tool-result: {}", e),
                    }
                }
            }
            "error" => {
                let msg = if let Ok(data) = serde_json::from_str::<Value>(&event.data) {
                    data["message"].as_str().unwrap_or(&event.data).to_string()
                } else {
                    event.data.clone()
                };
                error!("[SSE] Error event: {}", msg);
                if reply_text.is_empty() {
                    reply_text = format!("Error: {}", msg);
                }
            }
            "meta" | "done" => {
                debug!("[SSE] {}: {}", event_type, event.data);
            }
            _ => {
                debug!("[SSE] Unknown event type '{}': {}", event_type, event.data);
            }
        }
    }

    if reply_text.is_empty() {
        reply_text = "(No response)".to_string();
    }

    Ok(BotReply { text: reply_text })
}

/// Reuse core logic from web_server handle_chat, without the SSE/HTTP parts:
/// 1. Load agent -> create executor
/// 2. Create WorkflowContext, set $input.message
/// 3. Execute workflow or direct chat
/// 4. Collect all Token events -> concatenate into reply text
pub async fn run_agent_for_message(
    config: &JuglansConfig,
    project_root: &Path,
    agent_slug: &str,
    message: &PlatformMessage,
    tool_executor: Option<&dyn ToolExecutor>,
) -> Result<BotReply> {
    // 1. Find workflow file by slug (agent_slug is now a workflow name)
    let wf_path = {
        let jg_pattern = project_root
            .join(format!("**/{}.jg", agent_slug))
            .to_string_lossy()
            .to_string();
        glob::glob(&jg_pattern)
            .ok()
            .and_then(|mut paths| paths.find_map(|p| p.ok()))
            .ok_or_else(|| anyhow!("Workflow '{}' not found in workspace", agent_slug))?
    };

    let wf_content = fs::read_to_string(&wf_path)
        .map_err(|e| anyhow!("Workflow File Error: {} (tried {:?})", e, wf_path))?;

    // 2. Create runtime + executor (prefer local providers if configured)
    let runtime: Arc<dyn JuglansRuntime> = if config.ai.has_providers() {
        Arc::new(LocalRuntime::new_with_config(&config.ai))
    } else {
        Arc::new(Jug0Client::new(config))
    };

    let mut prompt_registry = PromptRegistry::new();
    let _ = prompt_registry.load_from_paths(&[
        project_root.join("**/*.jgx").to_string_lossy().to_string(),
        project_root
            .join("**/*.jgprompt")
            .to_string_lossy()
            .to_string(),
    ]);

    let mut executor =
        WorkflowExecutor::new_with_debug(Arc::new(prompt_registry), runtime, config.debug.clone())
            .await;

    // Load tool definitions
    {
        use crate::core::tool_loader::ToolLoader;
        use crate::services::tool_registry::ToolRegistry;
        let tool_pattern = project_root.join("**/*.json").to_string_lossy().to_string();
        if let Ok(tools) = ToolLoader::load_from_glob(&tool_pattern, project_root) {
            if !tools.is_empty() {
                let mut registry = ToolRegistry::new();
                registry.register_all(tools);
                executor.set_tool_registry(Arc::new(registry));
            }
        }
    }

    // Parse workflow + expand decorators
    let mut wf_graph = GraphParser::parse(&wf_content)?;
    crate::core::macro_expand::expand_decorators(&mut wf_graph)?;
    let parsed_workflow = Some(Arc::new(wf_graph));

    // Initialize Python runtime + load workflow tools (requires &mut self, must be before Arc)
    if let Some(ref wf) = parsed_workflow {
        executor.load_tools(wf).await;
        executor.apply_limits(&config.limits);
        if let Err(e) = executor.init_python_runtime(wf, config.limits.python_workers) {
            warn!("Failed to initialize Python runtime: {}", e);
        }
    }

    let executor = Arc::new(executor);
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    // 3. Create context + event channel (for collecting tokens)
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // Set standardized event input
    ctx.set("input.platform".into(), json!(message.platform))
        .ok();
    ctx.set("input.event_type".into(), json!(message.event_type))
        .ok();
    ctx.set("input.event_data".into(), message.event_data.clone())
        .ok();
    ctx.set("input.user_id".into(), json!(message.platform_user_id))
        .ok();
    ctx.set("input.chat_id".into(), json!(message.platform_chat_id))
        .ok();
    ctx.set("input.text".into(), json!(message.text)).ok();
    ctx.set("input.message".into(), json!(message.text)).ok(); // backward compat
    ctx.set(
        "input.platform_chat_id".into(),
        json!(message.platform_chat_id),
    )
    .ok();
    ctx.set(
        "input.platform_user_id".into(),
        json!(message.platform_user_id),
    )
    .ok();
    if let Some(ref username) = message.username {
        ctx.set("input.username".into(), json!(username)).ok();
    }

    // Inject juglans.toml config into $config
    if let Ok(config_value) = serde_json::to_value(config) {
        ctx.set("config".to_string(), config_value).ok();
    }

    // Try to parse message as JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&message.text) {
        if let Some(obj) = parsed.as_object() {
            for (k, v) in obj {
                ctx.set(format!("input.{}", k), v.clone()).ok();
            }
        }
    }

    // 4. Execute workflow asynchronously
    let executor_clone = executor.clone();
    let agent_slug_owned = agent_slug.to_string();

    let exec_handle = tokio::spawn(async move {
        let result = if let Some(workflow) = parsed_workflow {
            executor_clone.execute_graph(workflow, &ctx).await
        } else {
            // Direct chat mode (remote agent slug)
            let mut params = HashMap::new();
            params.insert("agent".to_string(), agent_slug_owned);
            params.insert("message".to_string(), "$input.message".to_string());

            executor_clone
                .execute_tool_internal("chat", &params, &ctx)
                .await
                .map(|_| ())
        };

        if let Err(e) = result {
            error!("Bot execution error: {}", e);
            let _ = tx.send(WorkflowEvent::Error(e.to_string()));
        }
    });

    // 5. Collect all Token events -> concatenate into reply text
    let mut reply_text = String::new();

    while let Some(event) = rx.recv().await {
        match event {
            WorkflowEvent::Token(t) => {
                reply_text.push_str(&t);
            }
            WorkflowEvent::Status(s) => {
                debug!("[Bot Status] {}", s);
            }
            WorkflowEvent::Error(e) => {
                if reply_text.is_empty() {
                    reply_text = format!("Error: {}", e);
                }
            }
            WorkflowEvent::ToolCall {
                tools, result_tx, ..
            } => {
                if let Some(executor) = tool_executor {
                    let mut results = vec![];
                    for tool in &tools {
                        let tool_name = tool["name"]
                            .as_str()
                            .or_else(|| tool.pointer("/function/name").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");
                        let args_str = tool["arguments"]
                            .as_str()
                            .or_else(|| {
                                tool.pointer("/function/arguments").and_then(|v| v.as_str())
                            })
                            .unwrap_or("{}");
                        let tool_call_id = tool["id"].as_str().unwrap_or("").to_string();
                        let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

                        info!("🔧 [Bot] Executing tool: {}({})", tool_name, args_str);
                        let content = match executor.execute(tool_name, args).await {
                            Ok(result) => result,
                            Err(e) => {
                                error!("🔧 [Bot] Tool {} failed: {}", tool_name, e);
                                format!("Error: {}", e)
                            }
                        };
                        results.push(crate::core::context::ToolResultPayload {
                            tool_call_id,
                            content,
                        });
                    }
                    let _ = result_tx.send((results, None));
                } else {
                    warn!("[Bot] Client tool call received but no executor available, skipping");
                    let _ = result_tx.send((vec![], None));
                }
            }
            WorkflowEvent::Meta(_)
            | WorkflowEvent::Yield(_)
            | WorkflowEvent::ToolStart(_)
            | WorkflowEvent::ToolComplete(_)
            | WorkflowEvent::NodeStart(_)
            | WorkflowEvent::NodeComplete(_) => {
                // Bot mode ignores meta / yield / tool / node events
            }
        }
    }

    // Wait for execution to finish
    let _ = exec_handle.await;

    // If reply_text is empty, try to get from context
    if reply_text.is_empty() {
        if let Ok(Some(val)) = WorkflowContext::new().resolve_path("reply.output") {
            if let Some(s) = val.as_str() {
                reply_text = s.to_string();
            }
        }
    }

    if reply_text.is_empty() {
        reply_text = "(No response)".to_string();
    }

    Ok(BotReply { text: reply_text })
}
