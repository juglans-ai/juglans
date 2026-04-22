// src/services/mod.rs
pub mod prompt_loader;
pub mod tool_registry;

#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod deploy;
#[cfg(not(target_arch = "wasm32"))]
pub mod github;
#[cfg(not(target_arch = "wasm32"))]
pub mod history;
#[cfg(not(target_arch = "wasm32"))]
pub mod local_runtime;
#[cfg(not(target_arch = "wasm32"))]
pub mod web_server;
