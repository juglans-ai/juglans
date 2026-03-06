// src/core/context.rs
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use crate::core::graph::WorkflowGraph;
use crate::core::jvalue::JValue;

/// Type alias for pending tool start info: (tool_name, params, start_time)
type PendingToolStarts = Arc<Mutex<HashMap<String, (String, HashMap<String, String>, Instant)>>>;

/// Client tool 执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub content: String,
}

/// Tool 执行 trace 条目
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
    pub params: HashMap<String, String>,
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

/// 工作流执行过程中的实时事件
pub enum WorkflowEvent {
    Token(String),
    Status(String),
    Error(String),
    /// Meta 信息 — 转发 jug0 的 meta 事件到前端（chat_id, user_message_id 等）
    Meta(Value),
    /// Client tool call — 发给前端执行，通过 result_tx 等待结果返回
    ToolCall {
        call_id: String,
        tools: Vec<Value>,
        result_tx: oneshot::Sender<Vec<ToolResultPayload>>,
    },
    /// Tool execution start event (AI sub-tool calls)
    ToolStart(ToolStartEvent),
    /// Tool execution complete event (AI sub-tool calls)
    ToolComplete(ToolCompleteEvent),
    /// Workflow node execution start event
    NodeStart(NodeStartEvent),
    /// Workflow node execution complete event
    NodeComplete(NodeCompleteEvent),
}

/// A thread-safe, shared state for a single workflow execution.
#[derive(Debug, Clone)]
pub struct WorkflowContext {
    data: Arc<RwLock<Value>>,
    /// 【新增】用于流式输出的信道
    event_sender: Option<UnboundedSender<WorkflowEvent>>,
    /// 【新增】执行栈追踪，用于防止无限递归
    /// 格式：["agent_slug:workflow_name", ...]
    execution_stack: Arc<Mutex<Vec<String>>>,
    /// 【新增】最大嵌套深度
    max_depth: usize,
    /// 【新增】当前执行的 workflow（供 on_tool=[node] handler 使用）
    current_workflow: Arc<RwLock<Option<Arc<WorkflowGraph>>>>,
    /// 【新增】是否向前端推送 tool 执行事件（默认 false）
    stream_tool_events: Arc<AtomicBool>,
    /// 是否向前端推送 node 执行事件（默认 false）
    stream_node_events: Arc<AtomicBool>,
    /// Tool 执行 trace（记录所有 tool 调用的结果，供 assert 查询）
    tool_trace: Arc<Mutex<Vec<ToolTraceEntry>>>,
    /// Pending tool starts（tool_start 时记录时间，tool_complete 时计算 duration）
    pending_tool_starts: PendingToolStarts,
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
            event_sender: None,
            execution_stack: Arc::new(Mutex::new(Vec::new())),
            max_depth: 10,
            current_workflow: Arc::new(RwLock::new(None)),
            stream_tool_events: Arc::new(AtomicBool::new(false)),
            stream_node_events: Arc::new(AtomicBool::new(false)),
            tool_trace: Arc::new(Mutex::new(Vec::new())),
            pending_tool_starts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 设置最大嵌套深度
    pub fn _set_max_depth(&mut self, max_depth: usize) {
        self.max_depth = max_depth;
    }

    /// 【新增】创建带信道的上下文
    pub fn with_sender(sender: UnboundedSender<WorkflowEvent>) -> Self {
        Self {
            data: Arc::new(RwLock::new(json!({}))),
            event_sender: Some(sender),
            execution_stack: Arc::new(Mutex::new(Vec::new())),
            max_depth: 10,
            current_workflow: Arc::new(RwLock::new(None)),
            stream_tool_events: Arc::new(AtomicBool::new(false)),
            stream_node_events: Arc::new(AtomicBool::new(false)),
            tool_trace: Arc::new(Mutex::new(Vec::new())),
            pending_tool_starts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 【新增】进入嵌套执行（push 到栈）
    /// 返回 Err 如果检测到递归或超过最大深度
    pub fn enter_execution(&self, identifier: String) -> Result<()> {
        let mut stack = self
            .execution_stack
            .lock()
            .map_err(|_| anyhow!("Failed to acquire execution stack lock"))?;

        // 检查深度限制
        if stack.len() >= self.max_depth {
            return Err(anyhow!(
                "Maximum execution depth ({}) exceeded. Current stack: {:?}",
                self.max_depth,
                stack
            ));
        }

        // 检查循环引用
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

    /// 【新增】退出嵌套执行（pop 栈）
    pub fn exit_execution(&self) -> Result<()> {
        let mut stack = self
            .execution_stack
            .lock()
            .map_err(|_| anyhow!("Failed to acquire execution stack lock"))?;

        if stack.is_empty() {
            return Err(anyhow!("Execution stack is already empty"));
        }

        stack.pop();
        Ok(())
    }

    /// 【新增】获取当前执行栈（用于调试）
    pub fn _get_execution_stack(&self) -> Result<Vec<String>> {
        let stack = self
            .execution_stack
            .lock()
            .map_err(|_| anyhow!("Failed to acquire execution stack lock"))?;
        Ok(stack.clone())
    }

    /// 【新增】获取当前嵌套深度
    pub fn _get_execution_depth(&self) -> Result<usize> {
        let stack = self
            .execution_stack
            .lock()
            .map_err(|_| anyhow!("Failed to acquire execution stack lock"))?;
        Ok(stack.len())
    }

    /// 【新增】发送事件
    pub fn emit(&self, event: WorkflowEvent) {
        if let Some(sender) = &self.event_sender {
            let _ = sender.send(event);
        }
    }

    /// 发送 client tool call 并等待前端返回结果
    pub async fn emit_tool_call_and_wait(
        &self,
        call_id: String,
        tools: Vec<Value>,
        timeout_secs: u64,
    ) -> Result<Vec<ToolResultPayload>> {
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

    /// 【新增】获取 Token 专用 Sender 的适配器
    /// 这个方法会将 Runtime 需要的 String 类型转化为 Context 需要的 WorkflowEvent 类型
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

    /// 获取 Meta 专用 Sender 的适配器
    /// 将 Value 类型转化为 WorkflowEvent::Meta 类型
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

    /// 设置当前执行的 workflow（供 on_tool=[node] handler 读取）
    pub fn set_current_workflow(&self, workflow: Arc<WorkflowGraph>) {
        if let Ok(mut guard) = self.current_workflow.write() {
            *guard = Some(workflow);
        }
    }

    /// 获取当前执行的 workflow
    pub fn get_current_workflow(&self) -> Option<Arc<WorkflowGraph>> {
        self.current_workflow.read().ok()?.clone()
    }

    /// 设置是否推送 tool 执行事件
    pub fn set_stream_tool_events(&self, enabled: bool) {
        self.stream_tool_events.store(enabled, Ordering::Relaxed);
    }

    /// 设置是否推送 node 执行事件
    pub fn set_stream_node_events(&self, enabled: bool) {
        self.stream_node_events.store(enabled, Ordering::Relaxed);
    }

    /// 发送 tool_start 事件（同时记录到 trace）
    pub fn emit_tool_start(&self, node_id: &str, tool: &str, params: &HashMap<String, String>) {
        // 记录开始时间到 pending（用 node_id 作为 key）
        if let Ok(mut pending) = self.pending_tool_starts.lock() {
            pending.insert(
                node_id.to_string(),
                (tool.to_string(), params.clone(), Instant::now()),
            );
        }

        if !self.stream_tool_events.load(Ordering::Relaxed) {
            return;
        }
        self.emit(WorkflowEvent::ToolStart(ToolStartEvent {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            params: params.clone(),
        }));
    }

    /// 发送 tool_complete 事件（同时写入 trace）
    pub fn emit_tool_complete(&self, node_id: &str, tool: &str, result: &Result<Option<Value>>) {
        // 从 pending 中取出开始时间，计算 duration，写入 trace
        let start_info = self
            .pending_tool_starts
            .lock()
            .ok()
            .and_then(|mut p| p.remove(node_id));

        let (params, duration) = match start_info {
            Some((_, params, started)) => (params, started.elapsed()),
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

        if let Ok(mut trace) = self.tool_trace.lock() {
            trace.push(entry);
        }

        // Stream event to frontend if enabled
        if !self.stream_tool_events.load(Ordering::Relaxed) {
            return;
        }
        match result {
            Ok(val) => self.emit(WorkflowEvent::ToolComplete(ToolCompleteEvent {
                node_id: node_id.to_string(),
                tool: tool.to_string(),
                status: "success".to_string(),
                result: val.clone(),
                error: None,
            })),
            Err(e) => self.emit(WorkflowEvent::ToolComplete(ToolCompleteEvent {
                node_id: node_id.to_string(),
                tool: tool.to_string(),
                status: "error".to_string(),
                result: None,
                error: Some(e.to_string()),
            })),
        }
    }

    /// 发送 node_start 事件（workflow 节点开始执行，同时记录到 trace）
    pub fn emit_node_start(&self, node_id: &str, tool: &str, params: &HashMap<String, String>) {
        // 记录开始时间到 pending（用 node_id 作为 key）
        if let Ok(mut pending) = self.pending_tool_starts.lock() {
            pending.insert(
                node_id.to_string(),
                (tool.to_string(), params.clone(), Instant::now()),
            );
        }

        if !self.stream_node_events.load(Ordering::Relaxed) {
            return;
        }
        self.emit(WorkflowEvent::NodeStart(NodeStartEvent {
            node_id: node_id.to_string(),
            tool: tool.to_string(),
            params: params.clone(),
        }));
    }

    /// 发送 node_complete 事件（workflow 节点执行完成，同时写入 trace）
    pub fn emit_node_complete(&self, node_id: &str, tool: &str, result: &Result<Option<Value>>) {
        // 从 pending 中取出开始时间，计算 duration，写入 trace
        let start_info = self
            .pending_tool_starts
            .lock()
            .ok()
            .and_then(|mut p| p.remove(node_id));

        let (params, duration) = match start_info {
            Some((_, params, started)) => (params, started.elapsed()),
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

        if let Ok(mut trace) = self.tool_trace.lock() {
            trace.push(entry);
        }

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

    /// 获取所有 trace 条目
    pub fn trace_entries(&self) -> Vec<ToolTraceEntry> {
        self.tool_trace
            .lock()
            .map(|t| t.clone())
            .unwrap_or_default()
    }

    /// 查询指定 tool 的调用记录
    #[allow(dead_code)]
    pub fn trace_tool_called(&self, tool_name: &str) -> Vec<ToolTraceEntry> {
        self.trace_entries()
            .into_iter()
            .filter(|e| e.tool == tool_name)
            .collect()
    }

    /// 计算 trace 总耗时
    #[allow(dead_code)]
    pub fn trace_total_duration(&self) -> Duration {
        self.trace_entries().iter().map(|e| e.duration).sum()
    }

    /// 清空 trace（测试隔离用）
    pub fn _clear_trace(&self) {
        if let Ok(mut trace) = self.tool_trace.lock() {
            trace.clear();
        }
        if let Ok(mut pending) = self.pending_tool_starts.lock() {
            pending.clear();
        }
    }

    /// Sets a value in the context using a dot-notation path.
    pub fn set(&self, path: String, value: Value) -> Result<()> {
        // 如果更新了 reply.status，自动同步 emit 状态事件
        if path == "reply.status" {
            if let Some(s) = value.as_str() {
                self.emit(WorkflowEvent::Status(s.to_string()));
            }
        }

        let mut data = self
            .data
            .write()
            .map_err(|_| anyhow!("Failed to acquire context write lock"))?;

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
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Ok(None);
        }

        let data = self
            .data
            .read()
            .map_err(|_| anyhow!("Failed to acquire context read lock"))?;

        // 快速路径：直接 JSON pointer
        let pointer = format!("/{}", parts.join("/"));
        if let Some(v) = data.pointer(&pointer) {
            return Ok(Some(v.clone()));
        }

        // 慢速路径：逐段导航，遇到 JSON 字符串自动解析
        let mut current: Value = match data.get(parts[0]) {
            Some(v) => v.clone(),
            None => return Ok(None),
        };
        for part in parts.iter().skip(1) {
            current = match current {
                Value::Object(map) => match map.get(*part) {
                    Some(v) => v.clone(),
                    None => return Ok(None),
                },
                Value::String(ref s) => {
                    let parsed: Value = match serde_json::from_str(s) {
                        Ok(v) => v,
                        Err(_) => return Ok(None),
                    };
                    match parsed.get(*part) {
                        Some(v) => v.clone(),
                        None => return Ok(None),
                    }
                }
                _ => return Ok(None),
            };
        }
        Ok(Some(current))
    }

    /// Returns a snapshot of the context as a serde_json::Value.
    pub fn get_as_value(&self) -> Result<Value> {
        let data = self
            .data
            .read()
            .map_err(|_| anyhow!("Failed to acquire context read lock"))?;
        Ok(data.clone())
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
