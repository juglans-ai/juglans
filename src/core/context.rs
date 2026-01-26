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
}

impl WorkflowContext {
    /// Creates a new, empty context, initialized as a JSON object.
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(json!({}))),
            event_sender: None,
        }
    }

    /// 【新增】创建带信道的上下文
    pub fn with_sender(sender: UnboundedSender<WorkflowEvent>) -> Self {
        Self {
            data: Arc::new(Mutex::new(json!({}))),
            event_sender: Some(sender),
        }
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
