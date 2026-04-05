// src/providers/llm/mcp_types.rs
//
// MCP session types for Claude Code provider.
// Shared between providers/llm/claude_code.rs and handlers/mcp_endpoint.rs.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// A pending tool call from MCP → to be yielded as SSE tool_call event
pub struct PendingToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// Session state shared between MCP endpoint and claude_code.rs
pub struct McpSession {
    pub tools: Vec<McpToolDef>,
    /// MCP endpoint sends tool calls here → claude_code.rs picks them up
    pub tool_call_tx: mpsc::UnboundedSender<PendingToolCall>,
    /// Per-call result channels: call_id → oneshot sender (MCP endpoint waits on receiver)
    pub result_senders: Arc<DashMap<String, oneshot::Sender<String>>>,
}

pub fn openai_tools_to_mcp(tools: &[Value]) -> Vec<McpToolDef> {
    tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function")?;
            Some(McpToolDef {
                name: func.get("name")?.as_str()?.to_string(),
                description: func
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: func
                    .get("parameters")
                    .cloned()
                    .unwrap_or(json!({"type": "object", "properties": {}})),
            })
        })
        .collect()
}
