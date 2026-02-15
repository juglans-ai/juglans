// src/services/mod.rs
pub mod agent_loader;
pub mod interface;
pub mod prompt_loader;
pub mod tool_registry;

#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod github;
#[cfg(not(target_arch = "wasm32"))]
pub mod jug0;
#[cfg(not(target_arch = "wasm32"))]
pub mod mcp;
#[cfg(not(target_arch = "wasm32"))]
pub mod web_server; // 【新增】
