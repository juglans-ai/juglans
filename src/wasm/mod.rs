// src/wasm/mod.rs — WASM-specific modules
pub mod bridge;
pub mod executor;
pub mod language;

pub use executor::WasmExecutor;
