// src/core/context.rs
use anyhow::{anyhow, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::mpsc::UnboundedSender;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::oneshot;

use crate::core::graph::{ClassDef, WorkflowGraph};
use crate::core::instance_arena::{InstanceArena, InstanceId, MethodScope, TypedSlot};
use crate::core::jvalue::JValue;

/// Type alias for pending tool start info: (tool_name, params, start_time)
#[cfg(not(target_arch = "wasm32"))]
type PendingToolStarts = Arc<Mutex<HashMap<String, (String, HashMap<String, String>, Instant)>>>;
#[cfg(target_arch = "wasm32")]
type PendingToolStarts = Arc<Mutex<HashMap<String, (String, HashMap<String, String>)>>>;

/// Client tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub content: String,
}

/// Tool execution trace entry
#[derive(Debug, Clone, Serialize)]
pub struct ToolTraceEntry {
    pub node_id: String,
    pub tool: String,
    pub params: HashMap<String, String>,
    pub result: Option<Value>,
    pub duration: Duration,
    pub status: TraceStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum TraceStatus {
    Success,
    Error(String),
}

/// Tool execution start event (structured)
#[derive(Debug, Clone, Serialize)]
pub struct ToolStartEvent {
    pub node_id: String,
    pub tool: String,
    pub params: Value,
}

/// Tool execution complete event (structured)
#[derive(Debug, Clone, Serialize)]
pub struct ToolCompleteEvent {
    pub node_id: String,
    pub tool: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Workflow node execution start event (structured)
#[derive(Debug, Clone, Serialize)]
pub struct NodeStartEvent {
    pub node_id: String,
    pub tool: String,
    pub params: HashMap<String, String>,
}

/// Workflow node execution complete event (structured)
#[derive(Debug, Clone, Serialize)]
pub struct NodeCompleteEvent {
    pub node_id: String,
    pub tool: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One streaming reply session. Created via `ChannelEgress::start_stream`
/// when both the channel and the chat/reply call agree on streaming
/// (channel can stream + workflow asked for stream + state is user-visible).
/// The egress driver pushes tokens as they arrive on the workflow's event
/// stream and finalizes when the speech node completes.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
pub trait StreamHandle: Send + Sync {
    /// Append a token to the stream. Idempotent on transport-level retries
    /// is the channel impl's responsibility (e.g. TG editMessageText keeps
    /// retrying with the same buffer; sendMessageDraft is server-side merged).
    async fn push_token(&mut self, text: &str) -> Result<()>;
    /// Finalize the stream — flush any debounce buffer and seal the message.
    /// Called once on NodeComplete for the streaming chat/reply node.
    async fn finalize(self: Box<Self>) -> Result<()>;
}

/// Egress side of a channel — split out of the full `Channel` trait (which
/// lives in `adapters/mod.rs`) so `WorkflowContext` can carry an `Arc<dyn
/// ChannelEgress>` without `core` depending on `adapters`. The full `Channel`
/// trait inherits this; impls provide `send` (always) and optionally
/// `start_stream`.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
pub trait ChannelEgress: Send + Sync {
    /// Push a message back to a conversation on this channel — batch / one-shot.
    /// Default fails loudly so misrouted egress surfaces as a clear error.
    async fn send(&self, _conversation: &str, _text: &str) -> Result<()> {
        Err(anyhow!("channel does not support egress"))
    }

    /// Start a streaming reply session. Default: not supported — callers
    /// should fall back to `send` at finalization. Channels that have a
    /// native streaming primitive (Telegram `sendMessageDraft`, web SSE)
    /// override this to return a real `StreamHandle`.
    async fn start_stream(
        &self,
        _conversation: &str,
    ) -> Result<Box<dyn StreamHandle>> {
        Err(anyhow!("channel does not support streaming"))
    }
}

/// Where a workflow run came from. Set on `WorkflowContext` by the channel
/// that dispatched the inbound message; consumed by `reply()` and the channel
/// egress driver to route replies back to that channel + conversation.
///
/// `None` on `WorkflowContext` means "no channel origin" — workflow was
/// triggered by CLI run, scheduled job, or test harness. `reply()` falls back
/// to writing to `reply.output` (and stdout for CLI) in that case.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct ChannelOrigin {
    pub channel: Arc<dyn ChannelEgress>,
    pub conversation: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for ChannelOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelOrigin")
            .field("conversation", &self.conversation)
            .finish()
    }
}

/// Real-time events during workflow execution
pub enum WorkflowEvent {
    Token(String),
    Status(String),
    Error(String),
    /// Meta info — forward backend meta events to frontend (chat_id, user_message_id, etc.)
    Meta(Value),
    /// Client tool call — sent to frontend for execution, awaits result via result_tx
    #[cfg(not(target_arch = "wasm32"))]
    ToolCall {
        call_id: String,
        tools: Vec<Value>,
        result_tx: oneshot::Sender<(Vec<ToolResultPayload>, Option<Vec<Value>>)>,
    },
    /// Tool execution start event (AI sub-tool calls)
    ToolStart(ToolStartEvent),
    /// Tool execution complete event (AI sub-tool calls)
    ToolComplete(ToolCompleteEvent),
    /// Workflow node execution start event
    NodeStart(NodeStartEvent),
    /// Workflow node execution complete event
    NodeComplete(NodeCompleteEvent),
    /// Explicit yield from workflow — emitted as SSE event
    Yield(Value),
}

/// A thread-safe, shared state for a single workflow execution.
#[derive(Debug, Clone)]
pub struct WorkflowContext {
    data: Arc<RwLock<Value>>,
    /// Channel for streaming output
    #[cfg(not(target_arch = "wasm32"))]
    event_sender: Option<UnboundedSender<WorkflowEvent>>,
    /// Execution stack trace for preventing infinite recursion
    /// Format: ["agent_slug:workflow_name", ...]
    execution_stack: Arc<Mutex<Vec<String>>>,
    /// Maximum nesting depth
    max_depth: usize,
    /// Currently executing workflow (used by on_tool=[node] handler)
    current_workflow: Arc<RwLock<Option<Arc<WorkflowGraph>>>>,
    /// Root (top-level) workflow — set once at first execute_graph, never overwritten
    root_workflow: Arc<RwLock<Option<Arc<WorkflowGraph>>>>,
    /// Tool event verbosity level: 0=silent, 1=info, 2=verbose
    tool_event_level: Arc<AtomicU8>,
    /// Whether to push node execution events to frontend (default false)
    stream_node_events: Arc<AtomicBool>,
    /// Tool execution trace (records all tool call results for assert queries)
    tool_trace: Arc<Mutex<Vec<ToolTraceEntry>>>,
    /// Pending tool starts (records time at tool_start, computes duration at tool_complete)
    pending_tool_starts: PendingToolStarts,
    /// Class definition registry for instance field index lookup (avoids embedding __field_index__ in each instance)
    class_registry: Arc<RwLock<HashMap<String, Arc<ClassDef>>>>,
    /// Instance arena: class instances stored independently, outside the JSON tree
    instance_arena: InstanceArena,
    /// Method execution scope stack (nested method calls)
    method_scopes: Arc<RwLock<Vec<MethodScope>>>,
    /// Typed variable shadow store: scalar variables stored as TypedSlot
    /// for zero-JSON-overhead reads. Dual-written alongside the JSON tree.
    typed_store: Arc<RwLock<HashMap<String, TypedSlot>>>,
    /// Channel that dispatched this workflow run, if any. Set by the
    /// dispatcher entry point; read by `reply()` and the channel egress
    /// driver. `None` for CLI / scheduled / test invocations.
    #[cfg(not(target_arch = "wasm32"))]
    origin: Arc<RwLock<Option<ChannelOrigin>>>,
}

impl Default for WorkflowContext {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowContext {
    /// Creates a new, empty context, initialized as a JSON object.
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(json!({}))),
            #[cfg(not(target_arch = "wasm32"))]
            event_sender: None,
            execution_stack: Arc::new(Mutex::new(Vec::new())),
            max_depth: 10,
            current_workflow: Arc::new(RwLock::new(None)),
            root_workflow: Arc::new(RwLock::new(None)),
            tool_event_level: Arc::new(AtomicU8::new(0)),
            stream_node_events: Arc::new(AtomicBool::new(false)),
            tool_trace: Arc::new(Mutex::new(Vec::new())),
            pending_tool_starts: Arc::new(Mutex::new(HashMap::new())),
            class_registry: Arc::new(RwLock::new(HashMap::new())),
            instance_arena: InstanceArena::new(),
            method_scopes: Arc::new(RwLock::new(Vec::new())),
            typed_store: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(not(target_arch = "wasm32"))]
            origin: Arc::new(RwLock::new(None)),
        }
    }

    /// Set maximum nesting depth
    pub fn _set_max_depth(&mut self, max_depth: usize) {
        self.max_depth = max_depth;
    }

    /// Create a context with an event channel
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_sender(sender: UnboundedSender<WorkflowEvent>) -> Self {
        Self {
            data: Arc::new(RwLock::new(json!({}))),
            event_sender: Some(sender),
            execution_stack: Arc::new(Mutex::new(Vec::new())),
            max_depth: 10,
            current_workflow: Arc::new(RwLock::new(None)),
            root_workflow: Arc::new(RwLock::new(None)),
            tool_event_level: Arc::new(AtomicU8::new(0)),
            stream_node_events: Arc::new(AtomicBool::new(false)),
            tool_trace: Arc::new(Mutex::new(Vec::new())),
            pending_tool_starts: Arc::new(Mutex::new(HashMap::new())),
            class_registry: Arc::new(RwLock::new(HashMap::new())),
            instance_arena: InstanceArena::new(),
            method_scopes: Arc::new(RwLock::new(Vec::new())),
            typed_store: Arc::new(RwLock::new(HashMap::new())),
            origin: Arc::new(RwLock::new(None)),
        }
    }

    /// Fork: deep-clone data for isolated parallel execution (e.g. foreach parallel).
    /// Shares arena, class_registry, event_sender, etc. via Arc.
    pub fn fork(&self) -> Self {
        Self {
            data: Arc::new(RwLock::new(self.data.read().clone())),
            #[cfg(not(target_arch = "wasm32"))]
            event_sender: self.event_sender.clone(),
            execution_stack: self.execution_stack.clone(),
            max_depth: self.max_depth,
            current_workflow: self.current_workflow.clone(),
            root_workflow: self.root_workflow.clone(),
            tool_event_level: self.tool_event_level.clone(),
            stream_node_events: self.stream_node_events.clone(),
            tool_trace: self.tool_trace.clone(),
            pending_tool_starts: self.pending_tool_starts.clone(),
            class_registry: self.class_registry.clone(),
            instance_arena: self.instance_arena.clone(),
            method_scopes: Arc::new(RwLock::new(Vec::new())),
            typed_store: Arc::new(RwLock::new(self.typed_store.read().clone())),
            #[cfg(not(target_arch = "wasm32"))]
            origin: self.origin.clone(),
        }
    }

    /// Enter nested execution (push onto stack).
    /// Returns Err if recursion is detected or max depth is exceeded.
    pub fn enter_execution(&self, identifier: String) -> Result<()> {
        let mut stack = self.execution_stack.lock();

        // Check depth limit
        if stack.len() >= self.max_depth {
            return Err(anyhow!(
                "Maximum execution depth ({}) exceeded. Current stack: {:?}",
                self.max_depth,
                stack
            ));
        }

        // Check for circular references
        if stack.contains(&identifier) {
            return Err(anyhow!(
                "Circular execution detected: '{}' is already in the call stack: {:?}",
                identifier,
                stack
            ));
        }

        stack.push(identifier);
        Ok(())
    }

    /// Exit nested execution (pop from stack)
    pub fn exit_execution(&self) -> Result<()> {
        let mut stack = self.execution_stack.lock();

        if stack.is_empty() {
            return Err(anyhow!("Execution stack is already empty"));
        }

        stack.pop();
        Ok(())
    }

    /// Get current execution stack (for debugging)
    pub fn _get_execution_stack(&self) -> Result<Vec<String>> {
        let stack = self.execution_stack.lock();
        Ok(stack.clone())
    }

    /// Get current nesting depth
    pub fn _get_execution_depth(&self) -> Result<usize> {
        let stack = self.execution_stack.lock();
        Ok(stack.len())
    }

    /// Emit an event
    #[cfg(not(target_arch = "wasm32"))]
    pub fn emit(&self, event: WorkflowEvent) {
        if let Some(sender) = &self.event_sender {
            let _ = sender.send(event);
        }
    }

    /// WASM: no-op event emission
    #[cfg(target_arch = "wasm32")]
    pub fn emit(&self, _event: WorkflowEvent) {}

    /// Whether an event sender exists (true in TUI mode, false in CLI mode)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_event_sender(&self) -> bool {
        self.event_sender.is_some()
    }

    #[cfg(target_arch = "wasm32")]
    pub fn has_event_sender(&self) -> bool {
        false
    }

    /// Send a client tool call and wait for the frontend to return results
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn emit_tool_call_and_wait(
        &self,
        call_id: String,
        tools: Vec<Value>,
        timeout_secs: u64,
    ) -> Result<(Vec<ToolResultPayload>, Option<Vec<Value>>)> {
        let (result_tx, result_rx) = oneshot::channel();
        self.emit(WorkflowEvent::ToolCall {
            call_id: call_id.clone(),
            tools,
            result_tx,
        });

        tokio::time::timeout(Duration::from_secs(timeout_secs), result_rx)
            .await
            .map_err(|_| {
                anyhow!(
                    "Client tool execution timed out after {}s (call_id: {})",
                    timeout_secs,
                    call_id
                )
            })?
            .map_err(|_| anyhow!("Client tool result channel dropped (call_id: {})", call_id))
    }

    /// Get a token sender adapter.
    /// Converts the String type needed by Runtime into the WorkflowEvent type needed by Context.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_token_sender_adapter(&self) -> Option<UnboundedSender<String>> {
        let event_sender = self.event_sender.clone()?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            while let Some(token) = rx.recv().await {
                let _ = event_sender.send(WorkflowEvent::Token(token));
            }
        });

        Some(tx)
    }

    /// Get a meta sender adapter.
    /// Converts Value type into WorkflowEvent::Meta type.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_meta_sender_adapter(&self) -> Option<UnboundedSender<Value>> {
        let event_sender = self.event_sender.clone()?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();

        tokio::spawn(async move {
            while let Some(meta) = rx.recv().await {
                let _ = event_sender.send(WorkflowEvent::Meta(meta));
            }
        });

        Some(tx)
    }

    /// Set class definition registry (called when workflow execution starts)
    pub fn set_class_registry(&self, classes: &HashMap<String, Arc<ClassDef>>) {
        *self.class_registry.write() = classes.clone();
    }

    // ============================================================
    // Instance Arena API
    // ============================================================

    /// Get arena reference (used when executor operates directly)
    pub fn arena(&self) -> &InstanceArena {
        &self.instance_arena
    }

    /// Allocate an instance to the arena and store a proxy reference in the JSON tree
    pub fn alloc_instance(
        &self,
        path: String,
        class_name: String,
        class_def: Arc<ClassDef>,
        fields: Vec<Value>,
    ) -> Result<InstanceId> {
        let id = self
            .instance_arena
            .alloc(path.clone(), class_name, class_def, fields);
        // Store a lightweight proxy in the JSON tree for resolve_path detection
        self.set(path, json!({"__arena_ref__": id.0}))?;
        Ok(id)
    }

    /// Look up an arena instance by variable name
    pub fn lookup_instance(&self, name: &str) -> Result<InstanceId> {
        // Direct arena lookup by instance name
        if let Some(id) = self.instance_arena.lookup_by_name(name) {
            return Ok(id);
        }
        // Indirect: context variable holding an arena ref (e.g. foreach loop var)
        // Read raw data to avoid materialization (resolve_path materializes arena refs)
        let data = self.data.read();
        if let Some(val) = data.get(name) {
            if let Some(id) = Self::resolve_arena_ref(val) {
                return Ok(id);
            }
        }
        Err(anyhow!("Instance '{}' not found in arena", name))
    }

    /// Push a method execution scope
    pub fn push_method_scope(&self, scope: MethodScope) -> Result<()> {
        self.method_scopes.write().push(scope);
        Ok(())
    }

    /// Pop a method execution scope
    pub fn pop_method_scope(&self) -> Result<Option<MethodScope>> {
        Ok(self.method_scopes.write().pop())
    }

    /// Phase C-2: Set method parameter TypedSlot values (called after method parameter binding)
    pub fn set_method_param_values(&self, values: Vec<TypedSlot>) -> Result<()> {
        let mut scopes = self.method_scopes.write();
        if let Some(scope) = scopes.last_mut() {
            scope.param_values = values;
        }
        Ok(())
    }

    /// Phase C-2: Execute a closure on the method scope (zero-copy access to field_cache/param_values).
    /// Returns None if not within a method scope.
    pub fn with_method_scope<T>(&self, f: impl FnOnce(&MethodScope) -> T) -> Option<T> {
        let scopes = self.method_scopes.read();
        scopes.last().map(f)
    }

    /// Flush dirty fields into the arena
    pub fn flush_dirty_to_arena(&self, scope: &MethodScope) {
        if scope.dirty.is_empty() {
            return;
        }
        let updates: Vec<(usize, Value)> = scope
            .dirty
            .iter()
            .filter_map(|(name, val)| {
                scope
                    .class_def
                    .field_index
                    .get(name.as_str())
                    .map(|&idx| (idx, val.clone()))
            })
            .collect();
        self.instance_arena
            .set_fields_batch(scope.instance_id, &updates);
    }

    /// Check whether a Value is an arena proxy reference
    fn resolve_arena_ref(val: &Value) -> Option<InstanceId> {
        val.as_object()
            .and_then(|m| m.get("__arena_ref__"))
            .and_then(|v| v.as_u64())
            .map(InstanceId)
    }

    /// Set the currently executing workflow (read by on_tool=[node] handler)
    pub fn set_current_workflow(&self, workflow: Arc<WorkflowGraph>) {
        *self.current_workflow.write() = Some(workflow);
    }

    /// Get the currently executing workflow
    pub fn get_current_workflow(&self) -> Option<Arc<WorkflowGraph>> {
        self.current_workflow.read().clone()
    }

    /// Set the root (top-level) workflow — only sets if not already set
    pub fn set_root_workflow(&self, workflow: Arc<WorkflowGraph>) {
        let mut w = self.root_workflow.write();
        if w.is_none() {
            *w = Some(workflow);
        }
    }

    /// Get the root workflow (top-level, never overwritten by sub-graph execution)
    pub fn get_root_workflow(&self) -> Option<Arc<WorkflowGraph>> {
        self.root_workflow.read().clone()
    }

    /// Set the channel origin for this run. Called by dispatchers when invoked
    /// from a channel; idempotent — only sets once per context.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_origin(&self, origin: ChannelOrigin) {
        let mut o = self.origin.write();
        if o.is_none() {
            *o = Some(origin);
        }
    }

    /// Read the channel origin, if any. Used by `reply()` and the egress
    /// driver to route replies back to the originating channel. The egress
    /// driver currently consumes origin via the run_agent_for_message helper
    /// which captures it at function entry; this accessor lands when phase 9
    /// (workflow `reply(channel="...")` cross-channel addressing) plugs in.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    pub fn origin(&self) -> Option<ChannelOrigin> {
        self.origin.read().clone()
    }

    /// Set tool event verbosity level: 0=silent, 1=info, 2=verbose
    pub fn set_tool_event_level(&self, level: u8) {
        self.tool_event_level.store(level, Ordering::Relaxed);
    }

    /// Set whether to push node execution events
    pub fn set_stream_node_events(&self, enabled: bool) {
        self.stream_node_events.store(enabled, Ordering::Relaxed);
    }

    /// Emit tool_start event (also records to trace)
    pub fn emit_tool_start(&self, node_id: &str, tool: &str, params_json: &str) {
        // Parse params for trace recording
        let params_map: HashMap<String, String> =
            serde_json::from_str(params_json).unwrap_or_default();

        // Record start time in pending (keyed by node_id)
        #[cfg(not(target_arch = "wasm32"))]
        self.pending_tool_starts.lock().insert(
            node_id.to_string(),
            (tool.to_string(), params_map, Instant::now()),
        );
        #[cfg(target_arch = "wasm32")]
        self.pending_tool_starts
            .lock()
            .insert(node_id.to_string(), (tool.to_string(), params_map));

        let level = self.tool_event_level.load(Ordering::Relaxed);
        if level == 0 {
            return;
        }
        let event_params = if level >= 2 {
            serde_json::from_str::<Value>(params_json).unwrap_or(json!({}))
        } else {
            json!({})
        };
        self.emit(WorkflowEvent::ToolStart(ToolStartEvent {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            params: event_params,
        }));
    }

    /// Emit tool_complete event (also writes to trace)
    pub fn emit_tool_complete(&self, node_id: &str, tool: &str, result: &Result<Option<Value>>) {
        // Retrieve start time from pending, compute duration, write to trace
        let start_info = self.pending_tool_starts.lock().remove(node_id);

        #[cfg(not(target_arch = "wasm32"))]
        let (params, duration) = match start_info {
            Some((_, params, started)) => (params, started.elapsed()),
            None => (HashMap::new(), Duration::ZERO),
        };
        #[cfg(target_arch = "wasm32")]
        let (params, duration) = match start_info {
            Some((_, params)) => (params, Duration::ZERO),
            None => (HashMap::new(), Duration::ZERO),
        };

        let (trace_result, trace_status) = match result {
            Ok(val) => (val.clone(), TraceStatus::Success),
            Err(e) => (None, TraceStatus::Error(e.to_string())),
        };

        let entry = ToolTraceEntry {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            params,
            result: trace_result,
            duration,
            status: trace_status,
        };

        self.tool_trace.lock().push(entry);

        // Stream event to frontend based on level
        let level = self.tool_event_level.load(Ordering::Relaxed);
        if level == 0 {
            return;
        }
        let (status, evt_result, evt_error) = match result {
            Ok(val) => (
                "success".to_string(),
                if level >= 2 { val.clone() } else { None },
                None,
            ),
            Err(e) => (
                "error".to_string(),
                None,
                if level >= 2 {
                    Some(e.to_string())
                } else {
                    None
                },
            ),
        };
        self.emit(WorkflowEvent::ToolComplete(ToolCompleteEvent {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            status,
            result: evt_result,
            error: evt_error,
        }));
    }

    /// Emit node_start event (workflow node starts execution, also records to trace)
    pub fn emit_node_start(&self, node_id: &str, tool: &str, params: &HashMap<String, String>) {
        // Record start time in pending (keyed by node_id)
        #[cfg(not(target_arch = "wasm32"))]
        self.pending_tool_starts.lock().insert(
            node_id.to_string(),
            (tool.to_string(), params.clone(), Instant::now()),
        );
        #[cfg(target_arch = "wasm32")]
        self.pending_tool_starts
            .lock()
            .insert(node_id.to_string(), (tool.to_string(), params.clone()));

        if !self.stream_node_events.load(Ordering::Relaxed) {
            return;
        }
        self.emit(WorkflowEvent::NodeStart(NodeStartEvent {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            params: params.clone(),
        }));
    }

    /// Emit node_complete event (workflow node finished execution, also writes to trace)
    pub fn emit_node_complete(&self, node_id: &str, tool: &str, result: &Result<Option<Value>>) {
        // Retrieve start time from pending, compute duration, write to trace
        let start_info = self.pending_tool_starts.lock().remove(node_id);

        #[cfg(not(target_arch = "wasm32"))]
        let (params, duration) = match start_info {
            Some((_, params, started)) => (params, started.elapsed()),
            None => (HashMap::new(), Duration::ZERO),
        };
        #[cfg(target_arch = "wasm32")]
        let (params, duration) = match start_info {
            Some((_, params)) => (params, Duration::ZERO),
            None => (HashMap::new(), Duration::ZERO),
        };

        let (trace_result, trace_status) = match result {
            Ok(val) => (val.clone(), TraceStatus::Success),
            Err(e) => (None, TraceStatus::Error(e.to_string())),
        };

        let entry = ToolTraceEntry {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            params,
            result: trace_result,
            duration,
            status: trace_status,
        };

        self.tool_trace.lock().push(entry);

        if !self.stream_node_events.load(Ordering::Relaxed) {
            return;
        }
        match result {
            Ok(val) => self.emit(WorkflowEvent::NodeComplete(NodeCompleteEvent {
                node_id: node_id.to_string(),
                tool: tool.to_string(),
                status: "success".to_string(),
                result: val.clone(),
                error: None,
            })),
            Err(e) => self.emit(WorkflowEvent::NodeComplete(NodeCompleteEvent {
                node_id: node_id.to_string(),
                tool: tool.to_string(),
                status: "error".to_string(),
                result: None,
                error: Some(e.to_string()),
            })),
        }
    }

    /// Get all trace entries
    pub fn trace_entries(&self) -> Vec<ToolTraceEntry> {
        self.tool_trace.lock().clone()
    }

    /// Query call records for a specific tool
    #[allow(dead_code)]
    pub fn trace_tool_called(&self, tool_name: &str) -> Vec<ToolTraceEntry> {
        self.trace_entries()
            .into_iter()
            .filter(|e| e.tool == tool_name)
            .collect()
    }

    /// Compute total trace duration
    #[allow(dead_code)]
    pub fn trace_total_duration(&self) -> Duration {
        self.trace_entries().iter().map(|e| e.duration).sum()
    }

    /// Clear trace (for test isolation)
    pub fn _clear_trace(&self) {
        self.tool_trace.lock().clear();
        self.pending_tool_starts.lock().clear();
    }

    /// Sets a value in the context using a dot-notation path.
    pub fn set(&self, path: String, value: Value) -> Result<()> {
        // If reply.status was updated, automatically emit a status event
        if path == "reply.status" {
            if let Some(s) = value.as_str() {
                self.emit(WorkflowEvent::Status(s.to_string()));
            }
        }

        // Method scope: write field names to dirty map instead of JSON tree.
        // Phase C-2: also update field_cache so ResolvedField direct indexing stays correct.
        if !path.contains('.') {
            let mut scopes = self.method_scopes.write();
            if let Some(scope) = scopes.last_mut() {
                if let Some(&idx) = scope.class_def.field_index.get(&path) {
                    if idx < scope.field_cache.len() {
                        scope.field_cache[idx] = TypedSlot::from_value(value.clone());
                    }
                    scope.dirty.insert(path, value);
                    return Ok(());
                }
                // Method scope active but path is not a field — store in dirty for later access
                scope.dirty.insert(path, value);
                return Ok(());
            }
        }

        // Dual-write to typed store: scalars are shadowed for zero-JSON reads
        match &value {
            Value::Number(_) | Value::Bool(_) | Value::String(_) | Value::Null => {
                self.typed_store
                    .write()
                    .insert(path.clone(), TypedSlot::from_value(value.clone()));
            }
            _ => {
                // Complex type (array/object): remove stale scalar entry if any
                self.typed_store.write().remove(&path);
            }
        }

        let mut data = self.data.write();

        let parts: Vec<&str> = path.split('.').collect();
        let (last_key, parent_parts) = parts
            .split_last()
            .ok_or_else(|| anyhow!("Cannot set a value with an empty path"))?;

        let mut current = &mut *data;
        for part in parent_parts {
            current = current
                .as_object_mut()
                .ok_or_else(|| anyhow!(format!("Path part '{}' is not an object", part)))?
                .entry(part.to_string())
                .or_insert_with(|| json!({}));
        }

        if let Some(obj) = current.as_object_mut() {
            obj.insert(last_key.to_string(), value);
        } else {
            return Err(anyhow!("Final path segment is not an object"));
        }

        Ok(())
    }

    /// Set a value from an existing TypedSlot, avoiding redundant from_value() classification.
    /// Writes to both typed_store and JSON tree for backward compatibility.
    #[allow(dead_code)]
    pub fn set_typed(&self, path: String, slot: TypedSlot) -> Result<()> {
        let value = slot.to_value();

        // Method scope handling (same as set())
        if !path.contains('.') {
            let mut scopes = self.method_scopes.write();
            if let Some(scope) = scopes.last_mut() {
                if let Some(&idx) = scope.class_def.field_index.get(&path) {
                    if idx < scope.field_cache.len() {
                        scope.field_cache[idx] = slot;
                    }
                    scope.dirty.insert(path, value);
                    return Ok(());
                }
                scope.dirty.insert(path, value);
                return Ok(());
            }
        }

        // Write to typed store directly (no classification needed — caller already has TypedSlot)
        self.typed_store.write().insert(path.clone(), slot);

        // Write JSON tree (backward compat)
        let mut data = self.data.write();
        let parts: Vec<&str> = path.split('.').collect();
        let (last_key, parent_parts) = parts
            .split_last()
            .ok_or_else(|| anyhow!("Cannot set a value with an empty path"))?;

        let mut current = &mut *data;
        for part in parent_parts {
            current = current
                .as_object_mut()
                .ok_or_else(|| anyhow!(format!("Path part '{}' is not an object", part)))?
                .entry(part.to_string())
                .or_insert_with(|| json!({}));
        }

        if let Some(obj) = current.as_object_mut() {
            obj.insert(last_key.to_string(), value);
        } else {
            return Err(anyhow!("Final path segment is not an object"));
        }

        Ok(())
    }

    /// Resolves a dot-notation path to a value in the context.
    pub fn resolve_path(&self, path: &str) -> Result<Option<Value>> {
        // Phase 1: method scope check (zero JSON tree access)
        {
            let scopes = self.method_scopes.read();
            if let Some(scope) = scopes.last() {
                if let Some(result) = self.resolve_in_method_scope(scope, path) {
                    return Ok(Some(result));
                }
            }
        }

        // Phase 2: JSON tree + arena proxy
        let data = self.data.read();

        // Single-segment path (no '.'): direct get, zero allocation
        if !path.contains('.') {
            let val = data.get(path);
            // Detect arena proxy -> materialize
            if let Some(v) = val {
                if let Some(id) = Self::resolve_arena_ref(v) {
                    drop(data);
                    return Ok(self.instance_arena.materialize(id));
                }
            }
            return Ok(val.cloned());
        }

        // Get class registry reference (for instance field lookup)
        let registry_guard = self.class_registry.read();
        let registry = Some(&*registry_guard);

        // Multi-segment path: manual split traversal (avoids Vec allocation)
        let mut segments = path.splitn(2, '.');
        let first = segments.next().unwrap();
        let rest = segments.next().unwrap_or("");

        let root = match data.get(first) {
            Some(v) => v,
            None => return Ok(None),
        };

        // Detect arena proxy -> resolve field from arena
        if let Some(id) = Self::resolve_arena_ref(root) {
            drop(data);
            return Ok(self.resolve_arena_field(id, rest));
        }

        // Common case with single rest segment: $instance.field (two-segment path)
        if !rest.contains('.') {
            return Ok(Some(resolve_field(root, rest, registry)));
        }

        // Multiple segments: navigate step by step
        let mut current = root.clone();
        for part in rest.split('.') {
            current = resolve_field(&current, part, registry);
            if current.is_null() {
                return Ok(None);
            }
        }
        Ok(Some(current))
    }

    /// TypedSlot fast path: resolve variable as TypedSlot, avoiding JSON overhead.
    /// Phase 1: method scope fields. Phase 2 (new): typed_store for all scalar variables.
    pub fn resolve_path_typed(&self, path: &str) -> Option<TypedSlot> {
        // 1. Method scope check (original logic, but no early `?` return on missing scope)
        {
            let scopes = self.method_scopes.read();
            if let Some(scope) = scopes.last() {
                if !path.contains('.') {
                    if let Some(val) = scope.dirty.get(path) {
                        return Some(TypedSlot::from_value(val.clone()));
                    }
                    if let Some(&idx) = scope.class_def.field_index.get(path) {
                        return scope.field_cache.get(idx).cloned();
                    }
                } else {
                    // $self.field -> return TypedSlot directly
                    let mut segments = path.splitn(2, '.');
                    let first = segments.next().unwrap();
                    let rest = segments.next().unwrap_or("");

                    if first == "self" && !rest.contains('.') && !rest.is_empty() {
                        if let Some(val) = scope.dirty.get(rest) {
                            return Some(TypedSlot::from_value(val.clone()));
                        }
                        if let Some(&idx) = scope.class_def.field_index.get(rest) {
                            return scope.field_cache.get(idx).cloned();
                        }
                    }
                }
            }
        }

        // 2. Typed store: scalar variables shadowed for zero-JSON reads (works for all scopes)
        self.typed_store.read().get(path).cloned()
    }

    /// Resolve path within method scope (lock-free field_cache read + dirty-first priority)
    fn resolve_in_method_scope(&self, scope: &MethodScope, path: &str) -> Option<Value> {
        if !path.contains('.') {
            // $self -> materialize current instance (including dirty fields)
            if path == "self" {
                return self
                    .instance_arena
                    .materialize_with_dirty(scope.instance_id, &scope.dirty);
            }
            // Check dirty first (values modified within method body)
            if let Some(val) = scope.dirty.get(path) {
                return Some(val.clone());
            }
            // Then check if it's a field name -> lock-free read from field_cache
            if let Some(&idx) = scope.class_def.field_index.get(path) {
                return scope.field_cache.get(idx).map(|s| s.to_value());
            }
            // Not a field -> return None, fall through to JSON tree
            return None;
        }

        // Multi-segment path
        let mut segments = path.splitn(2, '.');
        let first = segments.next().unwrap();
        let rest = segments.next().unwrap_or("");

        // $self.field or $self.field.nested
        if first == "self" {
            return self.resolve_self_field(scope, rest);
        }

        // $field.nested or $local_var.nested (dirty or class field)
        let root_val = if let Some(val) = scope.dirty.get(first) {
            Some(val.clone())
        } else if let Some(&idx) = scope.class_def.field_index.get(first) {
            scope.field_cache.get(idx).map(|s| s.to_value())
        } else {
            None
        };
        if let Some(root) = root_val {
            let registry_guard = self.class_registry.read();
            let registry = Some(&*registry_guard);
            if !rest.contains('.') {
                return Some(resolve_field(&root, rest, registry));
            }
            let mut current = root;
            for part in rest.split('.') {
                current = resolve_field(&current, part, registry);
                if current.is_null() {
                    return None;
                }
            }
            return Some(current);
        }

        None // Not a method scope path -> fall through
    }

    /// Resolve $self.field[.nested] path (lock-free field_cache read)
    fn resolve_self_field(&self, scope: &MethodScope, rest: &str) -> Option<Value> {
        if rest.is_empty() {
            // $self -> materialize
            return self
                .instance_arena
                .materialize_with_dirty(scope.instance_id, &scope.dirty);
        }

        let mut segments = rest.splitn(2, '.');
        let field_name = segments.next().unwrap();
        let further = segments.next();

        // Get field value (dirty-first, otherwise lock-free read from field_cache)
        let field_val = if let Some(val) = scope.dirty.get(field_name) {
            val.clone()
        } else if let Some(&idx) = scope.class_def.field_index.get(field_name) {
            scope
                .field_cache
                .get(idx)
                .map(|s| s.to_value())
                .unwrap_or(Value::Null)
        } else {
            return None;
        };

        // No further navigation -> return directly
        if further.is_none() {
            return Some(field_val);
        }

        // Continue navigating nested path
        let further = further.unwrap();
        let registry_guard = self.class_registry.read();
        let registry = Some(&*registry_guard);
        if !further.contains('.') {
            return Some(resolve_field(&field_val, further, registry));
        }
        let mut current = field_val;
        for part in further.split('.') {
            current = resolve_field(&current, part, registry);
            if current.is_null() {
                return None;
            }
        }
        Some(current)
    }

    /// Resolve instance field path from arena
    fn resolve_arena_field(&self, id: InstanceId, rest: &str) -> Option<Value> {
        if rest.is_empty() {
            return self.instance_arena.materialize(id);
        }

        let class_def = self.instance_arena.class_def(id)?;

        let mut segments = rest.splitn(2, '.');
        let field_name = segments.next().unwrap();
        let further = segments.next();

        // Look up field
        let field_val = if let Some(&idx) = class_def.field_index.get(field_name) {
            self.instance_arena.get_field(id, idx)?
        } else if field_name == "__class__" {
            Value::String(self.instance_arena.class_name(id)?)
        } else {
            return None;
        };

        if further.is_none() {
            return Some(field_val);
        }

        // Continue navigating
        let further = further.unwrap();
        let registry_guard = self.class_registry.read();
        let registry = Some(&*registry_guard);
        if !further.contains('.') {
            return Some(resolve_field(&field_val, further, registry));
        }
        let mut current = field_val;
        for part in further.split('.') {
            current = resolve_field(&current, part, registry);
            if current.is_null() {
                return None;
            }
        }
        Some(current)
    }

    /// Returns a snapshot of the context as a serde_json::Value.
    pub fn get_as_value(&self) -> Result<Value> {
        Ok(self.data.read().clone())
    }

    /// Chainable value access via dot-notation path.
    pub fn get_jvalue(&self, path: &str) -> JValue {
        JValue::from(self.resolve_path(path).ok().flatten())
    }

    /// Shortcut: get a String value at path.
    pub fn get_str(&self, path: &str) -> Option<String> {
        self.get_jvalue(path).string()
    }

    /// Shortcut: get an i64 value at path.
    #[allow(dead_code)]
    pub fn get_i64(&self, path: &str) -> Option<i64> {
        self.get_jvalue(path).i64()
    }

    /// Shortcut: get an f64 value at path.
    #[allow(dead_code)]
    pub fn get_f64(&self, path: &str) -> Option<f64> {
        self.get_jvalue(path).f64()
    }
}

/// Resolve a single field on a Value, with class instance `__fields__` awareness.
/// Uses class_registry to look up field index by class name (no __field_index__ in instance).
fn resolve_field(
    val: &Value,
    field: &str,
    class_registry: Option<&HashMap<String, Arc<ClassDef>>>,
) -> Value {
    match val {
        Value::Object(map) => {
            // Safety net: raw arena proxy (no __class__) — should be resolved by resolve_path
            if map.contains_key("__arena_ref__") && !map.contains_key("__class__") {
                return Value::Null;
            }
            // Fast path: class instance with __class__ + __fields__
            if let (Some(Value::String(class_name)), Some(fields_arr)) =
                (map.get("__class__"), map.get("__fields__"))
            {
                if let Some(arr) = fields_arr.as_array() {
                    // Look up field index from class registry
                    if let Some(registry) = class_registry {
                        if let Some(class_def) = registry.get(class_name.as_str()) {
                            if let Some(&idx) = class_def.field_index.get(field) {
                                return arr.get(idx).cloned().unwrap_or(Value::Null);
                            }
                        }
                    }
                    // Fallback: check __field_index__ in instance (backward compat)
                    if let Some(index_map) = map.get("__field_index__") {
                        if let Some(idx_obj) = index_map.as_object() {
                            if let Some(idx_val) = idx_obj.get(field) {
                                if let Some(idx) = idx_val.as_u64() {
                                    return arr.get(idx as usize).cloned().unwrap_or(Value::Null);
                                }
                            }
                        }
                    }
                    // Allow direct access to __class__ etc.
                    if field.starts_with("__") {
                        return map.get(field).cloned().unwrap_or(Value::Null);
                    }
                    return Value::Null;
                }
            }
            // Normal object
            map.get(field).cloned().unwrap_or(Value::Null)
        }
        Value::Array(arr) => {
            // Numeric index: array.0, array.1, etc.
            if let Ok(idx) = field.parse::<usize>() {
                arr.get(idx).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        Value::String(s) => {
            // JSON-encoded string: try parsing and navigating
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                resolve_field(&parsed, field, class_registry)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}
