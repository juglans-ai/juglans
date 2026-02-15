// src/core/validator.rs
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::Direction;
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::core::graph::{NodeType, WorkflowGraph};

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
        Self::validate_with_scope(graph, &HashSet::new())
    }

    /// Validate with additional variable prefixes from parent scope (e.g. foreach item)
    fn validate_with_scope(
        graph: &WorkflowGraph,
        parent_vars: &HashSet<String>,
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

        // Check 8: Validate variable reference prefixes
        Self::check_variable_references(graph, parent_vars, &mut result);

        // Check 9: Validate switch routes
        Self::check_switch_routes(graph, &mut result);

        // Check 10: Validate edge conditions
        Self::check_edge_conditions(graph, &mut result);

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
        let known_tools: HashSet<&str> = [
            // AI tools
            "chat",
            "p",
            "memory_search",
            "history",
            // Network tools
            "fetch_url",
            "fetch",
            // System tools
            "timer",
            "notify",
            "reply",
            "set_context",
            "feishu_webhook",
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
                    result.add_error(
                        "E007",
                        "chat() requires 'message' parameter",
                        Some(node_id),
                    );
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

    /// Check variable reference prefixes in task parameters
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

        // Include variables from parent scope (e.g. foreach item variables)
        valid_prefixes.extend(parent_vars.iter().cloned());

        // Collect all node IDs as valid variable prefixes (for $node_id.output references)
        // Also add namespace root prefixes (e.g., "trading" from "trading.extract")
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            valid_prefixes.insert(node.id.clone());
            if let Some(root) = node.id.split('.').next() {
                valid_prefixes.insert(root.to_string());
            }
            if let NodeType::Foreach { item, .. } = &node.node_type {
                valid_prefixes.insert(item.clone());
            }
        }

        let var_re = Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_.]*)").unwrap();

        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if let NodeType::Task(action) = &node.node_type {
                for (_param_name, param_value) in &action.params {
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
                    result.add_warning(
                        "W009",
                        "Edge has an empty condition expression",
                        None,
                    );
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
                    let nested_result = Self::validate_with_scope(body, &nested_vars);
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
                    let nested_result = Self::validate_with_scope(body, parent_vars);
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
}
