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
use crate::services::interface::{ChatRequest, ChatToolHandler, JuglansRuntime};
use crate::services::jug0::ChatOutput;
use crate::services::prompt_loader::PromptRegistry;

lazy_static! {
    static ref TEMPLATE_VAR_RE: Regex = Regex::new(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}").unwrap();
}

pub struct Chat {
    _prompt_registry: Arc<PromptRegistry>,
    runtime: Arc<dyn JuglansRuntime>,
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
}

impl Chat {
    pub fn new(prompt_registry: Arc<PromptRegistry>, runtime: Arc<dyn JuglansRuntime>) -> Self {
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
        // Store dynamic tools from frontend for jug0.rs to pick up
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
        // should_stream based on output_state (whether AI response is visible to user)
        let should_stream = output_state == "context_visible" || output_state == "display_only";
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
        let inline_agent: Option<Value> = serde_json::from_str::<Value>(agent_param)
            .ok()
            .filter(|v| v.is_object());

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

        let custom_tools_json_schema = if let Some(schema_raw) = tools_json_str {
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

        let chat_messages_buffer = vec![json!({
            "type": "text",
            "role": "user",
            "content": user_message_body
        })];

        let active_session_id = if let Some(explicit_id) = params.get("chat_id") {
            if explicit_id.starts_with("[Missing:") || explicit_id.trim().is_empty() {
                debug!("Explicit chat_id parameter invalid or empty, treating as None.");
                None
            } else {
                debug!("Using explicit chat_id from parameters: {}", explicit_id);
                Some(explicit_id.clone())
            }
        } else if should_persist {
            if let Ok(Some(ctx_val)) = context.resolve_path("reply.chat_id") {
                if let Some(ctx_str) = ctx_val.as_str() {
                    debug!("Inheriting chat_id from context: {}", ctx_str);
                    Some(ctx_str.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            debug!(
                "Non-persist state ({}): Starting fresh session.",
                input_state
            );
            None
        };

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
            // Plain string: treat as remote agent slug (jug0)
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
        let effective_meta_sender = if should_stream { meta_sender } else { None };

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

        // Create tool execution callback — tool_call handled within SSE stream, no tool loop needed
        // Priority: map handler (from declarative tools) > on_tool=[node] > on_tool_call=path > default
        let handler: Arc<dyn ChatToolHandler> = if let Some(map) = tool_handler_map.take() {
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
        } else {
            Arc::new(WorkflowToolHandler {
                builtin_registry: self.builtin_registry.clone(),
                context: context.clone(),
                stream_tool_events,
                pending_tools: Arc::new(Mutex::new(None)),
            })
        };

        let api_result = self
            .runtime
            .chat(ChatRequest {
                agent_config: final_agent_config,
                messages: chat_messages_buffer,
                tools: custom_tools_json_schema,
                chat_id: active_session_id.clone(),
                token_sender: effective_token_sender,
                meta_sender: effective_meta_sender,
                state: Some(state_raw.clone()),
                history: history_param.map(|s| s.to_string()),
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
                    context.set("reply.chat_id".to_string(), json!(chat_id))?;

                    let current_display_buffer = context
                        .resolve_path("reply.output")
                        .ok()
                        .flatten()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let new_display_buffer = format!("{}{}", current_display_buffer, text);
                    context.set("reply.output".to_string(), json!(new_display_buffer))?;
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

pub struct MemorySearch {
    runtime: Arc<dyn JuglansRuntime>,
}

impl MemorySearch {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for MemorySearch {
    fn name(&self) -> &str {
        "memory_search"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let query_text = params
            .get("query")
            .ok_or_else(|| anyhow!("MemorySearch: 'query' parameter is required."))?;

        let limit_val: u64 = params
            .get("limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(5);

        info!(
            "🧠 Executing Semantic Memory Search: '{}' (limit: {})",
            query_text, limit_val
        );

        let search_results = self.runtime.search_memories(query_text, limit_val).await?;

        Ok(Some(json!(search_results)))
    }
}

// ─── Web Search Builtin ─────────────────────────────────

pub struct WebSearch {
    runtime: Arc<dyn JuglansRuntime>,
}

impl WebSearch {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let query = params
            .get("query")
            .ok_or_else(|| anyhow!("web_search: 'query' parameter is required."))?;

        info!("🔍 Executing Web Search: '{}'", query);

        let result = self.runtime.web_search(query).await?;
        Ok(Some(result))
    }
}

// ─── Vector Builtins ─────────────────────────────────────

pub struct VectorCreateSpace {
    runtime: Arc<dyn JuglansRuntime>,
}

impl VectorCreateSpace {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for VectorCreateSpace {
    fn name(&self) -> &str {
        "vector_create_space"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let space = params
            .get("space")
            .ok_or_else(|| anyhow!("vector_create_space: 'space' parameter is required."))?;

        let model = params.get("model").map(|s| s.as_str());
        let public = params
            .get("public")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        info!(
            "📦 Creating vector space: '{}' (model: {:?}, public: {})",
            space, model, public
        );

        let result = self
            .runtime
            .vector_create_space(space, model, public)
            .await?;

        Ok(Some(result))
    }
}

pub struct VectorUpsert {
    runtime: Arc<dyn JuglansRuntime>,
}

impl VectorUpsert {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for VectorUpsert {
    fn name(&self) -> &str {
        "vector_upsert"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let space = params
            .get("space")
            .ok_or_else(|| anyhow!("vector_upsert: 'space' parameter is required."))?;

        let points_str = params
            .get("points")
            .ok_or_else(|| anyhow!("vector_upsert: 'points' parameter is required."))?;

        let points: Vec<Value> = serde_json::from_str(points_str)
            .map_err(|e| anyhow!("vector_upsert: invalid JSON in 'points': {}", e))?;

        let model = params.get("model").map(|s| s.as_str());

        info!(
            "📥 Vector upsert: {} points into space '{}' (model: {:?})",
            points.len(),
            space,
            model
        );

        let result = self.runtime.vector_upsert(space, points, model).await?;

        Ok(Some(result))
    }
}

pub struct VectorSearch {
    runtime: Arc<dyn JuglansRuntime>,
}

impl VectorSearch {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for VectorSearch {
    fn name(&self) -> &str {
        "vector_search"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let query = params
            .get("query")
            .ok_or_else(|| anyhow!("vector_search: 'query' parameter is required."))?;

        let space = params.get("space").map(|s| s.as_str()).unwrap_or("default");

        let limit: u64 = params
            .get("limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(5);

        let model = params.get("model").map(|s| s.as_str());

        info!(
            "🔍 Vector search: '{}' in space '{}' (limit: {}, model: {:?})",
            query, space, limit, model
        );

        let results = self
            .runtime
            .vector_search(space, query, limit, model)
            .await?;

        Ok(Some(json!(results)))
    }
}

pub struct VectorListSpaces {
    runtime: Arc<dyn JuglansRuntime>,
}

impl VectorListSpaces {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for VectorListSpaces {
    fn name(&self) -> &str {
        "vector_list_spaces"
    }

    async fn execute(
        &self,
        _params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        info!("📋 Vector list spaces");
        let results = self.runtime.vector_list_spaces().await?;
        Ok(Some(json!(results)))
    }
}

pub struct VectorDeleteSpace {
    runtime: Arc<dyn JuglansRuntime>,
}

impl VectorDeleteSpace {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for VectorDeleteSpace {
    fn name(&self) -> &str {
        "vector_delete_space"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let space = params
            .get("space")
            .ok_or_else(|| anyhow!("vector_delete_space: 'space' parameter is required."))?;

        info!("🗑️ Vector delete space: '{}'", space);
        let result = self.runtime.vector_delete_space(space).await?;
        Ok(Some(result))
    }
}

pub struct VectorDelete {
    runtime: Arc<dyn JuglansRuntime>,
}

impl VectorDelete {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for VectorDelete {
    fn name(&self) -> &str {
        "vector_delete"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let space = params
            .get("space")
            .ok_or_else(|| anyhow!("vector_delete: 'space' parameter is required."))?;

        let ids_raw = params
            .get("ids")
            .ok_or_else(|| anyhow!("vector_delete: 'ids' parameter is required."))?;

        // Support JSON array or comma-separated string
        let ids: Vec<String> = if ids_raw.trim_start().starts_with('[') {
            serde_json::from_str(ids_raw)
                .unwrap_or_else(|_| ids_raw.split(',').map(|s| s.trim().to_string()).collect())
        } else {
            ids_raw.split(',').map(|s| s.trim().to_string()).collect()
        };

        info!(
            "🗑️ Vector delete {} point(s) from space '{}'",
            ids.len(),
            space
        );
        let result = self.runtime.vector_delete(space, ids).await?;
        Ok(Some(result))
    }
}

pub struct History {
    runtime: Arc<dyn JuglansRuntime>,
}

impl History {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for History {
    fn name(&self) -> &str {
        "history"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params
            .get("chat_id")
            .ok_or_else(|| anyhow!("history() requires 'chat_id' parameter"))?;

        let include_all = params
            .get("include_all")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        info!(
            "📚 Fetching chat history for: {} (include_all: {})",
            chat_id, include_all
        );

        let messages = self
            .runtime
            .fetch_chat_history(chat_id, include_all)
            .await?;

        info!("📚 Retrieved {} messages", messages.len());

        Ok(Some(json!(messages)))
    }
}

pub struct Prompt {
    registry: Arc<PromptRegistry>,
    runtime: Arc<dyn JuglansRuntime>,
}

impl Prompt {
    pub fn new(registry: Arc<PromptRegistry>, runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { registry, runtime }
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
        let target_slug = params
            .get("slug")
            .or_else(|| params.get("file"))
            .ok_or_else(|| anyhow!("Prompt Tool: 'slug' parameter is required."))?;

        let template_raw_string = if let Some(local_content) = self.registry.get(target_slug) {
            local_content.clone()
        } else {
            self.runtime.fetch_prompt(target_slug).await?
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
