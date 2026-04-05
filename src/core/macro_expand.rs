// src/core/macro_expand.rs
//! Macro expansion phase: processes @decorator applications.
//!
//! After parsing, decorator applications are recorded as DecoratorApplication entries.
//! This phase resolves each application by finding the decorator function definition,
//! extracting its annotation effects, and applying them to the target function.

use crate::core::graph::WorkflowGraph;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Process all decorator applications in a workflow graph.
///
/// For each @decorator application:
/// 1. Find the target function in wf.functions
/// 2. Find the decorator function definition
/// 3. Extract annotation effects from the decorator body
/// 4. Apply annotations to the target function
pub fn expand_decorators(wf: &mut WorkflowGraph) -> Result<()> {
    if wf.decorator_applications.is_empty() {
        return Ok(());
    }

    let applications = wf.decorator_applications.clone();
    debug!(
        "[macro_expand] Processing {} decorator application(s)",
        applications.len()
    );

    // Process decorators in reverse order (bottom-up for stacked decorators)
    for app in &applications {
        debug!(
            "[macro_expand] @{}({:?}) on [{}]",
            app.decorator_fn, app.args, app.target_node_id
        );

        // Find the decorator function definition
        let deco_fn = wf.functions.get(&app.decorator_fn).cloned();

        if let Some(deco_fn_def) = deco_fn {
            // Extract annotations from the decorator function body
            let annotations =
                extract_annotations_from_body(&deco_fn_def.body, &deco_fn_def.params, &app.args);

            if !annotations.is_empty() {
                // Apply annotations to the target function
                if let Some(target_fn) = wf.functions.get_mut(&app.target_node_id) {
                    for (key, value) in &annotations {
                        debug!(
                            "[macro_expand] Annotating [{}] with {}: {}",
                            app.target_node_id, key, value
                        );
                        target_fn.annotations.insert(key.clone(), value.clone());
                    }
                } else {
                    warn!(
                        "[macro_expand] Target function '{}' not found for @{}",
                        app.target_node_id, app.decorator_fn
                    );
                }
            }
        } else {
            // Decorator function not found as a user-defined function.
            // Try to handle it as a well-known built-in decorator pattern.
            let annotations = resolve_builtin_decorator(&app.decorator_fn, &app.args);
            if !annotations.is_empty() {
                if let Some(target_fn) = wf.functions.get_mut(&app.target_node_id) {
                    for (key, value) in &annotations {
                        debug!(
                            "[macro_expand] Built-in @{} annotating [{}] with {}: {}",
                            app.decorator_fn, app.target_node_id, key, value
                        );
                        target_fn.annotations.insert(key.clone(), value.clone());
                    }
                }
            } else {
                warn!(
                    "[macro_expand] Decorator function '{}' not found",
                    app.decorator_fn
                );
            }
        }
    }

    // Clear processed applications
    wf.decorator_applications.clear();

    Ok(())
}

/// Extract annotation key-value pairs from a decorator function body.
///
/// Scans the function body's nodes for `annotate(key, value)` calls
/// and resolves parameter references against the provided arguments.
fn extract_annotations_from_body(
    body: &WorkflowGraph,
    params: &[String],
    args: &[String],
) -> HashMap<String, Value> {
    let mut annotations = HashMap::new();

    // Build parameter → argument mapping (excluding last param which is `item`)
    let param_map: HashMap<&str, &str> = params
        .iter()
        .filter(|p| *p != "item") // skip the implicit item parameter
        .enumerate()
        .filter_map(|(i, p)| args.get(i).map(|a| (p.as_str(), a.as_str())))
        .collect();

    // Scan all nodes in the body for `annotate(...)` calls (Task or AssignCall)
    for (_idx, node) in body.graph.node_indices().map(|i| (i, &body.graph[i])) {
        let action = match &node.node_type {
            crate::core::graph::NodeType::Task(a) => Some(a),
            crate::core::graph::NodeType::AssignCall { action, .. } => Some(action),
            _ => None,
        };

        if let Some(action) = action {
            let is_annotate = action.name == "annotate" || action.name.ends_with(".annotate");
            if !is_annotate {
                continue;
            }

            // Support both positional (arg0/arg1) and named (key/value) params
            let key_raw = action
                .params
                .get("key")
                .or_else(|| action.params.get("arg0"));
            let val_raw = action
                .params
                .get("value")
                .or_else(|| action.params.get("arg1"));

            if let (Some(key_raw), Some(val_raw)) = (key_raw, val_raw) {
                let key = resolve_param(key_raw, &param_map);
                let val = resolve_param_to_value(val_raw, &param_map);
                annotations.insert(key, val);
            }
        }
    }

    annotations
}

/// Resolve a parameter reference to its string value.
/// Strips quotes and substitutes parameter references.
fn resolve_param(raw: &str, param_map: &HashMap<&str, &str>) -> String {
    let trimmed = raw.trim().trim_matches('"');
    // Check if it's a parameter reference
    if let Some(val) = param_map.get(trimmed) {
        val.trim_matches('"').to_string()
    } else {
        trimmed.to_string()
    }
}

/// Resolve a parameter to a JSON Value.
/// Handles JSON objects, parameter substitution, and string literals.
fn resolve_param_to_value(raw: &str, param_map: &HashMap<&str, &str>) -> Value {
    let trimmed = raw.trim();

    // Try to parse as JSON first (for object literals like { method: "GET", path: path })
    if trimmed.starts_with('{') {
        // Substitute parameter references in the JSON string
        let mut resolved = trimmed.to_string();
        for (param, arg) in param_map {
            let arg_clean = arg.trim_matches('"');
            // Use word-boundary-aware replacement to avoid partial matches
            // Replace `: param` patterns (JSON value position) with `: "resolved_value"`
            let patterns = [
                (format!(": {}", param), format!(": \"{}\"", arg_clean)),
                (format!(":{}", param), format!(":\"{}\"", arg_clean)),
                // Also handle bare param as JSON value (without colon prefix)
                (format!(" {} ", param), format!(" \"{}\" ", arg_clean)),
                (format!(" {}}}", param), format!(" \"{}\"}}", arg_clean)),
                (format!(" {},", param), format!(" \"{}\",", arg_clean)),
            ];
            for (from, to) in &patterns {
                resolved = resolved.replace(from.as_str(), to.as_str());
            }
        }
        debug!(
            "[macro_expand] resolve_param_to_value: resolved JSON = {}",
            resolved
        );
        if let Ok(val) = serde_json::from_str::<Value>(&resolved) {
            return val;
        }
    }

    // Check if it's a parameter reference
    if let Some(val) = param_map.get(trimmed) {
        let clean = val.trim_matches('"');
        if let Ok(v) = serde_json::from_str::<Value>(clean) {
            return v;
        }
        return json!(clean);
    }

    // String literal
    let clean = trimmed.trim_matches('"');
    json!(clean)
}

/// Handle well-known built-in decorator patterns when no user function is found.
fn resolve_builtin_decorator(name: &str, args: &[String]) -> HashMap<String, Value> {
    let mut annotations = HashMap::new();

    match name {
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" => {
            let method = name.to_uppercase();
            let path = args
                .first()
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|| "/".to_string());
            annotations.insert(
                "route".to_string(),
                json!({ "method": method, "path": path }),
            );
        }
        "test" => {
            annotations.insert("test".to_string(), json!(true));
        }
        "cron" => {
            if let Some(schedule) = args.first() {
                annotations.insert(
                    "cron".to_string(),
                    json!({ "schedule": schedule.trim_matches('"') }),
                );
            }
        }
        _ => {}
    }

    annotations
}
