// src/core/context.rs
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

/// 工作流执行过程中的实时事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "content")]
pub enum WorkflowEvent {
    #[serde(rename = "token")]
    Token(String),
    #[serde(rename = "status")]
    Status(String),
    #[serde(rename = "error")]
    Error(String),
}

/// A thread-safe, shared state for a single workflow execution.
#[derive(Debug, Clone)]
pub struct WorkflowContext {
    data: Arc<Mutex<Value>>,
    /// 【新增】用于流式输出的信道
    event_sender: Option<UnboundedSender<WorkflowEvent>>,
    /// 【新增】执行栈追踪，用于防止无限递归
    /// 格式：["agent_slug:workflow_name", ...]
    execution_stack: Arc<Mutex<Vec<String>>>,
    /// 【新增】最大嵌套深度
    max_depth: usize,
}

impl WorkflowContext {
    /// Creates a new, empty context, initialized as a JSON object.
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(json!({}))),
            event_sender: None,
            execution_stack: Arc::new(Mutex::new(Vec::new())),
            max_depth: 10, // 默认最大深度 10 层
        }
    }

    /// 【新增】创建带信道的上下文
    pub fn with_sender(sender: UnboundedSender<WorkflowEvent>) -> Self {
        Self {
            data: Arc::new(Mutex::new(json!({}))),
            event_sender: Some(sender),
            execution_stack: Arc::new(Mutex::new(Vec::new())),
            max_depth: 10,
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
    pub fn get_execution_stack(&self) -> Result<Vec<String>> {
        let stack = self
            .execution_stack
            .lock()
            .map_err(|_| anyhow!("Failed to acquire execution stack lock"))?;
        Ok(stack.clone())
    }

    /// 【新增】获取当前嵌套深度
    pub fn get_execution_depth(&self) -> Result<usize> {
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
            .lock()
            .map_err(|_| anyhow!("Failed to acquire context lock"))?;

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
            .lock()
            .map_err(|_| anyhow!("Failed to acquire context lock"))?;

        let pointer = format!("/{}", parts.join("/"));
        Ok(data.pointer(&pointer).cloned())
    }

    /// Returns a snapshot of the context as a serde_json::Value.
    pub fn get_as_value(&self) -> Result<Value> {
        let data = self
            .data
            .lock()
            .map_err(|_| anyhow!("Failed to acquire context lock"))?;
        Ok(data.clone())
    }
}
