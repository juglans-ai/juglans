// src/lib.rs

// ============================================================================
// 模块定义 (Modules)
// ============================================================================

pub mod core;

#[cfg(not(target_arch = "wasm32"))]
pub mod services;

#[cfg(not(target_arch = "wasm32"))]
pub mod builtins;

#[cfg(not(target_arch = "wasm32"))]
pub mod templates;

// ============================================================================
// 公共导出 (Public Exports)
// ============================================================================

pub use core::agent_parser::{AgentParser, AgentResource};
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
pub use services::agent_loader::AgentRegistry;

#[cfg(not(target_arch = "wasm32"))]
pub use services::config::JuglansConfig;

#[cfg(not(target_arch = "wasm32"))]
pub use services::mcp::McpClient;

// ============================================================================
// WASM 专用接口 (仅在 wasm32 目标时编译)
// ============================================================================

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct JuglansEngine;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl JuglansEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    pub fn parse_prompt(&self, content: &str) -> Result<JsValue, JsValue> {
        let resource = crate::core::prompt_parser::PromptParser::parse(content)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        serde_wasm_bindgen::to_value(&resource).map_err(|e| JsValue::from_str(&e.to_string()))
    }

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
}
