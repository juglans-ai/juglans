// src/builtins/system.rs
use super::Tool;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use async_trait::async_trait;
use crate::core::context::WorkflowContext;

pub struct Timer;
#[async_trait]
impl Tool for Timer {
    fn name(&self) -> &str { "timer" }
    async fn execute(&self, params: &HashMap<String, String>, _context: &WorkflowContext) -> Result<Option<Value>> {
        let seconds: u64 = params.get("seconds").and_then(|s| s.parse().ok()).unwrap_or(1);
        println!("⏳ Sleeping for {} seconds...", seconds);
        tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
        Ok(Some(json!({ "status": "finished", "duration": seconds })))
    }
}

pub struct SetContext;
#[async_trait]
impl Tool for SetContext {
    fn name(&self) -> &str { "set_context" }
    async fn execute(&self, params: &HashMap<String, String>, context: &WorkflowContext) -> Result<Option<Value>> {
        let path = params.get("path").ok_or_else(|| anyhow!("Missing path"))?;
        let value_str = params.get("value").ok_or_else(|| anyhow!("Missing value"))?;
        let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
        let stripped_path = path.strip_prefix("$ctx.").unwrap_or(path).trim_matches('"');
        context.set(stripped_path.to_string(), value)?;
        Ok(None)
    }
}

pub struct Notify;
#[async_trait]
impl Tool for Notify {
    fn name(&self) -> &str { "notify" }
    async fn execute(&self, params: &HashMap<String, String>, context: &WorkflowContext) -> Result<Option<Value>> {
        // 如果传入 status，则更新 ctx.reply.status，实现透明思维流
        if let Some(status) = params.get("status") {
            context.set("reply.status".to_string(), json!(status))?;
            println!("💡 [Status] {}", status);
        }

        let msg = params.get("message").map(|s| s.as_str()).unwrap_or("");
        if !msg.is_empty() {
            println!("🔔 [Notification] {}", msg);
        }
        
        Ok(Some(json!({ "status": "sent", "content": msg })))
    }
}