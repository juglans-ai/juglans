// src/testing/mod.rs
//
// Test runner for `juglans test`.
// Discovers `test_*` nodes in .jg files and executes them as independent subgraphs.

pub mod reporter;

use anyhow::{anyhow, Result};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

use crate::core::context::WorkflowContext;
use crate::core::executor::WorkflowExecutor;
use crate::core::graph::{self, WorkflowGraph};
use crate::core::parser::GraphParser;
use crate::services::interface::JuglansRuntime;
use crate::services::prompt_loader::PromptRegistry;

/// Result of a single test case
#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration: Duration,
    pub error: Option<String>,
    pub assertions: usize,
    pub failed_assertions: Vec<String>,
}

/// Result of running all tests in a single file
#[derive(Debug)]
pub struct FileTestResult {
    pub path: PathBuf,
    pub results: Vec<TestResult>,
}

impl FileTestResult {
    pub fn passed_count(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    pub fn failed_count(&self) -> usize {
        self.results.iter().filter(|r| !r.passed).count()
    }

    pub fn _total_duration(&self) -> Duration {
        self.results.iter().map(|r| r.duration).sum()
    }
}

/// Main test runner
pub struct TestRunner {
    runtime: Arc<dyn JuglansRuntime>,
}

impl TestRunner {
    pub fn new(runtime: Arc<dyn JuglansRuntime>, _prompt_registry: Arc<PromptRegistry>) -> Self {
        Self { runtime }
    }

    /// Run all tests in a single .jg file
    pub async fn run_file_filtered(
        &self,
        path: &Path,
        filter: Option<&str>,
    ) -> Result<FileTestResult> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read {}: {}", path.display(), e))?;

        let workflow = GraphParser::parse(&content)
            .map_err(|e| anyhow!("Failed to parse {}: {}", path.display(), e))?;

        // Find test roots: test_* nodes with in-degree == 0
        let test_roots = find_test_roots(&workflow);

        if test_roots.is_empty() {
            return Ok(FileTestResult {
                path: path.to_path_buf(),
                results: Vec::new(),
            });
        }

        // Load prompts declared in the test file
        let base_dir = path.parent().unwrap_or(Path::new("."));
        let mut prompt_registry = PromptRegistry::new();

        if !workflow.prompt_patterns.is_empty() {
            let resolved: Vec<String> = workflow
                .prompt_patterns
                .iter()
                .map(|p| base_dir.join(p).to_string_lossy().to_string())
                .collect();
            let _ = prompt_registry.load_from_paths(&resolved);
        }

        // Create executor with loaded registries
        let executor =
            Arc::new(WorkflowExecutor::new(Arc::new(prompt_registry), self.runtime.clone()).await);
        executor
            .get_registry()
            .set_executor(Arc::downgrade(&executor));

        let workflow_arc = Arc::new(workflow);

        // Run each test subgraph (skip non-matching if filter is set)
        let mut results = Vec::new();
        for root_idx in &test_roots {
            let root_name = workflow_arc.graph[*root_idx].id.clone();
            if let Some(f) = filter {
                if !root_name.contains(f) {
                    continue;
                }
            }
            let subgraph_nodes = collect_subgraph(&workflow_arc, *root_idx);
            let result = self
                .run_subgraph_test(&executor, &workflow_arc, &root_name, &subgraph_nodes)
                .await;
            results.push(result);
        }

        Ok(FileTestResult {
            path: path.to_path_buf(),
            results,
        })
    }

    /// Run a single test subgraph
    async fn run_subgraph_test(
        &self,
        executor: &Arc<WorkflowExecutor>,
        workflow: &Arc<WorkflowGraph>,
        root_name: &str,
        subgraph_nodes: &[NodeIndex],
    ) -> TestResult {
        let started = Instant::now();
        let context = WorkflowContext::new();

        // Build a sub-workflow containing only the test nodes
        let sub_workflow = extract_subworkflow(workflow, subgraph_nodes);
        let sub_arc = Arc::new(sub_workflow);

        debug!(
            "Running test: {} ({} nodes)",
            root_name,
            subgraph_nodes.len()
        );

        // Execute the sub-DAG
        let exec_result = executor.clone().execute_graph(sub_arc, &context).await;

        // Count assertions from trace
        let trace = context.trace_entries();
        let assertions = trace.iter().filter(|e| e.tool == "assert").count();

        // Collect all failed assertions
        let failed_assertions: Vec<String> = trace
            .iter()
            .filter(|e| {
                e.tool == "assert"
                    && matches!(e.status, crate::core::context::TraceStatus::Error(_))
            })
            .map(|e| match &e.status {
                crate::core::context::TraceStatus::Error(msg) => msg.clone(),
                _ => unreachable!(),
            })
            .collect();

        let (passed, error) = match exec_result {
            Err(e) => {
                let err_msg = e.to_string();
                // If the error is an assertion failure not yet in failed_assertions, include it
                if failed_assertions.is_empty() {
                    (false, Some(err_msg))
                } else {
                    (false, failed_assertions.first().cloned())
                }
            }
            Ok(_) => {
                if !failed_assertions.is_empty() {
                    (false, failed_assertions.first().cloned())
                } else {
                    // Also check context $error
                    match context.resolve_path("error") {
                        Ok(Some(err_val)) if !err_val.is_null() => {
                            let msg = err_val
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("unknown error");
                            (false, Some(msg.to_string()))
                        }
                        _ => (true, None),
                    }
                }
            }
        };

        TestResult {
            name: root_name.to_string(),
            passed,
            duration: started.elapsed(),
            error,
            assertions,
            failed_assertions,
        }
    }

    /// Discover test files in a directory
    pub fn discover_test_files(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        if dir.is_file() {
            if dir.extension().is_some_and(|ext| ext == "jg") {
                files.push(dir.to_path_buf());
            }
            return Ok(files);
        }

        if !dir.is_dir() {
            return Err(anyhow!("Path does not exist: {}", dir.display()));
        }

        Self::walk_dir(dir, &mut files)?;
        files.sort();
        Ok(files)
    }

    fn walk_dir(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                Self::walk_dir(&path, files)?;
            } else if path.extension().is_some_and(|ext| ext == "jg") {
                // Only include files that contain test_* nodes
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if content.contains("[test_") {
                        files.push(path);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Find test root nodes: `test_*` nodes with no incoming edges
fn find_test_roots(workflow: &WorkflowGraph) -> Vec<NodeIndex> {
    let mut roots: Vec<NodeIndex> = workflow
        .graph
        .node_indices()
        .filter(|&idx| {
            let node = &workflow.graph[idx];
            if !graph::is_test_node_id(&node.id) {
                return false;
            }
            // Must have no incoming edges (in-degree == 0)
            workflow
                .graph
                .edges_directed(idx, Direction::Incoming)
                .count()
                == 0
        })
        .collect();
    roots.sort_by_key(|idx| workflow.graph[*idx].id.clone());
    roots
}

/// BFS forward from root to collect all reachable nodes
fn collect_subgraph(workflow: &WorkflowGraph, root: NodeIndex) -> Vec<NodeIndex> {
    let mut visited = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root);

    while let Some(idx) = queue.pop_front() {
        if !seen.insert(idx) {
            continue;
        }
        visited.push(idx);
        for neighbor in workflow.graph.neighbors_directed(idx, Direction::Outgoing) {
            if !seen.contains(&neighbor) {
                queue.push_back(neighbor);
            }
        }
    }

    visited
}

/// Strip `test_` prefix from node ID so execute_graph doesn't skip it
fn strip_test_prefix(id: &str) -> String {
    // test_foo → _root, test_foo.__1 → __1
    if let Some(suffix) = id.strip_prefix("test_") {
        if let Some(pos) = suffix.find(".__") {
            suffix[pos + 1..].to_string()
        } else {
            "_root".to_string()
        }
    } else {
        id.to_string()
    }
}

/// Extract a sub-workflow from a set of node indices
fn extract_subworkflow(source: &WorkflowGraph, node_indices: &[NodeIndex]) -> WorkflowGraph {
    let mut sub = WorkflowGraph::default();
    let mut idx_map: HashMap<NodeIndex, NodeIndex> = HashMap::new();

    // Copy nodes, stripping test_ prefix so execute_graph doesn't skip them
    for &idx in node_indices {
        let mut node = source.graph[idx].clone();
        node.id = strip_test_prefix(&node.id);
        let new_idx = sub.graph.add_node(node.clone());
        sub.node_map.insert(node.id.clone(), new_idx);
        idx_map.insert(idx, new_idx);
    }

    // Copy edges between nodes in the subgraph
    for &idx in node_indices {
        for edge in source.graph.edges_directed(idx, Direction::Outgoing) {
            if let Some(&new_target) = idx_map.get(&edge.target()) {
                let new_source = idx_map[&idx];
                sub.graph
                    .add_edge(new_source, new_target, edge.weight().clone());
            }
        }
    }

    // Set entry to the root (first node, with test_ prefix stripped)
    if let Some(&first) = node_indices.first() {
        sub.entry_node = strip_test_prefix(&source.graph[first].id);
    }

    // Copy switch routes for nodes in the subgraph
    for &idx in node_indices {
        let id = &source.graph[idx].id;
        if let Some(route) = source.switch_routes.get(id) {
            sub.switch_routes.insert(id.clone(), route.clone());
        }
    }

    // Copy functions (tests may call functions defined in the same file)
    sub.functions = source.functions.clone();

    sub
}
