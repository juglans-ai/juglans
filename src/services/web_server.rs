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

use crate::adapters::feishu::FeishuWebhookHandler;
use crate::adapters::telegram::TelegramWebhookHandler;

// --- File Watcher (hot-reload feedback) ---

fn watch_workspace(project_root: PathBuf) {
    use notify::{Config, EventKind, RecursiveMode, Watcher};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::RecommendedWatcher::new(tx, Config::default()) {
        Ok(w) => w,
        Err(e) => {
            warn!("File watcher failed to start: {}", e);
            return;
        }
    };

    if let Err(e) = watcher.watch(&project_root, RecursiveMode::Recursive) {
        warn!("File watcher failed to watch {:?}: {}", project_root, e);
        return;
    }

    info!("👀 Watching {:?} for file changes...", project_root);

    let extensions = ["jg", "jgflow", "jgprompt", "jgagent"];

    for event in rx.into_iter().flatten() {
        if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
            continue;
        }
        for path in &event.paths {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !extensions.contains(&ext) {
                continue;
            }
            let rel = path.strip_prefix(&project_root).unwrap_or(path);
            log_file_change(path, rel, ext);
        }
    }
}

fn log_file_change(path: &Path, rel: &Path, ext: &str) {
    use crate::core::agent_parser::AgentParser;

    match ext {
        "jgagent" => {
            if let Ok(content) = fs::read_to_string(path) {
                match AgentParser::parse(&content) {
                    Ok(agent) => info!(
                        "🔄 [Hot-reload] {} — agent: {}, model: {}, temp: {}",
                        rel.display(),
                        agent.slug,
                        agent.model,
                        agent
                            .temperature
                            .map(|t| t.to_string())
                            .unwrap_or("-".into())
                    ),
                    Err(e) => warn!("🔄 [Hot-reload] {} — parse error: {}", rel.display(), e),
                }
            }
        }
        "jgprompt" => {
            info!("🔄 [Hot-reload] {} — prompt updated", rel.display());
        }
        "jg" => {
            info!("🔄 [Hot-reload] {} — workflow updated", rel.display());
        }
        "jgflow" => {
            info!("🔄 [Hot-reload] {} — flow metadata updated", rel.display());
        }
        _ => {}
    }
}

// --- API Models (Jug0 compatible) ---

#[derive(Serialize)]
pub struct AgentApiModel {
    pub id: Uuid,
    pub slug: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    // Simplified: no system_prompt_id locally, fill with empty or generated value
    pub system_prompt_id: Option<Uuid>,
    pub default_model: String,
    pub temperature: Option<f64>,
    pub skills: Option<Vec<String>>,
    // Extra field: Juglans-specific
    pub source: Option<String>,
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

// Jug0-compatible chat request structure
#[derive(Deserialize, Clone)]
pub struct ChatRequest {
    // jug0 standard fields
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

    // Juglans extra fields
    pub variables: Option<Value>,
    /// Message state: context_visible | context_hidden | display_only | silent
    pub state: Option<String>,
    /// User message ID from jug0 (used in workflow mode to retroactively update user message state)
    pub user_message_id: Option<i32>,
    /// Whether to push internal tool execution events to the SSE stream (default false)
    pub stream_tool_events: Option<bool>,
    /// Whether to push workflow node execution events to the SSE stream (default false)
    pub stream_node_events: Option<bool>,
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

// Helper: generate deterministic UUID v5 from slug
fn generate_deterministic_id(slug: &str) -> Uuid {
    let namespace = Uuid::NAMESPACE_DNS;
    Uuid::new_v5(&namespace, slug.as_bytes())
}

// Format uptime duration
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

/// Scan all .jg/.jgflow files under project_root, find workflow containing serve() node
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

    // Parse workflow
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

    // Pre-flight validation
    let validation = WorkflowValidator::validate(&graph);
    if !validation.is_valid {
        let err_json = validation.to_error_json();
        error!("❌ [Serve] Validation failed: {}", err_json);
        let body = serde_json::to_vec(&err_json).unwrap_or_default();
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
    }

    // Build executor
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

    // Load tool definitions
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
    let executor = Arc::new(executor);
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    // Create context, inject request data into $input
    let (tx, _rx) = mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx);

    // Parse query string
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

    // Parse body -- detect multipart or regular body
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.starts_with("multipart/form-data") {
        // Multipart: text fields -> $input.fields, files -> write to temp -> $input.files
        let multipart_result: Result<Multipart, _> = Multipart::from_request(request, &()).await;
        match multipart_result {
            Ok(mut multipart) => {
                let mut fields: HashMap<String, Value> = HashMap::new();
                let mut files: HashMap<String, Value> = HashMap::new();

                while let Ok(Some(field)) = multipart.next_field().await {
                    let field_name = field.name().unwrap_or("unnamed").to_string();
                    let file_name = field.file_name().map(|f| f.to_string());

                    if let Some(filename) = file_name {
                        // File field -> write to temp directory
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
                        // Text field
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
        // Regular body (JSON / string)
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

    // Inject path_parts -- split by /, convenient for workflow to extract path params
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

    // Execute workflow
    if let Err(e) = executor.execute_graph(Arc::new(graph), &ctx).await {
        error!("❌ [Serve] Execution error: {}", e);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Execution error: {}", e),
        );
    }

    // Read response
    let status_code = ctx.get_jvalue("response.status").i64().unwrap_or(200) as u16;
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK);

    let resp_headers = ctx.resolve_path("response.headers").ok().flatten();
    let resp_file = ctx.get_str("response.file");

    let mut builder = Response::builder().status(status);
    let mut has_content_type = false;

    // Apply custom headers
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
        // Binary file response
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
        // JSON response (default)
        // Fallback: if no response.body but $error exists, return 500
        let resp_body_jv = ctx.get_jvalue("response.body");
        if resp_body_jv.is_null() {
            let error_jv = ctx.get_jvalue("error");
            if !error_jv.is_null() {
                let msg_jv = error_jv.get("message");
                let msg = msg_jv.str_or("Internal error");
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, msg);
            }
        }

        let response_body = if resp_body_jv.is_null() {
            ctx.get_jvalue("output").into_inner()
        } else {
            resp_body_jv.into_inner()
        };
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

/// Health check (serverless platform liveness probe)
async fn health_check() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

/// Feishu webhook event receiver (serverless deployment)
async fn handle_feishu_webhook(
    Extension(handler): Extension<Arc<FeishuWebhookHandler>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    Json(handler.handle_event(body).await)
}

/// Telegram webhook event receiver (serverless deployment)
async fn handle_telegram_webhook(
    Extension(handler): Extension<Arc<TelegramWebhookHandler>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    Json(handler.handle_update(body).await)
}

/// Dashboard page
async fn dashboard(Extension(state): Extension<Arc<WebState>>) -> Html<String> {
    let uptime = format_uptime(state.start_time.elapsed().as_secs());

    // Scan prompts
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

    // Scan agents
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
                .source
                .as_ref()
                .map(|w| format!(" [source: {}]", w))
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

    // Scan workflows (.jg and .jgflow)
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
    let cron_jobs_html = String::new();
    let cron_count = 0;

    let wf_paths = glob::glob(&wf_pattern_jg)
        .into_iter()
        .chain(glob::glob(&wf_pattern_jgflow))
        .flatten()
        .flatten();

    for entry in wf_paths {
        // Skip .jgflow manifests — they are not standalone workflows
        if entry.extension().and_then(|e| e.to_str()) == Some("jgflow") {
            continue;
        }
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

<h2>Cron Jobs ({} scheduled)</h2>
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
        prompt_registry.keys().len(),
        prompts_html,
        agent_registry.keys().len(),
        agents_html,
        workflow_summary,
        workflows_html,
        cron_count,
        if cron_jobs_html.is_empty() {
            "    (none)\n".to_string()
        } else {
            cron_jobs_html
        },
    );

    Html(html)
}

pub async fn start_web_server(
    host: String,
    port: u16,
    project_root: PathBuf,
) -> anyhow::Result<()> {
    let config = JuglansConfig::load().ok();

    // Scan for serve() workflow
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
        pending_tool_calls: Arc::new(Mutex::new(HashMap::new())),
        serve_workflow: serve_workflow.clone(),
    });

    let mut app = Router::new()
        .route("/", get(dashboard))
        .route("/api/agents", get(list_local_agents))
        .route("/api/prompts", get(list_local_prompts))
        .route("/api/workflows", get(list_local_workflows))
        .route("/api/chat", post(handle_chat))
        .route("/api/chat/tool-result", post(handle_tool_result))
        .route("/health", get(health_check));

    // Feishu Webhook (if feishu app_id + app_secret are configured)
    let feishu_enabled = if let Some(ref cfg) = config {
        if let Some(handler) = FeishuWebhookHandler::from_config(cfg, &project_root) {
            let handler = Arc::new(handler);
            app = app
                .route("/webhook/feishu", post(handle_feishu_webhook))
                .layer(Extension(handler));
            true
        } else {
            false
        }
    } else {
        false
    };

    // Telegram Webhook (if telegram token is configured)
    let telegram_enabled = if let Some(ref cfg) = config {
        if let Some(handler) = TelegramWebhookHandler::from_config(cfg, &project_root) {
            let handler = Arc::new(handler);
            app = app
                .route("/webhook/telegram", post(handle_telegram_webhook))
                .layer(Extension(handler));
            true
        } else {
            false
        }
    } else {
        false
    };

    // If serve() workflow is found, register catch-all fallback
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
    if feishu_enabled {
        info!("   - POST /webhook/feishu");
    }
    if telegram_enabled {
        info!("   - POST /webhook/telegram");
    }
    if let Some(ref sw) = serve_workflow {
        info!("🌐 HTTP Backend: {} (entry: [{}])", sw.slug, sw.entry_node);
        info!("   - All other routes -> serve() workflow");
    }
    info!("--------------------------------------------------");

    // File watcher for hot-reload feedback
    tokio::task::spawn_blocking({
        let project_root = project_root.clone();
        move || watch_workspace(project_root)
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("🚀 Server is ready and waiting for requests...");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn list_local_agents(
    Extension(state): Extension<Arc<WebState>>,
    Query(params): Query<AgentQuery>,
) -> Json<Vec<AgentApiModel>> {
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
                    // Convert to compatible model
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
                        source: agent.source.clone(),
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
                            is_system: false, // Cannot be inferred from file, default false
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

    // Extract agent slug (jug0-compatible format)
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

    // Extract user message (jug0-compatible messages array format)
    let message_text = if let Some(ref msgs) = req.messages {
        // Get the last user/text message
        msgs.iter()
            .rfind(|m| m.role.as_deref() == Some("user") || m.part_type == "text")
            .and_then(|m| m.content.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Extract chat_id (for session continuation)
    // Only extract UUID, @handle means new chat
    let chat_id_str = match &req.chat_id {
        Some(ChatIdInput::Uuid(id)) => Some(id.to_string()),
        Some(ChatIdInput::Handle(_)) => None, // @handle = new chat
        None => None,
    };

    // Extract custom tools
    let custom_tools = req
        .tools
        .clone()
        .or_else(|| req.agent.as_ref().and_then(|a| a.tools.clone()));

    // Extract system_prompt override
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

    // Load tool definitions (search for *.json tool files under project_root)
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
    let executor = Arc::new(executor);
    // Inject executor reference into BuiltinRegistry so chat() can resolve tool slugs
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    let (tx, rx) = mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // Set stream_tool_events flag
    if req.stream_tool_events.unwrap_or(false) {
        ctx.set_stream_tool_events(true);
    }

    // Set stream_node_events flag
    if req.stream_node_events.unwrap_or(false) {
        ctx.set_stream_node_events(true);
    }

    // Set input context
    ctx.set("input.message".to_string(), json!(message_text.clone()))
        .ok();

    // Try to parse message content as JSON; if successful, expand into $input.*
    // This allows workflow to route directly using fields like $input.event_type
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

    // If chat_id exists, store in context for session continuation
    if let Some(ref cid) = chat_id_str {
        ctx.set("reply.chat_id".to_string(), json!(cid)).ok();
    }

    // If user_message_id exists, store in context for reply()/chat() to retroactively update user message state
    if let Some(umid) = req.user_message_id {
        ctx.set("reply.user_message_id".to_string(), json!(umid))
            .ok();
    }

    // variables field overrides values parsed from message (higher priority)
    if let Some(vars) = req.variables {
        if let Some(obj) = vars.as_object() {
            for (k, v) in obj {
                ctx.set(format!("input.{}", k), v.clone()).ok();
            }
        }
    }

    // Build chat parameters
    let tools_json = custom_tools.map(|t| serde_json::to_string(&t).unwrap_or_default());
    // Inject frontend client tools into context for chat() builtin and DSL access within workflow
    if let Some(ref tools_str) = tools_json {
        ctx.set("input.tools".to_string(), json!(tools_str)).ok();
    }
    let sys_prompt = system_prompt_override;
    let project_root = state.project_root.clone();

    tokio::spawn(async move {
        let result = if let Some(wf_ref) = &agent_meta.source {
            // Determine if it's a file path or slug
            let is_file_path = wf_ref.ends_with(".jg")
                || wf_ref.ends_with(".jgflow")
                || wf_ref.starts_with("./")
                || wf_ref.starts_with("../")
                || Path::new(wf_ref).is_absolute();

            let wf_result: Result<(String, PathBuf), anyhow::Error> = if is_file_path {
                // File path format: parse using existing logic
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
                // Slug format: search **/{slug}.jg under project_root, fall back to **/{slug}.jgflow
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
                Ok((content, wf_path)) => {
                    // .jgflow manifest: follow source: field to load .jg file and merge metadata
                    // Track source file's directory for correct flow import resolution
                    let mut source_base_dir: Option<PathBuf> = None;
                    let parse_result = if wf_path.extension().and_then(|e| e.to_str())
                        == Some("jgflow")
                    {
                        match GraphParser::parse_manifest(&content) {
                            Ok(manifest) if !manifest.source.is_empty() => {
                                let source_path = wf_path
                                    .parent()
                                    .unwrap_or(Path::new("."))
                                    .join(&manifest.source);
                                match fs::read_to_string(&source_path) {
                                    Ok(source_content) => match GraphParser::parse(&source_content)
                                    {
                                        Ok(mut g) => {
                                            source_base_dir = Some(
                                                source_path
                                                    .parent()
                                                    .unwrap_or(Path::new("."))
                                                    .to_path_buf(),
                                            );
                                            manifest.apply_to(&mut g);
                                            Ok(g)
                                        }
                                        Err(e) => Err(anyhow::anyhow!(
                                            "Workflow Source Parse Error: {}",
                                            e
                                        )),
                                    },
                                    Err(e) => Err(anyhow::anyhow!(
                                        "Workflow Source Read Error: {} (tried {:?})",
                                        e,
                                        source_path
                                    )),
                                }
                            }
                            _ => GraphParser::parse(&content)
                                .map_err(|e| anyhow::anyhow!("Workflow Parse Error: {}", e)),
                        }
                    } else {
                        GraphParser::parse(&content)
                            .map_err(|e| anyhow::anyhow!("Workflow Parse Error: {}", e))
                    };

                    match parse_result {
                        Ok(mut graph) => {
                            // Resolve lib imports + flow imports
                            // .jgflow + source: flow imports are relative to the source .jg file directory
                            let wf_base_dir = source_base_dir
                                .as_deref()
                                .unwrap_or_else(|| wf_path.parent().unwrap_or(Path::new(".")));
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
                                        Ok(()) => {
                                            // Pre-flight validation
                                            let validation = WorkflowValidator::validate(&graph);
                                            if !validation.is_valid {
                                                let err_msgs: Vec<String> = validation
                                                    .errors
                                                    .iter()
                                                    .map(|e| format!("[{}] {}", e.code, e.message))
                                                    .collect();
                                                Err(anyhow::anyhow!(
                                                    "Validation failed: {}",
                                                    err_msgs.join("; ")
                                                ))
                                            } else {
                                                executor.execute_graph(Arc::new(graph), &ctx).await
                                            }
                                        }
                                        Err(e) => Err(anyhow::anyhow!("Flow Import Error: {}", e)),
                                    }
                                }
                                Err(e) => Err(anyhow::anyhow!("Lib Import Error: {}", e)),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            // Direct chat mode
            let mut params = std::collections::HashMap::new();
            params.insert("agent".to_string(), agent_meta.slug.clone());
            params.insert("message".to_string(), message_text.clone());

            // Pass state parameter
            if let Some(ref state_val) = req.state {
                params.insert("state".to_string(), state_val.clone());
            }

            // Pass custom tools
            if let Some(tools_str) = tools_json {
                params.insert("tools".to_string(), tools_str);
            }

            // Pass system_prompt override
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

    // SSE event format aligned with Jug0 (using standard SSE event types)
    let pending_calls = state.pending_tool_calls.clone();
    let stream = UnboundedReceiverStream::new(rx).map(move |event| {
        match event {
            // Token stream: content format consistent with jug0
            WorkflowEvent::Token(t) => {
                Ok(Event::default().data(json!({ "type": "content", "text": t }).to_string()))
            }
            // Status -> event: meta (workflow status update)
            WorkflowEvent::Status(s) => Ok(Event::default()
                .event("meta")
                .data(json!({ "type": "meta", "status": s }).to_string())),
            // Meta -> event: meta (chat_id, user_message_id, etc.)
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
            // Tool start → event: tool_event
            WorkflowEvent::ToolStart(evt) => {
                let mut data = serde_json::to_value(&evt).unwrap_or_default();
                data["type"] = serde_json::Value::String("tool_start".to_string());
                Ok(Event::default().event("tool_event").data(data.to_string()))
            }
            // Tool complete → event: tool_event
            WorkflowEvent::ToolComplete(evt) => {
                let mut data = serde_json::to_value(&evt).unwrap_or_default();
                data["type"] = serde_json::Value::String("tool_complete".to_string());
                Ok(Event::default().event("tool_event").data(data.to_string()))
            }
            // Node start → event: node_event
            WorkflowEvent::NodeStart(evt) => {
                let mut data = serde_json::to_value(&evt).unwrap_or_default();
                data["type"] = serde_json::Value::String("node_start".to_string());
                Ok(Event::default().event("node_event").data(data.to_string()))
            }
            // Node complete → event: node_event
            WorkflowEvent::NodeComplete(evt) => {
                let mut data = serde_json::to_value(&evt).unwrap_or_default();
                data["type"] = serde_json::Value::String("node_complete".to_string());
                Ok(Event::default().event("node_event").data(data.to_string()))
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
