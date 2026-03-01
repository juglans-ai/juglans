// src/core/validator.rs
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::Direction;
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::core::graph::{NodeType, WorkflowGraph};

/// 项目级资源上下文（跨文件校验用）
/// 由 handle_check 两遍扫描后构建，传入 validate_with_project
#[derive(Debug, Default)]
pub struct ProjectContext {
    /// 所有 .jgagent 中解析出的 slug
    pub agent_slugs: HashSet<String>,
    /// 所有 .jgprompt 中解析出的 slug
    pub prompt_slugs: HashSet<String>,
    /// 所有 .jg/.jgflow 的绝对路径
    pub flow_paths: HashSet<PathBuf>,
    /// check 目录基准路径
    pub base_dir: PathBuf,
    /// 当前文件所在目录（用于解析相对路径，如 flows: ./trading.jg）
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

        // Check 4: Exit nodes reachability
        Self::check_exit_nodes(graph, &mut result);

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
            Self::check_resource_patterns(graph, &mut result);

            // Check 18: Lib import paths
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

            // Check which nodes are unreachable
            for idx in graph.graph.node_indices() {
                if !reachable.contains(&idx) {
                    let node = &graph.graph[idx];
                    result.add_warning(
                        "W002",
                        &format!("Node '{}' is not reachable from entry node", node.id),
                        Some(&node.id),
                    );
                }
            }
        }
    }

    /// Check if exit nodes exist and are reachable
    fn check_exit_nodes(graph: &WorkflowGraph, result: &mut ValidationResult) {
        for exit_id in &graph.exit_nodes {
            if !graph.node_map.contains_key(exit_id) {
                result.add_error(
                    "E003",
                    &format!("Exit node '{}' does not exist in the graph", exit_id),
                    Some(exit_id),
                );
            }
        }

        // If no exit nodes defined, check for terminal nodes (no outgoing edges)
        if graph.exit_nodes.is_empty() && graph.graph.node_count() > 0 {
            let terminal_nodes: Vec<_> = graph
                .graph
                .node_indices()
                .filter(|&idx| {
                    graph
                        .graph
                        .neighbors_directed(idx, Direction::Outgoing)
                        .count()
                        == 0
                })
                .map(|idx| graph.graph[idx].id.clone())
                .collect();

            if terminal_nodes.is_empty() && graph.graph.node_count() > 1 {
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
            // System tools
            "timer",
            "notify",
            "reply",
            "set_context",
            "feishu_webhook",
            // HTTP backend tools
            "http_serve",
            "http_response",
            // Devtools
            "read_file",
            "write_file",
            "edit_file",
            "glob",
            "grep",
            "bash",
            "sh",
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
                if !known_tools.contains(action.name.as_str()) {
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

        let var_re = Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_.]*)").unwrap();

        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                for param_value in action.params.values() {
                    for cap in var_re.captures_iter(param_value) {
                        let var_path = &cap[1];
                        let root = var_path.split('.').next().unwrap_or("");
                        if !valid_prefixes.contains(root) {
                            result.add_warning(
                                "W006",
                                &format!(
                                    "Variable '${}' has unknown prefix '{}'. Known: $input, $output, $ctx, $reply, $error, $config",
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
                                            "Variable '${}' references node '{}' which is not a DAG predecessor of '{}' — output may not be available",
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

                    // Check param count
                    if actual_params.len() != expected_params.len() {
                        result.add_error(
                            "E017",
                            &format!(
                                "Function '{}' expects {} parameter(s) ({}) but got {}",
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
    fn check_lib_imports(
        graph: &WorkflowGraph,
        project: &ProjectContext,
        result: &mut ValidationResult,
    ) {
        use crate::registry::package::is_registry_import;

        for (namespace, rel_path) in &graph.lib_imports {
            let path = rel_path.trim().trim_matches('"');

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
    fn check_resource_patterns(graph: &WorkflowGraph, result: &mut ValidationResult) {
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
            match glob::glob(pattern) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::parser::GraphParser;

    #[test]
    fn test_valid_workflow() {
        let content = r#"
name: "Test Workflow"
entry: [start]

[start]: chat(agent="default", message="hello")
[end]: notify(message="done")

[start] -> [end]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(result.is_valid, "errors: {:?}", result.errors);
    }

    #[test]
    fn test_missing_entry() {
        let content = r#"
name: "Test Workflow"
entry: [missing]

[start]: chat(agent="default", message="hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "E001"));
    }

    #[test]
    fn test_missing_required_param_chat() {
        let content = r#"
name: "Test"
entry: [start]
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
name: "Test"
entry: [start]
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
name: "Test"
entry: [start]
[start]: notify(message=$unknown.var)
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(result.warnings.iter().any(|w| w.code == "W006"));
    }

    #[test]
    fn test_valid_variable_prefix() {
        let content = r#"
name: "Test"
entry: [start]
[start]: notify(message=$input.query)
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.warnings.iter().any(|w| w.code == "W006"));
    }

    #[test]
    fn test_switch_missing_default() {
        let content = r#"
name: "Test"
entry: [start]

[start]: notify(message="test")
[a]: notify(message="a")
[b]: notify(message="b")

[start] -> switch $type {
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
name: "Test"
entry: [start]

[start]: notify(message="test")
[a]: notify(message="a")
[fallback]: notify(message="fb")

[start] -> switch $type {
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
name: "Test"
entry: [step1]
[greet(name)]: bash(command="echo " + $name)
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
    fn test_function_call_wrong_param_count() {
        let content = r#"
name: "Test"
entry: [step1]
[greet(name, greeting)]: bash(command="echo " + $greeting + " " + $name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(
            result.errors.iter().any(|e| e.code == "E017"),
            "Expected E017 for param count mismatch, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_function_call_unknown_param_name() {
        let content = r#"
name: "Test"
entry: [step1]
[greet(name)]: bash(command="echo " + $name)
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
name: "Test"
entry: [step1]
[greet(name)]: bash(command="echo " + $name)
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
name: "Test"
entry: [step1]
[bad_func(x)]: bash(command=$unknown_var)
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
name: "Test"
entry: [step1]
[greet(name)]: bash(command=$name)
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
name: "Test"
entry: [handler]
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
name: "Test"
entry: [chat_node]
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
name: "Test"
entry: [chat_node]
[handle(name, args)]: bash(command="echo " + $name)
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
name: "Test"
entry: [step1]
[step1]: notify(message="hello")
[step2]: notify(message=$step1.output)
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
name: "Test"
entry: [step1]
[step1]: notify(message="hello")
[step2]: notify(message=$step3.output)
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
name: "Test"
entry: [start]
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
name: "Test"
entry: [start]
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
name: "Test"
entry: [start]
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
name: "Test"
flows: { auth: "./nonexistent.jg" }
entry: [start]
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
        // execute_workflow, http_serve, http_response should be known
        let content = r#"
name: "Test"
entry: [start]
[start]: execute_workflow(path="x")
[serve]: http_serve(port="8080")
[resp]: http_response(body="ok")
[start] -> [serve] -> [resp]
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
            "execute_workflow/http_serve/http_response should be known. W004s: {:?}",
            unknown_w004
        );
    }

    #[test]
    fn test_multi_step_function_validation() {
        let content = r#"
name: "Test"
entry: [step1]
[build(dir)]: {
  bash(command="cd " + $dir + " && make");
  bash(command="cd " + $dir + " && test")
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
name: "Test"
libs: { mylib: "./nonexistent_lib.jg" }
entry: [start]
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
name: "Test"
libs: { db: "./libs/sqlite.jg", http: "./libs/http.jg" }
entry: [start]
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
name: "Test"
libs: ["./libs/sqlite.jg", "./libs/http_client.jg"]
entry: [start]
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
