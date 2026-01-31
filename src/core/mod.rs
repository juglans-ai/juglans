// src/core/mod.rs
pub mod agent_parser;
pub mod context;
pub mod graph;
pub mod parser;
pub mod prompt_parser;
pub mod renderer;
pub mod tool_loader;
pub mod validator;

// 【关键修复】执行器涉及大量 I/O 和多线程，禁止在 WASM 目标下编译
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
