// src/handlers/mcp_endpoint.rs
//
// MCP Streamable HTTP endpoint for Claude Code.
// Types are defined in providers::llm::mcp_types (shared with claude_code.rs).

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

// Re-export types from canonical location
pub use crate::providers::llm::mcp_types::{
    openai_tools_to_mcp, McpSession, McpToolDef, PendingToolCall,
};

// ---------------------------------------------------------------------------
// JSON-RPC
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i32, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

// ---------------------------------------------------------------------------
// POST /mcp/:session_id
// ---------------------------------------------------------------------------

pub async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    body: String,
) -> Response {
    let req: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, axum::Json(error(Value::Null, -32700, &format!("Parse error: {}", e)))).into_response(),
    };

    let id = req.id.clone().unwrap_or(Value::Null);

    if req.id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }

    tracing::info!("[MCP] {} session={}", req.method, session_id);

    let result = match req.method.as_str() {
        "initialize" => {
            let resp = success(id, json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "jug0", "version": "1.0.0"}
            }));
            return (StatusCode::OK, [("Content-Type", "application/json"), ("Mcp-Session-Id", &*session_id)], axum::Json(resp)).into_response();
        }

        "tools/list" => {
            let tools = state.tool_sessions.get(&session_id)
                .map(|s| s.tools.clone())
                .unwrap_or_default();
            success(id, json!({"tools": tools}))
        }

        "tools/call" => {
            let tool_name = req.params.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let arguments = req.params.get("arguments").cloned().unwrap_or(json!({}));
            let call_id = uuid::Uuid::new_v4().to_string();

            let session = state.tool_sessions.get(&session_id);
            if session.is_none() {
                return (StatusCode::OK, [("Content-Type", "application/json")],
                    axum::Json(error(id, -32001, "Session not found"))).into_response();
            }
            let session = session.unwrap();

            // Create oneshot for receiving tool result from frontend
            let (result_tx, result_rx) = oneshot::channel::<String>();
            session.result_senders.insert(call_id.clone(), result_tx);

            // Notify claude_code.rs → yield SSE tool_call event
            let _ = session.tool_call_tx.send(PendingToolCall {
                call_id: call_id.clone(),
                name: tool_name.clone(),
                arguments: serde_json::to_string(&arguments).unwrap_or_default(),
            });

            tracing::info!("[MCP] tools/call {} → waiting for frontend execution (call_id={})", tool_name, call_id);
            drop(session);

            // Wait for tool_result from frontend (1 minute timeout)
            match tokio::time::timeout(Duration::from_secs(60), result_rx).await {
                Ok(Ok(result_text)) => {
                    tracing::info!("[MCP] tools/call {} → got result ({} chars)", tool_name, result_text.len());
                    success(id, json!({"content": [{"type": "text", "text": result_text}]}))
                }
                Ok(Err(_)) => error(id, -32000, "Tool result channel closed"),
                Err(_) => {
                    if let Some(s) = state.tool_sessions.get(&session_id) {
                        s.result_senders.remove(&call_id);
                    }
                    error(id, -32000, "Tool execution timeout (60s)")
                }
            }
        }

        "notifications/initialized" | "notifications/cancelled" => {
            return StatusCode::ACCEPTED.into_response();
        }

        _ => error(id, -32601, &format!("Method '{}' not found", req.method)),
    };

    (StatusCode::OK, [("Content-Type", "application/json")], axum::Json(result)).into_response()
}

pub async fn mcp_get_handler() -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}
