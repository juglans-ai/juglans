// src/adapters/mod.rs
#![cfg(not(target_arch = "wasm32"))]

pub mod telegram;
pub mod feishu;

use anyhow::{anyhow, Result};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, error, warn};

use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::core::executor::WorkflowExecutor;
use crate::core::parser::GraphParser;
use crate::services::agent_loader::AgentRegistry;
use crate::services::config::JuglansConfig;
use crate::services::interface::JuglansRuntime;
use crate::services::jug0::Jug0Client;
use crate::services::prompt_loader::PromptRegistry;

/// 平台消息的统一抽象
pub struct PlatformMessage {
    pub platform_user_id: String,
    pub platform_chat_id: String,
    pub text: String,
    pub username: Option<String>,
}

/// Bot 回复
pub struct BotReply {
    pub text: String,
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
        let tool_pattern = project_root
            .join("**/*.json")
            .to_string_lossy()
            .to_string();
        if let Ok(tools) = ToolLoader::load_from_glob(&tool_pattern, project_root) {
            if !tools.is_empty() {
                let mut registry = ToolRegistry::new();
                registry.register_all(tools);
                executor.set_tool_registry(Arc::new(registry));
            }
        }
    }
    executor.load_mcp_tools(config).await;

    let executor = Arc::new(executor);
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    // 3. 创建 context + 事件通道（用于收集 Token）
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // 设置输入
    ctx.set("input.message".to_string(), json!(message.text.clone()))
        .ok();

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
    let project_root_clone = project_root.to_path_buf();
    let agent_meta_clone = agent_meta.clone();

    let exec_handle = tokio::spawn(async move {
        let result = if let Some(wf_ref) = &agent_meta_clone.workflow {
            let is_file_path = wf_ref.ends_with(".jgflow")
                || wf_ref.starts_with("./")
                || wf_ref.starts_with("../")
                || Path::new(wf_ref).is_absolute();

            let wf_content = if is_file_path {
                let full_wf_path = if Path::new(wf_ref).is_absolute() {
                    PathBuf::from(wf_ref)
                } else {
                    agent_dir.join(wf_ref)
                };
                fs::read_to_string(&full_wf_path).map_err(|e| {
                    anyhow!("Workflow File Error: {} (tried {:?})", e, full_wf_path)
                })
            } else {
                let pattern = project_root_clone
                    .join(format!("**/{}.jgflow", wf_ref))
                    .to_string_lossy()
                    .to_string();
                let found = glob::glob(&pattern)
                    .ok()
                    .and_then(|mut paths| paths.find_map(|p| p.ok()));
                match found {
                    Some(path) => fs::read_to_string(&path).map_err(|e| {
                        anyhow!("Workflow File Error: {} (tried {:?})", e, path)
                    }),
                    None => Err(anyhow!(
                        "Workflow '{}' not found in workspace {:?}",
                        wf_ref,
                        project_root_clone
                    )),
                }
            };

            match wf_content {
                Ok(content) => match GraphParser::parse(&content) {
                    Ok(graph) => executor_clone
                        .execute_graph(Arc::new(graph), &ctx)
                        .await,
                    Err(e) => Err(anyhow!("Workflow Parse Error: {}", e)),
                },
                Err(e) => Err(e),
            }
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
            WorkflowEvent::ToolCall { result_tx, .. } => {
                // Bot 模式不支持 client tool bridge，返回空结果
                warn!("[Bot] Client tool call received but not supported in bot mode, skipping");
                let _ = result_tx.send(vec![]);
            }
        }
    }

    // 等待执行结束
    let _ = exec_handle.await;

    // 如果 reply_text 为空，尝试从 context 获取
    if reply_text.is_empty() {
        if let Ok(Some(val)) = WorkflowContext::new()
            .resolve_path("reply.output")
        {
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
