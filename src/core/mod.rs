// src/core/mod.rs
pub mod context;
pub mod expr_ast;
pub mod expr_eval;
pub mod expr_parser;
pub mod graph;
pub mod instance_arena;
pub mod jvalue;
pub mod jwl_lexer;
pub mod jwl_parser;
pub mod jwl_token;
pub mod manifest_parser;
pub mod parser;
pub mod prompt_parser;
pub mod renderer;
pub mod resolver;
pub mod skill_parser;
pub mod stdlib;
pub mod tool_loader;
pub mod type_checker;
pub mod types;
pub mod validator;

// Critical: executor involves heavy I/O and multithreading, must not compile for WASM target
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
