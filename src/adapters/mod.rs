// src/adapters/mod.rs
#![cfg(not(target_arch = "wasm32"))]

pub mod feishu;
pub mod telegram;

use anyhow::{anyhow, Result};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::core::executor::WorkflowExecutor;
use crate::core::parser::GraphParser;
use crate::services::agent_loader::AgentRegistry;
use crate::services::config::JuglansConfig;
use crate::services::interface::JuglansRuntime;
use crate::services::jug0::Jug0Client;
use crate::services::prompt_loader::PromptRegistry;

/// 标准化平台事件信封
///
/// 所有平台事件（消息、卡片回调等）统一格式，workflow 通过 $input.event_type 路由。
pub struct PlatformMessage {
    /// 事件类型: "message" | "card_action" | ...
    pub event_type: String,
    /// 事件专属数据（message: {"text": "..."}, card_action: {"action": "confirm", ...}）
    pub event_data: Value,
    pub platform_user_id: String,
    pub platform_chat_id: String,
    /// 便捷字段：消息文本（message 时有值，其他事件为空）
    pub text: String,
    pub username: Option<String>,
}

/// Bot 回复
pub struct BotReply {
    pub text: String,
}

/// 工具执行器 trait — 由平台适配器实现
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, tool_name: &str, args: Value) -> Result<String>;
}

/// 通过 jug0 聊天（SSE 客户端模式）
///
/// 发送消息到 jug0 /api/chat，读取 SSE 流：
/// - content 事件 → 拼接回复文本
/// - tool_call 事件 → 调用 tool_executor 执行工具 → POST /tool-result
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

    // 使用 eventsource-stream 解析 SSE
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
                // 默认事件 = content token
                if let Ok(data) = serde_json::from_str::<Value>(&event.data) {
                    if let Some(text) = data["text"].as_str() {
                        reply_text.push_str(text);
                    }
                }
            }
            "tool_call" => {
                // Client tool bridge: 执行工具并返回结果
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

/// 复用 web_server handle_chat 的核心逻辑，去除 SSE/HTTP 部分：
/// 1. 加载 agent → 创建 executor
/// 2. 创建 WorkflowContext，设置 $input.message
/// 3. 执行 workflow 或 direct chat
/// 4. 收集所有 Token 事件 → 拼接为回复文本
pub async fn run_agent_for_message(
    config: &JuglansConfig,
    project_root: &Path,
    agent_slug: &str,
    message: &PlatformMessage,
    tool_executor: Option<&dyn ToolExecutor>,
) -> Result<BotReply> {
    // 1. 加载 agent registry
    let mut agent_registry = AgentRegistry::new();
    let agent_pattern = project_root
        .join("**/*.jgagent")
        .to_string_lossy()
        .to_string();
    agent_registry.load_from_paths(&[agent_pattern])?;

    let (agent_meta, agent_file_path) = agent_registry
        .get_with_path(agent_slug)
        .ok_or_else(|| anyhow!("Agent '{}' not found in workspace", agent_slug))?;

    let agent_meta = agent_meta.clone();
    let agent_dir = agent_file_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    // 2. 创建 runtime + executor
    let runtime: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(config));

    let mut prompt_registry = PromptRegistry::new();
    let _ = prompt_registry.load_from_paths(&[project_root
        .join("**/*.jgprompt")
        .to_string_lossy()
        .to_string()]);

    let mut executor = WorkflowExecutor::new_with_debug(
        Arc::new(prompt_registry),
        Arc::new(agent_registry),
        runtime,
        config.debug.clone(),
    )
    .await;

    // 加载 tool definitions
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
    executor.load_mcp_tools(config).await;

    // 提前解析 workflow（在 Arc 包装前），以便调用 init_python_runtime / load_tools
    let parsed_workflow = if let Some(wf_ref) = &agent_meta.workflow {
        let is_file_path = wf_ref.ends_with(".jgflow")
            || wf_ref.starts_with("./")
            || wf_ref.starts_with("../")
            || Path::new(wf_ref).is_absolute();

        let wf_content =
            if is_file_path {
                let full_wf_path = if Path::new(wf_ref).is_absolute() {
                    PathBuf::from(wf_ref)
                } else {
                    agent_dir.join(wf_ref)
                };
                Some(fs::read_to_string(&full_wf_path).map_err(|e| {
                    anyhow!("Workflow File Error: {} (tried {:?})", e, full_wf_path)
                })?)
            } else {
                let pattern = project_root
                    .join(format!("**/{}.jgflow", wf_ref))
                    .to_string_lossy()
                    .to_string();
                glob::glob(&pattern)
                    .ok()
                    .and_then(|mut paths| paths.find_map(|p| p.ok()))
                    .map(|path| fs::read_to_string(&path))
                    .transpose()
                    .map_err(|e| anyhow!("Workflow File Error: {}", e))?
            };

        match wf_content {
            Some(content) => Some(Arc::new(GraphParser::parse(&content)?)),
            None => None,
        }
    } else {
        None
    };

    // 初始化 Python runtime + load workflow tools（需要 &mut self，必须在 Arc 前）
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

    // 3. 创建 context + 事件通道（用于收集 Token）
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // 设置标准化事件输入
    ctx.set("input.event_type".into(), json!(message.event_type))
        .ok();
    ctx.set("input.event_data".into(), message.event_data.clone())
        .ok();
    ctx.set("input.user_id".into(), json!(message.platform_user_id))
        .ok();
    ctx.set("input.chat_id".into(), json!(message.platform_chat_id))
        .ok();
    ctx.set("input.text".into(), json!(message.text)).ok();
    ctx.set("input.message".into(), json!(message.text)).ok(); // 兼容
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

    // 注入 juglans.toml 配置到 $config
    if let Ok(config_value) = serde_json::to_value(config) {
        ctx.set("config".to_string(), config_value).ok();
    }

    // 尝试解析消息为 JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&message.text) {
        if let Some(obj) = parsed.as_object() {
            for (k, v) in obj {
                ctx.set(format!("input.{}", k), v.clone()).ok();
            }
        }
    }

    // 4. 异步执行 workflow
    let executor_clone = executor.clone();
    let agent_meta_clone = agent_meta.clone();

    let exec_handle = tokio::spawn(async move {
        let result = if let Some(workflow) = parsed_workflow {
            executor_clone.execute_graph(workflow, &ctx).await
        } else {
            // 直接 chat 模式
            let mut params = HashMap::new();
            params.insert("agent".to_string(), agent_meta_clone.slug.clone());
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

    // 5. 收集所有 Token 事件 → 拼接为回复文本
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
                    let _ = result_tx.send(results);
                } else {
                    warn!("[Bot] Client tool call received but no executor available, skipping");
                    let _ = result_tx.send(vec![]);
                }
            }
            WorkflowEvent::Meta(_) => {
                // Bot 模式忽略 meta 事件
            }
        }
    }

    // 等待执行结束
    let _ = exec_handle.await;

    // 如果 reply_text 为空，尝试从 context 获取
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
