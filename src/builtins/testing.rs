// src/builtins/testing.rs
//
// Builtin tools for the `juglans test` framework.
// - `config`: Stores test configuration into context

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

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
