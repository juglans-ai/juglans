// src/core/validator.rs
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::core::graph::{self, NodeType, WorkflowGraph};
use petgraph::graph::NodeIndex;

/// Project-level resource context (for cross-file validation)
/// Built after handle_check two-pass scan, passed to validate_with_project
#[derive(Debug, Default)]
pub struct ProjectContext {
    /// All slugs parsed from .jgagent files
    pub agent_slugs: HashSet<String>,
    /// All slugs parsed from .jgprompt files
    pub prompt_slugs: HashSet<String>,
    /// Absolute paths of all .jg/.jgflow files
    pub flow_paths: HashSet<PathBuf>,
    /// Base directory path for check
    pub base_dir: PathBuf,
    /// Current file's directory (for resolving relative paths, e.g. flows: ./trading.jg)
    pub file_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub code: String,
    pub message: String,
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationResult {
    pub fn new() -> Self {
        ValidationResult {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn add_error(&mut self, code: &str, message: &str, node_id: Option<&str>) {
        self.is_valid = false;
        self.errors.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            code: code.to_string(),
            message: message.to_string(),
            node_id: node_id.map(|s| s.to_string()),
        });
    }

    pub fn add_warning(&mut self, code: &str, message: &str, node_id: Option<&str>) {
        self.warnings.push(ValidationIssue {
            severity: ValidationSeverity::Warning,
            code: code.to_string(),
            message: message.to_string(),
            node_id: node_id.map(|s| s.to_string()),
        });
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    pub fn warning_count(&self) -> usize {
        self.warnings.len()
    }

    /// Format validation results as colored terminal output
    pub fn format_report(&self, file_display: &str) -> String {
        let mut out = String::new();
        if !self.is_valid {
            out.push_str(&format!(
                "\x1b[1;31merror\x1b[0m: validation failed for '{}'\n",
                file_display
            ));
        } else if !self.warnings.is_empty() {
            out.push_str(&format!("\x1b[1;33mwarning\x1b[0m: '{}'\n", file_display));
        }
        for issue in &self.errors {
            let node_suffix = issue
                .node_id
                .as_ref()
                .map(|n| format!(" in [{}]", n))
                .unwrap_or_default();
            out.push_str(&format!(
                "  \x1b[31m[{}]\x1b[0m {}{}\n",
                issue.code, issue.message, node_suffix
            ));
        }
        for issue in &self.warnings {
            let node_suffix = issue
                .node_id
                .as_ref()
                .map(|n| format!(" in [{}]", n))
                .unwrap_or_default();
            out.push_str(&format!(
                "  \x1b[33m[{}]\x1b[0m {}{}\n",
                issue.code, issue.message, node_suffix
            ));
        }
        out
    }

    /// Convert to JSON error response (for web server)
    pub fn to_error_json(&self) -> serde_json::Value {
        json!({
            "error": "Workflow validation failed",
            "errors": self.errors,
            "warnings": self.warnings,
        })
    }
}

pub struct WorkflowValidator;

impl WorkflowValidator {
    /// Validate a workflow graph and return all issues found
    pub fn validate(graph: &WorkflowGraph) -> ValidationResult {
        Self::validate_with_scope(graph, &HashSet::new(), None)
    }

    /// Validate with project-level cross-file context
    pub fn validate_with_project(
        graph: &WorkflowGraph,
        project: &ProjectContext,
    ) -> ValidationResult {
        Self::validate_with_scope(graph, &HashSet::new(), Some(project))
    }

    /// Validate with additional variable prefixes from parent scope (e.g. foreach item)
    fn validate_with_scope(
        graph: &WorkflowGraph,
        parent_vars: &HashSet<String>,
        project: Option<&ProjectContext>,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Check 1: Entry node exists
        Self::check_entry_node(graph, &mut result);

        // Check 2: Cycles detection (DAG validation)
        Self::check_cycles(graph, &mut result);

        // Check 3: Unreachable nodes
        Self::check_unreachable_nodes(graph, &mut result);

        // Check 4: Terminal nodes warning
        Self::check_terminal_nodes(graph, &mut result);

        // Check 5: Empty workflow
        Self::check_empty_workflow(graph, &mut result);

        // Check 6: Validate node references (tools, prompts, agents)
        Self::check_node_references(graph, &mut result);

        // Check 7: Validate nested workflows (foreach, loop)
        Self::check_nested_workflows(graph, parent_vars, &mut result);

        // Check 8: Validate variable reference prefixes + DAG predecessor check
        Self::check_variable_references(graph, parent_vars, &mut result);

        // Check 9: Validate switch routes
        Self::check_switch_routes(graph, &mut result);

        // Check 10: Validate edge conditions
        Self::check_edge_conditions(graph, &mut result);

        // Check 11: Validate function definitions (body sub-graphs)
        Self::check_function_definitions(graph, parent_vars, &mut result);

        // Check 12: Validate function call arguments
        Self::check_function_calls(graph, &mut result);

        // Check 13: Validate on_tool=[node] references
        Self::check_on_tool_references(graph, &mut result);

        // Cross-file checks (only when ProjectContext is available)
        if let Some(project) = project {
            // Check 14: Agent slug references
            Self::check_agent_references(graph, project, &mut result);

            // Check 15: Prompt slug references
            Self::check_prompt_references(graph, project, &mut result);

            // Check 16: Flow import paths
            Self::check_flow_imports(graph, project, &mut result);

            // Check 17: Resource glob patterns
            Self::check_resource_patterns(graph, project, &mut result);

            // Check 18: Lib import paths
            #[cfg(not(target_arch = "wasm32"))]
            Self::check_lib_imports(graph, project, &mut result);
        }

        result
    }

    /// Check if entry node exists
    fn check_entry_node(graph: &WorkflowGraph, result: &mut ValidationResult) {
        if graph.entry_node.is_empty() {
            if graph.graph.node_count() > 0 {
                result.add_warning(
                    "W001",
                    "No entry node specified; using first node as entry point",
                    None,
                );
            }
        } else if !graph.node_map.contains_key(&graph.entry_node) {
            result.add_error(
                "E001",
                &format!(
                    "Entry node '{}' does not exist in the graph",
                    graph.entry_node
                ),
                Some(&graph.entry_node),
            );
        }
    }

    /// Check for cycles in the graph
    fn check_cycles(graph: &WorkflowGraph, result: &mut ValidationResult) {
        if is_cyclic_directed(&graph.graph) {
            // Find nodes involved in cycles using toposort failure
            match toposort(&graph.graph, None) {
                Ok(_) => {} // No cycle
                Err(cycle) => {
                    let node = &graph.graph[cycle.node_id()];
                    result.add_error(
                        "E002",
                        &format!(
                            "Cycle detected involving node '{}'. Workflows must be acyclic (DAG).",
                            node.id
                        ),
                        Some(&node.id),
                    );
                }
            }
        }
    }

    /// Check for unreachable nodes (nodes not reachable from entry)
    fn check_unreachable_nodes(graph: &WorkflowGraph, result: &mut ValidationResult) {
        if graph.graph.node_count() == 0 {
            return;
        }

        // Find entry node index
        let entry_id = if graph.entry_node.is_empty() {
            // Use first node
            graph
                .graph
                .node_indices()
                .next()
                .map(|idx| graph.graph[idx].id.clone())
                .unwrap_or_default()
        } else {
            graph.entry_node.clone()
        };

        if let Some(&entry_idx) = graph.node_map.get(&entry_id) {
            // BFS/DFS to find all reachable nodes
            let mut reachable = HashSet::new();
            let mut stack = vec![entry_idx];

            while let Some(idx) = stack.pop() {
                if reachable.insert(idx) {
                    // Add all outgoing neighbors
                    for neighbor in graph.graph.neighbors_directed(idx, Direction::Outgoing) {
                        if !reachable.contains(&neighbor) {
                            stack.push(neighbor);
                        }
                    }
                }
            }

            // Check which nodes are unreachable (exempt test nodes and their descendants)
            for idx in graph.graph.node_indices() {
                if !reachable.contains(&idx) {
                    let node = &graph.graph[idx];
                    // Skip test_* nodes — they are intentionally disconnected
                    if graph::is_test_node_id(&node.id) {
                        continue;
                    }
                    // Skip nodes only reachable from test_* roots
                    if Self::is_test_only_descendant(graph, idx) {
                        continue;
                    }
                    result.add_warning(
                        "W002",
                        &format!("Node '{}' is not reachable from entry node", node.id),
                        Some(&node.id),
                    );
                }
            }
        }
    }

    /// Check if a node is only reachable from test_* root nodes (reverse BFS)
    fn is_test_only_descendant(wf: &WorkflowGraph, idx: NodeIndex) -> bool {
        let mut stack = vec![idx];
        let mut visited = HashSet::new();
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            let in_edges: Vec<_> = wf
                .graph
                .edges_directed(current, Direction::Incoming)
                .collect();
            if in_edges.is_empty() {
                // Root node — must be a test node for this to qualify
                if !graph::is_test_node_id(&wf.graph[current].id) {
                    return false;
                }
            } else {
                for edge in in_edges {
                    stack.push(edge.source());
                }
            }
        }
        true
    }

    /// Check for terminal nodes (no outgoing edges)
    fn check_terminal_nodes(graph: &WorkflowGraph, result: &mut ValidationResult) {
        if graph.graph.node_count() > 1 {
            let has_terminal = graph.graph.node_indices().any(|idx| {
                graph
                    .graph
                    .neighbors_directed(idx, Direction::Outgoing)
                    .count()
                    == 0
            });
            if !has_terminal {
                result.add_warning(
                    "W003",
                    "No terminal nodes found (nodes with no outgoing edges). All paths may loop.",
                    None,
                );
            }
        }
    }

    /// Check if workflow is empty
    fn check_empty_workflow(graph: &WorkflowGraph, result: &mut ValidationResult) {
        if graph.graph.node_count() == 0 {
            result.add_error("E004", "Workflow contains no nodes", None);
        }
    }

    /// Check node references for known tools
    fn check_node_references(graph: &WorkflowGraph, result: &mut ValidationResult) {
        // Known built-in tools (must match BuiltinRegistry::new() in builtins/mod.rs)
        let mut known_tools: HashSet<&str> = [
            // AI tools
            "chat",
            "p",
            "memory_search",
            "history",
            "execute_workflow",
            // Network tools
            "fetch_url",
            "fetch",
            "http_request",
            "oauth_token",
            // System tools
            "timer",
            "notify",
            "reply",
            "print",
            "return",
            "feishu_webhook",
            // HTTP backend tools
            "serve",
            "response",
            // Devtools
            "read_file",
            "write_file",
            "edit_file",
            "glob",
            "grep",
            "bash",
            "sh",
            // Testing
            "assert",
            "config",
            // Database ORM
            "db.connect",
            "db.disconnect",
            "db.query",
            "db.exec",
            "db.find",
            "db.find_one",
            "db.create",
            "db.create_many",
            "db.upsert",
            "db.update",
            "db.delete",
            "db.count",
            "db.aggregate",
            "db.begin",
            "db.commit",
            "db.rollback",
            "db.create_table",
            "db.drop_table",
            "db.alter_table",
            "db.tables",
            "db.columns",
            // Vector
            "vector_create_space",
            "vector_upsert",
            "vector_search",
            "vector_list_spaces",
            "vector_delete_space",
            "vector_delete",
        ]
        .iter()
        .copied()
        .collect();

        // Function nodes are also valid tool names (callers use them like tools)
        let func_names: Vec<String> = graph.functions.keys().cloned().collect();
        for name in &func_names {
            known_tools.insert(name.as_str());
        }

        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];

            if let NodeType::Task(action) = &node.node_type {
                // Check if tool is known (warning only, as custom tools are allowed)
                // Skip W004 for struct/class instantiation (Name(field=value) style)
                if !known_tools.contains(action.name.as_str())
                    && !graph.classes.contains_key(&action.name)
                {
                    result.add_warning(
                        "W004",
                        &format!(
                            "Unknown tool '{}'. Ensure it's defined in MCP servers or libs.",
                            action.name
                        ),
                        Some(&node.id),
                    );
                }

                // Check required parameters for known tools
                Self::check_tool_params(&action.name, &action.params, &node.id, result);
            }
        }
    }

    /// Check required parameters for specific tools
    fn check_tool_params(
        tool_name: &str,
        params: &HashMap<String, String>,
        node_id: &str,
        result: &mut ValidationResult,
    ) {
        match tool_name {
            "chat" => {
                if !params.contains_key("message") {
                    result.add_error("E007", "chat() requires 'message' parameter", Some(node_id));
                }
                if !params.contains_key("agent") && !params.contains_key("system_prompt") {
                    result.add_warning(
                        "W005",
                        "chat() should have 'agent' or 'system_prompt' parameter",
                        Some(node_id),
                    );
                }
            }
            "p" => {
                if !params.contains_key("slug") && !params.contains_key("file") {
                    result.add_error(
                        "E008",
                        "p() requires 'slug' or 'file' parameter",
                        Some(node_id),
                    );
                }
            }
            "memory_search" => {
                if !params.contains_key("query") {
                    result.add_error(
                        "E009",
                        "memory_search() requires 'query' parameter",
                        Some(node_id),
                    );
                }
            }
            "history" => {
                if !params.contains_key("chat_id") {
                    result.add_error(
                        "E010",
                        "history() requires 'chat_id' parameter",
                        Some(node_id),
                    );
                }
            }
            "fetch" | "fetch_url" => {
                if !params.contains_key("url") {
                    result.add_error(
                        "E011",
                        &format!("{}() requires 'url' parameter", tool_name),
                        Some(node_id),
                    );
                }
            }
            "read_file" => {
                if !params.contains_key("file_path") {
                    result.add_error(
                        "E012",
                        "read_file() requires 'file_path' parameter",
                        Some(node_id),
                    );
                }
            }
            "write_file" => {
                if !params.contains_key("file_path") {
                    result.add_error(
                        "E012",
                        "write_file() requires 'file_path' parameter",
                        Some(node_id),
                    );
                }
                if !params.contains_key("content") {
                    result.add_error(
                        "E012",
                        "write_file() requires 'content' parameter",
                        Some(node_id),
                    );
                }
            }
            "edit_file" => {
                for req in &["file_path", "old_string", "new_string"] {
                    if !params.contains_key(*req) {
                        result.add_error(
                            "E012",
                            &format!("edit_file() requires '{}' parameter", req),
                            Some(node_id),
                        );
                    }
                }
            }
            "bash" | "sh" => {
                if !params.contains_key("command") && !params.contains_key("cmd") {
                    result.add_error(
                        "E013",
                        &format!("{}() requires 'command' or 'cmd' parameter", tool_name),
                        Some(node_id),
                    );
                }
            }
            "feishu_webhook" => {
                if !params.contains_key("message") {
                    result.add_error(
                        "E014",
                        "feishu_webhook() requires 'message' parameter",
                        Some(node_id),
                    );
                }
            }
            "notify" => {
                if !params.contains_key("message") && !params.contains_key("status") {
                    result.add_error(
                        "E015",
                        "notify() requires 'message' or 'status' parameter",
                        Some(node_id),
                    );
                }
            }
            _ => {}
        }
    }

    /// Check variable reference prefixes in task parameters + DAG predecessor validation
    fn check_variable_references(
        graph: &WorkflowGraph,
        parent_vars: &HashSet<String>,
        result: &mut ValidationResult,
    ) {
        let mut valid_prefixes: HashSet<String> =
            ["input", "output", "ctx", "reply", "error", "config", "bot"]
                .iter()
                .map(|s| s.to_string())
                .collect();

        // Include variables from parent scope (e.g. foreach item variables, function params)
        valid_prefixes.extend(parent_vars.iter().cloned());

        // Collect all node IDs as valid variable prefixes (for $node_id.output references)
        // Also add namespace root prefixes (e.g., "trading" from "trading.extract")
        let mut node_ids: HashSet<String> = HashSet::new();
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            valid_prefixes.insert(node.id.clone());
            node_ids.insert(node.id.clone());
            if let Some(root) = node.id.split('.').next() {
                valid_prefixes.insert(root.to_string());
            }
            if let NodeType::Foreach { item, .. } = &node.node_type {
                valid_prefixes.insert(item.clone());
            }
        }

        // Function names are also valid prefixes (in case someone references them)
        for func_name in graph.functions.keys() {
            valid_prefixes.insert(func_name.clone());
        }

        // Build topological order map for DAG predecessor checking
        // topo_order[node_id] = position in topological sort (lower = earlier)
        let topo_order: HashMap<String, usize> = match toposort(&graph.graph, None) {
            Ok(sorted) => sorted
                .iter()
                .enumerate()
                .map(|(i, &idx)| (graph.graph[idx].id.clone(), i))
                .collect(),
            Err(_) => HashMap::new(), // Cycle detected — skip DAG checks (covered by Check 2)
        };

        // Legacy $var.path regex (for resolver-produced references)
        let dollar_var_re = Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_.]*)").unwrap();
        // Bare identifier.path regex (new syntax: no $ prefix)
        // Matches identifier.path patterns not preceded by word chars or dots
        let bare_var_re =
            Regex::new(r"(?:^|[^a-zA-Z0-9_.])([a-zA-Z_][a-zA-Z0-9_]*\.[a-zA-Z0-9_.]+)").unwrap();
        let bare_start_re = Regex::new(r"^([a-zA-Z_][a-zA-Z0-9_]*\.[a-zA-Z0-9_.]+)").unwrap();

        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                for param_value in action.params.values() {
                    // Collect variable references from both legacy $var and bare var.path patterns
                    let mut var_refs: Vec<String> = Vec::new();

                    // Legacy $var.path references
                    for cap in dollar_var_re.captures_iter(param_value) {
                        var_refs.push(cap[1].to_string());
                    }

                    // Bare identifier.path references (not inside quotes)
                    // Strip quoted strings first, then match bare refs
                    let unquoted = strip_quoted_strings(param_value);
                    for cap in bare_var_re.captures_iter(&unquoted) {
                        let var_path = cap[1].to_string();
                        // Skip if already captured via $ prefix
                        if !var_refs.contains(&var_path) {
                            var_refs.push(var_path);
                        }
                    }
                    // Also match bare var.path at start of string
                    for cap in bare_start_re.captures_iter(&unquoted) {
                        let var_path = cap[1].to_string();
                        if !var_refs.contains(&var_path) {
                            var_refs.push(var_path);
                        }
                    }

                    for var_path in &var_refs {
                        let root = var_path.split('.').next().unwrap_or("");
                        if !valid_prefixes.contains(root) {
                            result.add_warning(
                                "W006",
                                &format!(
                                    "Variable '{}' has unknown prefix '{}'. Known: input, output, ctx, reply, error, config",
                                    var_path, root
                                ),
                                Some(&node.id),
                            );
                        } else if node_ids.contains(root) && !topo_order.is_empty() {
                            // root is a node ID — check DAG predecessor ordering
                            if let (Some(&ref_pos), Some(&cur_pos)) =
                                (topo_order.get(root), topo_order.get(&node.id))
                            {
                                if ref_pos >= cur_pos {
                                    result.add_warning(
                                        "W010",
                                        &format!(
                                            "Variable '{}' references node '{}' which is not a DAG predecessor of '{}' — output may not be available",
                                            var_path, root, node.id
                                        ),
                                        Some(&node.id),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check switch routes for missing default case and duplicate values
    fn check_switch_routes(graph: &WorkflowGraph, result: &mut ValidationResult) {
        for (source_id, route) in &graph.switch_routes {
            // Check for missing default case
            let has_default = route.cases.iter().any(|c| c.value.is_none());
            if !has_default {
                result.add_warning(
                    "W007",
                    &format!(
                        "Switch at [{}] has no 'default' case. Unmatched values will have no route.",
                        source_id
                    ),
                    Some(source_id),
                );
            }

            // Check for duplicate case values
            let mut seen: HashSet<&str> = HashSet::new();
            for case in &route.cases {
                if let Some(ref val) = case.value {
                    if !seen.insert(val.as_str()) {
                        result.add_warning(
                            "W008",
                            &format!(
                                "Switch at [{}] has duplicate case value '{}'",
                                source_id, val
                            ),
                            Some(source_id),
                        );
                    }
                }
            }
        }
    }

    /// Check edge conditions are not empty
    fn check_edge_conditions(graph: &WorkflowGraph, result: &mut ValidationResult) {
        for edge in graph.graph.edge_weights() {
            if let Some(ref condition) = edge.condition {
                if condition.trim().is_empty() {
                    result.add_warning("W009", "Edge has an empty condition expression", None);
                }
            }
        }
    }

    /// Recursively validate nested workflows (foreach, loop bodies)
    fn check_nested_workflows(
        graph: &WorkflowGraph,
        parent_vars: &HashSet<String>,
        result: &mut ValidationResult,
    ) {
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];

            match &node.node_type {
                NodeType::Foreach { item, body, .. } => {
                    // Pass foreach item variable to nested scope
                    let mut nested_vars = parent_vars.clone();
                    nested_vars.insert(item.clone());
                    let nested_result = Self::validate_with_scope(body, &nested_vars, None);
                    for err in nested_result.errors {
                        result.add_error(
                            &format!("{}/nested", err.code),
                            &format!("[in foreach '{}'] {}", node.id, err.message),
                            err.node_id.as_deref(),
                        );
                    }
                    for warn in nested_result.warnings {
                        result.add_warning(
                            &format!("{}/nested", warn.code),
                            &format!("[in foreach '{}'] {}", node.id, warn.message),
                            warn.node_id.as_deref(),
                        );
                    }
                }
                NodeType::Loop { body, .. } => {
                    let nested_result = Self::validate_with_scope(body, parent_vars, None);
                    for err in nested_result.errors {
                        result.add_error(
                            &format!("{}/nested", err.code),
                            &format!("[in loop '{}'] {}", node.id, err.message),
                            err.node_id.as_deref(),
                        );
                    }
                    for warn in nested_result.warnings {
                        result.add_warning(
                            &format!("{}/nested", warn.code),
                            &format!("[in loop '{}'] {}", node.id, warn.message),
                            warn.node_id.as_deref(),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    /// Check 11: Validate function definitions — recursively validate function body sub-graphs
    fn check_function_definitions(
        graph: &WorkflowGraph,
        parent_vars: &HashSet<String>,
        result: &mut ValidationResult,
    ) {
        for (func_name, func_def) in &graph.functions {
            // Function params become valid variable prefixes in the function body
            let mut func_vars = parent_vars.clone();
            for param in &func_def.params {
                func_vars.insert(param.clone());
            }

            let nested_result = Self::validate_with_scope(&func_def.body, &func_vars, None);
            for err in nested_result.errors {
                result.add_error(
                    &format!("{}/nested", err.code),
                    &format!("[in function '{}'] {}", func_name, err.message),
                    err.node_id.as_deref(),
                );
            }
            for warn in nested_result.warnings {
                result.add_warning(
                    &format!("{}/nested", warn.code),
                    &format!("[in function '{}'] {}", func_name, warn.message),
                    warn.node_id.as_deref(),
                );
            }
        }
    }

    /// Check 12: Validate function call arguments — param count and name matching
    fn check_function_calls(graph: &WorkflowGraph, result: &mut ValidationResult) {
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                if let Some(func_def) = graph.functions.get(&action.name) {
                    let expected_params: HashSet<&str> =
                        func_def.params.iter().map(|s| s.as_str()).collect();
                    let actual_params: HashSet<&str> =
                        action.params.keys().map(|s| s.as_str()).collect();

                    // Check param count — too many is an error; fewer is allowed (optional params)
                    if actual_params.len() > expected_params.len() {
                        result.add_error(
                            "E017",
                            &format!(
                                "Function '{}' expects at most {} parameter(s) ({}) but got {}",
                                action.name,
                                expected_params.len(),
                                func_def.params.join(", "),
                                actual_params.len()
                            ),
                            Some(&node.id),
                        );
                    }

                    // Check unknown param names
                    for param_name in &actual_params {
                        if !expected_params.contains(param_name) {
                            result.add_error(
                                "E018",
                                &format!(
                                    "Function '{}' has no parameter named '{}'. Expected: {}",
                                    action.name,
                                    param_name,
                                    func_def.params.join(", ")
                                ),
                                Some(&node.id),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Check 13: Validate on_tool=[node] references in chat() nodes
    fn check_on_tool_references(graph: &WorkflowGraph, result: &mut ValidationResult) {
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                if action.name == "chat" {
                    if let Some(on_tool_val) = action.params.get("on_tool") {
                        let trimmed = on_tool_val.trim();
                        if trimmed.starts_with('[') && trimmed.ends_with(']') {
                            let ref_name = &trimmed[1..trimmed.len() - 1];
                            if !graph.node_map.contains_key(ref_name)
                                && !graph.functions.contains_key(ref_name)
                            {
                                result.add_error(
                                    "E019",
                                    &format!(
                                        "on_tool=[{}] references '{}' which is neither a node nor a function in this workflow",
                                        ref_name, ref_name
                                    ),
                                    Some(&node.id),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check 14: Validate agent slug references in chat() nodes
    fn check_agent_references(
        graph: &WorkflowGraph,
        project: &ProjectContext,
        result: &mut ValidationResult,
    ) {
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                if action.name == "chat" {
                    if let Some(agent_val) = action.params.get("agent") {
                        let slug = agent_val.trim().trim_matches('"');
                        // Skip variable references
                        if !slug.starts_with('$') && !project.agent_slugs.contains(slug) {
                            result.add_warning(
                                "W012",
                                &format!(
                                    "chat(agent=\"{}\") — agent slug '{}' not found in project .jgagent files",
                                    slug, slug
                                ),
                                Some(&node.id),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Check 15: Validate prompt slug references in p() nodes
    fn check_prompt_references(
        graph: &WorkflowGraph,
        project: &ProjectContext,
        result: &mut ValidationResult,
    ) {
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                if action.name == "p" {
                    if let Some(slug_val) = action.params.get("slug") {
                        let slug = slug_val.trim().trim_matches('"');
                        if !slug.starts_with('$') && !project.prompt_slugs.contains(slug) {
                            result.add_warning(
                                "W013",
                                &format!(
                                    "p(slug=\"{}\") — prompt slug '{}' not found in project .jgprompt files",
                                    slug, slug
                                ),
                                Some(&node.id),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Check 16: Validate flow import paths exist
    fn check_flow_imports(
        graph: &WorkflowGraph,
        project: &ProjectContext,
        result: &mut ValidationResult,
    ) {
        for (alias, rel_path) in &graph.flow_imports {
            let path = rel_path.trim().trim_matches('"');
            let abs_path = if std::path::Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else if !project.file_dir.as_os_str().is_empty() {
                project.file_dir.join(path)
            } else {
                project.base_dir.join(path)
            };
            if !abs_path.exists() {
                result.add_error(
                    "E020",
                    &format!(
                        "Flow import '{}' references path '{}' which does not exist",
                        alias, rel_path
                    ),
                    None,
                );
            }
        }
    }

    /// Check 18: Validate lib import paths exist
    #[cfg(not(target_arch = "wasm32"))]
    fn check_lib_imports(
        graph: &WorkflowGraph,
        project: &ProjectContext,
        result: &mut ValidationResult,
    ) {
        use crate::registry::package::is_registry_import;

        for (namespace, rel_path) in &graph.lib_imports {
            let path = rel_path.trim().trim_matches('"');

            // Embedded stdlib (std/ prefix) — skip if found in compiled-in stdlib
            if path.starts_with("std/") {
                let stdlib_name = path.strip_prefix("std/").unwrap().trim_end_matches(".jg");
                if crate::core::stdlib::get(stdlib_name).is_some() {
                    continue;
                }
            }

            // Registry packages — check jg_modules/ if available, otherwise warn
            if is_registry_import(path) {
                let jg_modules_path = project.base_dir.join("jg_modules").join(namespace);
                if !jg_modules_path.exists() {
                    result.add_warning(
                        "W022",
                        &format!(
                            "Registry package '{}' is not installed. Run 'juglans add {}' or it will be auto-installed at runtime.",
                            rel_path, rel_path
                        ),
                        None,
                    );
                }
                continue;
            }

            // Local file path validation (existing logic)
            let abs_path = if std::path::Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else if !project.file_dir.as_os_str().is_empty() {
                project.file_dir.join(path)
            } else {
                project.base_dir.join(path)
            };
            if !abs_path.exists() {
                result.add_error(
                    "E021",
                    &format!(
                        "Lib import '{}' references path '{}' which does not exist",
                        namespace, rel_path
                    ),
                    None,
                );
            }
        }
    }

    /// Check 17: Validate resource glob patterns match at least one file
    fn check_resource_patterns(
        graph: &WorkflowGraph,
        project: &ProjectContext,
        result: &mut ValidationResult,
    ) {
        let all_patterns: Vec<(&str, &str)> = graph
            .prompt_patterns
            .iter()
            .map(|p| (p.as_str(), "prompts"))
            .chain(graph.agent_patterns.iter().map(|p| (p.as_str(), "agents")))
            .chain(graph.tool_patterns.iter().map(|p| (p.as_str(), "tools")))
            .collect();

        for (pattern, kind) in all_patterns {
            // Skip variable patterns and @/ patterns (can't resolve without runtime context)
            if pattern.starts_with('$') || pattern.starts_with("@/") {
                continue;
            }
            // Resolve relative patterns against the .jg file's directory
            let resolved = if !std::path::Path::new(pattern).is_absolute()
                && !project.file_dir.as_os_str().is_empty()
            {
                project.file_dir.join(pattern).to_string_lossy().to_string()
            } else {
                pattern.to_string()
            };
            match glob::glob(&resolved) {
                Ok(paths) => {
                    if paths.count() == 0 {
                        result.add_warning(
                            "W014",
                            &format!("{} pattern '{}' did not match any files", kind, pattern),
                            None,
                        );
                    }
                }
                Err(_) => {
                    result.add_warning(
                        "W014",
                        &format!("{} pattern '{}' is not a valid glob", kind, pattern),
                        None,
                    );
                }
            }
        }
    }
}

/// Strip double-quoted strings from a value, replacing them with spaces.
/// This allows regex matching on only the non-quoted (variable reference) parts.
fn strip_quoted_strings(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_quote = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && in_quote {
            // Skip escaped character inside quotes
            chars.next();
            result.push(' ');
            result.push(' ');
        } else if c == '"' {
            in_quote = !in_quote;
            result.push(' ');
        } else if in_quote {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::parser::GraphParser;

    #[test]
    fn test_valid_workflow() {
        let content = r#"
[start]: chat(agent="default", message="hello")
[end]: notify(message="done")

[start] -> [end]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(result.is_valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn test_missing_required_param_chat() {
        let content = r#"
[start]: chat(agent="test")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "E007"));
    }

    #[test]
    fn test_missing_required_param_fetch() {
        let content = r#"
[start]: fetch(method="GET")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "E011"));
    }

    #[test]
    fn test_unknown_variable_prefix() {
        let content = r#"
[start]: notify(message=unknown.var)
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(result.warnings.iter().any(|w| w.code == "W006"));
    }

    #[test]
    fn test_valid_variable_prefix() {
        let content = r#"
[start]: notify(message=input.query)
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.warnings.iter().any(|w| w.code == "W006"));
    }

    #[test]
    fn test_switch_missing_default() {
        let content = r#"
[start]: notify(message="test")
[a]: notify(message="a")
[b]: notify(message="b")

[start] -> switch type {
    "a": [a]
    "b": [b]
}
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(result.warnings.iter().any(|w| w.code == "W007"));
    }

    #[test]
    fn test_switch_with_default_no_warning() {
        let content = r#"
[start]: notify(message="test")
[a]: notify(message="a")
[fallback]: notify(message="fb")

[start] -> switch type {
    "a": [a]
    default: [fallback]
}
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.warnings.iter().any(|w| w.code == "W007"));
    }

    // =========================================================================
    // Function Node Validation (Check 11-13)
    // =========================================================================

    #[test]
    fn test_function_call_correct_params() {
        let content = r#"
[greet(name)]: bash(command="echo " + name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        // No E017/E018 errors
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.code == "E017" || e.code == "E018"),
            "errors: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_function_call_too_many_params() {
        let content = r#"
[greet(name)]: bash(command="echo " + name)
[step1]: greet(name="world", extra="oops")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == "E017" || e.code == "E018"),
            "Expected E017/E018 for too many params, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_function_call_fewer_params_allowed() {
        let content = r#"
[greet(name, greeting)]: bash(command="echo " + name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            !result.errors.iter().any(|e| e.code == "E017"),
            "Fewer params should be allowed (optional params), got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_function_call_unknown_param_name() {
        let content = r#"
[greet(name)]: bash(command="echo " + name)
[step1]: greet(unknown="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            result.errors.iter().any(|e| e.code == "E018"),
            "Expected E018 for unknown param name, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_function_not_w004_unknown() {
        // Function calls should NOT trigger W004 (unknown tool)
        let content = r#"
[greet(name)]: bash(command="echo " + name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.code == "W004" && w.message.contains("greet")),
            "Function call should not trigger W004, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_function_body_validation() {
        // Function body with unknown variable should produce nested warning
        let content = r#"
[bad_func(x)]: bash(command=unknown_var.path)
[step1]: bad_func(x="test")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.code.starts_with("W006") && w.message.contains("function")),
            "Expected nested W006 in function body, got warnings: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_function_body_valid_param_usage() {
        // Function parameters should be valid variable prefixes in body
        let content = r#"
[greet(name)]: bash(command=name.value)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        // No W006 about "name" being unknown
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.code.starts_with("W006") && w.message.contains("name")),
            "Function param should be valid prefix, got: {:?}",
            result.warnings
        );
    }

    // =========================================================================
    // on_tool=[node] Validation (Check 13)
    // =========================================================================

    #[test]
    fn test_on_tool_valid_reference() {
        let content = r#"
[handler]: bash(command="echo tool")
[chat_node]: chat(agent="test", message="hi", on_tool=[handler])
[handler] -> [chat_node]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            !result.errors.iter().any(|e| e.code == "E019"),
            "on_tool=[handler] should be valid, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_on_tool_invalid_reference() {
        let content = r#"
[chat_node]: chat(agent="test", message="hi", on_tool=[nonexistent])
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            result.errors.iter().any(|e| e.code == "E019"),
            "Expected E019 for nonexistent on_tool ref, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_on_tool_function_reference() {
        let content = r#"
[handle(name, args)]: bash(command="echo " + name)
[chat_node]: chat(agent="test", message="hi", on_tool=[handle])
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            !result.errors.iter().any(|e| e.code == "E019"),
            "on_tool=[handle] referencing function should be valid, got: {:?}",
            result.errors
        );
    }

    // =========================================================================
    // DAG Predecessor Variable Validation (Enhanced Check 8)
    // =========================================================================

    #[test]
    fn test_variable_dag_predecessor_valid() {
        let content = r#"
[step1]: notify(message="hello")
[step2]: notify(message=step1.output)
[step1] -> [step2]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            !result.warnings.iter().any(|w| w.code == "W010"),
            "step1 is predecessor of step2, no W010 expected. Got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_variable_dag_predecessor_invalid() {
        // step2 references step3 which comes AFTER it in the DAG
        let content = r#"
[step1]: notify(message="hello")
[step2]: notify(message=step3.output)
[step3]: notify(message="world")
[step1] -> [step2] -> [step3]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            result.warnings.iter().any(|w| w.code == "W010"),
            "step3 is NOT predecessor of step2, W010 expected. Got: {:?}",
            result.warnings
        );
    }

    // =========================================================================
    // Cross-file Validation (Check 14-17)
    // =========================================================================

    #[test]
    fn test_agent_reference_found() {
        let content = r#"
[start]: chat(agent="my-agent", message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let mut project = ProjectContext::default();
        project.agent_slugs.insert("my-agent".to_string());
        let result = WorkflowValidator::validate_with_project(&graph, &project);
        assert!(
            !result.warnings.iter().any(|w| w.code == "W012"),
            "Agent exists, no W012. Got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_agent_reference_not_found() {
        let content = r#"
[start]: chat(agent="missing-agent", message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let project = ProjectContext::default();
        let result = WorkflowValidator::validate_with_project(&graph, &project);
        assert!(
            result.warnings.iter().any(|w| w.code == "W012"),
            "Agent missing, W012 expected. Got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_prompt_reference_not_found() {
        let content = r#"
[start]: p(slug="missing-prompt", file="x")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let project = ProjectContext::default();
        let result = WorkflowValidator::validate_with_project(&graph, &project);
        assert!(
            result.warnings.iter().any(|w| w.code == "W013"),
            "Prompt missing, W013 expected. Got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn test_flow_import_missing_path() {
        let content = r#"
flows: { auth: "./nonexistent.jg" }
[start]: notify(message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let project = ProjectContext {
            base_dir: PathBuf::from("/tmp/nonexistent_dir"),
            ..Default::default()
        };
        let result = WorkflowValidator::validate_with_project(&graph, &project);
        assert!(
            result.errors.iter().any(|e| e.code == "E020"),
            "Flow path doesn't exist, E020 expected. Got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_known_tools_no_false_positive() {
        // execute_workflow, serve, response, db.find should be known
        let content = r#"
[start]: execute_workflow(path="x")
[srv]: serve(port="8080")
[resp]: response(body="ok")
[db]: db.find(table="users", where={})
[start] -> [srv] -> [resp] -> [db]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        let unknown_w004: Vec<_> = result
            .warnings
            .iter()
            .filter(|w| w.code == "W004")
            .collect();
        assert!(
            unknown_w004.is_empty(),
            "execute_workflow/serve/response/db.find should be known. W004s: {:?}",
            unknown_w004
        );
    }

    #[test]
    fn test_multi_step_function_validation() {
        let content = r#"
[build(dir)]: {
  bash(command="cd " + dir + " && make");
  bash(command="cd " + dir + " && test")
}
[step1]: build(dir="/app")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        // No E017/E018 errors, no W004 for "build"
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.code == "E017" || e.code == "E018"),
            "Multi-step function call should be valid. Errors: {:?}",
            result.errors
        );
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.code == "W004" && w.message.contains("build")),
            "build should be known function. Warnings: {:?}",
            result.warnings
        );
    }

    // =========================================================================
    // Lib Import Validation (Check 18)
    // =========================================================================

    #[test]
    fn test_lib_import_missing_path() {
        let content = r#"
libs: { mylib: "./nonexistent_lib.jg" }
[start]: notify(message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let project = ProjectContext {
            base_dir: PathBuf::from("/tmp/nonexistent_dir"),
            ..Default::default()
        };
        let result = WorkflowValidator::validate_with_project(&graph, &project);
        assert!(
            result.errors.iter().any(|e| e.code == "E021"),
            "Lib path doesn't exist, E021 expected. Got: {:?}",
            result.errors
        );
    }

    // =========================================================================
    // Lib Import Parser Tests
    // =========================================================================

    #[test]
    fn test_parse_libs_map_form() {
        let content = r#"
libs: { db: "./libs/sqlite.jg", http: "./libs/http.jg" }
[start]: notify(message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert_eq!(graph.lib_imports.len(), 2);
        assert_eq!(graph.lib_imports.get("db").unwrap(), "./libs/sqlite.jg");
        assert_eq!(graph.lib_imports.get("http").unwrap(), "./libs/http.jg");
        // Map form should NOT be in auto_namespaces
        assert!(!graph.lib_auto_namespaces.contains("db"));
        assert!(!graph.lib_auto_namespaces.contains("http"));
    }

    #[test]
    fn test_parse_libs_list_form() {
        let content = r#"
libs: ["./libs/sqlite.jg", "./libs/http_client.jg"]
[start]: notify(message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert_eq!(graph.lib_imports.len(), 2);
        // List form: namespace = file stem
        assert!(graph.lib_imports.contains_key("sqlite"));
        assert!(graph.lib_imports.contains_key("http_client"));
        // List form should be in auto_namespaces (can be overridden by slug)
        assert!(graph.lib_auto_namespaces.contains("sqlite"));
        assert!(graph.lib_auto_namespaces.contains("http_client"));
    }
}
