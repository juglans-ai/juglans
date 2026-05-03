// src/builtins/ai.rs
use super::Tool;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tracing::{debug, error, info, warn};

use crate::core::context::WorkflowContext;
use crate::core::graph::WorkflowGraph;
use crate::core::prompt_parser::PromptParser;
use crate::services::local_runtime::{ChatOutput, ChatRequest, ChatToolHandler, LocalRuntime};
use crate::services::prompt_loader::PromptRegistry;

/// Rough token estimate (4 chars ≈ 1 token). Good enough for history
/// budget accounting; real tokenization is provider-specific.
fn estimate_tokens(s: &str) -> u32 {
    s.len().div_ceil(4) as u32
}

// ─── Native MCP support for `chat(mcp=…)` ────────────────────────────
//
// Minimal MCP (Model Context Protocol) client. When a chat() call
// includes `mcp={"server_name": "http://..."}` or the expanded form
// `{"server_name": {"url": "...", "token": "..."}}`, the chat builtin:
//
//   1. For each server, performs a JSON-RPC `initialize` + `tools/list`
//      handshake (`McpHttpServer::discover`), captures any returned
//      `mcp-session-id` header, and converts the MCP tool definitions
//      into OpenAI function-calling schemas. Tool names are prefixed
//      with the server name (e.g. `wallet.list_positions`).
//   2. Appends those schemas to the existing `tools=` array so the LLM
//      sees a single flat tools list.
//   3. Wraps the chat's base tool handler in `McpAwareToolHandler`,
//      which intercepts any LLM tool call whose name begins with a
//      registered `server.` prefix and dispatches it via `tools/call`
//      on that server. Non-MCP calls fall through to the base handler.
//
// This replaces the legacy `libs: ["std/mcps.jg"]` + `mcps.MCP(…)` +
// `on_tool=[mcps.handle]` DSL-side implementation. `std/mcps.jg` still
// works for backward compatibility.
#[derive(Debug)]
struct McpHttpServer {
    client: reqwest::Client,
    url: String,
    token: Option<String>,
    session_id: Option<String>,
}

impl McpHttpServer {
    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "application/json, text/event-stream".parse().unwrap(),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        if let Some(ref t) = self.token {
            if let Ok(v) = format!("Bearer {}", t).parse() {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        if let Some(ref sid) = self.session_id {
            if let Ok(v) = sid.parse() {
                headers.insert("mcp-session-id", v);
            }
        }
        headers
    }

    /// Perform `initialize` + `tools/list` against an MCP server and
    /// return (session handle, prefixed OpenAI schemas). Called once
    /// per chat() invocation.
    async fn discover(
        prefix: String,
        url: String,
        token: Option<String>,
    ) -> Result<(Self, Vec<Value>)> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let mut server = Self {
            client,
            url,
            token,
            session_id: None,
        };

        // Step 1: initialize handshake — server may return a session id
        // in the `mcp-session-id` response header (Streamable HTTP
        // profile) or in the JSON body. We check the header first.
        let init_body = json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "juglans", "version": env!("CARGO_PKG_VERSION")}
            },
            "id": "init"
        });
        let init_resp = server
            .client
            .post(&server.url)
            .headers(server.auth_headers())
            .json(&init_body)
            .send()
            .await
            .with_context(|| format!("MCP `{}` initialize failed", prefix))?;

        if let Some(sid) = init_resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            server.session_id = Some(sid.to_string());
        }
        // Drain the body even though we only care about the headers —
        // some servers stall if the body isn't consumed.
        let _ = init_resp.text().await.ok();

        // Step 2: tools/list
        let list_body = json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": "discover"
        });
        let list_resp = server
            .client
            .post(&server.url)
            .headers(server.auth_headers())
            .json(&list_body)
            .send()
            .await
            .with_context(|| format!("MCP `{}` tools/list failed", prefix))?;
        let list_value: Value = list_resp
            .json()
            .await
            .with_context(|| format!("MCP `{}` tools/list: bad JSON", prefix))?;

        let mcp_tools = list_value
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();

        let schemas: Vec<Value> = mcp_tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(|v| v.as_str())?;
                let description = t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let parameters = t
                    .get("inputSchema")
                    .or_else(|| t.get("input_schema"))
                    .cloned()
                    .unwrap_or(json!({"type": "object", "properties": {}}));
                Some(json!({
                    "type": "function",
                    "function": {
                        "name": format!("{}.{}", prefix, name),
                        "description": description,
                        "parameters": parameters
                    }
                }))
            })
            .collect();

        Ok((server, schemas))
    }

    /// Dispatch an LLM tool call to this MCP server. `tool_name` is
    /// already unprefixed (the `server.` segment has been stripped).
    async fn call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        let arguments: Value =
            serde_json::from_str(arguments_json).unwrap_or(Value::Object(Default::default()));
        let body = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": tool_name, "arguments": arguments},
            "id": "call"
        });
        let resp = self
            .client
            .post(&self.url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await
            .with_context(|| format!("MCP tools/call `{}` transport error", tool_name))?;
        let value: Value = resp
            .json()
            .await
            .with_context(|| format!("MCP tools/call `{}` bad JSON", tool_name))?;

        // Happy path: result.content[0].text
        if let Some(text) = value
            .pointer("/result/content/0/text")
            .and_then(|v| v.as_str())
        {
            return Ok(text.to_string());
        }
        // Error path: error.message
        if let Some(err) = value.pointer("/error/message").and_then(|v| v.as_str()) {
            return Err(anyhow!("MCP tool `{}` error: {}", tool_name, err));
        }
        // Fallback: whole result as JSON string
        Ok(value
            .get("result")
            .map(|r| r.to_string())
            .unwrap_or_else(|| value.to_string()))
    }
}

/// Wraps an inner `ChatToolHandler` with MCP-aware dispatch. Any tool
/// call whose name contains a `.` and whose prefix matches a registered
/// MCP server is routed to that server via `tools/call`. Everything
/// else falls through to the inner handler (declarative map, on_tool
/// node, default workflow handler, etc).
struct McpAwareToolHandler {
    dispatcher: Arc<HashMap<String, Arc<McpHttpServer>>>,
    inner: Arc<dyn ChatToolHandler>,
}

#[async_trait]
impl ChatToolHandler for McpAwareToolHandler {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        if let Some(dot_idx) = tool_name.find('.') {
            let prefix = &tool_name[..dot_idx];
            let inner_name = &tool_name[dot_idx + 1..];
            if let Some(server) = self.dispatcher.get(prefix) {
                return server.call(inner_name, arguments_json).await;
            }
        }
        self.inner.handle_tool_call(tool_name, arguments_json).await
    }

    fn take_pending_tools(&self) -> Option<Vec<Value>> {
        self.inner.take_pending_tools()
    }
}

/// Parse the user-facing `mcp=` parameter into `(prefix, url, token)`
/// triples. Accepts three shapes:
///
///   - `{"wallet": "http://..."}`                          (shorthand)
///   - `{"wallet": {"url": "http://..."}}`
///   - `{"wallet": {"url": "http://...", "token": "..."}}`
///
/// Also tolerates the whole value being wrapped in `{"output": {...}}`
/// (when the map comes through as another node's output).
fn parse_mcp_param(raw: &str) -> Result<Vec<(String, String, Option<String>)>> {
    let parsed: Value = serde_json::from_str(raw)
        .with_context(|| format!("chat(mcp=…): expected JSON object, got: {}", raw))?;
    let obj = parsed
        .get("output")
        .and_then(|v| v.as_object())
        .cloned()
        .or_else(|| parsed.as_object().cloned())
        .ok_or_else(|| anyhow!("chat(mcp=…): must be a JSON object"))?;

    let mut servers = Vec::new();
    for (prefix, value) in obj {
        if prefix.contains('.') {
            return Err(anyhow!(
                "chat(mcp=…): server name `{}` cannot contain a dot",
                prefix
            ));
        }
        let (url, token) = match value {
            Value::String(url) => (url, None),
            Value::Object(ref m) => {
                let url = m
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("chat(mcp=…) `{}`: missing `url`", prefix))?
                    .to_string();
                let token = m
                    .get("token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                (url, token)
            }
            _ => {
                return Err(anyhow!(
                    "chat(mcp=…) `{}`: must be a URL string or an object with `url`/`token`",
                    prefix
                ))
            }
        };
        servers.push((prefix, url, token));
    }
    Ok(servers)
}

lazy_static! {
    static ref TEMPLATE_VAR_RE: Regex = Regex::new(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}").unwrap();
}

pub struct Chat {
    _prompt_registry: Arc<PromptRegistry>,
    runtime: Arc<LocalRuntime>,
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
}

impl Chat {
    pub fn new(prompt_registry: Arc<PromptRegistry>, runtime: Arc<LocalRuntime>) -> Self {
        Self {
            _prompt_registry: prompt_registry,
            runtime,
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }

    fn clean_json_output_verbose(&self, raw_content: &str) -> String {
        let trimmed_content = raw_content.trim();
        if trimmed_content.starts_with("```json") {
            if let Some(end_index) = trimmed_content.rfind("```") {
                if end_index > 7 {
                    return trimmed_content[7..end_index].trim().to_string();
                }
            }
        }
        if trimmed_content.starts_with("```") {
            if let Some(end_index) = trimmed_content.rfind("```") {
                if end_index > 3 {
                    return trimmed_content[3..end_index].trim().to_string();
                }
            }
        }

        // Extract last JSON block from mixed prose+JSON text (e.g. DeepSeek output)
        if let Some(json_block) = Self::extract_last_json_block(trimmed_content) {
            return json_block;
        }

        trimmed_content.to_string()
    }

    /// Extract the last valid JSON object/array from mixed text.
    ///
    /// Scans forward tracking `{}`/`[]` depth with string-awareness
    /// (handles escaped quotes and braces inside strings), collects all
    /// top-level blocks, then validates from last to first.
    fn extract_last_json_block(text: &str) -> Option<String> {
        let bytes = text.as_bytes();
        let len = bytes.len();
        if len == 0 {
            return None;
        }

        let mut blocks: Vec<(usize, usize)> = Vec::new();
        let mut in_string = false;
        let mut depth: i32 = 0;
        let mut block_start: usize = 0;
        let mut i = 0;

        while i < len {
            let ch = bytes[i];
            if in_string {
                if ch == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if ch == b'"' {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            match ch {
                b'"' => {
                    in_string = true;
                }
                b'{' | b'[' => {
                    if depth == 0 {
                        block_start = i;
                    }
                    depth += 1;
                }
                b'}' | b']' => {
                    if depth > 0 {
                        depth -= 1;
                        if depth == 0 {
                            blocks.push((block_start, i));
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }

        for &(start, end) in blocks.iter().rev() {
            let candidate = &text[start..=end];
            if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                return Some(candidate.to_string());
            }
        }
        None
    }

    /// Try executing a tool in BuiltinRegistry, returns None if not found
    async fn _try_execute_builtin(
        &self,
        tool_name: &str,
        args_str: &str,
        ctx: &WorkflowContext,
    ) -> Option<String> {
        let weak_registry = self.builtin_registry.as_ref()?;
        let registry_strong = weak_registry.upgrade()?;
        let tool_instance = registry_strong.get(tool_name)?;

        let args_map: HashMap<String, String> = serde_json::from_str(args_str).unwrap_or_default();

        info!("  🔧 [Builtin Tool] Executing: {} ...", tool_name);

        let result = match tool_instance.execute(&args_map, ctx).await {
            Ok(Some(output_val)) => {
                let s = match output_val {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                info!(
                    "  ✅ [Builtin Tool] Result: {:.80}...",
                    s.replace("\n", " ")
                );
                s
            }
            Ok(None) => {
                info!("  ✅ [Builtin Tool] Finished (No Output)");
                "Tool executed successfully.".to_string()
            }
            Err(e) => {
                error!("  ❌ [Builtin Tool] Error: {}", e);
                format!("Error during tool execution: {}", e)
            }
        };
        Some(result)
    }

    /// Legacy compatibility: try builtin tools in order, return error message on failure
    async fn _execute_local_tool(
        &self,
        tool_name: &str,
        args_str: &str,
        ctx: &WorkflowContext,
    ) -> String {
        if let Some(result) = self._try_execute_builtin(tool_name, args_str, ctx).await {
            return result;
        }
        format!("Error: Tool '{}' is not registered.", tool_name)
    }
}

/// Tool execution callback — encapsulates builtin / client bridge dispatch logic,
/// used by runtime.chat() to handle tool_call events within the SSE stream.
struct WorkflowToolHandler {
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
    context: WorkflowContext,
    stream_tool_events: bool,
    pending_tools: Arc<Mutex<Option<Vec<Value>>>>,
}

#[async_trait]
impl ChatToolHandler for WorkflowToolHandler {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // Emit tool_start event
        if self.stream_tool_events {
            self.context
                .emit_tool_start("chat", tool_name, arguments_json);
        }

        let result = self.dispatch_tool_call(tool_name, arguments_json).await;

        // Emit tool_complete event
        if self.stream_tool_events {
            match &result {
                Ok(s) => self
                    .context
                    .emit_tool_complete("chat", tool_name, &Ok(Some(json!(s)))),
                Err(e) => {
                    self.context
                        .emit_tool_complete("chat", tool_name, &Err(anyhow!("{}", e)))
                }
            }
        }

        result
    }

    fn take_pending_tools(&self) -> Option<Vec<Value>> {
        self.pending_tools.lock().take()
    }
}

impl WorkflowToolHandler {
    async fn dispatch_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // 1. Try builtin tool
        if let Some(weak) = &self.builtin_registry {
            if let Some(registry) = weak.upgrade() {
                if let Some(tool) = registry.get(tool_name) {
                    let args_map: HashMap<String, String> =
                        serde_json::from_str(arguments_json).unwrap_or_default();
                    info!("  🔧 [Builtin Tool] Executing: {} ...", tool_name);
                    match tool.execute(&args_map, &self.context).await {
                        Ok(Some(val)) => {
                            let s = match val {
                                Value::String(s) => s,
                                other => other.to_string(),
                            };
                            info!(
                                "  ✅ [Builtin Tool] Result: {:.80}...",
                                s.replace('\n', " ")
                            );
                            return Ok(s);
                        }
                        Ok(None) => {
                            info!("  ✅ [Builtin Tool] Finished (No Output)");
                            return Ok("Tool executed successfully.".to_string());
                        }
                        Err(e) => {
                            error!("  ❌ [Builtin Tool] Error: {}", e);
                            return Ok(format!("Error during tool execution: {}", e));
                        }
                    }
                }
            }
        }

        // 2. Client bridge — forward to frontend via SSE
        info!(
            "  🌉 [Client Tool Bridge] Forwarding: {} to frontend",
            tool_name
        );
        let call = json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": tool_name,
            "arguments": arguments_json
        });
        let (results, tools) = self
            .context
            .emit_tool_call_and_wait(uuid::Uuid::new_v4().to_string(), vec![call], 120)
            .await?;
        // Store dynamic tools from frontend for the next chat() invocation
        if let Some(ref t) = tools {
            if !t.is_empty() {
                info!(
                    "  🔧 [Client Tool Bridge] Received {} dynamic tool definitions",
                    t.len()
                );
                *self.pending_tools.lock() = tools;
            }
        }
        Ok(results
            .first()
            .map(|r| r.content.clone())
            .unwrap_or_default())
    }
}

/// Tool call handler that routes unresolved tool calls through a workflow
/// instead of the client bridge.
struct OnToolCallHandler {
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
    context: WorkflowContext,
    workflow_path: String,
    base_dir: std::path::PathBuf,
    stream_tool_events: bool,
}

#[async_trait]
impl ChatToolHandler for OnToolCallHandler {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // Emit tool_start event
        if self.stream_tool_events {
            self.context
                .emit_tool_start("chat", tool_name, arguments_json);
        }

        let result = self.dispatch_tool_call(tool_name, arguments_json).await;

        // Emit tool_complete event
        if self.stream_tool_events {
            match &result {
                Ok(s) => self
                    .context
                    .emit_tool_complete("chat", tool_name, &Ok(Some(json!(s)))),
                Err(e) => {
                    self.context
                        .emit_tool_complete("chat", tool_name, &Err(anyhow!("{}", e)))
                }
            }
        }

        result
    }
}

impl OnToolCallHandler {
    async fn dispatch_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // 1. Try builtin tool
        if let Some(weak) = &self.builtin_registry {
            if let Some(registry) = weak.upgrade() {
                if let Some(tool) = registry.get(tool_name) {
                    let args_map: HashMap<String, String> =
                        serde_json::from_str(arguments_json).unwrap_or_default();
                    info!("  🔧 [Builtin Tool] Executing: {} ...", tool_name);
                    match tool.execute(&args_map, &self.context).await {
                        Ok(Some(val)) => {
                            let s = match val {
                                Value::String(s) => s,
                                other => other.to_string(),
                            };
                            return Ok(s);
                        }
                        Ok(None) => return Ok("Tool executed successfully.".to_string()),
                        Err(e) => return Ok(format!("Error during tool execution: {}", e)),
                    }
                }
            }
        }

        // 2. Route to workflow (replaces client bridge)
        info!(
            "  🌉 [On Tool Call] Routing {} to workflow: {}",
            tool_name, self.workflow_path
        );

        let args_value: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
        self.context.set(
            "input.tool_call".to_string(),
            json!({
                "name": tool_name,
                "arguments": args_value
            }),
        )?;

        if let Some(weak) = &self.builtin_registry {
            if let Some(registry) = weak.upgrade() {
                let identifier = format!("on_tool_call:{}", tool_name);
                let output = registry
                    .execute_nested_workflow(
                        &self.workflow_path,
                        &self.base_dir,
                        &self.context,
                        identifier,
                    )
                    .await?;

                return Ok(match output {
                    Value::String(s) => s,
                    other => other.to_string(),
                });
            }
        }

        Err(anyhow!("Unable to handle tool call: {}", tool_name))
    }
}

/// Tool call handler that routes unresolved tool calls to a node in the same workflow.
/// Used with on_tool=[node_name] parameter in chat().
struct OnToolNodeHandler {
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
    context: WorkflowContext,
    node_name: String,
    workflow: Arc<WorkflowGraph>,
    stream_tool_events: bool,
    pending_tools: Arc<Mutex<Option<Vec<Value>>>>,
}

#[async_trait]
impl ChatToolHandler for OnToolNodeHandler {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // Emit tool_start event
        if self.stream_tool_events {
            self.context
                .emit_tool_start("chat", tool_name, arguments_json);
        }

        let result = self.dispatch_tool_call(tool_name, arguments_json).await;

        // Emit tool_complete event
        if self.stream_tool_events {
            match &result {
                Ok(s) => self
                    .context
                    .emit_tool_complete("chat", tool_name, &Ok(Some(json!(s)))),
                Err(e) => {
                    self.context
                        .emit_tool_complete("chat", tool_name, &Err(anyhow!("{}", e)))
                }
            }
        }

        result
    }

    fn take_pending_tools(&self) -> Option<Vec<Value>> {
        self.pending_tools.lock().take()
    }
}

impl OnToolNodeHandler {
    async fn dispatch_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // 1. Try builtin tool
        if let Some(weak) = &self.builtin_registry {
            if let Some(registry) = weak.upgrade() {
                if let Some(tool) = registry.get(tool_name) {
                    let args_map: HashMap<String, String> =
                        serde_json::from_str(arguments_json).unwrap_or_default();
                    info!("  🔧 [Builtin Tool] Executing: {} ...", tool_name);
                    match tool.execute(&args_map, &self.context).await {
                        Ok(Some(val)) => {
                            let s = match val {
                                Value::String(s) => s,
                                other => other.to_string(),
                            };
                            return Ok(s);
                        }
                        Ok(None) => return Ok("Tool executed successfully.".to_string()),
                        Err(e) => return Ok(format!("Error during tool execution: {}", e)),
                    }
                }
            }
        }

        // 2. Route to node (on_tool=[node])
        info!(
            "  🌉 [On Tool Node] Routing {} to node [{}]",
            tool_name, self.node_name
        );

        let args_value: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
        self.context.set(
            "input.tool_call".to_string(),
            json!({
                "name": tool_name,
                "arguments": args_value
            }),
        )?;

        // Get executor
        let executor = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .and_then(|r| r.get_executor())
            .ok_or_else(|| anyhow!("Executor not available for on_tool handler"))?;

        // Check if target is a function node
        let result = if self.workflow.functions.contains_key(&self.node_name) {
            let mut args = HashMap::new();
            args.insert("name".to_string(), json!(tool_name));
            args.insert("arguments".to_string(), args_value);
            executor
                .execute_function(
                    self.node_name.clone(),
                    args,
                    self.workflow.clone(),
                    &self.context,
                )
                .await?
        } else {
            // Target is a regular node
            executor
                .clone()
                .run_single_node_by_name(&self.node_name, &self.workflow, &self.context)
                .await?
        };

        // Check if the handler wants to bridge to frontend
        let bridge = match &result {
            Some(Value::Object(map)) => map
                .get("__bridge__")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            _ => false,
        };
        if bridge {
            info!(
                "  🌉 [On Tool Node] {} returned __bridge__, forwarding to frontend",
                tool_name
            );
            let call = json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "name": tool_name,
                "arguments": arguments_json
            });
            let (results, tools) = self
                .context
                .emit_tool_call_and_wait(uuid::Uuid::new_v4().to_string(), vec![call], 120)
                .await?;
            if let Some(ref t) = tools {
                if !t.is_empty() {
                    info!(
                        "  🔧 [On Tool Node] Received {} dynamic tool definitions",
                        t.len()
                    );
                    *self.pending_tools.lock() = tools;
                }
            }
            return Ok(results
                .first()
                .map(|r| r.content.clone())
                .unwrap_or_default());
        }

        Ok(match result {
            Some(Value::String(s)) => s,
            Some(v) => v.to_string(),
            None => "OK".to_string(),
        })
    }
}

/// Tool call handler that dispatches by tool name to different nodes.
/// Used when tools parameter uses declarative map format with per-tool handler bindings.
struct MapToolHandler {
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
    context: WorkflowContext,
    handler_map: HashMap<String, String>, // tool_name → node_name
    workflow: Arc<WorkflowGraph>,
    stream_tool_events: bool,
}

#[async_trait]
impl ChatToolHandler for MapToolHandler {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        if self.stream_tool_events {
            self.context
                .emit_tool_start("chat", tool_name, arguments_json);
        }

        let result = self.dispatch_tool_call(tool_name, arguments_json).await;

        if self.stream_tool_events {
            match &result {
                Ok(s) => self
                    .context
                    .emit_tool_complete("chat", tool_name, &Ok(Some(json!(s)))),
                Err(e) => {
                    self.context
                        .emit_tool_complete("chat", tool_name, &Err(anyhow!("{}", e)))
                }
            }
        }

        result
    }
}

impl MapToolHandler {
    async fn dispatch_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String> {
        // 1. Try builtin tool first
        if let Some(weak) = &self.builtin_registry {
            if let Some(registry) = weak.upgrade() {
                if let Some(tool) = registry.get(tool_name) {
                    let args_map: HashMap<String, String> =
                        serde_json::from_str(arguments_json).unwrap_or_default();
                    info!("  🔧 [Builtin Tool] Executing: {} ...", tool_name);
                    match tool.execute(&args_map, &self.context).await {
                        Ok(Some(val)) => {
                            let s = match val {
                                Value::String(s) => s,
                                other => other.to_string(),
                            };
                            return Ok(s);
                        }
                        Ok(None) => return Ok("Tool executed successfully.".to_string()),
                        Err(e) => return Ok(format!("Error during tool execution: {}", e)),
                    }
                }
            }
        }

        // 2. Lookup handler node by tool name
        let node_name = self.handler_map.get(tool_name).ok_or_else(|| {
            anyhow!(
                "No handler defined for tool '{}'. Available: {:?}",
                tool_name,
                self.handler_map.keys().collect::<Vec<_>>()
            )
        })?;

        info!(
            "  🎯 [Map Tool Handler] Routing '{}' to node [{}]",
            tool_name, node_name
        );

        let args_value: Value = serde_json::from_str(arguments_json).unwrap_or(json!({}));
        self.context.set(
            "input.tool_call".to_string(),
            json!({
                "name": tool_name,
                "arguments": args_value
            }),
        )?;

        let executor = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .and_then(|r| r.get_executor())
            .ok_or_else(|| anyhow!("Executor not available for map tool handler"))?;

        let result = if self.workflow.functions.contains_key(node_name) {
            let mut args = HashMap::new();
            args.insert("name".to_string(), json!(tool_name));
            args.insert("arguments".to_string(), args_value);
            executor
                .execute_function(
                    node_name.clone(),
                    args,
                    self.workflow.clone(),
                    &self.context,
                )
                .await?
        } else {
            executor
                .clone()
                .run_single_node_by_name(node_name, &self.workflow, &self.context)
                .await?
        };

        Ok(match result {
            Some(Value::String(s)) => s,
            Some(v) => v.to_string(),
            None => "OK".to_string(),
        })
    }
}

// ==================== ExecuteWorkflow Tool ====================

pub struct ExecuteWorkflow {
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
}

impl Default for ExecuteWorkflow {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecuteWorkflow {
    pub fn new() -> Self {
        Self {
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }
}

#[async_trait]
impl Tool for ExecuteWorkflow {
    fn name(&self) -> &str {
        "execute_workflow"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let path = params
            .get("path")
            .ok_or_else(|| anyhow!("execute_workflow: Missing 'path' parameter"))?;

        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("execute_workflow: BuiltinRegistry not available"))?;

        let base_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let identifier = format!("execute_workflow:{}", path);

        // Optional: pass input to override child workflow's input
        if let Some(input_json) = params.get("input") {
            if let Ok(input_val) = serde_json::from_str::<Value>(input_json) {
                context.set("input".to_string(), input_val)?;
            }
        }

        info!("│   ⚡ execute_workflow: {}", path);
        let output = registry
            .execute_nested_workflow(path, &base_dir, context, identifier)
            .await?;
        Ok(Some(output))
    }
}

#[async_trait]
impl Tool for Chat {
    fn name(&self) -> &str {
        "chat"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let agent_param = params.get("agent").map(|s| s.as_str()).unwrap_or("default");
        let user_message_body = params
            .get("message")
            .ok_or_else(|| anyhow!("Chat Tool Error: Mandatory parameter 'message' is missing."))?;

        // Message state: supports compound syntax input:output
        // Single value: state="silent" -> input=silent, output=silent
        // Compound: state="context_hidden:context_visible" -> input=hidden, output=visible
        let state_raw = params
            .get("state")
            .cloned()
            .unwrap_or_else(|| "context_visible".to_string());
        let (input_state, output_state) = match state_raw.split_once(':') {
            Some((i, o)) => (i.to_string(), o.to_string()),
            None => (state_raw.clone(), state_raw.clone()),
        };
        // Visibility: only user-facing states stream tokens out. AI-internal
        // and silent states never stream regardless of the explicit `stream`
        // flag — there's nothing for a stream to be visible on.
        let visible = output_state == "context_visible" || output_state == "display_only";
        // Explicit stream override (default: stream when visible). Lets a
        // workflow ask for "user-visible but batched" by passing stream=false.
        let stream_param = params
            .get("stream")
            .map(|s| s.as_str())
            .map(|s| s != "false" && s != "0" && s != "no");
        let should_stream = visible && stream_param.unwrap_or(true);
        // should_persist: if either input or output needs persistence, inherit chat_id
        let should_persist = input_state == "context_visible"
            || input_state == "context_hidden"
            || output_state == "context_visible"
            || output_state == "context_hidden";
        let system_prompt_manual_override = params.get("system_prompt").cloned();
        let requested_format_mode = params
            .get("format")
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "text".to_string());

        // Parse inline agent map: agent param may be a JSON object (from Literal node)
        // When referenced via node output, the value is wrapped as {"output": {...}} — unwrap it.
        let inline_agent: Option<Value> = serde_json::from_str::<Value>(agent_param)
            .ok()
            .filter(|v| v.is_object())
            .map(|v| {
                if let Some(inner) = v.get("output").filter(|o| o.is_object()) {
                    inner.clone()
                } else {
                    v
                }
            });

        // Extract tools from inline agent config (serialized back to string for downstream parsing)
        let inline_agent_tools: Option<String> =
            inline_agent.as_ref().and_then(|a| a.get("tools")).map(|t| {
                if let Some(s) = t.as_str() {
                    s.to_string()
                } else {
                    t.to_string()
                }
            });

        // Tools resolution priority:
        // 1. Node parameter tools=... (explicitly specified)
        // 2. Inline agent's tools field
        let tools_json_str = params.get("tools").or(inline_agent_tools.as_ref());

        // Handler map for declarative tool definitions (tool_name → node_name)
        let mut tool_handler_map: Option<HashMap<String, String>> = None;

        let mut custom_tools_json_schema = if let Some(schema_raw) = tools_json_str {
            // Parse tools: supports inline JSON, single reference (@slug), multiple references ([slugs]),
            // or declarative map format {"tool_name": {"description": "...", "params": {...}, "handler": "node"}}
            let parsed: Vec<Value> = if let Some(slug) = schema_raw.strip_prefix('@') {
                // Single reference: @web-tools
                debug!("Resolving tool reference: {}", slug);

                // Get ToolRegistry from BuiltinRegistry
                if let Some(builtin_reg_weak) = &self.builtin_registry {
                    if let Some(builtin_reg) = builtin_reg_weak.upgrade() {
                        if let Some(executor) = builtin_reg.get_executor() {
                            let tool_registry = executor.get_tool_registry();
                            if let Some(tool_resource) = tool_registry.get(slug) {
                                tool_resource.tools.clone()
                            } else {
                                return Err(anyhow!("Tool resource '{}' not found", slug));
                            }
                        } else {
                            return Err(anyhow!("Executor not available for tool resolution"));
                        }
                    } else {
                        return Err(anyhow!("BuiltinRegistry not available"));
                    }
                } else {
                    return Err(anyhow!("BuiltinRegistry not set for Chat builtin"));
                }
            } else if let Ok(slugs) = serde_json::from_str::<Vec<String>>(schema_raw) {
                // Multiple references: ["devtools", "web-tools", "data-tools"]
                debug!("Resolving tool references: {:?}", slugs);

                if let Some(builtin_reg_weak) = &self.builtin_registry {
                    if let Some(builtin_reg) = builtin_reg_weak.upgrade() {
                        // Try resolving via ToolRegistry
                        let resolve_result = if let Some(executor) = builtin_reg.get_executor() {
                            let tool_registry = executor.get_tool_registry();
                            tool_registry.resolve_tools(&slugs).ok()
                        } else {
                            None
                        };

                        if let Some(tools) = resolve_result {
                            tools
                        } else {
                            // Fallback: resolve slugs one by one, supports "devtools" from builtin schemas
                            let mut all_tools = Vec::new();
                            let tool_registry_opt = builtin_reg
                                .get_executor()
                                .map(|e| e.get_tool_registry().clone());

                            for slug in &slugs {
                                // Try ToolRegistry first
                                if let Some(ref registry) = tool_registry_opt {
                                    if let Some(resource) = registry.get(slug) {
                                        all_tools.extend(resource.tools.clone());
                                        continue;
                                    }
                                }
                                // Fallback: "devtools" → builtin schemas
                                if slug == "devtools" {
                                    all_tools.extend(builtin_reg.list_schemas());
                                } else {
                                    return Err(anyhow!("Tool resource '{}' not found", slug));
                                }
                            }
                            all_tools
                        }
                    } else {
                        return Err(anyhow!("BuiltinRegistry not available"));
                    }
                } else {
                    return Err(anyhow!("BuiltinRegistry not set for Chat builtin"));
                }
            } else if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(schema_raw) {
                // Declarative map format: {"tool_name": {"description": "...", "params": {...}, "handler": "node"}}
                // Auto-generates OpenAI function calling schemas and extracts handler mappings
                let mut schemas = Vec::new();
                let mut handlers = HashMap::new();
                for (tool_name, def) in &map {
                    let description = def
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let params_raw = def.get("params").cloned().unwrap_or(json!({}));

                    // Expand shorthand params: {"city": "string"} → {"city": {"type": "string"}}
                    let properties = if let Value::Object(params_obj) = &params_raw {
                        let mut expanded = serde_json::Map::new();
                        for (param_name, param_val) in params_obj {
                            match param_val {
                                Value::String(type_str) => {
                                    expanded.insert(param_name.clone(), json!({"type": type_str}));
                                }
                                Value::Object(_) => {
                                    expanded.insert(param_name.clone(), param_val.clone());
                                }
                                _ => {
                                    expanded.insert(param_name.clone(), json!({"type": "string"}));
                                }
                            }
                        }
                        Value::Object(expanded)
                    } else {
                        json!({})
                    };

                    let required: Vec<&str> = if let Value::Object(params_obj) = &params_raw {
                        params_obj.keys().map(|k| k.as_str()).collect()
                    } else {
                        vec![]
                    };

                    schemas.push(json!({
                        "type": "function",
                        "function": {
                            "name": tool_name,
                            "description": description,
                            "parameters": {
                                "type": "object",
                                "properties": properties,
                                "required": required
                            }
                        }
                    }));

                    if let Some(handler) = def.get("handler").and_then(|v| v.as_str()) {
                        handlers.insert(tool_name.clone(), handler.to_string());
                    }
                }
                if !handlers.is_empty() {
                    tool_handler_map = Some(handlers);
                }
                schemas
            } else {
                // Inline JSON: [{...}, {...}]
                serde_json::from_str(schema_raw).with_context(|| {
                    format!(
                        "Failed to parse 'tools' parameter as JSON array. Input was: {}",
                        schema_raw
                    )
                })?
            };

            if !parsed.is_empty() {
                let tool_names: Vec<&str> = parsed
                    .iter()
                    .filter_map(|t| {
                        t.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                    })
                    .collect();
                info!(
                    "🛠️ Attaching {} custom tools to the request: {:?}",
                    parsed.len(),
                    tool_names
                );
                Some(parsed)
            } else {
                None
            }
        } else {
            None
        };

        info!("│   Message content: {}", user_message_body);

        let history_param = params.get("history").map(|s| s.as_str());

        // Resolve chat_id (4-tier): explicit param → reply.chat_id (in-run
        // chain) → input.chat_id (adapter-injected) → None. An empty or
        // "[Missing:...]" explicit value means "caller deliberately cleared
        // the id" and falls through to None; this lets a node explicitly
        // opt out of persistence without needing a separate flag.
        let active_chat_id: Option<String> = if let Some(explicit_id) = params.get("chat_id") {
            if explicit_id.starts_with("[Missing:") || explicit_id.trim().is_empty() {
                debug!("Explicit chat_id empty/missing, treating as stateless.");
                None
            } else {
                Some(explicit_id.clone())
            }
        } else {
            let from_reply = context
                .resolve_path("reply.chat_id")
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            let from_input = context
                .resolve_path("input.chat_id")
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()));
            from_reply.or(from_input)
        };

        // Auto-load history when:
        //   - history param not explicitly supplied by the caller
        //   - should_persist (state allows it)
        //   - a chat_id was resolved
        //   - a global store is configured
        let auto_loaded_history: Vec<Value> =
            if history_param.is_none() && should_persist && active_chat_id.is_some() {
                if let Some(store) = crate::services::history::global_store() {
                    let cid = active_chat_id.as_deref().unwrap();
                    let cfg = crate::services::history::global_config();
                    match store.load(cid, cfg.max_messages).await {
                        Ok(msgs) => {
                            debug!(
                                "│   Loaded {} prior messages for chat_id={}",
                                msgs.len(),
                                cid
                            );
                            msgs.into_iter()
                                .map(|m| {
                                    json!({
                                        "type": "text",
                                        "role": m.role,
                                        "content": m.content,
                                    })
                                })
                                .collect()
                        }
                        Err(e) => {
                            warn!("│   history.load failed for {}: {}", cid, e);
                            Vec::new()
                        }
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

        // Build message buffer. Explicit `history` param wins over auto-load.
        let mut chat_messages_buffer: Vec<Value> = if let Some(history_str) = history_param {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(history_str) {
                arr.into_iter()
                    .filter_map(|m| {
                        let role = m.get("role")?.as_str()?;
                        let content = m.get("content")?.as_str()?;
                        Some(json!({
                            "type": "text",
                            "role": role,
                            "content": content
                        }))
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            auto_loaded_history
        };

        // Append current user message
        chat_messages_buffer.push(json!({
            "type": "text",
            "role": "user",
            "content": user_message_body
        }));

        let final_agent_config = if let Some(ref agent_obj) = inline_agent {
            // Inline agent map: extract config from JSON object
            let model = agent_obj
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("gpt-4o");
            let system_prompt = agent_obj
                .get("system_prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let temperature = agent_obj.get("temperature").and_then(|v| v.as_f64());
            let slug = agent_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("inline");

            // Node-level overrides take precedence
            let final_system_prompt = system_prompt_manual_override
                .as_deref()
                .unwrap_or(system_prompt);
            let final_model = params.get("model").map(|s| s.as_str()).unwrap_or(model);
            let final_temp = params
                .get("temperature")
                .and_then(|t| t.parse::<f64>().ok())
                .or(temperature);

            info!("│   Inline agent: {} (model: {})", slug, final_model);

            json!({
                "slug": slug,
                "model": final_model,
                "system_prompt": final_system_prompt,
                "temperature": final_temp,
            })
        } else {
            // Plain string: treat as agent slug to be resolved by the runtime
            debug!("│   Using remote agent: {}", agent_param);
            let mut base_config = json!({ "slug": agent_param });
            if let Some(map) = base_config.as_object_mut() {
                if let Some(override_val) = system_prompt_manual_override {
                    map.insert("system_prompt".to_string(), json!(override_val));
                }
                if let Some(model) = params.get("model") {
                    map.insert("model".to_string(), json!(model));
                }
                if let Some(temp) = params.get("temperature") {
                    if let Ok(t) = temp.parse::<f64>() {
                        map.insert("temperature".to_string(), json!(t));
                    }
                }
            }
            base_config
        };

        // on_token=[handler] — extract handler name for per-token callback
        let on_token_handler = params.get("on_token").map(|s| {
            s.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });

        // Get Token and Meta adapters from context (SSE output depends on state)
        let effective_token_sender = if let Some(ref handler_name) = on_token_handler {
            // Custom adapter: each token → call handler function
            let registry = self
                .builtin_registry
                .as_ref()
                .and_then(|w| w.upgrade())
                .ok_or_else(|| anyhow!("on_token: BuiltinRegistry not available"))?;
            let executor = registry
                .get_executor()
                .ok_or_else(|| anyhow!("on_token: WorkflowExecutor not available"))?;
            let workflow = context
                .get_root_workflow()
                .or_else(|| context.get_current_workflow())
                .ok_or_else(|| anyhow!("on_token: no workflow found"))?;

            let ctx = context.clone();
            let handler = handler_name.clone();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

            info!("│   on_token: [{}]", handler_name);

            tokio::spawn(async move {
                while let Some(token) = rx.recv().await {
                    let mut args = HashMap::new();
                    args.insert("chunk".to_string(), json!(token));
                    if let Err(e) = executor
                        .clone()
                        .execute_function(handler.clone(), args, workflow.clone(), &ctx)
                        .await
                    {
                        error!("on_token handler [{}] error: {}", handler, e);
                    }
                }
            });

            Some(tx)
        } else if should_stream {
            context.get_token_sender_adapter()
        } else if context.has_event_sender() {
            // TUI/web mode: provide a dummy sender to prevent the runtime
            // from falling back to stdout printing (which corrupts the TUI).
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            Some(tx)
        } else {
            None
        };
        let meta_sender = context.get_meta_sender_adapter();
        let _effective_meta_sender = if should_stream { meta_sender } else { None };

        // Read tool_event parameter: "silent" (default), "info", "verbose"
        // Backward compat: stream_tool_events=true → verbose
        let tool_event_level: u8 = if let Some(te) = params.get("tool_event") {
            match te.trim().trim_matches('"') {
                "verbose" => 2,
                "info" => 1,
                _ => 0,
            }
        } else if params
            .get("stream_tool_events")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
        {
            2 // backward compat
        } else {
            0
        };
        let stream_tool_events = tool_event_level > 0;
        if tool_event_level > 0 {
            context.set_tool_event_level(tool_event_level);
        }

        // ── Native `chat(mcp=…)` discovery ──
        //
        // If the caller passed an `mcp` parameter, handshake with each
        // declared server (initialize + tools/list), convert the
        // discovered tools into prefixed OpenAI schemas, append them to
        // `custom_tools_json_schema`, and build a dispatcher the
        // handler wrapper will use below.
        let mcp_dispatcher: Option<Arc<HashMap<String, Arc<McpHttpServer>>>> =
            if let Some(mcp_raw) = params.get("mcp") {
                let servers = parse_mcp_param(mcp_raw)?;
                if servers.is_empty() {
                    None
                } else {
                    info!(
                        "│   mcp: discovering {} server(s): {:?}",
                        servers.len(),
                        servers.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
                    );
                    let mut dispatch = HashMap::new();
                    let mut discovered_schemas: Vec<Value> = Vec::new();
                    for (prefix, url, token) in servers {
                        let (server, schemas) =
                            McpHttpServer::discover(prefix.clone(), url, token).await?;
                        info!("│     mcp/{}: {} tool(s)", prefix, schemas.len());
                        discovered_schemas.extend(schemas);
                        dispatch.insert(prefix, Arc::new(server));
                    }
                    // Merge into the flat `custom_tools_json_schema` the
                    // runtime sees. If the user also passed explicit
                    // `tools=`, MCP tools are appended after those.
                    custom_tools_json_schema = Some(match custom_tools_json_schema.take() {
                        Some(mut existing) => {
                            existing.extend(discovered_schemas);
                            existing
                        }
                        None => discovered_schemas,
                    });
                    Some(Arc::new(dispatch))
                }
            } else {
                None
            };

        // Create tool execution callback — tool_call handled within SSE stream, no tool loop needed
        // Priority: map handler (from declarative tools) > on_tool=[node] > on_tool_call=path > default
        let base_handler: Arc<dyn ChatToolHandler> = if let Some(map) = tool_handler_map.take() {
            // Declarative map format: tools={"name": {..., "handler": "node"}}
            let workflow = context
                .get_current_workflow()
                .ok_or_else(|| anyhow!("Map tool handler: No current workflow in context"))?;
            info!(
                "│   tools (map): {} tool handlers bound: {:?}",
                map.len(),
                map.keys().collect::<Vec<_>>()
            );
            Arc::new(MapToolHandler {
                builtin_registry: self.builtin_registry.clone(),
                context: context.clone(),
                handler_map: map,
                workflow,
                stream_tool_events,
            })
        } else if let Some(on_tool_ref) = params.get("on_tool") {
            // on_tool=[node_name] — extract node name (strip brackets)
            let node_name = on_tool_ref
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            let workflow = context.get_current_workflow().ok_or_else(|| {
                anyhow!("on_tool=[{}]: No current workflow in context", node_name)
            })?;
            info!("│   on_tool: [{}] (unresolved tools → node)", node_name);
            Arc::new(OnToolNodeHandler {
                builtin_registry: self.builtin_registry.clone(),
                context: context.clone(),
                node_name,
                workflow,
                stream_tool_events,
                pending_tools: Arc::new(Mutex::new(None)),
            })
        } else if let Some(on_tool_call_path) = params.get("on_tool_call") {
            let base_dir =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            info!(
                "│   on_tool_call: {} (unresolved tools → workflow)",
                on_tool_call_path
            );
            Arc::new(OnToolCallHandler {
                builtin_registry: self.builtin_registry.clone(),
                context: context.clone(),
                workflow_path: on_tool_call_path.clone(),
                base_dir,
                stream_tool_events,
            })
        } else if let Some(host_handler) = self.runtime.default_tool_handler() {
            // Host process injected a default ChatToolHandler (e.g. Tauri
            // app embedding juglans wired its own SQLite-query tool router).
            // Prefer it over the WorkflowToolHandler client-bridge fallback —
            // workflows that don't declare `on_tool` still get to use Rust
            // tools without each .jg having to bridge them by hand.
            info!("│   default tool_handler: host-provided (Rust)");
            host_handler
        } else {
            Arc::new(WorkflowToolHandler {
                builtin_registry: self.builtin_registry.clone(),
                context: context.clone(),
                stream_tool_events,
                pending_tools: Arc::new(Mutex::new(None)),
            })
        };

        // If `mcp=` was declared, wrap the base handler so any tool call
        // with a `server.` prefix gets routed to the matching MCP server
        // before the inner handler sees it.
        let handler: Arc<dyn ChatToolHandler> = match mcp_dispatcher {
            Some(dispatcher) => Arc::new(McpAwareToolHandler {
                dispatcher,
                inner: base_handler,
            }),
            None => base_handler,
        };

        let api_result = self
            .runtime
            .chat(ChatRequest {
                agent_config: final_agent_config,
                messages: chat_messages_buffer,
                tools: custom_tools_json_schema,
                token_sender: effective_token_sender,
                tool_handler: Some(handler),
            })
            .await?;

        // on_result=[handler] — extract handler name for post-completion callback
        let on_result_handler = params.get("on_result").map(|s| {
            s.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });

        match api_result {
            ChatOutput::Final { text, chat_id } => {
                debug!("│   ✓ Response completed (session: {})", chat_id);

                if should_persist {
                    // Prefer the resolved active_chat_id over the provider's
                    // per-request id so subsequent nodes in the same run
                    // stay on the same stored thread.
                    let exposed_chat_id = active_chat_id.clone().unwrap_or_else(|| chat_id.clone());
                    context.set("reply.chat_id".to_string(), json!(exposed_chat_id))?;

                    let current_display_buffer = context
                        .resolve_path("reply.output")
                        .ok()
                        .flatten()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let new_display_buffer = format!("{}{}", current_display_buffer, text);
                    context.set("reply.output".to_string(), json!(new_display_buffer))?;

                    // Persist user+assistant turn to the configured store.
                    // Only when (a) we resolved a chat_id, (b) the global
                    // store is configured, and (c) this wasn't a one-shot
                    // history-override call (explicit `history` param).
                    let explicit_history = params.get("history").is_some();
                    if let (Some(ref cid), false) = (active_chat_id.as_ref(), explicit_history) {
                        if let Some(store) = crate::services::history::global_store() {
                            use crate::services::history::ChatMessage;
                            let user_msg = ChatMessage::new("user", user_message_body.clone())
                                .with_tokens(estimate_tokens(user_message_body));
                            let asst_msg = ChatMessage::new("assistant", text.clone())
                                .with_tokens(estimate_tokens(&text));
                            if let Err(e) = store.append(cid, user_msg).await {
                                warn!("│   history.append(user) failed: {}", e);
                            }
                            if let Err(e) = store.append(cid, asst_msg).await {
                                warn!("│   history.append(assistant) failed: {}", e);
                            } else {
                                debug!("│   Persisted turn to chat_id={}", cid);
                            }
                        }
                    }
                }

                if requested_format_mode == "json" {
                    let clean_json_str = self.clean_json_output_verbose(&text);
                    info!(
                        "│   📋 [JSON mode] Raw text: {}",
                        &text.chars().take(500).collect::<String>()
                    );
                    info!(
                        "│   📋 [JSON mode] Cleaned: {}",
                        &clean_json_str.chars().take(500).collect::<String>()
                    );
                    let parsed = serde_json::from_str::<Value>(&clean_json_str);
                    if let Err(ref e) = parsed {
                        warn!("│   ⚠️ [JSON mode] Parse failed: {}", e);
                    }
                    return Ok(Some(parsed.unwrap_or(json!(text))));
                }

                // on_result: call handler with full response text
                if let Some(ref handler_name) = on_result_handler {
                    let registry = self
                        .builtin_registry
                        .as_ref()
                        .and_then(|w| w.upgrade())
                        .ok_or_else(|| anyhow!("on_result: BuiltinRegistry not available"))?;
                    let executor = registry
                        .get_executor()
                        .ok_or_else(|| anyhow!("on_result: WorkflowExecutor not available"))?;
                    let workflow = context
                        .get_root_workflow()
                        .or_else(|| context.get_current_workflow())
                        .ok_or_else(|| anyhow!("on_result: no workflow found"))?;

                    let mut args = HashMap::new();
                    args.insert("text".to_string(), json!(&text));
                    info!("│   on_result: [{}]", handler_name);

                    let result = executor
                        .execute_function(handler_name.clone(), args, workflow, context)
                        .await?;
                    return Ok(result);
                }

                Ok(Some(json!(text)))
            }
            ChatOutput::ToolCalls { .. } => {
                // Should not reach this branch when tool_handler is provided
                Err(anyhow!(
                    "Unexpected ToolCalls response — tool_handler should have handled inline"
                ))
            }
        }
    }
}

pub struct Prompt {
    registry: Arc<PromptRegistry>,
}

impl Prompt {
    pub fn new(registry: Arc<PromptRegistry>) -> Self {
        Self { registry }
    }

    fn render_template_verbose(
        &self,
        raw_body: &str,
        node_params: &HashMap<String, String>,
        flow_ctx: &WorkflowContext,
    ) -> String {
        TEMPLATE_VAR_RE
            .replace_all(raw_body, |caps: &regex::Captures| {
                let variable_name = &caps[1];
                if let Some(explicit_value) = node_params.get(variable_name) {
                    return explicit_value.clone();
                }
                match flow_ctx.resolve_path(variable_name) {
                    Ok(Some(ctx_value)) => ctx_value
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| ctx_value.to_string()),
                    _ => {
                        format!("{{{{{}}}}}", variable_name)
                    }
                }
            })
            .to_string()
    }
}

#[async_trait]
impl Tool for Prompt {
    fn name(&self) -> &str {
        "p"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // `file=` takes priority over `slug=`: it reads the template
        // directly from disk relative to the workflow's project root.
        // The `SetCwd` guard in runner.rs:113 has already chdir'd us
        // into that root before execution, so a plain relative path
        // resolves naturally. This is the preferred form — it avoids
        // the pre-populated `PromptRegistry` and the `prompts:` header
        // entirely. `slug=` is kept as a legacy path for workflows
        // that still declare prompt globs in their header.
        let template_raw_string = if let Some(file_path) = params.get("file") {
            std::fs::read_to_string(file_path).map_err(|e| {
                anyhow!(
                    "Prompt Tool: failed to read prompt file '{}': {}",
                    file_path,
                    e
                )
            })?
        } else if let Some(slug) = params.get("slug") {
            self.registry.get(slug).cloned().ok_or_else(|| {
                anyhow!(
                    "Prompt Tool: prompt slug '{}' not found in local registry. \
                     Use p(file=\"path/to/template.jgx\") or add the file to a \
                     `prompts:` glob in your .jg workflow.",
                    slug
                )
            })?
        } else {
            return Err(anyhow!(
                "Prompt Tool: 'file' or 'slug' parameter is required"
            ));
        };

        let finalized_output = match PromptParser::parse(&template_raw_string) {
            Ok(parsed_resource) if !parsed_resource.ast.is_empty() => {
                // Build context JSON from params + workflow context
                let mut ctx_map = serde_json::Map::new();
                // Add explicit params first
                for (k, v) in params {
                    if k != "slug" && k != "file" {
                        let val = serde_json::from_str(v).unwrap_or(Value::String(v.clone()));
                        ctx_map.insert(k.clone(), val);
                    }
                }
                // Add workflow context variables as fallback
                if let Ok(Some(ctx_val)) = context.resolve_path("ctx") {
                    if let Some(obj) = ctx_val.as_object() {
                        for (k, v) in obj {
                            if !ctx_map.contains_key(k) {
                                ctx_map.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                let ctx_json = Value::Object(ctx_map);
                let renderer = crate::core::renderer::JwlRenderer::new();
                renderer.render(&parsed_resource.ast, &ctx_json)?
            }
            Ok(parsed_resource) => {
                self.render_template_verbose(&parsed_resource.content, params, context)
            }
            Err(_) => self.render_template_verbose(&template_raw_string, params, context),
        };

        Ok(Some(Value::String(finalized_output)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pure_json() {
        let input = r#"{"is_trade": true}"#;
        let result = Chat::extract_last_json_block(input);
        assert_eq!(result, Some(r#"{"is_trade": true}"#.to_string()));
    }

    #[test]
    fn extract_json_from_prose_prefix() {
        let input = "Based on user input analysis, this is a trading intent.\n\n{\"is_trade\": true, \"symbol\": \"BTC\", \"direction\": \"long\", \"leverage\": 10}";
        let result = Chat::extract_last_json_block(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["is_trade"], json!(true));
        assert_eq!(parsed["symbol"], json!("BTC"));
    }

    #[test]
    fn extract_handles_braces_inside_strings() {
        let input = r#"{"key": "value { } inside"}"#;
        let result = Chat::extract_last_json_block(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["key"], json!("value { } inside"));
    }

    #[test]
    fn extract_handles_escaped_quotes() {
        let input = r#"{"msg": "say \"hi\""}"#;
        let result = Chat::extract_last_json_block(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["msg"], json!("say \"hi\""));
    }

    #[test]
    fn extract_returns_last_of_multiple_blocks() {
        let input = r#"{"first": 1} some text {"second": 2}"#;
        let result = Chat::extract_last_json_block(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["second"], json!(2));
    }

    #[test]
    fn extract_returns_none_for_plain_text() {
        let input = "This is plain text without any JSON content";
        assert!(Chat::extract_last_json_block(input).is_none());
    }

    #[test]
    fn extract_handles_nested_objects() {
        let input = r#"prefix text {"outer": {"inner": [1, 2, 3]}} suffix"#;
        let result = Chat::extract_last_json_block(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["outer"]["inner"], json!([1, 2, 3]));
    }

    #[test]
    fn extract_handles_array_block() {
        let input = r#"result: [{"a": 1}, {"b": 2}]"#;
        let result = Chat::extract_last_json_block(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["a"], json!(1));
    }

    #[test]
    fn extract_returns_none_for_empty_string() {
        assert!(Chat::extract_last_json_block("").is_none());
    }
}
