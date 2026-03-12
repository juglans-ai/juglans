// src/core/resolver.rs
//
// Flow Import Resolver — compile-time graph merging
//
// Resolves flow_imports declarations in WorkflowGraph, loads sub-workflow files,
// merges sub-workflow nodes and edges into the parent graph with namespace prefixes,
// and finally resolves pending_edges.

use anyhow::{anyhow, Context, Result};
use regex::{Captures, Regex};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use petgraph::visit::EdgeRef;

use crate::core::graph::{Action, Node, NodeType, SwitchCase, SwitchRoute, WorkflowGraph};
use crate::core::parser::GraphParser;
#[cfg(not(target_arch = "wasm32"))]
use crate::registry::cache::find_entry_in_dir;
#[cfg(not(target_arch = "wasm32"))]
use crate::registry::package::{is_registry_import, parse_registry_import};

lazy_static::lazy_static! {
    /// Match variable references: $identifier.path.segments
    static ref VAR_REF_RE: Regex = Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_]*)(\.[a-zA-Z0-9_.]+)?").unwrap();
}

/// Expand "@/" prefix to base_path (project_root + config.paths.base).
/// When at_base = None, the feature is disabled and the string is returned as-is.
pub fn expand_at_prefix(pattern: &str, at_base: Option<&Path>) -> String {
    let Some(base) = at_base else {
        return pattern.to_string();
    };
    if let Some(rest) = pattern.strip_prefix("@/") {
        base.join(rest).to_string_lossy().replace('\\', "/")
    } else {
        pattern.to_string()
    }
}

/// Batch expand "@/" prefixes
pub fn expand_at_prefixes(patterns: &[String], at_base: Option<&Path>) -> Vec<String> {
    patterns
        .iter()
        .map(|p| expand_at_prefix(p, at_base))
        .collect()
}

/// Resolve lib imports — load library files, extract function defs and register them
/// into the parent workflow with namespace prefixes.
///
/// Unlike flow imports, libs only extract functions without merging graph nodes/edges.
///
/// Namespace two-level priority (high -> low):
/// 1. Explicit naming via object form (determined at parser stage, not in lib_auto_namespaces)
/// 2. Filename stem (default for list form, stored as key at parser stage)
#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_lib_imports(
    workflow: &mut WorkflowGraph,
    base_dir: &Path,
    import_stack: &mut Vec<PathBuf>,
    at_base: Option<&Path>,
) -> Result<()> {
    if workflow.lib_imports.is_empty() {
        return Ok(());
    }

    let imports: Vec<(String, String)> = workflow.lib_imports.clone().into_iter().collect();
    let auto_namespaces = workflow.lib_auto_namespaces.clone();

    for (parser_namespace, rel_path) in imports {
        // Registry package detection — non-local paths are treated as registry packages
        if is_registry_import(&rel_path) {
            let (pkg_name, version_req) = parse_registry_import(&rel_path)?;

            // Search for jg_modules/ from project root upward
            let jg_modules_path = find_jg_modules_dir(base_dir).map(|d| d.join(&pkg_name));

            let entry_path = if let Some(ref pkg_dir) = jg_modules_path {
                if pkg_dir.exists() {
                    // Already installed -> read entry file
                    find_entry_in_dir(pkg_dir)?
                } else {
                    // Not installed -> attempt auto-install
                    auto_install_package(&pkg_name, version_req.as_deref(), base_dir)?
                }
            } else {
                // No jg_modules -> attempt auto-install
                auto_install_package(&pkg_name, version_req.as_deref(), base_dir)?
            };

            // Parse the library file (same logic as local lib)
            let canonical = entry_path.canonicalize().with_context(|| {
                format!(
                    "Lib import error: Cannot resolve registry package '{}' entry at {:?}",
                    pkg_name, entry_path
                )
            })?;

            if import_stack.contains(&canonical) {
                return Err(anyhow!(
                    "Circular lib import detected: '{}' ({:?})\nImport chain: {:?}",
                    parser_namespace,
                    canonical,
                    import_stack
                ));
            }
            import_stack.push(canonical.clone());

            let content = std::fs::read_to_string(&canonical)
                .with_context(|| format!("Lib import error: Cannot read '{:?}'", canonical))?;
            let mut lib_graph = GraphParser::parse_lib(&content)
                .with_context(|| format!("Lib import error: Failed to parse '{:?}'", canonical))?;

            let lib_base_dir = canonical.parent().unwrap_or(Path::new("."));
            resolve_lib_imports(&mut lib_graph, lib_base_dir, import_stack, at_base)?;

            // Registry package namespace priority:
            // 1. Explicit naming via object form (parser_namespace not in auto_namespaces)
            // 2. Package name (default for list form)
            let namespace = if !auto_namespaces.contains(&parser_namespace) {
                parser_namespace.clone()
            } else {
                pkg_name.clone()
            };

            for (func_name, func_def) in lib_graph.functions {
                let namespaced = format!("{}.{}", namespace, func_name);
                workflow.functions.insert(namespaced, func_def);
            }
            for (class_name, class_def) in lib_graph.classes {
                let namespaced = format!("{}.{}", namespace, class_name);
                workflow.classes.insert(namespaced, class_def);
            }
            for (type_name, method_name, func_def) in lib_graph.pending_methods {
                let namespaced_type = format!("{}.{}", namespace, type_name);
                workflow
                    .pending_methods
                    .push((namespaced_type, method_name, func_def));
            }

            import_stack.pop();
            continue;
        }

        // Local file path resolution (existing logic)
        let expanded = expand_at_prefix(&rel_path, at_base);
        let abs_path = if Path::new(&expanded).is_absolute() {
            PathBuf::from(&expanded)
        } else {
            base_dir.join(&expanded)
        };
        let canonical = abs_path.canonicalize().with_context(|| {
            format!(
                "Lib import error: Cannot resolve path '{}' (base: {:?})",
                rel_path, base_dir
            )
        })?;

        // 2. Circular import detection
        if import_stack.contains(&canonical) {
            return Err(anyhow!(
                "Circular lib import detected: '{}' ({:?})\nImport chain: {:?}",
                parser_namespace,
                canonical,
                import_stack
            ));
        }
        import_stack.push(canonical.clone());

        // 3. Parse the library file
        let content = std::fs::read_to_string(&canonical)
            .with_context(|| format!("Lib import error: Cannot read '{:?}'", canonical))?;
        let mut lib_graph = GraphParser::parse_lib(&content)
            .with_context(|| format!("Lib import error: Failed to parse '{:?}'", canonical))?;

        // 4. Recursively resolve the library's own lib imports
        let lib_base_dir = canonical.parent().unwrap_or(Path::new("."));
        resolve_lib_imports(&mut lib_graph, lib_base_dir, import_stack, at_base)?;

        // 5. Determine final namespace (two-level priority)
        // Explicit naming via object form > filename stem (default for list form)
        let namespace = parser_namespace.clone();

        // 6. Extract function defs, register to parent workflow with namespace prefix
        for (func_name, func_def) in lib_graph.functions {
            let namespaced = format!("{}.{}", namespace, func_name);
            workflow.functions.insert(namespaced, func_def);
        }
        for (class_name, class_def) in lib_graph.classes {
            let namespaced = format!("{}.{}", namespace, class_name);
            workflow.classes.insert(namespaced, class_def);
        }
        for (type_name, method_name, func_def) in lib_graph.pending_methods {
            let namespaced_type = format!("{}.{}", namespace, type_name);
            workflow
                .pending_methods
                .push((namespaced_type, method_name, func_def));
        }

        import_stack.pop();
    }

    Ok(())
}

/// Resolve flow imports and merge sub-graphs into the parent workflow.
///
/// - `workflow`: Parent workflow (will be modified)
/// - `base_dir`: Directory containing the parent workflow file (for resolving relative paths)
/// - `import_stack`: Stack of imported file absolute paths (for circular import detection)
/// - `at_base`: Base directory for @ path alias (None = disabled)
#[cfg(not(target_arch = "wasm32"))]
pub fn resolve_flow_imports(
    workflow: &mut WorkflowGraph,
    base_dir: &Path,
    import_stack: &mut Vec<PathBuf>,
    at_base: Option<&Path>,
) -> Result<()> {
    if workflow.flow_imports.is_empty() {
        // Even without flow_imports, pending_edges must be resolved (may have misspelled namespace refs)
        commit_pending_edges(workflow)?;
        expand_wildcard_edges(workflow)?;
        return Ok(());
    }

    // Clone imports to avoid borrow conflict
    let imports: Vec<(String, String)> = workflow.flow_imports.clone().into_iter().collect();

    for (alias, rel_path) in imports {
        // 1. Expand @/ prefix and resolve absolute path
        let expanded_rel = expand_at_prefix(&rel_path, at_base);
        let abs_path = if Path::new(&expanded_rel).is_absolute() {
            PathBuf::from(&expanded_rel)
        } else {
            base_dir.join(&expanded_rel)
        };
        let canonical = abs_path.canonicalize().with_context(|| {
            format!(
                "Flow import error: Cannot resolve path '{}' (base: {:?})",
                rel_path, base_dir
            )
        })?;

        // 2. Circular import detection
        if import_stack.contains(&canonical) {
            return Err(anyhow!(
                "Circular flow import detected: '{}' ({:?})\nImport chain: {:?}",
                alias,
                canonical,
                import_stack
            ));
        }
        import_stack.push(canonical.clone());

        // 3. Load and parse the sub-workflow
        let content = std::fs::read_to_string(&canonical)
            .with_context(|| format!("Flow import error: Cannot read '{:?}'", canonical))?;
        let mut child_graph = GraphParser::parse(&content)
            .with_context(|| format!("Flow import error: Failed to parse '{:?}'", canonical))?;

        // 4. Recursively resolve the sub-workflow's own flow imports
        let child_base_dir = canonical.parent().unwrap_or(Path::new("."));
        resolve_flow_imports(&mut child_graph, child_base_dir, import_stack, at_base)?;

        // 5. Merge sub-graph into parent graph
        merge_subgraph(workflow, &child_graph, &alias, child_base_dir, at_base)?;

        import_stack.pop();
    }

    // 6. All sub-graphs merged, resolve pending_edges
    commit_pending_edges(workflow)?;
    expand_wildcard_edges(workflow)?;

    Ok(())
}

/// Merge sub-workflow nodes, edges, and switch routes into the parent graph
fn merge_subgraph(
    parent: &mut WorkflowGraph,
    child: &WorkflowGraph,
    prefix: &str,
    child_base_dir: &Path,
    at_base: Option<&Path>,
) -> Result<()> {
    // Collect all node IDs of the sub-workflow (for variable namespace conversion)
    let child_node_ids: HashSet<String> = child
        .graph
        .node_indices()
        .map(|idx| child.graph[idx].id.clone())
        .collect();

    // --- 1. Merge nodes ---
    for idx in child.graph.node_indices() {
        let child_node = &child.graph[idx];
        let prefixed_id = format!("{}.{}", prefix, child_node.id);

        // Clone node_type and perform variable namespace conversion
        let prefixed_node_type = prefix_node_type(&child_node.node_type, prefix, &child_node_ids);

        let new_node = Node {
            id: prefixed_id.clone(),
            node_type: prefixed_node_type,
        };

        let new_idx = parent.graph.add_node(new_node);
        parent.node_map.insert(prefixed_id, new_idx);
    }

    // --- 2. Merge edges ---
    for edge_ref in child.graph.edge_references() {
        let from_id = format!("{}.{}", prefix, child.graph[edge_ref.source()].id);
        let to_id = format!("{}.{}", prefix, child.graph[edge_ref.target()].id);
        let mut edge = edge_ref.weight().clone();

        // Variables in condition expressions also need conversion
        if let Some(ref cond) = edge.condition {
            edge.condition = Some(prefix_variables(cond, prefix, &child_node_ids));
        }

        // Both nodes have been added to parent at this point, can commit directly
        let f_idx = *parent.node_map.get(&from_id).ok_or_else(|| {
            anyhow!(
                "Merge error: source node '{}' not found after merge",
                from_id
            )
        })?;
        let t_idx = *parent
            .node_map
            .get(&to_id)
            .ok_or_else(|| anyhow!("Merge error: target node '{}' not found after merge", to_id))?;
        parent.graph.add_edge(f_idx, t_idx, edge);
    }

    // --- 3. Merge switch routes ---
    for (key, route) in &child.switch_routes {
        let prefixed_key = format!("{}.{}", prefix, key);
        let prefixed_route = SwitchRoute {
            subject: prefix_variables(&route.subject, prefix, &child_node_ids),
            cases: route
                .cases
                .iter()
                .map(|c| SwitchCase {
                    value: c.value.clone(),
                    target: format!("{}.{}", prefix, c.target),
                    is_ok: c.is_ok,
                    is_err: c.is_err,
                    err_kind: c.err_kind.clone(),
                })
                .collect(),
        };
        parent.switch_routes.insert(prefixed_key, prefixed_route);
    }

    // --- 4. Merge sub-workflow pending_edges (transfer to parent graph with prefix) ---
    for (f_id, t_id, mut edge) in child.pending_edges.clone() {
        let prefixed_f = format!("{}.{}", prefix, f_id);
        let prefixed_t = format!("{}.{}", prefix, t_id);
        if let Some(ref cond) = edge.condition {
            edge.condition = Some(prefix_variables(cond, prefix, &child_node_ids));
        }
        parent.pending_edges.push((prefixed_f, prefixed_t, edge));
    }

    // --- 5. Merge resource patterns (expand @/ alias first, adjust non-absolute paths relative to sub-workflow dir) ---
    for pattern in &child.prompt_patterns {
        let expanded = expand_at_prefix(pattern, at_base);
        if Path::new(&expanded).is_absolute() {
            parent.prompt_patterns.push(expanded);
        } else {
            parent
                .prompt_patterns
                .push(child_base_dir.join(&expanded).to_string_lossy().to_string());
        }
    }
    for pattern in &child.agent_patterns {
        let expanded = expand_at_prefix(pattern, at_base);
        if Path::new(&expanded).is_absolute() {
            parent.agent_patterns.push(expanded);
        } else {
            parent
                .agent_patterns
                .push(child_base_dir.join(&expanded).to_string_lossy().to_string());
        }
    }
    for pattern in &child.tool_patterns {
        let expanded = expand_at_prefix(pattern, at_base);
        if Path::new(&expanded).is_absolute() {
            parent.tool_patterns.push(expanded);
        } else {
            parent
                .tool_patterns
                .push(child_base_dir.join(&expanded).to_string_lossy().to_string());
        }
    }
    for import in &child.python_imports {
        if !parent.python_imports.contains(import) {
            parent.python_imports.push(import.clone());
        }
    }

    Ok(())
}

/// Resolve and commit all pending_edges (called after flow merging is complete)
fn commit_pending_edges(workflow: &mut WorkflowGraph) -> Result<()> {
    let pending = std::mem::take(&mut workflow.pending_edges);

    for (f_id, t_id, edge) in pending {
        let f_idx = *workflow.node_map.get(&f_id).ok_or_else(|| {
            anyhow!(
                "Graph Error: Pending edge references undefined node '{}'. \
                 Did you declare it in 'flows:' and define it in the imported workflow?",
                f_id
            )
        })?;
        let t_idx = *workflow.node_map.get(&t_id).ok_or_else(|| {
            anyhow!(
                "Graph Error: Pending edge references undefined node '{}'. \
                 Did you declare it in 'flows:' and define it in the imported workflow?",
                t_id
            )
        })?;
        workflow.graph.add_edge(f_idx, t_idx, edge);
    }

    Ok(())
}

/// Expand wildcard edges (glob pattern matching on node IDs)
fn expand_wildcard_edges(workflow: &mut WorkflowGraph) -> Result<()> {
    let pending = std::mem::take(&mut workflow.pending_wildcard_edges);
    if pending.is_empty() {
        return Ok(());
    }

    for (from_pattern, to_pattern, edge) in pending {
        let from_ids = expand_glob(&from_pattern, &workflow.node_map);
        let to_ids = expand_glob(&to_pattern, &workflow.node_map);

        if from_ids.is_empty() {
            return Err(anyhow!(
                "Wildcard edge: pattern '{}' matched no nodes",
                from_pattern
            ));
        }
        if to_ids.is_empty() {
            return Err(anyhow!(
                "Wildcard edge: pattern '{}' matched no nodes",
                to_pattern
            ));
        }

        for f_id in &from_ids {
            for t_id in &to_ids {
                if f_id == t_id {
                    continue; // skip self-loop
                }
                let f_idx = workflow.node_map[f_id.as_str()];
                let t_idx = workflow.node_map[t_id.as_str()];
                workflow.graph.add_edge(f_idx, t_idx, edge.clone());
            }
        }
    }

    Ok(())
}

/// Glob pattern matching: `*` matches zero or more characters
fn expand_glob(
    pattern: &str,
    node_map: &std::collections::HashMap<String, petgraph::graph::NodeIndex>,
) -> Vec<String> {
    if !pattern.contains('*') {
        // Exact match
        return if node_map.contains_key(pattern) {
            vec![pattern.to_string()]
        } else {
            vec![]
        };
    }

    // Convert glob to regex: escape literal parts, replace * with .*
    let parts: Vec<&str> = pattern.split('*').collect();
    let escaped: Vec<String> = parts.iter().map(|p| regex::escape(p)).collect();
    let re_str = format!("^{}$", escaped.join(".*"));
    let re = match Regex::new(&re_str) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    node_map
        .keys()
        .filter(|k| re.is_match(k))
        .cloned()
        .collect()
}

// =============================================================================
// Variable namespace conversion
// =============================================================================

/// Add namespace prefix to variable references in a string.
///
/// Rule: only variables whose first segment matches a sub-workflow node ID get prefixed.
/// - $verify.output       -> $prefix.verify.output   (verify is a sub-flow node)
/// - $ctx.some_var        -> $ctx.some_var            (ctx is not a node, unchanged)
/// - $input.message       -> $input.message           (unchanged)
/// - $output              -> $output                  (unchanged)
fn prefix_variables(text: &str, prefix: &str, child_node_ids: &HashSet<String>) -> String {
    VAR_REF_RE
        .replace_all(text, |caps: &Captures| {
            let first_segment = &caps[1]; // First segment of the variable (e.g. verify, ctx, input)
            if child_node_ids.contains(first_segment) {
                // Is a sub-flow node -> add prefix
                let rest = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                format!("${}.{}{}", prefix, first_segment, rest)
            } else {
                // Not a node (ctx, input, output, etc.) -> keep unchanged
                caps[0].to_string()
            }
        })
        .to_string()
}

/// Perform namespace conversion on variable references inside NodeType
fn prefix_node_type(
    node_type: &NodeType,
    prefix: &str,
    child_node_ids: &HashSet<String>,
) -> NodeType {
    match node_type {
        NodeType::Task(action) => {
            let prefixed_params: std::collections::HashMap<String, String> = action
                .params
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::Task(Action {
                name: action.name.clone(),
                params: prefixed_params,
            })
        }
        NodeType::Loop { condition, body } => {
            let prefixed_cond = prefix_variables(condition, prefix, child_node_ids);
            // Recursively process nodes inside the loop body
            let prefixed_body = prefix_subgraph_body(body, prefix, child_node_ids);
            NodeType::Loop {
                condition: prefixed_cond,
                body: Box::new(prefixed_body),
            }
        }
        NodeType::Foreach {
            item,
            list,
            body,
            parallel,
        } => {
            let prefixed_list = prefix_variables(list, prefix, child_node_ids);
            let prefixed_body = prefix_subgraph_body(body, prefix, child_node_ids);
            NodeType::Foreach {
                item: item.clone(),
                list: prefixed_list,
                body: Box::new(prefixed_body),
                parallel: *parallel,
            }
        }
        NodeType::Literal(val) => NodeType::Literal(val.clone()),
        NodeType::_ExternalCall {
            call_path,
            args,
            kwargs,
        } => {
            let prefixed_args: Vec<String> = args
                .iter()
                .map(|a| prefix_variables(a, prefix, child_node_ids))
                .collect();
            let prefixed_kwargs: std::collections::HashMap<String, String> = kwargs
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::_ExternalCall {
                call_path: call_path.clone(),
                args: prefixed_args,
                kwargs: prefixed_kwargs,
            }
        }
        NodeType::NewInstance { class_name, args } => {
            let prefixed_args: std::collections::HashMap<String, String> = args
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::NewInstance {
                class_name: class_name.clone(),
                args: prefixed_args,
            }
        }
        NodeType::MethodCall {
            instance_path,
            method_name,
            args,
        } => {
            let prefixed_args: std::collections::HashMap<String, String> = args
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::MethodCall {
                instance_path: instance_path.clone(),
                method_name: method_name.clone(),
                args: prefixed_args,
            }
        }
        NodeType::Assert(expr_str) => {
            NodeType::Assert(prefix_variables(expr_str, prefix, child_node_ids))
        }
        NodeType::AssignCall { var, action } => {
            let prefixed_params: std::collections::HashMap<String, String> = action
                .params
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::AssignCall {
                var: var.clone(),
                action: Action {
                    name: action.name.clone(),
                    params: prefixed_params,
                },
            }
        }
        NodeType::ReturnErr(val) => {
            // ReturnErr contains a JSON object — no variable references to prefix
            NodeType::ReturnErr(val.clone())
        }
    }
}

/// Perform variable conversion on nested workflow body (loop/foreach body)
fn prefix_subgraph_body(
    body: &WorkflowGraph,
    prefix: &str,
    child_node_ids: &HashSet<String>,
) -> WorkflowGraph {
    let mut new_body = body.clone();
    // Convert variable references in body's internal nodes
    for idx in new_body.graph.node_indices() {
        let node = &new_body.graph[idx];
        let new_type = prefix_node_type(&node.node_type, prefix, child_node_ids);
        new_body.graph[idx].node_type = new_type;
    }
    // Convert condition expressions in body's internal edges
    for edge_idx in new_body.graph.edge_indices() {
        let edge = &new_body.graph[edge_idx];
        if let Some(ref cond) = edge.condition {
            let new_cond = prefix_variables(cond, prefix, child_node_ids);
            new_body.graph[edge_idx].condition = Some(new_cond);
        }
    }
    new_body
}

/// Find the jg_modules directory by searching from base_dir upward
#[cfg(not(target_arch = "wasm32"))]
fn find_jg_modules_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("jg_modules");
        if candidate.is_dir() {
            return Some(candidate);
        }
        // Also check if jgpackage.toml exists here (project root)
        if dir.join("jgpackage.toml").exists() {
            return Some(candidate); // Return even if doesn't exist yet — installer will create it
        }
        if !dir.pop() {
            break;
        }
    }
    // Fallback: create jg_modules in the start directory
    Some(start.join("jg_modules"))
}

/// Auto-install a registry package using the registry client.
/// This bridges from sync resolver code into async installer via tokio runtime.
#[cfg(not(target_arch = "wasm32"))]
fn auto_install_package(
    pkg_name: &str,
    version_req: Option<&str>,
    project_dir: &Path,
) -> Result<PathBuf> {
    tracing::info!("Auto-installing registry package '{}' ...", pkg_name);

    // Load registry URL from config, or use default
    let registry_url = crate::services::config::JuglansConfig::load()
        .ok()
        .and_then(|c| c.registry.map(|r| r.url))
        .unwrap_or_else(|| "https://jgr.juglans.ai".to_string());

    let installer = crate::registry::installer::PackageInstaller::with_defaults(&registry_url)
        .with_context(|| "Failed to create package installer")?;

    // Bridge into async: we're called from sync code, but the caller runs inside tokio
    let handle = tokio::runtime::Handle::try_current().with_context(|| {
        format!(
            "Cannot auto-install package '{}': no async runtime available. \
             Run 'juglans add {}' first, or ensure the workflow is run with 'juglans'.",
            pkg_name, pkg_name
        )
    })?;

    let name = pkg_name.to_string();
    let ver = version_req.map(|s| s.to_string());
    let proj = project_dir.to_path_buf();

    let installed = handle
        .block_on(async move { installer.install(&name, ver.as_deref(), &proj).await })
        .with_context(|| format!("Failed to auto-install package '{}'", pkg_name))?;

    Ok(installed.entry_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_variables_basic() {
        let mut node_ids = HashSet::new();
        node_ids.insert("verify".to_string());
        node_ids.insert("extract".to_string());

        // Sub-flow node reference -> add prefix
        assert_eq!(
            prefix_variables("$verify.output", "auth", &node_ids),
            "$auth.verify.output"
        );
        assert_eq!(
            prefix_variables("$extract.output.intent", "auth", &node_ids),
            "$auth.extract.output.intent"
        );

        // Global variables -> unchanged
        assert_eq!(
            prefix_variables("$ctx.some_var", "auth", &node_ids),
            "$ctx.some_var"
        );
        assert_eq!(
            prefix_variables("$input.message", "auth", &node_ids),
            "$input.message"
        );
        assert_eq!(prefix_variables("$output", "auth", &node_ids), "$output");
    }

    #[test]
    fn test_prefix_variables_mixed() {
        let mut node_ids = HashSet::new();
        node_ids.insert("classify".to_string());

        let input = r#"$classify.output.intent == "trade" && $ctx.ready"#;
        let result = prefix_variables(input, "trading", &node_ids);
        assert_eq!(
            result,
            r#"$trading.classify.output.intent == "trade" && $ctx.ready"#
        );
    }

    #[test]
    fn test_expand_at_prefix() {
        let base = Path::new("/project/src");

        // Starts with @/ -> expand to base + remaining part
        assert_eq!(
            expand_at_prefix("@/prompts/foo.jgprompt", Some(base)),
            "/project/src/prompts/foo.jgprompt"
        );

        // Does not start with @/ -> return as-is
        assert_eq!(expand_at_prefix("./local/file", Some(base)), "./local/file");
        assert_eq!(
            expand_at_prefix("relative/path", Some(base)),
            "relative/path"
        );

        // Only @ without / -> return as-is
        assert_eq!(expand_at_prefix("@noslash", Some(base)), "@noslash");

        // at_base = None -> feature disabled, return as-is
        assert_eq!(
            expand_at_prefix("@/prompts/foo.jgprompt", None),
            "@/prompts/foo.jgprompt"
        );
    }

    #[test]
    fn test_expand_at_prefixes_batch() {
        let base = Path::new("/project");
        let patterns = vec![
            "@/prompts/*.jgprompt".to_string(),
            "./local/file.jgprompt".to_string(),
            "@/agents/my-agent.jgagent".to_string(),
        ];
        let result = expand_at_prefixes(&patterns, Some(base));
        assert_eq!(result[0], "/project/prompts/*.jgprompt");
        assert_eq!(result[1], "./local/file.jgprompt");
        assert_eq!(result[2], "/project/agents/my-agent.jgagent");
    }

    #[test]
    fn test_prefix_variables_no_match() {
        let node_ids = HashSet::new(); // Empty set

        assert_eq!(
            prefix_variables("$output + $ctx.x", "ns", &node_ids),
            "$output + $ctx.x"
        );
    }

    #[test]
    fn test_resolve_lib_imports_explicit_namespace() {
        use std::io::Write;

        // Create temporary lib file
        let dir = std::env::temp_dir().join("juglans_test_lib_explicit");
        let _ = std::fs::create_dir_all(&dir);
        let lib_path = dir.join("sqlite.jg");
        let mut f = std::fs::File::create(&lib_path).unwrap();
        writeln!(
            f,
            r#"
[read(table)]: bash(command="sqlite3 db.sqlite 'SELECT * FROM " + $table + "'")
[write(table, data)]: bash(command="echo " + $data)
"#
        )
        .unwrap();

        // Main workflow
        let main_content = format!(
            r#"
libs: {{ db: "{}" }}
[step1]: db.read(table="users")
"#,
            lib_path.to_string_lossy()
        );

        let mut graph = GraphParser::parse(&main_content).unwrap();

        // Verify parser stored lib_imports (explicit namespace)
        assert_eq!(
            graph.lib_imports.get("db").unwrap(),
            lib_path.to_str().unwrap()
        );
        assert!(!graph.lib_auto_namespaces.contains("db"));

        // Resolve lib imports
        let mut import_stack = vec![];
        resolve_lib_imports(&mut graph, &dir, &mut import_stack, None).unwrap();

        // Explicit namespace "db" (ignores slug "sqlite3" from file)
        assert!(
            graph.functions.contains_key("db.read"),
            "functions: {:?}",
            graph.functions.keys().collect::<Vec<_>>()
        );
        assert!(graph.functions.contains_key("db.write"));
        assert!(!graph.functions.contains_key("sqlite3.read")); // Should not use slug
    }

    #[test]
    fn test_resolve_lib_imports_auto_namespace_with_stem() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("juglans_test_lib_slug");
        let _ = std::fs::create_dir_all(&dir);
        let lib_path = dir.join("my_sqlite_lib.jg");
        let mut f = std::fs::File::create(&lib_path).unwrap();
        writeln!(
            f,
            r#"
[query(sql)]: bash(command="sqlite3 db.sqlite '" + $sql + "'")
"#
        )
        .unwrap();

        // List form — stem = "my_sqlite_lib"
        let main_content = format!(
            r#"
libs: ["{}"]
[step1]: my_sqlite_lib.query(sql="SELECT 1")
"#,
            lib_path.to_string_lossy()
        );

        let mut graph = GraphParser::parse(&main_content).unwrap();

        // Verify parser stored stem as placeholder
        assert!(graph.lib_imports.contains_key("my_sqlite_lib"));
        assert!(graph.lib_auto_namespaces.contains("my_sqlite_lib"));

        // Resolve lib imports
        let mut import_stack = vec![];
        resolve_lib_imports(&mut graph, &dir, &mut import_stack, None).unwrap();

        // List form -> use filename stem "my_sqlite_lib"
        assert!(
            graph.functions.contains_key("my_sqlite_lib.query"),
            "functions: {:?}",
            graph.functions.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_resolve_lib_imports_auto_namespace_no_slug() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("juglans_test_lib_stem");
        let _ = std::fs::create_dir_all(&dir);
        let lib_path = dir.join("utils.jg");
        let mut f = std::fs::File::create(&lib_path).unwrap();
        writeln!(
            f,
            r#"
[helper(x)]: bash(command="echo " + $x)
"#
        )
        .unwrap();

        let main_content = format!(
            r#"
libs: ["{}"]
[step1]: utils.helper(x="test")
"#,
            lib_path.to_string_lossy()
        );

        let mut graph = GraphParser::parse(&main_content).unwrap();

        let mut import_stack = vec![];
        resolve_lib_imports(&mut graph, &dir, &mut import_stack, None).unwrap();

        // List form -> use filename stem "utils"
        assert!(
            graph.functions.contains_key("utils.helper"),
            "functions: {:?}",
            graph.functions.keys().collect::<Vec<_>>()
        );
    }
}
