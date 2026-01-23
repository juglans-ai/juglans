// src/services/web_server.rs
#![cfg(not(target_arch = "wasm32"))]

use axum::{
    routing::{get, post},
    extract::{Extension, Query},
    response::sse::{Event, Sse},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer; 
use std::sync::Arc;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{info, debug, warn, error}; 
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use futures::{Stream, StreamExt}; 
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid; // 【新增】
use chrono::Utc; // 【新增】

use crate::services::agent_loader::AgentRegistry;
use crate::services::prompt_loader::PromptRegistry;
use crate::services::config::JuglansConfig;
use crate::services::jug0::Jug0Client;
use crate::services::interface::JuglansRuntime;
use crate::core::agent_parser::{AgentResource, AgentParser};
use crate::core::prompt_parser::{PromptResource, PromptParser};
use crate::core::executor::WorkflowExecutor;
use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::core::parser::GraphParser;

// --- API Models (兼容 Jug0) ---

#[derive(Serialize)]
pub struct AgentApiModel {
    pub id: Uuid,
    pub slug: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    // 简化处理：本地没有 system_prompt_id，这里填空或造一个
    pub system_prompt_id: Option<Uuid>, 
    pub default_model: String,
    pub temperature: Option<f64>,
    pub skills: Option<Vec<String>>,
    // 额外字段：Juglans 特有
    pub workflow: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct PromptApiModel {
    pub id: Uuid,
    pub slug: String,
    pub user_id: String,
    pub name: String,
    pub content: String,
    pub input_variables: Value,
    pub r#type: String,
    pub is_public: bool,
    pub is_system: bool,
    pub created_at: String,
}

struct WebState {
    pub project_root: PathBuf,
}

#[derive(Deserialize)]
pub struct AgentQuery {
    pub pattern: Option<String>,
}

#[derive(Deserialize)]
pub struct PromptQuery {
    pub pattern: Option<String>,
}

// 兼容 Jug0 的 Chat 请求结构
#[derive(Deserialize, Clone)]
pub struct ChatRequest {
    // jug0 标准字段
    pub chat_id: Option<Uuid>,
    pub messages: Option<Vec<MessagePart>>,
    pub agent: Option<AgentConfigInput>,
    pub model: Option<String>,
    pub tools: Option<Vec<Value>>,
    pub stream: Option<bool>,
    pub memory: Option<bool>,

    // Juglans 额外字段
    pub variables: Option<Value>,
    /// 是否无状态模式（不继承 chat_id）
    pub stateless: Option<bool>,
}

#[derive(Deserialize, Clone)]
pub struct AgentConfigInput {
    pub slug: Option<String>,
    pub id: Option<Uuid>,
    pub model: Option<String>,
    pub tools: Option<Vec<Value>>,
    pub system_prompt: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct MessagePart {
    #[serde(rename = "type", default = "default_message_type")]
    pub part_type: String,
    pub role: Option<String>,
    pub content: Option<String>,
    pub data: Option<Value>,
    pub tool_call_id: Option<String>,
}

fn default_message_type() -> String {
    "text".to_string()
}

// 辅助函数：根据 Slug 生成确定的 UUID v5
fn generate_deterministic_id(slug: &str) -> Uuid {
    let namespace = Uuid::NAMESPACE_DNS;
    Uuid::new_v5(&namespace, slug.as_bytes())
}

pub async fn start_web_server(host: String, port: u16, project_root: PathBuf) -> anyhow::Result<()> {
    let state = Arc::new(WebState {
        project_root: project_root.clone(),
    });

    let app = Router::new()
        .route("/api/agents", get(list_local_agents))
        .route("/api/prompts", get(list_local_prompts))
        .route("/api/chat", post(handle_chat)) 
        .layer(TraceLayer::new_for_http()) 
        .layer(CorsLayer::permissive())
        .layer(Extension(state));

    let ip_addr: std::net::IpAddr = host.parse().unwrap_or_else(|_| {
        warn!("Invalid host '{}', falling back to 127.0.0.1", host);
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))
    });

    let addr = SocketAddr::from((ip_addr, port));
    
    info!("--------------------------------------------------");
    info!("✨ Juglans Web Services Initialized");
    info!("📡 Listening on: http://{}", addr);
    info!("📂 Project Root: {:?}", project_root);
    info!("🔌 Endpoints (Jug0 Compatible):");
    info!("   - GET  /api/agents");
    info!("   - GET  /api/prompts");
    info!("   - POST /api/chat");
    info!("--------------------------------------------------");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("🚀 Server is ready and waiting for requests...");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn list_local_agents(
    Extension(state): Extension<Arc<WebState>>,
    Query(params): Query<AgentQuery>,
) -> Json<Vec<AgentApiModel>> { // 【修改】返回类型
    let mut registry = AgentRegistry::new();
    let pattern = params.pattern.unwrap_or_else(|| "**/*.jgagent".to_string());
    let full_pattern = state.project_root.join(&pattern).to_string_lossy().to_string();

    let mut results = Vec::new();
    match registry.load_from_paths(&[full_pattern]) {
        Ok(_) => {
            for key in registry.keys() {
                if let Some(agent) = registry.get(&key) {
                    // 转换为兼容模型
                    results.push(AgentApiModel {
                        id: generate_deterministic_id(&agent.slug),
                        slug: agent.slug.clone(),
                        user_id: "local".to_string(),
                        name: agent.name.clone(),
                        description: agent.description.clone(),
                        system_prompt_id: agent.system_prompt_slug.as_ref().map(|s| generate_deterministic_id(s)),
                        default_model: agent.model.clone(),
                        temperature: agent.temperature,
                        skills: Some(agent.skills.clone()),
                        workflow: agent.workflow.clone(),
                        created_at: Utc::now().to_rfc3339(),
                    });
                }
            }
        },
        Err(e) => warn!("❌ Failed to scan agents: {}", e),
    }
    Json(results)
}

async fn list_local_prompts(
    Extension(state): Extension<Arc<WebState>>,
    Query(params): Query<PromptQuery>,
) -> Json<Vec<PromptApiModel>> { // 【修改】返回类型
    let mut registry = PromptRegistry::new();
    let pattern = params.pattern.unwrap_or_else(|| "**/*.jgprompt".to_string());
    let full_pattern = state.project_root.join(&pattern).to_string_lossy().to_string();

    let mut results = Vec::new();
    match registry.load_from_paths(&[full_pattern]) {
        Ok(_) => {
            for slug in registry.keys() {
                if let Some(content) = registry.get(&slug) {
                    if let Ok(res) = PromptParser::parse(content) {
                        results.push(PromptApiModel {
                            id: generate_deterministic_id(&res.slug),
                            slug: res.slug,
                            user_id: "local".to_string(),
                            name: res.name,
                            content: res.content,
                            input_variables: res.inputs,
                            r#type: res.r#type,
                            is_public: true,
                            is_system: false, // 暂无法从文件推断，默认 false
                            created_at: Utc::now().to_rfc3339(),
                        });
                    }
                }
            }
        },
        Err(e) => warn!("❌ Failed to scan prompts: {}", e),
    }
    Json(results)
}

async fn handle_chat(
    Extension(state): Extension<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, Json<Value>> {
    let mut agent_registry = AgentRegistry::new();
    let agent_pattern = state.project_root.join("**/*.jgagent").to_string_lossy().to_string();
    agent_registry.load_from_paths(&[agent_pattern]).map_err(|e| Json(json!({"error": e.to_string()})))?;

    // 提取 agent slug（兼容 jug0 格式）
    let agent_slug = if let Some(ref agent_config) = req.agent {
        agent_config.slug.clone()
            .or(agent_config.id.map(|u| u.to_string()))
            .unwrap_or_else(|| "default".to_string())
    } else {
        "default".to_string()
    };

    // 提取 user message（兼容 jug0 的 messages 数组格式）
    let message_text = if let Some(ref msgs) = req.messages {
        // 取最后一条 user/text 消息
        msgs.iter()
            .filter(|m| m.role.as_deref() == Some("user") || m.part_type == "text")
            .last()
            .and_then(|m| m.content.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

    // 提取 chat_id（用于继承会话）
    let chat_id_str = req.chat_id.map(|id| id.to_string());

    // 是否无状态模式
    let is_stateless = req.stateless.unwrap_or(false) || req.chat_id.is_none();

    // 提取自定义 tools
    let custom_tools = req.tools.clone()
        .or_else(|| req.agent.as_ref().and_then(|a| a.tools.clone()));

    // 提取 system_prompt 覆盖
    let system_prompt_override = req.agent.as_ref().and_then(|a| a.system_prompt.clone());

    let (agent_meta, agent_file_path) = agent_registry.get_with_path(&agent_slug)
        .ok_or_else(|| Json(json!({"error": format!("Agent '{}' not found in workspace", agent_slug)})))?;

    let agent_meta = agent_meta.clone();
    let agent_dir = agent_file_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let config = JuglansConfig::load().map_err(|e| Json(json!({"error": e.to_string()})))?;
    let runtime: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(&config));

    let mut prompt_registry = PromptRegistry::new();
    let _ = prompt_registry.load_from_paths(&[state.project_root.join("**/*.jgprompt").to_string_lossy().to_string()]);

    let executor = Arc::new(WorkflowExecutor::new(
        Arc::new(prompt_registry),
        Arc::new(agent_registry),
        runtime
    ).await);

    let (tx, rx) = mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // 设置输入上下文
    ctx.set("input.message".to_string(), json!(message_text)).ok();

    // 如果有 chat_id，存入上下文供后续继承
    if let Some(ref cid) = chat_id_str {
        ctx.set("reply.chat_id".to_string(), json!(cid)).ok();
    }

    if let Some(vars) = req.variables {
        if let Some(obj) = vars.as_object() {
            for (k, v) in obj { ctx.set(format!("input.{}", k), v.clone()).ok(); }
        }
    }

    // 构建 chat 参数
    let stateless_flag = is_stateless;
    let tools_json = custom_tools.map(|t| serde_json::to_string(&t).unwrap_or_default());
    let sys_prompt = system_prompt_override;

    tokio::spawn(async move {
        let result = if let Some(wf_path) = &agent_meta.workflow {
            let full_wf_path = if Path::new(wf_path).is_absolute() {
                PathBuf::from(wf_path)
            } else {
                agent_dir.join(wf_path)
            };

            debug!("📂 Resolving workflow file: {:?}", full_wf_path);

            match fs::read_to_string(&full_wf_path) {
                Ok(content) => match GraphParser::parse(&content) {
                    Ok(graph) => executor.execute_graph(Arc::new(graph), &ctx).await,
                    Err(e) => Err(anyhow::anyhow!("Workflow Parse Error (in {:?}): {}", full_wf_path, e)),
                },
                Err(e) => Err(anyhow::anyhow!("Workflow File Error: {} (tried {:?})", e, full_wf_path)),
            }
        } else {
            // 直接 chat 模式
            let mut params = std::collections::HashMap::new();
            params.insert("agent".to_string(), agent_meta.slug.clone());
            params.insert("message".to_string(), "$input.message".to_string());

            // 传递 stateless 参数
            if stateless_flag {
                params.insert("stateless".to_string(), "true".to_string());
            }

            // 传递自定义 tools
            if let Some(tools_str) = tools_json {
                params.insert("tools".to_string(), tools_str);
            }

            // 传递 system_prompt 覆盖
            if let Some(sp) = sys_prompt {
                params.insert("system_prompt".to_string(), sp);
            }

            executor.execute_tool_internal("chat", &params, &ctx).await.map(|_| ())
        };

        if let Err(e) = result {
            error!("❌ Execution Error: {}", e);
            let _ = tx.send(WorkflowEvent::Error(e.to_string()));
        }
    });

    // SSE 事件格式对齐 Jug0
    let stream = UnboundedReceiverStream::new(rx).map(|event| {
        let json_event = match event {
            // 对齐 Jug0: { "type": "content", "text": "..." }
            WorkflowEvent::Token(t) => json!({ "type": "content", "text": t }),
            // Juglans 特有状态 -> meta 事件
            WorkflowEvent::Status(s) => json!({ "type": "meta", "status": s }),
            WorkflowEvent::Error(e) => json!({ "type": "error", "message": e }),
        };
        Ok(Event::default().data(json_event.to_string()))
    });

    Ok(Sse::new(stream))
}