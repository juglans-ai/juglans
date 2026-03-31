// src/lib.rs

// ============================================================================
// Module Definitions
// ============================================================================

pub mod core;

#[cfg(not(target_arch = "wasm32"))]
pub mod adapters;

#[cfg(not(target_arch = "wasm32"))]
pub mod services;

#[cfg(not(target_arch = "wasm32"))]
pub mod builtins;

#[cfg(not(target_arch = "wasm32"))]
pub mod templates;

#[cfg(not(target_arch = "wasm32"))]
pub mod ui;

#[cfg(not(target_arch = "wasm32"))]
pub mod registry;

#[cfg(not(target_arch = "wasm32"))]
pub mod runtime;

#[cfg(not(target_arch = "wasm32"))]
pub mod lsp;

#[cfg(not(target_arch = "wasm32"))]
pub mod testing;

#[cfg(not(target_arch = "wasm32"))]
pub mod doctest;

#[cfg(not(target_arch = "wasm32"))]
pub mod runner;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

// ============================================================================
// Public Exports
// ============================================================================

pub use core::context::WorkflowContext;
pub use core::graph::WorkflowGraph;
pub use core::parser::GraphParser;
pub use core::prompt_parser::{PromptParser, PromptResource};
pub use core::renderer::JwlRenderer;

#[cfg(not(target_arch = "wasm32"))]
pub use core::executor::WorkflowExecutor;

#[cfg(not(target_arch = "wasm32"))]
pub use services::interface::JuglansRuntime;

#[cfg(not(target_arch = "wasm32"))]
pub use services::jug0::{ChatOutput, Jug0Client};

#[cfg(not(target_arch = "wasm32"))]
pub use services::prompt_loader::PromptRegistry;

#[cfg(not(target_arch = "wasm32"))]
pub use services::config::JuglansConfig;

#[cfg(not(target_arch = "wasm32"))]
pub use runtime::python::{PythonRuntime, PythonWorkerPool};

// ============================================================================
// WASM-only Interface (compiled only for wasm32 target)
// ============================================================================

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use std::collections::{HashMap, HashSet};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct JuglansEngine {
    workflow: Option<crate::core::graph::WorkflowGraph>,
    files: HashMap<String, String>,
    tool_handler: Option<js_sys::Function>,
    tool_names: HashSet<String>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl JuglansEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            workflow: None,
            files: HashMap::new(),
            tool_handler: None,
            tool_names: HashSet::new(),
        }
    }

    /// Parse a .jg workflow source
    pub fn parse(&mut self, source: &str) -> Result<(), JsValue> {
        let wf = crate::core::parser::GraphParser::parse(source)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.workflow = Some(wf);
        Ok(())
    }

    /// Parse a .jgx template
    pub fn parse_prompt(&self, content: &str) -> Result<JsValue, JsValue> {
        let resource = crate::core::prompt_parser::PromptParser::parse(content)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&resource).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Render a .jgx template with context
    pub fn render_prompt(&self, content: &str, context_json: &str) -> Result<String, JsValue> {
        let resource = crate::core::prompt_parser::PromptParser::parse(content)
            .map_err(|e| JsValue::from_str(&format!("Parse Error: {}", e)))?;
        let context: serde_json::Value = serde_json::from_str(context_json)
            .map_err(|e| JsValue::from_str(&format!("JSON Error: {}", e)))?;
        let renderer = crate::core::renderer::JwlRenderer::new();
        renderer
            .render(&resource.ast, &context)
            .map_err(|e| JsValue::from_str(&format!("Render Error: {}", e)))
    }

    /// Register a file for flow/lib imports
    pub fn register_file(&mut self, path: &str, content: &str) {
        self.files.insert(path.to_string(), content.to_string());
    }

    /// Set the tool handler callback: (name: string, params: object) → Promise<result>
    pub fn set_tool_handler(&mut self, handler: js_sys::Function) {
        self.tool_handler = Some(handler);
    }

    /// Register known tool names (for expression disambiguation)
    pub fn set_tool_names(&mut self, names: JsValue) -> Result<(), JsValue> {
        let names_vec: Vec<String> = serde_wasm_bindgen::from_value(names)
            .map_err(|e| JsValue::from_str(&format!("Expected string array: {}", e)))?;
        self.tool_names = names_vec.into_iter().collect();
        Ok(())
    }

    /// Execute the parsed workflow with input data
    pub async fn run(&self, input: JsValue) -> Result<JsValue, JsValue> {
        let workflow = self
            .workflow
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No workflow parsed. Call parse() first."))?;

        let handler = self.tool_handler.as_ref().ok_or_else(|| {
            JsValue::from_str("No tool handler set. Call setToolHandler() first.")
        })?;

        let executor = crate::wasm::WasmExecutor::new(handler.clone(), self.tool_names.clone());
        let context = crate::core::context::WorkflowContext::new();

        // Set input data
        if !input.is_undefined() && !input.is_null() {
            let input_val: serde_json::Value = serde_wasm_bindgen::from_value(input)
                .map_err(|e| JsValue::from_str(&format!("Invalid input: {}", e)))?;
            if let Some(obj) = input_val.as_object() {
                for (key, val) in obj {
                    context
                        .set(format!("input.{}", key), val.clone())
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                }
            }
            context
                .set("input".to_string(), input_val)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
        }

        executor
            .execute_graph(workflow, &context)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Return the final context output
        let output = context
            .resolve_path("output")
            .ok()
            .flatten()
            .unwrap_or(serde_json::Value::Null);
        serde_wasm_bindgen::to_value(&output).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Validate the parsed workflow
    pub fn validate(&self) -> Result<JsValue, JsValue> {
        let workflow = self
            .workflow
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No workflow parsed"))?;
        let result = crate::core::validator::WorkflowValidator::validate(workflow);
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Evaluate a standalone expression (for debugging)
    pub fn eval_expr(&self, expr: &str, context_json: &str) -> Result<JsValue, JsValue> {
        let context: serde_json::Value = serde_json::from_str(context_json)
            .map_err(|e| JsValue::from_str(&format!("JSON Error: {}", e)))?;
        let evaluator = crate::core::expr_eval::ExprEvaluator::new();
        let resolver = |path: &str| -> Option<serde_json::Value> {
            let pointer = format!("/{}", path.replace('.', "/"));
            context.pointer(&pointer).cloned()
        };
        let result = evaluator
            .eval(expr, &resolver)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    // ================================================================
    // LSP Intelligence (for Monaco Editor)
    // ================================================================

    /// Compute diagnostics (parse errors + validation warnings)
    pub fn diagnostics(&self, source: &str) -> Result<JsValue, JsValue> {
        let result = crate::wasm::language::compute_diagnostics(source);
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get completions at cursor position
    pub fn completions(&self, source: &str, line: u32, col: u32) -> Result<JsValue, JsValue> {
        let result = crate::wasm::language::completions(source, line, col);
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get hover info at cursor position
    pub fn hover(&self, source: &str, line: u32, col: u32) -> Result<JsValue, JsValue> {
        let result = crate::wasm::language::hover(source, line, col);
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Go-to-definition at cursor position
    pub fn definition(&self, source: &str, line: u32, col: u32) -> Result<JsValue, JsValue> {
        let result = crate::wasm::language::goto_definition(source, line, col);
        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
