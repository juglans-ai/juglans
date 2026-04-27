// src/adapters/mod.rs
#![cfg(not(target_arch = "wasm32"))]

pub mod discord;
pub mod feishu;
pub mod telegram;
pub mod wechat;

use anyhow::{anyhow, Result};
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

/// Pluggable message dispatch. Default behavior is the in-process workflow
/// runtime (see [`LocalDispatcher`]); orchestrator-style hosts (e.g.
/// juglans-wallet) wrap their own dispatcher so a long-running adapter loop
/// (`adapters::wechat::run_message_loop`, `adapters::telegram::start`, …)
/// can route incoming messages to whichever agent the orchestrator picks
/// without going through the public `run_agent_for_message` helper at all.
#[async_trait::async_trait]
pub trait MessageDispatcher: Send + Sync {
    async fn dispatch(&self, message: &PlatformMessage) -> Result<BotReply>;
}

/// Default dispatcher: load the workflow file from disk and run it in-process.
/// CLI-mode adapters use this; juglans-wallet supplies its own.
pub struct LocalDispatcher {
    pub config: JuglansConfig,
    pub project_root: std::path::PathBuf,
    pub agent_slug: String,
}

#[async_trait::async_trait]
impl MessageDispatcher for LocalDispatcher {
    async fn dispatch(&self, message: &PlatformMessage) -> Result<BotReply> {
        run_agent_for_message(
            &self.config,
            &self.project_root,
            &self.agent_slug,
            message,
            None,
        )
        .await
    }
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

    // 2. Create runtime + executor
    let runtime: Arc<LocalRuntime> = Arc::new(LocalRuntime::new_with_config(&config.ai));

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

    // Initialize the global conversation-history store from config (idempotent).
    if let Err(e) = crate::services::history::init_global(&config.history) {
        warn!("[history] init_global failed: {}", e);
    }

    // 3. Create context + event channel (for collecting tokens)
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // Derive a namespaced chat_id for history storage. Keeps different
    // platforms / users / agents on separate threads so a telegram chat
    // and a wechat chat for the same slug never collide.
    let derived_chat_id = format!(
        "{}:{}:{}",
        message.platform, message.platform_chat_id, agent_slug
    );

    // Set standardized event input
    ctx.set("input.platform".into(), json!(message.platform))
        .ok();
    ctx.set("input.event_type".into(), json!(message.event_type))
        .ok();
    ctx.set("input.event_data".into(), message.event_data.clone())
        .ok();
    ctx.set("input.user_id".into(), json!(message.platform_user_id))
        .ok();
    ctx.set("input.chat_id".into(), json!(derived_chat_id)).ok();
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
