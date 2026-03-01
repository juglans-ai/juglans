// src/testing/mod.rs
//
// Test runner for `juglans test`.
// Discovers `_test_*` function blocks in .jg files and executes them.

pub mod reporter;

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

use crate::core::context::WorkflowContext;
use crate::core::executor::WorkflowExecutor;
use crate::core::graph::WorkflowGraph;
use crate::core::parser::GraphParser;
use crate::services::agent_loader::AgentRegistry;
use crate::services::interface::JuglansRuntime;
use crate::services::prompt_loader::PromptRegistry;

/// Configuration extracted from `_test_config` block
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub agent: Option<String>,
    pub budget: f64,
    pub mock: bool,
    pub timeout: Duration,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            agent: None,
            budget: 10.0,
            mock: false,
            timeout: Duration::from_secs(60),
        }
    }
}

/// Result of a single test case
#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration: Duration,
    pub error: Option<String>,
    pub assertions: usize,
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
    prompt_registry: Arc<PromptRegistry>,
    agent_registry: Arc<AgentRegistry>,
}

impl TestRunner {
    pub fn new(
        runtime: Arc<dyn JuglansRuntime>,
        prompt_registry: Arc<PromptRegistry>,
        agent_registry: Arc<AgentRegistry>,
    ) -> Self {
        Self {
            runtime,
            prompt_registry,
            agent_registry,
        }
    }

    /// Run all tests in a single .jg file
    pub async fn run_file(&self, path: &Path) -> Result<FileTestResult> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read {}: {}", path.display(), e))?;

        let workflow = GraphParser::parse(&content)
            .map_err(|e| anyhow!("Failed to parse {}: {}", path.display(), e))?;

        // Classify _test_* function blocks
        let mut config_fn = None;
        let mut setup_fn = None;
        let mut teardown_fn = None;
        let mut test_cases: Vec<String> = Vec::new();

        for name in workflow.functions.keys() {
            if !name.starts_with("_test_") {
                continue;
            }

            match name.as_str() {
                "_test_config" => config_fn = Some(name.clone()),
                "_test_setup" => setup_fn = Some(name.clone()),
                "_test_teardown" => teardown_fn = Some(name.clone()),
                _ => test_cases.push(name.clone()),
            }
        }

        // Sort test cases for deterministic ordering
        test_cases.sort();

        if test_cases.is_empty() {
            return Ok(FileTestResult {
                path: path.to_path_buf(),
                results: Vec::new(),
            });
        }

        // Create executor
        let executor = Arc::new(
            WorkflowExecutor::new(
                self.prompt_registry.clone(),
                self.agent_registry.clone(),
                self.runtime.clone(),
            )
            .await,
        );
        executor
            .get_registry()
            .set_executor(Arc::downgrade(&executor));

        let workflow_arc = Arc::new(workflow);

        // Extract config if present
        let _config = if let Some(config_name) = &config_fn {
            self.run_config_block(&executor, &workflow_arc, config_name)
                .await
                .unwrap_or_default()
        } else {
            TestConfig::default()
        };

        // Run each test case
        let mut results = Vec::new();
        for test_name in &test_cases {
            let result = self
                .run_single_test(
                    &executor,
                    &workflow_arc,
                    test_name,
                    setup_fn.as_deref(),
                    teardown_fn.as_deref(),
                )
                .await;
            results.push(result);
        }

        Ok(FileTestResult {
            path: path.to_path_buf(),
            results,
        })
    }

    /// Execute the _test_config block and extract TestConfig
    async fn run_config_block(
        &self,
        executor: &Arc<WorkflowExecutor>,
        workflow: &Arc<WorkflowGraph>,
        config_name: &str,
    ) -> Result<TestConfig> {
        let context = WorkflowContext::new();
        let result = executor
            .clone()
            .execute_function(
                config_name.to_string(),
                std::collections::HashMap::new(),
                workflow.clone(),
                &context,
            )
            .await?;

        let mut config = TestConfig::default();

        if let Some(val) = result {
            if let Some(obj) = val.as_object() {
                if let Some(agent) = obj.get("agent").and_then(|v| v.as_str()) {
                    config.agent = Some(agent.to_string());
                }
                if let Some(budget) = obj.get("budget").and_then(|v| v.as_str()) {
                    config.budget = budget.parse().unwrap_or(10.0);
                }
                if let Some(mock) = obj.get("mock").and_then(|v| v.as_str()) {
                    config.mock = mock == "true";
                }
                if let Some(timeout) = obj.get("timeout").and_then(|v| v.as_str()) {
                    let secs: u64 = timeout.parse().unwrap_or(60);
                    config.timeout = Duration::from_secs(secs);
                }
            }
        }

        Ok(config)
    }

    /// Run a single test case with optional setup/teardown
    async fn run_single_test(
        &self,
        executor: &Arc<WorkflowExecutor>,
        workflow: &Arc<WorkflowGraph>,
        test_name: &str,
        setup_fn: Option<&str>,
        teardown_fn: Option<&str>,
    ) -> TestResult {
        let started = Instant::now();
        let context = WorkflowContext::new();

        // Run setup
        if let Some(setup) = setup_fn {
            debug!("Running setup for {}", test_name);
            if let Err(e) = executor
                .clone()
                .execute_function(
                    setup.to_string(),
                    std::collections::HashMap::new(),
                    workflow.clone(),
                    &context,
                )
                .await
            {
                return TestResult {
                    name: test_name.to_string(),
                    passed: false,
                    duration: started.elapsed(),
                    error: Some(format!("setup failed: {}", e)),
                    assertions: 0,
                };
            }
        }

        // Run the test
        let test_result = executor
            .clone()
            .execute_function(
                test_name.to_string(),
                std::collections::HashMap::new(),
                workflow.clone(),
                &context,
            )
            .await;

        // Count assertions from trace (tool calls to "assert")
        let trace = context.trace_entries();
        let assertions = trace.iter().filter(|e| e.tool == "assert").count();

        // Check for failures: either execute_function returned Err, or
        // any assert tool call had an error status in the trace
        let (passed, error) = match test_result {
            Err(e) => (false, Some(e.to_string())),
            Ok(_) => {
                // Check trace for failed assertions
                let failed_assert = trace.iter().find(|e| {
                    e.tool == "assert"
                        && matches!(e.status, crate::core::context::TraceStatus::Error(_))
                });

                if let Some(entry) = failed_assert {
                    match &entry.status {
                        crate::core::context::TraceStatus::Error(msg) => (false, Some(msg.clone())),
                        _ => (true, None),
                    }
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

        // Run teardown (even if test failed)
        if let Some(teardown) = teardown_fn {
            debug!("Running teardown for {}", test_name);
            let _ = executor
                .clone()
                .execute_function(
                    teardown.to_string(),
                    std::collections::HashMap::new(),
                    workflow.clone(),
                    &context,
                )
                .await;
        }

        TestResult {
            name: test_name.to_string(),
            passed,
            duration: started.elapsed(),
            error,
            assertions,
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
                // Only include files that contain _test_ blocks
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if content.contains("_test_") {
                        files.push(path);
                    }
                }
            }
        }
        Ok(())
    }

    /// Check if a .jg file contains test blocks (without full parsing)
    pub fn _is_test_file(path: &Path) -> bool {
        if !path.extension().is_some_and(|ext| ext == "jg") {
            return false;
        }
        std::fs::read_to_string(path)
            .map(|content| content.contains("_test_"))
            .unwrap_or(false)
    }
}
