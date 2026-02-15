// src/runtime/python/mod.rs
//
// Python runtime for executing Python function calls from Juglans workflows

mod protocol;
mod worker;

pub use protocol::{PythonError, PythonRequest, PythonResponse};
pub use worker::{PythonWorker, PythonWorkerPool};

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tracing::{debug, info};

/// Global Python runtime instance
static PYTHON_RUNTIME: OnceLock<Arc<PythonRuntime>> = OnceLock::new();

/// Python runtime for executing external Python calls
pub struct PythonRuntime {
    pool: PythonWorkerPool,
    /// Imported modules (normalized names, e.g. "bill_utils" from "./bill_utils.py")
    imported_modules: Vec<String>,
    /// Map from normalized module name back to original import path
    /// e.g. "bill_utils" -> "./bill_utils.py"
    import_path_map: HashMap<String, String>,
}

impl PythonRuntime {
    /// Create a new Python runtime
    pub fn new(max_workers: usize) -> Result<Self> {
        let pool = PythonWorkerPool::new(max_workers)?;
        Ok(Self {
            pool,
            imported_modules: Vec::new(),
            import_path_map: HashMap::new(),
        })
    }

    /// Initialize the global Python runtime
    pub fn init_global(max_workers: usize) -> Result<Arc<PythonRuntime>> {
        let runtime = Arc::new(Self::new(max_workers)?);
        PYTHON_RUNTIME
            .set(Arc::clone(&runtime))
            .map_err(|_| anyhow!("Python runtime already initialized"))?;
        Ok(runtime)
    }

    /// Get the global Python runtime
    pub fn global() -> Option<Arc<PythonRuntime>> {
        PYTHON_RUNTIME.get().cloned()
    }

    /// Set the imported modules for this runtime
    /// Normalizes file paths (e.g. "./bill_utils.py" -> "bill_utils") for matching,
    /// while preserving original paths for the Python worker to resolve.
    pub fn set_imports(&mut self, imports: Vec<String>) {
        self.imported_modules = Vec::new();
        self.import_path_map = HashMap::new();
        for import in &imports {
            let module_name = if import.ends_with(".py") {
                import
                    .rsplit('/')
                    .next()
                    .map(|f| f.trim_end_matches(".py"))
                    .unwrap_or(import)
                    .to_string()
            } else {
                import.clone()
            };
            self.import_path_map
                .insert(module_name.clone(), import.clone());
            self.imported_modules.push(module_name);
        }
    }

    /// Check if a module is imported
    pub fn is_module_imported(&self, module: &str) -> bool {
        // Check if the module or any parent module is imported
        // e.g., if "sklearn.ensemble" is imported, "sklearn" is also available
        for imported in &self.imported_modules {
            if module == imported || module.starts_with(&format!("{}.", imported)) {
                return true;
            }
            // Also check if imported starts with module (importing parent includes children)
            if imported.starts_with(&format!("{}.", module)) || imported == module {
                return true;
            }
        }
        false
    }

    /// Execute a Python function call
    ///
    /// # Arguments
    /// * `call_path` - Full path like "pandas.read_csv" or "my_module.process"
    /// * `args` - Positional arguments
    /// * `kwargs` - Keyword arguments
    pub fn call(
        &self,
        call_path: &str,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> Result<Value> {
        // Split call_path into module and method
        // e.g., "pandas.read_csv" -> target="pandas", method="read_csv"
        // e.g., "sklearn.ensemble.RandomForestClassifier" -> target="sklearn.ensemble", method="RandomForestClassifier"
        let (target, method) = self.parse_call_path(call_path)?;

        // Map normalized module name back to original import path
        // e.g. "bill_utils" -> "./bill_utils.py" so the worker can resolve file imports
        let actual_target = self
            .import_path_map
            .get(&target)
            .cloned()
            .unwrap_or(target.clone());

        debug!(
            "Python call: {}.{}({:?}, {:?}) (target: {})",
            target, method, args, kwargs, actual_target
        );

        let response = self.pool.call(&actual_target, &method, args, kwargs)?;

        if response.is_error() {
            let error = response.error.ok_or_else(|| anyhow!("Unknown Python error"))?;
            return Err(anyhow!(
                "Python error ({}): {}\n{}",
                error.error_type,
                error.message,
                error.traceback.unwrap_or_default()
            ));
        }

        // If we got a reference, wrap it in a special value
        if let Some(ref_id) = response.reference {
            // Return a JSON object with the reference info
            Ok(serde_json::json!({
                "__python_ref__": ref_id,
                "__type__": response.value.as_ref()
                    .and_then(|v| v.get("__type__"))
                    .cloned()
                    .unwrap_or(Value::Null)
            }))
        } else {
            response.value.ok_or_else(|| anyhow!("Python call returned no value"))
        }
    }

    /// Parse a call path like "pandas.read_csv" into (target, method)
    fn parse_call_path(&self, call_path: &str) -> Result<(String, String)> {
        // Find the longest matching import
        let mut best_match: Option<&str> = None;
        for imported in &self.imported_modules {
            if call_path.starts_with(imported) {
                if best_match.is_none() || imported.len() > best_match.unwrap().len() {
                    best_match = Some(imported);
                }
            }
        }

        if let Some(module) = best_match {
            // Extract method name after the module
            let remainder = &call_path[module.len()..];
            if remainder.starts_with('.') {
                let method = &remainder[1..]; // Skip the dot
                // Handle nested calls like "sklearn.ensemble.RandomForestClassifier"
                // target = "sklearn.ensemble", method = "RandomForestClassifier"
                if let Some(last_dot) = method.rfind('.') {
                    let full_target = format!("{}.{}", module, &method[..last_dot]);
                    let final_method = &method[last_dot + 1..];
                    return Ok((full_target, final_method.to_string()));
                }
                return Ok((module.to_string(), method.to_string()));
            }
        }

        // Fallback: treat everything before the last dot as module, after as method
        if let Some(last_dot) = call_path.rfind('.') {
            let module = &call_path[..last_dot];
            let method = &call_path[last_dot + 1..];
            Ok((module.to_string(), method.to_string()))
        } else {
            Err(anyhow!("Invalid call path: {}", call_path))
        }
    }

    /// Call a method on a Python object reference
    pub fn call_method(
        &self,
        ref_id: &str,
        method: &str,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> Result<Value> {
        debug!("Python method call: {}.{}({:?})", ref_id, method, args);

        let response = self.pool.call(ref_id, method, args, kwargs)?;

        if response.is_error() {
            let error = response.error.ok_or_else(|| anyhow!("Unknown Python error"))?;
            return Err(anyhow!(
                "Python error ({}): {}",
                error.error_type,
                error.message
            ));
        }

        if let Some(ref_id) = response.reference {
            Ok(serde_json::json!({
                "__python_ref__": ref_id,
                "__type__": response.value.as_ref()
                    .and_then(|v| v.get("__type__"))
                    .cloned()
                    .unwrap_or(Value::Null)
            }))
        } else {
            response.value.ok_or_else(|| anyhow!("Python method returned no value"))
        }
    }
}

/// Check if a value is a Python object reference
pub fn is_python_ref(value: &Value) -> bool {
    value.get("__python_ref__").is_some()
}

/// Extract Python reference ID from a value
pub fn get_python_ref(value: &Value) -> Option<&str> {
    value.get("__python_ref__").and_then(|v| v.as_str())
}
