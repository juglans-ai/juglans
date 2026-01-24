// src/core/validator.rs
use std::collections::{HashSet, HashMap};
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::Direction;
use serde::Serialize;

use crate::core::graph::{WorkflowGraph, NodeType};

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
        Self::check_nested_workflows(graph, &mut result);

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
                &format!("Entry node '{}' does not exist in the graph", graph.entry_node),
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
                        &format!("Cycle detected involving node '{}'. Workflows must be acyclic (DAG).", node.id),
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
            graph.graph.node_indices().next()
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
            let terminal_nodes: Vec<_> = graph.graph.node_indices()
                .filter(|&idx| graph.graph.neighbors_directed(idx, Direction::Outgoing).count() == 0)
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
            result.add_error(
                "E004",
                "Workflow contains no nodes",
                None,
            );
        }
    }

    /// Check node references for known tools
    fn check_node_references(graph: &WorkflowGraph, result: &mut ValidationResult) {
        // Known built-in tools
        let known_tools: HashSet<&str> = [
            // Core AI tools
            "chat", "prompt", "p",
            // HTTP & IO
            "http", "log", "emit",
            // Context & State
            "set", "get", "script",
            // Control flow
            "if", "switch", "parallel", "sleep", "retry", "cache",
            // Data transformation
            "json_parse", "json_stringify", "template", "transform", "render",
            // Notifications
            "notify",
        ].iter().copied().collect();

        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];

            if let NodeType::Task(action) = &node.node_type {
                // Check if tool is known (warning only, as custom tools are allowed)
                if !known_tools.contains(action.name.as_str()) {
                    result.add_warning(
                        "W004",
                        &format!("Unknown tool '{}'. Ensure it's defined in MCP servers or libs.", action.name),
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
                if !params.contains_key("agent") && !params.contains_key("system_prompt") {
                    result.add_warning(
                        "W005",
                        "chat tool should have 'agent' or 'system_prompt' parameter",
                        Some(node_id),
                    );
                }
            }
            "prompt" => {
                if !params.contains_key("slug") && !params.contains_key("content") {
                    result.add_error(
                        "E005",
                        "prompt tool requires 'slug' or 'content' parameter",
                        Some(node_id),
                    );
                }
            }
            "http" => {
                if !params.contains_key("url") {
                    result.add_error(
                        "E006",
                        "http tool requires 'url' parameter",
                        Some(node_id),
                    );
                }
            }
            _ => {}
        }
    }

    /// Recursively validate nested workflows (foreach, loop bodies)
    fn check_nested_workflows(graph: &WorkflowGraph, result: &mut ValidationResult) {
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];

            match &node.node_type {
                NodeType::Foreach { body, .. } => {
                    let nested_result = Self::validate(body);
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
                    let nested_result = Self::validate(body);
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
slug: "test"
name: "Test Workflow"
entry: [start]

[start] = chat(agent="default", message="hello")
[end] = log(message="done")

start -> end
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(result.is_valid);
    }

    #[test]
    fn test_missing_entry() {
        let content = r#"
slug: "test"
entry: [missing]

[start] = chat(agent="default")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let result = WorkflowValidator::validate(&graph);
        assert!(!result.is_valid);
        assert!(result.errors.iter().any(|e| e.code == "E001"));
    }
}
