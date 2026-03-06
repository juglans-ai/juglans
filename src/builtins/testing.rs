// src/builtins/testing.rs
//
// Builtin tools for the `juglans test` framework.
// - `config`: Stores test configuration into context
// - `mock`: Execute a workflow with injected node outputs

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Weak;
use tracing::info;

use super::Tool;
use crate::core::context::WorkflowContext;

/// `config` builtin — stores test configuration into context.
///
/// Parameters are stored as-is into the `_test_config` namespace in context.
/// Known keys: agent, budget, mock, timeout
pub struct Config;

#[async_trait]
impl Tool for Config {
    fn name(&self) -> &str {
        "config"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Return params as JSON value — the test runner reads this
        Ok(Some(serde_json::to_value(params)?))
    }
}

/// `mock` builtin — execute a workflow with injected node outputs.
///
/// Usage: mock(workflow="main.jg", inject={"node_id": value, ...})
///
/// Nodes listed in `inject` are skipped during execution and their output
/// is set to the injected value. All other nodes execute normally.
pub struct Mock {
    builtin_registry: Option<Weak<super::BuiltinRegistry>>,
}

impl Mock {
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
impl Tool for Mock {
    fn name(&self) -> &str {
        "mock"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let workflow_path = params
            .get("workflow")
            .ok_or_else(|| anyhow!("mock: Missing 'workflow' parameter"))?;

        // Parse inject map: {"node_id": value, ...}
        if let Some(inject_str) = params.get("inject") {
            let inject_val: Value =
                serde_json::from_str(inject_str).unwrap_or(Value::String(inject_str.clone()));

            if let Value::Object(map) = inject_val {
                for (node_id, value) in map {
                    info!("  🎭 Mock inject [{}]", node_id);
                    context.set(format!("_mocks.{}", node_id), value)?;
                }
            } else {
                return Err(anyhow!("mock: 'inject' must be a JSON object"));
            }
        }

        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("mock: BuiltinRegistry not available"))?;

        let base_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let identifier = format!("mock:{}", workflow_path);

        info!("  🎭 mock: executing {}", workflow_path);
        let output = registry
            .execute_nested_workflow(workflow_path, &base_dir, context, identifier)
            .await?;
        Ok(Some(output))
    }
}
