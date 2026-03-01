// src/services/web_server.rs
#![cfg(not(target_arch = "wasm32"))]

use axum::{
    body::Body,
    extract::{Extension, FromRequest, Multipart, Query},
    http::{HeaderMap, Method, Request, StatusCode, Uri},
    response::{
        sse::{Event, Sse},
        Html, Response,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::core::context::{ToolResultPayload, WorkflowContext, WorkflowEvent};
use crate::core::executor::WorkflowExecutor;
use crate::core::parser::GraphParser;
use crate::core::prompt_parser::PromptParser;
use crate::core::resolver;
use crate::core::validator::WorkflowValidator;
use crate::services::agent_loader::AgentRegistry;
use crate::services::config::JuglansConfig;
use crate::services::interface::JuglansRuntime;
use crate::services::jug0::Jug0Client;
use crate::services::prompt_loader::PromptRegistry;

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

/// Info about a discovered serve() workflow
#[derive(Clone)]
struct ServeWorkflowInfo {
    pub file_path: PathBuf,
    pub slug: String,
    pub entry_node: String,
}

struct WebState {
    pub project_root: PathBuf,
    pub start_time: Instant,
    pub start_datetime: DateTime<Utc>,
    pub host: String,
    pub port: u16,
    pub jug0_base_url: String,
    pub mcp_server_count: usize,
    /// Pending client tool calls waiting for frontend results
    pub pending_tool_calls: Arc<Mutex<HashMap<String, oneshot::Sender<Vec<ToolResultPayload>>>>>,
    /// Discovered serve() workflow for HTTP backend
    pub serve_workflow: Option<ServeWorkflowInfo>,
}

#[derive(Deserialize)]
pub struct AgentQuery {
    pub pattern: Option<String>,
}

#[derive(Deserialize)]
pub struct PromptQuery {
    pub pattern: Option<String>,
}

#[derive(Deserialize)]
pub struct WorkflowQuery {
    pub pattern: Option<String>,
}

#[derive(Serialize)]
pub struct WorkflowApiModel {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub node_count: usize,
    pub is_valid: bool,
    pub errors: usize,
    pub warnings: usize,
    pub issues: Vec<String>,
    pub created_at: String,
}

/// Chat ID input - can be UUID (existing chat) or @handle (start chat with agent)
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ChatIdInput {
    Uuid(Uuid),
    Handle(String),
}

// 兼容 Jug0 的 Chat 请求结构
#[derive(Deserialize, Clone)]
pub struct ChatRequest {
    // jug0 标准字段
    /// Chat ID: UUID for existing chat, or @handle to start with agent
    pub chat_id: Option<ChatIdInput>,
    pub messages: Option<Vec<MessagePart>>,
    pub agent: Option<AgentConfigInput>,
    #[serde(rename = "model")]
    pub _model: Option<String>,
    pub tools: Option<Vec<Value>>,
    #[serde(rename = "stream")]
    pub _stream: Option<bool>,
    #[serde(rename = "memory")]
    pub _memory: Option<bool>,

    // Juglans 额外字段
    pub variables: Option<Value>,
    /// 消息状态：context_visible | context_hidden | display_only | silent
    pub state: Option<String>,
    /// jug0 传入的用户消息 ID（workflow 模式下用于回溯更新用户消息状态）
    pub user_message_id: Option<i32>,
    /// 是否推送内部 tool 执行事件到 SSE 流（默认 false）
    pub stream_tool_events: Option<bool>,
}

#[derive(Deserialize, Clone)]
pub struct AgentConfigInput {
    pub slug: Option<String>,
    pub id: Option<Uuid>,
    #[serde(rename = "model")]
    pub _model: Option<String>,
    pub tools: Option<Vec<Value>>,
    pub system_prompt: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct MessagePart {
    #[serde(rename = "type", default = "default_message_type")]
    pub part_type: String,
    pub role: Option<String>,
    pub content: Option<String>,
    #[serde(rename = "data")]
    pub _data: Option<Value>,
    #[serde(rename = "tool_call_id")]
    pub _tool_call_id: Option<String>,
}

fn default_message_type() -> String {
    "text".to_string()
}

// 辅助函数：根据 Slug 生成确定的 UUID v5
fn generate_deterministic_id(slug: &str) -> Uuid {
    let namespace = Uuid::NAMESPACE_DNS;
    Uuid::new_v5(&namespace, slug.as_bytes())
}

// 格式化运行时长
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;

    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, mins, s)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins, s)
    } else if mins > 0 {
        format!("{}m {}s", mins, s)
    } else {
        format!("{}s", s)
    }
}

/// 扫描 project_root 下所有 .jg/.jgflow，找到含 serve() 节点的 workflow
fn discover_serve_workflow(project_root: &Path) -> Option<ServeWorkflowInfo> {
    use crate::core::graph::NodeType;

    let pattern_jg = project_root.join("**/*.jg").to_string_lossy().to_string();
    let pattern_jgflow = project_root
        .join("**/*.jgflow")
        .to_string_lossy()
        .to_string();

    let all_paths = glob::glob(&pattern_jg)
        .into_iter()
        .chain(glob::glob(&pattern_jgflow))
        .flatten()
        .flatten();

    for entry in all_paths {
        if let Ok(content) = fs::read_to_string(&entry) {
            if let Ok(graph) = GraphParser::parse(&content) {
                for node_idx in graph.graph.node_indices() {
                    let node = &graph.graph[node_idx];
                    if let NodeType::Task(action) = &node.node_type {
                        if action.name == "serve" {
                            let slug = if graph.slug.is_empty() {
                                entry
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown")
                                    .to_string()
                            } else {
                                graph.slug.clone()
                            };
                            info!(
                                "🌐 Discovered serve() workflow: {} (node: [{}]) at {:?}",
                                slug, node.id, entry
                            );
                            return Some(ServeWorkflowInfo {
                                file_path: entry,
                                slug,
                                entry_node: node.id.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    None
}

/// Convert HeaderMap to JSON object
fn headers_to_json(headers: &HeaderMap) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            map.insert(name.as_str().to_string(), json!(v));
        }
    }
    Value::Object(map)
}

/// Catch-all handler for serve() workflow
async fn handle_serve_request(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Extension(state): Extension<Arc<WebState>>,
    request: Request<Body>,
) -> Response {
    let serve_info = match &state.serve_workflow {
        Some(info) => info.clone(),
        None => {
            return error_response(StatusCode::NOT_FOUND, "No serve() workflow found");
        }
    };

    // 解析 workflow
    let content = match fs::read_to_string(&serve_info.file_path) {
        Ok(c) => c,
        Err(e) => {
            error!("❌ [Serve] Failed to read workflow: {}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to read workflow: {}", e),
            );
        }
    };

    let mut graph = match GraphParser::parse(&content) {
        Ok(g) => g,
        Err(e) => {
            error!("❌ [Serve] Failed to parse workflow: {}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Workflow parse error: {}", e),
            );
        }
    };

    // Resolve flow imports
    let wf_base_dir = serve_info.file_path.parent().unwrap_or(Path::new("."));
    let wf_canonical = serve_info
        .file_path
        .canonicalize()
        .unwrap_or(serve_info.file_path.clone());
    let mut import_stack = vec![wf_canonical.clone()];
    let at_base: Option<PathBuf> = JuglansConfig::load()
        .ok()
        .and_then(|c| c.paths.base.map(|b| state.project_root.join(b)));
    if let Err(e) = resolver::resolve_lib_imports(
        &mut graph,
        wf_base_dir,
        &mut import_stack,
        at_base.as_deref(),
    ) {
        error!("❌ [Serve] Lib import error: {}", e);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Lib import error: {}", e),
        );
    }
    import_stack = vec![wf_canonical];
    if let Err(e) = resolver::resolve_flow_imports(
        &mut graph,
        wf_base_dir,
        &mut import_stack,
        at_base.as_deref(),
    ) {
        error!("❌ [Serve] Flow import error: {}", e);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Flow import error: {}", e),
        );
    }

    // 构建 executor
    let config = match JuglansConfig::load() {
        Ok(c) => c,
        Err(e) => {
            error!("❌ [Serve] Failed to load config: {}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Config error: {}", e),
            );
        }
    };

    let runtime: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(&config));

    let mut prompt_registry = PromptRegistry::new();
    let _ = prompt_registry.load_from_paths(&[state
        .project_root
        .join("**/*.jgprompt")
        .to_string_lossy()
        .to_string()]);

    let mut agent_registry = AgentRegistry::new();
    let _ = agent_registry.load_from_paths(&[state
        .project_root
        .join("**/*.jgagent")
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
        let tool_pattern = state
            .project_root
            .join("**/*.json")
            .to_string_lossy()
            .to_string();
        if let Ok(tools) = ToolLoader::load_from_glob(&tool_pattern, &state.project_root) {
            if !tools.is_empty() {
                let mut registry = ToolRegistry::new();
                registry.register_all(tools);
                executor.set_tool_registry(Arc::new(registry));
            }
        }
    }
    executor.load_mcp_tools(&config).await;

    let executor = Arc::new(executor);
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    // 创建 context，注入请求数据到 $input
    let (tx, _rx) = mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx);

    // 解析 query string
    let query_map: HashMap<String, String> = uri
        .query()
        .map(|q| {
            q.split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    let key = parts.next()?.to_string();
                    let value = parts.next().unwrap_or("").to_string();
                    Some((key, value))
                })
                .collect()
        })
        .unwrap_or_default();

    // 解析 body — 检测 multipart 或常规 body
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("multipart/form-data") {
        // Multipart: 文本字段 → $input.fields, 文件 → 写临时文件 → $input.files
        let multipart_result: Result<Multipart, _> = Multipart::from_request(request, &()).await;
        match multipart_result {
            Ok(mut multipart) => {
                let mut fields: HashMap<String, Value> = HashMap::new();
                let mut files: HashMap<String, Value> = HashMap::new();

                while let Ok(Some(field)) = multipart.next_field().await {
                    let field_name = field.name().unwrap_or("unnamed").to_string();
                    let file_name = field.file_name().map(|f| f.to_string());

                    if let Some(filename) = file_name {
                        // 文件字段 → 写到临时目录
                        match field.bytes().await {
                            Ok(data) => {
                                let tmp_dir = std::env::temp_dir().join("jg_uploads");
                                let _ = fs::create_dir_all(&tmp_dir);
                                let safe_name = filename.replace(
                                    |c: char| {
                                        !c.is_alphanumeric() && c != '.' && c != '-' && c != '_'
                                    },
                                    "_",
                                );
                                let tmp_path = tmp_dir
                                    .join(format!("{}_{safe_name}", Uuid::new_v4().as_simple()));
                                if let Err(e) = std::fs::write(&tmp_path, &data) {
                                    error!("❌ [Serve] Failed to write upload: {}", e);
                                } else {
                                    files.insert(
                                        field_name,
                                        json!({
                                            "path": tmp_path.to_string_lossy().to_string(),
                                            "filename": filename,
                                            "size": data.len(),
                                        }),
                                    );
                                }
                            }
                            Err(e) => {
                                error!("❌ [Serve] Failed to read multipart field: {}", e);
                            }
                        }
                    } else {
                        // 文本字段
                        if let Ok(text) = field.text().await {
                            let val: Value =
                                serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
                            fields.insert(field_name, val);
                        }
                    }
                }

                ctx.set("input.fields".to_string(), json!(fields)).ok();
                ctx.set("input.files".to_string(), json!(files)).ok();
                ctx.set("input.body".to_string(), Value::Null).ok();
            }
            Err(e) => {
                error!("❌ [Serve] Multipart parse error: {}", e);
                return error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Multipart parse error: {}", e),
                );
            }
        }
    } else {
        // 常规 body (JSON / string)
        let body = match axum::body::to_bytes(request.into_body(), 64 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => {
                error!("❌ [Serve] Failed to read body: {}", e);
                return error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Failed to read body: {}", e),
                );
            }
        };
        let body_value: Value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body)
                .unwrap_or_else(|_| json!(String::from_utf8_lossy(&body).to_string()))
        };
        ctx.set("input.body".to_string(), body_value).ok();
    }

    ctx.set("input.method".to_string(), json!(method.as_str()))
        .ok();
    ctx.set("input.path".to_string(), json!(uri.path())).ok();
    ctx.set("input.query".to_string(), json!(query_map)).ok();
    ctx.set("input.headers".to_string(), headers_to_json(&headers))
        .ok();

    // 注入 path_parts — 按 / 拆分，方便工作流提取 path params
    let path_parts: Vec<&str> = uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    ctx.set("input.path_parts".to_string(), json!(path_parts))
        .ok();

    debug!(
        "🌐 [Serve] {} {} -> workflow '{}'",
        method,
        uri.path(),
        serve_info.slug
    );

    // 执行 workflow
    if let Err(e) = executor.execute_graph(Arc::new(graph), &ctx).await {
        error!("❌ [Serve] Execution error: {}", e);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Execution error: {}", e),
        );
    }

    // 读取 response
    let status_code = ctx
        .resolve_path("response.status")
        .ok()
        .flatten()
        .and_then(|v| v.as_u64())
        .unwrap_or(200) as u16;
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK);

    let resp_headers: Option<Value> = ctx.resolve_path("response.headers").ok().flatten();
    let resp_file: Option<String> = ctx
        .resolve_path("response.file")
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(|s| s.to_string()));

    let mut builder = Response::builder().status(status);
    let mut has_content_type = false;

    // 应用自定义 headers
    if let Some(Value::Object(map)) = resp_headers {
        for (k, v) in &map {
            if let Some(val) = v.as_str() {
                if k.to_lowercase() == "content-type" {
                    has_content_type = true;
                }
                builder = builder.header(k.as_str(), val);
            }
        }
    }

    if let Some(file_path) = resp_file {
        // 二进制文件响应
        match tokio::fs::read(&file_path).await {
            Ok(bytes) => {
                if !has_content_type {
                    builder = builder.header("content-type", "application/octet-stream");
                }
                builder.body(Body::from(bytes)).unwrap_or_else(|_| {
                    error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to build response",
                    )
                })
            }
            Err(e) => {
                error!("❌ [Serve] Failed to read file {}: {}", file_path, e);
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("File not found: {}", e),
                )
            }
        }
    } else {
        // JSON 响应（默认）
        let response_body = ctx
            .resolve_path("response.body")
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                ctx.resolve_path("output")
                    .ok()
                    .flatten()
                    .unwrap_or(json!(null))
            });
        if !has_content_type {
            builder = builder.header("content-type", "application/json");
        }
        let json_bytes = serde_json::to_vec(&response_body).unwrap_or_default();
        builder.body(Body::from(json_bytes)).unwrap_or_else(|_| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            )
        })
    }
}

/// Helper: JSON error response
fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::to_vec(&json!({"error": message})).unwrap_or_default();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("internal error"))
                .unwrap()
        })
}

/// Dashboard 页面
async fn dashboard(Extension(state): Extension<Arc<WebState>>) -> Html<String> {
    let uptime = format_uptime(state.start_time.elapsed().as_secs());

    // 扫描 prompts
    let mut prompt_registry = PromptRegistry::new();
    let prompt_pattern = state
        .project_root
        .join("**/*.jgprompt")
        .to_string_lossy()
        .to_string();
    let _ = prompt_registry.load_from_paths(&[prompt_pattern]);

    let mut prompts_html = String::new();
    for slug in prompt_registry.keys() {
        if let Some(content) = prompt_registry.get(&slug) {
            if let Ok(res) = PromptParser::parse(content) {
                prompts_html.push_str(&format!(
                    "    {} - {} (type: {})\n",
                    res.slug, res.name, res.r#type
                ));
            }
        }
    }
    if prompts_html.is_empty() {
        prompts_html = "    (none)\n".to_string();
    }

    // 扫描 agents
    let mut agent_registry = AgentRegistry::new();
    let agent_pattern = state
        .project_root
        .join("**/*.jgagent")
        .to_string_lossy()
        .to_string();
    let _ = agent_registry.load_from_paths(&[agent_pattern]);

    let mut agents_html = String::new();
    for key in agent_registry.keys() {
        if let Some(agent) = agent_registry.get(&key) {
            let wf_info = agent
                .workflow
                .as_ref()
                .map(|w| format!(" [workflow: {}]", w))
                .unwrap_or_default();
            agents_html.push_str(&format!(
                "    {} - {} (model: {}){}\n",
                agent.slug, agent.name, agent.model, wf_info
            ));
        }
    }
    if agents_html.is_empty() {
        agents_html = "    (none)\n".to_string();
    }

    // 扫描 workflows (.jg and .jgflow)
    let wf_pattern_jg = state
        .project_root
        .join("**/*.jg")
        .to_string_lossy()
        .to_string();
    let wf_pattern_jgflow = state
        .project_root
        .join("**/*.jgflow")
        .to_string_lossy()
        .to_string();
    let mut workflows_html = String::new();
    let mut workflow_count = 0;
    let mut workflow_valid_count = 0;
    let mut workflow_error_count = 0;

    let wf_paths = glob::glob(&wf_pattern_jg)
        .into_iter()
        .chain(glob::glob(&wf_pattern_jgflow))
        .flatten()
        .flatten();

    for entry in wf_paths {
        if let Ok(content) = fs::read_to_string(&entry) {
            let file_name = entry
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            workflow_count += 1;

            match GraphParser::parse(&content) {
                Ok(graph) => {
                    let slug = if graph.slug.is_empty() {
                        file_name.to_string()
                    } else {
                        graph.slug.clone()
                    };
                    let name = if graph.name.is_empty() {
                        "-".to_string()
                    } else {
                        graph.name.clone()
                    };
                    let node_count = graph.graph.node_count();

                    // Run validation
                    let validation = WorkflowValidator::validate(&graph);
                    let (status_icon, status_text) = if validation.is_valid {
                        if validation.warning_count() > 0 {
                            ("⚠️", format!("{} warning(s)", validation.warning_count()))
                        } else {
                            workflow_valid_count += 1;
                            ("✓", "valid".to_string())
                        }
                    } else {
                        workflow_error_count += 1;
                        (
                            "✗",
                            format!(
                                "{} error(s), {} warning(s)",
                                validation.error_count(),
                                validation.warning_count()
                            ),
                        )
                    };

                    workflows_html.push_str(&format!(
                        "    <span class=\"wf-status-{}\">{}</span> {} - {} ({} nodes) [{}]\n",
                        if validation.is_valid { "ok" } else { "err" },
                        status_icon,
                        slug,
                        name,
                        node_count,
                        status_text
                    ));

                    // Show first few issues
                    for err in validation.errors.iter().take(3) {
                        workflows_html.push_str(&format!(
                            "       <span class=\"error\">└─ {} {}</span>\n",
                            err.code, err.message
                        ));
                    }
                    for warn in validation.warnings.iter().take(2) {
                        workflows_html.push_str(&format!(
                            "       <span class=\"warning\">└─ {} {}</span>\n",
                            warn.code, warn.message
                        ));
                    }
                }
                Err(e) => {
                    workflow_error_count += 1;
                    workflows_html.push_str(&format!(
                        "    <span class=\"wf-status-err\">✗</span> {} <span class=\"error\">(parse error: {})</span>\n",
                        file_name,
                        e.to_string().lines().next().unwrap_or("unknown error")
                    ));
                }
            }
        }
    }
    if workflows_html.is_empty() {
        workflows_html = "    (none)\n".to_string();
    }

    // Build workflow summary
    let workflow_summary = if workflow_count > 0 {
        format!(
            "{} found — <span class=\"status\">{} valid</span>{}",
            workflow_count,
            workflow_valid_count,
            if workflow_error_count > 0 {
                format!(
                    ", <span class=\"error\">{} with errors</span>",
                    workflow_error_count
                )
            } else {
                "".to_string()
            }
        )
    } else {
        "0 found".to_string()
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Juglans Dashboard</title>
    <style>
        body {{ font-family: monospace; background: #1a1a1a; color: #e0e0e0; padding: 20px; line-height: 1.6; }}
        h1 {{ color: #4CAF50; }}
        h2 {{ color: #81C784; margin-top: 30px; }}
        pre {{ background: #2d2d2d; padding: 15px; border-radius: 5px; overflow-x: auto; }}
        .status {{ color: #4CAF50; }}
        .label {{ color: #888; }}
        .error {{ color: #ef5350; }}
        .warning {{ color: #ffa726; }}
        .wf-status-ok {{ color: #4CAF50; }}
        .wf-status-err {{ color: #ef5350; }}
    </style>
</head>
<body>
<h1>Juglans Dashboard</h1>

<h2>Server Status</h2>
<pre>
<span class="label">Address:</span>      http://{}:{}
<span class="label">Project Root:</span> {}
<span class="label">Started:</span>      {}
<span class="label">Uptime:</span>       {}
<span class="label">Jug0 API:</span>     {}
<span class="label">MCP Servers:</span>  {}
</pre>

<h2>Prompts ({} found)</h2>
<pre>
{}</pre>

<h2>Agents ({} found)</h2>
<pre>
{}</pre>

<h2>Workflows ({})</h2>
<pre>
{}</pre>

<h2>API Endpoints</h2>
<pre>
    GET  /               - This dashboard
    GET  /api/agents     - List agents
    GET  /api/prompts    - List prompts
    GET  /api/workflows  - List workflows (with validation)
    POST /api/chat       - Chat endpoint (jug0 compatible)
</pre>

</body>
</html>"#,
        state.host,
        state.port,
        state.project_root.display(),
        state.start_datetime.format("%Y-%m-%d %H:%M:%S UTC"),
        uptime,
        state.jug0_base_url,
        state.mcp_server_count,
        prompt_registry.keys().len(),
        prompts_html,
        agent_registry.keys().len(),
        agents_html,
        workflow_summary,
        workflows_html,
    );

    Html(html)
}

pub async fn start_web_server(
    host: String,
    port: u16,
    project_root: PathBuf,
) -> anyhow::Result<()> {
    let config = JuglansConfig::load().ok();

    // 扫描 serve() workflow
    let serve_workflow = discover_serve_workflow(&project_root);

    let state = Arc::new(WebState {
        project_root: project_root.clone(),
        start_time: Instant::now(),
        start_datetime: Utc::now(),
        host: host.clone(),
        port,
        jug0_base_url: config
            .as_ref()
            .map(|c| c.jug0.base_url.clone())
            .unwrap_or_else(|| "N/A".to_string()),
        mcp_server_count: config.as_ref().map(|c| c.mcp_servers.len()).unwrap_or(0),
        pending_tool_calls: Arc::new(Mutex::new(HashMap::new())),
        serve_workflow: serve_workflow.clone(),
    });

    let mut app = Router::new()
        .route("/", get(dashboard))
        .route("/api/agents", get(list_local_agents))
        .route("/api/prompts", get(list_local_prompts))
        .route("/api/workflows", get(list_local_workflows))
        .route("/api/chat", post(handle_chat))
        .route("/api/chat/tool-result", post(handle_tool_result));

    // 如果发现 serve() workflow，注册 catch-all fallback
    if serve_workflow.is_some() {
        app = app.fallback(handle_serve_request);
    }

    let app = app
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
    info!("   - GET  /api/workflows");
    info!("   - POST /api/chat");
    if let Some(ref sw) = serve_workflow {
        info!("🌐 HTTP Backend: {} (entry: [{}])", sw.slug, sw.entry_node);
        info!("   - All other routes -> serve() workflow");
    }
    info!("--------------------------------------------------");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("🚀 Server is ready and waiting for requests...");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn list_local_agents(
    Extension(state): Extension<Arc<WebState>>,
    Query(params): Query<AgentQuery>,
) -> Json<Vec<AgentApiModel>> {
    // 【修改】返回类型
    let mut registry = AgentRegistry::new();
    let pattern = params.pattern.unwrap_or_else(|| "**/*.jgagent".to_string());
    let full_pattern = state
        .project_root
        .join(&pattern)
        .to_string_lossy()
        .to_string();

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
                        system_prompt_id: agent
                            .system_prompt_slug
                            .as_ref()
                            .map(|s| generate_deterministic_id(s)),
                        default_model: agent.model.clone(),
                        temperature: agent.temperature,
                        skills: Some(agent.skills.clone()),
                        workflow: agent.workflow.clone(),
                        created_at: Utc::now().to_rfc3339(),
                    });
                }
            }
        }
        Err(e) => warn!("❌ Failed to scan agents: {}", e),
    }
    Json(results)
}

async fn list_local_prompts(
    Extension(state): Extension<Arc<WebState>>,
    Query(params): Query<PromptQuery>,
) -> Json<Vec<PromptApiModel>> {
    // 【修改】返回类型
    let mut registry = PromptRegistry::new();
    let pattern = params
        .pattern
        .unwrap_or_else(|| "**/*.jgprompt".to_string());
    let full_pattern = state
        .project_root
        .join(&pattern)
        .to_string_lossy()
        .to_string();

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
        }
        Err(e) => warn!("❌ Failed to scan prompts: {}", e),
    }
    Json(results)
}

async fn list_local_workflows(
    Extension(state): Extension<Arc<WebState>>,
    Query(params): Query<WorkflowQuery>,
) -> Json<Vec<WorkflowApiModel>> {
    let patterns: Vec<String> = if let Some(p) = params.pattern {
        vec![p]
    } else {
        vec!["**/*.jg".to_string(), "**/*.jgflow".to_string()]
    };

    let mut results = Vec::new();

    let all_paths = patterns.iter().flat_map(|pattern| {
        let full_pattern = state
            .project_root
            .join(pattern)
            .to_string_lossy()
            .to_string();
        glob::glob(&full_pattern).into_iter().flatten().flatten()
    });

    for entry in all_paths {
        if let Ok(content) = fs::read_to_string(&entry) {
            let file_name = entry
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            match GraphParser::parse(&content) {
                Ok(graph) => {
                    let validation = WorkflowValidator::validate(&graph);
                    let slug = if graph.slug.is_empty() {
                        file_name.clone()
                    } else {
                        graph.slug.clone()
                    };

                    // Collect issue messages
                    let mut issues: Vec<String> = validation
                        .errors
                        .iter()
                        .map(|e| format!("[{}] {}", e.code, e.message))
                        .collect();
                    issues.extend(
                        validation
                            .warnings
                            .iter()
                            .map(|w| format!("[{}] {}", w.code, w.message)),
                    );

                    results.push(WorkflowApiModel {
                        id: generate_deterministic_id(&slug),
                        slug,
                        name: if graph.name.is_empty() {
                            file_name
                        } else {
                            graph.name
                        },
                        description: graph.description,
                        node_count: graph.graph.node_count(),
                        is_valid: validation.is_valid,
                        errors: validation.error_count(),
                        warnings: validation.warning_count(),
                        issues,
                        created_at: Utc::now().to_rfc3339(),
                    });
                }
                Err(e) => {
                    // Include failed parses with error info
                    results.push(WorkflowApiModel {
                        id: generate_deterministic_id(&file_name),
                        slug: file_name.clone(),
                        name: file_name,
                        description: String::new(),
                        node_count: 0,
                        is_valid: false,
                        errors: 1,
                        warnings: 0,
                        issues: vec![format!(
                            "[PARSE] {}",
                            e.to_string().lines().next().unwrap_or("Parse error")
                        )],
                        created_at: Utc::now().to_rfc3339(),
                    });
                }
            }
        }
    }

    Json(results)
}

async fn handle_chat(
    headers: HeaderMap,
    Extension(state): Extension<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, Json<Value>> {
    // Extract X-Execution-Token if present (forwarded from jug0)
    let execution_token = headers
        .get("X-Execution-Token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if execution_token.is_some() {
        info!("🔐 [Web] Received X-Execution-Token from jug0, will use for subsequent jug0 calls");
    }

    let mut agent_registry = AgentRegistry::new();
    let agent_pattern = state
        .project_root
        .join("**/*.jgagent")
        .to_string_lossy()
        .to_string();
    agent_registry
        .load_from_paths(&[agent_pattern])
        .map_err(|e| Json(json!({"error": e.to_string()})))?;

    // Resolve @handle to agent if chat_id is a handle
    let handle_agent_slug = match &req.chat_id {
        Some(ChatIdInput::Handle(h)) => {
            let handle_name = h.strip_prefix('@').unwrap_or(h);
            // Find agent by username in the registry
            agent_registry
                .get_by_username(handle_name)
                .map(|a| a.slug.clone())
        }
        _ => None,
    };

    // 提取 agent slug（兼容 jug0 格式）
    // Priority: @handle > agent.slug > agent.id > default
    let agent_slug = if let Some(slug) = handle_agent_slug {
        slug
    } else if let Some(ref agent_config) = req.agent {
        agent_config
            .slug
            .clone()
            .or(agent_config.id.map(|u| u.to_string()))
            .unwrap_or_else(|| "default".to_string())
    } else {
        "default".to_string()
    };

    // 提取 user message（兼容 jug0 的 messages 数组格式）
    let message_text = if let Some(ref msgs) = req.messages {
        // 取最后一条 user/text 消息
        msgs.iter()
            .rfind(|m| m.role.as_deref() == Some("user") || m.part_type == "text")
            .and_then(|m| m.content.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

    // 提取 chat_id（用于继承会话）
    // Only extract UUID, @handle means new chat
    let chat_id_str = match &req.chat_id {
        Some(ChatIdInput::Uuid(id)) => Some(id.to_string()),
        Some(ChatIdInput::Handle(_)) => None, // @handle = new chat
        None => None,
    };

    // 提取自定义 tools
    let custom_tools = req
        .tools
        .clone()
        .or_else(|| req.agent.as_ref().and_then(|a| a.tools.clone()));

    // 提取 system_prompt 覆盖
    let system_prompt_override = req.agent.as_ref().and_then(|a| a.system_prompt.clone());

    let (agent_meta, agent_file_path) =
        agent_registry.get_with_path(&agent_slug).ok_or_else(|| {
            Json(json!({"error": format!("Agent '{}' not found in workspace", agent_slug)}))
        })?;

    let agent_meta = agent_meta.clone();
    let agent_dir = agent_file_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let config = JuglansConfig::load().map_err(|e| Json(json!({"error": e.to_string()})))?;

    // Create Jug0Client and inject execution token if present
    let jug0_client = Jug0Client::new(&config);
    if let Some(ref token) = execution_token {
        jug0_client.set_execution_token(Some(token.clone()));
        debug!("🔐 [Web] Injected execution token into Jug0Client");
    }
    let runtime: Arc<dyn JuglansRuntime> = Arc::new(jug0_client);

    let mut prompt_registry = PromptRegistry::new();
    let _ = prompt_registry.load_from_paths(&[state
        .project_root
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

    // 加载 tool definitions（从 project_root 下搜索 *.json tool files）
    {
        use crate::core::tool_loader::ToolLoader;
        use crate::services::tool_registry::ToolRegistry;
        let tool_pattern = state
            .project_root
            .join("**/*.json")
            .to_string_lossy()
            .to_string();
        if let Ok(tools) = ToolLoader::load_from_glob(&tool_pattern, &state.project_root) {
            if !tools.is_empty() {
                let mut registry = ToolRegistry::new();
                registry.register_all(tools);
                executor.set_tool_registry(Arc::new(registry));
            }
        }
    }
    executor.load_mcp_tools(&config).await;

    let executor = Arc::new(executor);
    // 注入 executor 引用到 BuiltinRegistry，让 chat() 能解析 tool slug
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    let (tx, rx) = mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // 设置 stream_tool_events 开关
    if req.stream_tool_events.unwrap_or(false) {
        ctx.set_stream_tool_events(true);
    }

    // 设置输入上下文
    ctx.set("input.message".to_string(), json!(message_text.clone()))
        .ok();

    // 尝试解析消息内容为 JSON，如果成功则展开到 $input.*
    // 这样 workflow 可以直接用 $input.event_type 等字段进行路由
    if let Ok(parsed) = serde_json::from_str::<Value>(&message_text) {
        if let Some(obj) = parsed.as_object() {
            for (k, v) in obj {
                ctx.set(format!("input.{}", k), v.clone()).ok();
            }
            debug!(
                "📦 [Web] Parsed message JSON into $input.* fields: {:?}",
                obj.keys().collect::<Vec<_>>()
            );
        }
    }

    // 如果有 chat_id，存入上下文供后续继承
    if let Some(ref cid) = chat_id_str {
        ctx.set("reply.chat_id".to_string(), json!(cid)).ok();
    }

    // 如果有 user_message_id，存入上下文供 reply()/chat() 回溯更新用户消息状态
    if let Some(umid) = req.user_message_id {
        ctx.set("reply.user_message_id".to_string(), json!(umid))
            .ok();
    }

    // variables 字段会覆盖从 message 解析的值（优先级更高）
    if let Some(vars) = req.variables {
        if let Some(obj) = vars.as_object() {
            for (k, v) in obj {
                ctx.set(format!("input.{}", k), v.clone()).ok();
            }
        }
    }

    // 构建 chat 参数
    let tools_json = custom_tools.map(|t| serde_json::to_string(&t).unwrap_or_default());
    // 将前端 client tools 注入 context，供 workflow 内 chat() builtin 及 DSL 访问
    if let Some(ref tools_str) = tools_json {
        ctx.set("input.tools".to_string(), json!(tools_str)).ok();
    }
    let sys_prompt = system_prompt_override;
    let project_root = state.project_root.clone();

    tokio::spawn(async move {
        let result = if let Some(wf_ref) = &agent_meta.workflow {
            // 判断是文件路径还是 slug
            let is_file_path = wf_ref.ends_with(".jg")
                || wf_ref.ends_with(".jgflow")
                || wf_ref.starts_with("./")
                || wf_ref.starts_with("../")
                || Path::new(wf_ref).is_absolute();

            let wf_result: Result<(String, PathBuf), anyhow::Error> = if is_file_path {
                // 文件路径格式：按现有逻辑解析
                let full_wf_path = if Path::new(wf_ref).is_absolute() {
                    PathBuf::from(wf_ref)
                } else {
                    agent_dir.join(wf_ref)
                };
                debug!("📂 Resolving workflow file: {:?}", full_wf_path);
                fs::read_to_string(&full_wf_path)
                    .map(|content| (content, full_wf_path.clone()))
                    .map_err(|e| {
                        anyhow::anyhow!("Workflow File Error: {} (tried {:?})", e, full_wf_path)
                    })
            } else {
                // Slug 格式：在 project_root 下搜索 **/{slug}.jg，回退到 **/{slug}.jgflow
                debug!(
                    "🔍 Resolving workflow by slug: '{}' in {:?}",
                    wf_ref, project_root
                );
                let pattern_jg = project_root
                    .join(format!("**/{}.jg", wf_ref))
                    .to_string_lossy()
                    .to_string();
                let pattern_jgflow = project_root
                    .join(format!("**/{}.jgflow", wf_ref))
                    .to_string_lossy()
                    .to_string();
                let found = glob::glob(&pattern_jg)
                    .ok()
                    .and_then(|mut paths| paths.find_map(|p| p.ok()))
                    .or_else(|| {
                        glob::glob(&pattern_jgflow)
                            .ok()
                            .and_then(|mut paths| paths.find_map(|p| p.ok()))
                    });
                match found {
                    Some(path) => {
                        info!("📂 Found workflow '{}' at {:?}", wf_ref, path);
                        fs::read_to_string(&path)
                            .map(|content| (content, path.clone()))
                            .map_err(|e| {
                                anyhow::anyhow!("Workflow File Error: {} (tried {:?})", e, path)
                            })
                    }
                    None => Err(anyhow::anyhow!(
                        "Workflow '{}' not found in workspace {:?}",
                        wf_ref,
                        project_root
                    )),
                }
            };

            match wf_result {
                Ok((content, wf_path)) => match GraphParser::parse(&content) {
                    Ok(mut graph) => {
                        // 解析 lib imports + flow imports
                        let wf_base_dir = wf_path.parent().unwrap_or(Path::new("."));
                        let wf_canonical = wf_path.canonicalize().unwrap_or(wf_path.clone());
                        let at_base: Option<PathBuf> = JuglansConfig::load()
                            .ok()
                            .and_then(|c| c.paths.base.map(|b| project_root.join(b)));
                        let mut import_stack = vec![wf_canonical.clone()];
                        match resolver::resolve_lib_imports(
                            &mut graph,
                            wf_base_dir,
                            &mut import_stack,
                            at_base.as_deref(),
                        ) {
                            Ok(()) => {
                                import_stack = vec![wf_canonical];
                                match resolver::resolve_flow_imports(
                                    &mut graph,
                                    wf_base_dir,
                                    &mut import_stack,
                                    at_base.as_deref(),
                                ) {
                                    Ok(()) => executor.execute_graph(Arc::new(graph), &ctx).await,
                                    Err(e) => Err(anyhow::anyhow!("Flow Import Error: {}", e)),
                                }
                            }
                            Err(e) => Err(anyhow::anyhow!("Lib Import Error: {}", e)),
                        }
                    }
                    Err(e) => Err(anyhow::anyhow!("Workflow Parse Error: {}", e)),
                },
                Err(e) => Err(e),
            }
        } else {
            // 直接 chat 模式
            let mut params = std::collections::HashMap::new();
            params.insert("agent".to_string(), agent_meta.slug.clone());
            params.insert("message".to_string(), message_text.clone());

            // 传递 state 参数
            if let Some(ref state_val) = req.state {
                params.insert("state".to_string(), state_val.clone());
            }

            // 传递自定义 tools
            if let Some(tools_str) = tools_json {
                params.insert("tools".to_string(), tools_str);
            }

            // 传递 system_prompt 覆盖
            if let Some(sp) = sys_prompt {
                params.insert("system_prompt".to_string(), sp);
            }

            executor
                .execute_tool_internal("chat", &params, &ctx)
                .await
                .map(|_| ())
        };

        if let Err(e) = result {
            error!("❌ Execution Error: {}", e);
            let _ = tx.send(WorkflowEvent::Error(e.to_string()));
        }
    });

    // SSE 事件格式对齐 Jug0 (使用标准 SSE event 类型)
    let pending_calls = state.pending_tool_calls.clone();
    let stream = UnboundedReceiverStream::new(rx).map(move |event| {
        match event {
            // Token 流: 与 jug0 一致的 content 格式
            WorkflowEvent::Token(t) => {
                Ok(Event::default().data(json!({ "type": "content", "text": t }).to_string()))
            }
            // Status → event: meta (workflow 状态更新)
            WorkflowEvent::Status(s) => Ok(Event::default()
                .event("meta")
                .data(json!({ "type": "meta", "status": s }).to_string())),
            // Meta → event: meta (chat_id, user_message_id 等)
            WorkflowEvent::Meta(data) => Ok(Event::default().event("meta").data(data.to_string())),
            // Error → event: error
            WorkflowEvent::Error(e) => Ok(Event::default()
                .event("error")
                .data(json!({ "type": "error", "message": e }).to_string())),
            // Tool call → event: tool_call
            WorkflowEvent::ToolCall {
                call_id,
                tools,
                result_tx,
            } => {
                if let Ok(mut map) = pending_calls.lock() {
                    map.insert(call_id.clone(), result_tx);
                }
                Ok(Event::default().event("tool_call").data(
                    json!({
                        "type": "tool_call",
                        "call_id": call_id,
                        "tools": tools,
                    })
                    .to_string(),
                ))
            }
            // Tool event → event: tool_event（内部 tool 执行的 start/complete 事件）
            WorkflowEvent::ToolEvent(data) => {
                Ok(Event::default().event("tool_event").data(data.to_string()))
            }
        }
    });

    Ok(Sse::new(stream))
}

// --- Tool Result Bridge Endpoint ---

#[derive(Deserialize)]
struct ToolResultRequest {
    call_id: String,
    results: Vec<ToolResultPayload>,
}

async fn handle_tool_result(
    Extension(state): Extension<Arc<WebState>>,
    Json(payload): Json<ToolResultRequest>,
) -> Json<Value> {
    let sender = {
        let mut map = match state.pending_tool_calls.lock() {
            Ok(map) => map,
            Err(_) => {
                return Json(json!({ "error": "Internal lock error" }));
            }
        };
        map.remove(&payload.call_id)
    };

    match sender {
        Some(tx) => {
            info!(
                "🌉 [Tool Result] Received {} results for call_id: {}",
                payload.results.len(),
                payload.call_id
            );
            let _ = tx.send(payload.results);
            Json(json!({ "ok": true }))
        }
        None => {
            warn!(
                "🌉 [Tool Result] No pending call found for call_id: {} (may have timed out)",
                payload.call_id
            );
            Json(json!({ "error": "No pending tool call found for this call_id" }))
        }
    }
}
