// src/core/mod.rs
pub mod agent_parser;
pub mod context;
pub mod expr_ast;
pub mod expr_eval;
pub mod graph;
pub mod jvalue;
pub mod jwl_lexer;
pub mod jwl_parser;
pub mod jwl_token;
pub mod parser;
pub mod prompt_parser;
pub mod renderer;
pub mod resolver;
pub mod skill_parser;
pub mod tool_loader;
pub mod validator;

// 【关键修复】执行器涉及大量 I/O 和多线程，禁止在 WASM 目标下编译
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
