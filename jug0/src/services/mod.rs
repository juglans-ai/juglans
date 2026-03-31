// src/services/mod.rs
pub mod cache;
pub mod memory;
pub mod qdrant;
pub mod search;

#[cfg(feature = "server")]
pub mod deploy;
#[cfg(feature = "server")]
pub mod mcp;
#[cfg(feature = "server")]
pub mod models;
#[cfg(feature = "server")]
pub mod quota;
// NOTE: scheduler is declared in main.rs (depends on AppState which is binary-only)
