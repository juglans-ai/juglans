// src/runtime/mod.rs
//
// Runtime modules for external language/tool execution

pub mod python;

pub use python::{get_python_ref, is_python_ref, PythonRuntime, PythonWorkerPool};
