// src/builtins/system.rs
use super::Tool;
use crate::core::context::{WorkflowContext, WorkflowEvent};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Parse a string into a context value, preserving large integers as strings to avoid f64 precision loss.
fn parse_context_value(value_str: &str) -> Value {
    match serde_json::from_str::<Value>(value_str) {
        Ok(Value::Number(n))
            if n.as_f64()
                .map(|f| f.abs() > 9_007_199_254_740_992.0)
                .unwrap_or(false)
                && value_str.bytes().all(|b| b.is_ascii_digit() || b == b'-') =>
        {
            // Large integer exceeding f64 precision (e.g. Google/Apple user ID), keep as string
            json!(value_str)
        }
        Ok(v) => v,
        Err(_) => json!(value_str),
    }
}

pub struct Timer;
#[async_trait]
impl Tool for Timer {
    fn name(&self) -> &str {
        "timer"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Support both 'ms' (preferred) and 'seconds' (backward compatible)
        let duration_ms: u64 = if let Some(ms) = params.get("ms") {
            ms.parse().unwrap_or(1000)
        } else if let Some(secs) = params.get("seconds") {
            secs.parse::<u64>().unwrap_or(1) * 1000
        } else {
            1000 // default 1 second
        };

        if !context.has_event_sender() {
            println!("⏳ Sleeping for {} ms...", duration_ms);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
        Ok(Some(
            json!({ "status": "finished", "duration_ms": duration_ms }),
        ))
    }
}

pub struct SetContext;
#[async_trait]
impl Tool for SetContext {
    fn name(&self) -> &str {
        "set_context"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Supports two modes:
        // 1. Legacy mode: set_context(path="key", value="val")
        // 2. Multi-field mode: set_context(key1=$input.val1, key2=$input.val2)

        let mut last_value: Option<Value> = None;

        if let (Some(path), Some(value_str)) = (params.get("path"), params.get("value")) {
            // Legacy mode
            let value = parse_context_value(value_str);
            let stripped_path = path.strip_prefix("$ctx.").unwrap_or(path).trim_matches('"');
            context.set(stripped_path.to_string(), value.clone())?;
            last_value = Some(value);
        } else {
            // Multi-field mode: set each key=value pair into ctx
            for (key, value_str) in params {
                // Skip reserved fields
                if key == "path" || key == "value" {
                    continue;
                }
                let value = parse_context_value(value_str);
                context.set(key.clone(), value.clone())?;
                last_value = Some(value);
            }
        }
        Ok(last_value)
    }
}

pub struct Notify;
#[async_trait]
impl Tool for Notify {
    fn name(&self) -> &str {
        "notify"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // If status is provided, update ctx.reply.status for transparent thinking stream
        if let Some(status) = params.get("status") {
            context.set("reply.status".to_string(), json!(status))?;
            if !context.has_event_sender() {
                println!("💡 [Status] {}", status);
            }
        }

        let msg = params.get("message").map(|s| s.as_str()).unwrap_or("");
        if !msg.is_empty() && !context.has_event_sender() {
            println!("🔔 [Notification] {}", msg);
        }

        Ok(Some(json!({ "status": "sent", "content": msg })))
    }
}

/// print(message="text") — plain output, no emoji prefix, does not modify context
/// Unlike notify, print only does println, suitable for debugging and Hello World
pub struct Print;
#[async_trait]
impl Tool for Print {
    fn name(&self) -> &str {
        "print"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let msg = params
            .get("message")
            .or_else(|| params.get("value"))
            .map(|s| s.as_str())
            .unwrap_or("");
        if !context.has_event_sender() {
            println!("{}", msg);
        }
        Ok(Some(json!(msg)))
    }
}

/// reply(message="content", state="context_visible") - return content directly without calling AI
/// Used for system event handling where fixed text is needed without going through the LLM
/// Supports state parameter for SSE/persistence control, including compound syntax input:output
pub struct Reply;

impl Reply {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Reply {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for Reply {
    fn name(&self) -> &str {
        "reply"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let message = params.get("message").map(|s| s.as_str()).unwrap_or("");

        // Support compound syntax input:output (consistent with chat())
        let state_raw = params
            .get("state")
            .map(|s| s.as_str())
            .unwrap_or("context_visible");
        let (input_state, output_state) = match state_raw.split_once(':') {
            Some((i, o)) => (i, o),
            None => (state_raw, state_raw),
        };

        // should_stream based on output_state
        let should_stream = output_state == "context_visible" || output_state == "display_only";

        // SSE output
        if should_stream {
            context.emit(WorkflowEvent::Token(message.to_string()));
        }

        // (Persistence to a backend was previously done here via runtime.create_message /
        // update_message_state. Removed when juglans became local-first; reply state is
        // now purely in-memory within the workflow context.)
        let _ = (input_state, output_state);

        // Update reply.output (consistent with chat())
        let current = context
            .resolve_path("reply.output")
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        context.set(
            "reply.output".to_string(),
            json!(format!("{}{}", current, message)),
        )?;

        Ok(Some(json!({
            "content": message,
            "status": "sent"
        })))
    }
}

/// return(value=expr) — Return the evaluated expression result as $output
/// Used in function definitions to return computed results: `[add(a, b)]: return(value=$ctx.a + $ctx.b)`
pub struct Return;
#[async_trait]
impl Tool for Return {
    fn name(&self) -> &str {
        "return"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        if let Some(value_str) = params.get("value") {
            let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
            Ok(Some(value))
        } else if let Some((_key, value_str)) = params.iter().next() {
            let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
            Ok(Some(value))
        } else {
            Ok(Some(Value::Null))
        }
    }
}

/// call(fn="function_name", ...args) — Dynamic function dispatch by string name
/// Looks up a function defined in the current workflow and executes it with the provided arguments.
/// The `fn` parameter specifies the function name; all other parameters are passed as arguments.
pub struct Call {
    builtin_registry: Option<std::sync::Weak<super::BuiltinRegistry>>,
}

impl Default for Call {
    fn default() -> Self {
        Self::new()
    }
}

impl Call {
    pub fn new() -> Self {
        Self {
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: std::sync::Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }
}

#[async_trait]
impl Tool for Call {
    fn name(&self) -> &str {
        "call"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let fn_name = params
            .get("fn")
            .ok_or_else(|| anyhow!("call() requires 'fn' parameter (function name)"))?;

        // Get executor via builtin registry
        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("call(): BuiltinRegistry not available"))?;

        let executor = registry
            .get_executor()
            .ok_or_else(|| anyhow!("call(): WorkflowExecutor not available"))?;

        // Get workflow: prefer root workflow (where functions are defined),
        // fall back to current workflow for nested contexts
        let workflow = context
            .get_root_workflow()
            .or_else(|| context.get_current_workflow())
            .ok_or_else(|| anyhow!("call(): no active workflow"))?;

        // Collect remaining params as function args (exclude "fn")
        let args: HashMap<String, Value> = params
            .iter()
            .filter(|(k, _)| k.as_str() != "fn")
            .map(|(k, v)| {
                let val = serde_json::from_str(v).unwrap_or(json!(v));
                (k.clone(), val)
            })
            .collect();

        executor
            .execute_function(fn_name.clone(), args, workflow, context)
            .await
    }
}

// Shell has been replaced by devtools::Bash (registered as "bash" + "sh" alias)
