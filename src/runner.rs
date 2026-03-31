// src/runner.rs
//
// High-level API for running .jg workflows from Rust code.
//
// ```rust,no_run
// # tokio_test::block_on(async {
// let output = juglans::runner::run_file("main.jg", Some(serde_json::json!({"query": "hello"}))).await?;
// # Ok::<(), anyhow::Error>(())
// # });
// ```

use crate::core::parser::GraphParser;
use crate::core::resolver;
use crate::core::validator::WorkflowValidator;
use crate::services::config::JuglansConfig;
use crate::services::interface::JuglansRuntime;
use crate::services::local_runtime::LocalRuntime;
use crate::services::prompt_loader::PromptRegistry;
use crate::WorkflowContext;
use crate::WorkflowExecutor;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Run a .jg workflow file with optional JSON input. Returns the final `output` value.
///
/// Equivalent to `juglans main.jg --input '{"key": "value"}'` but in-process.
pub async fn run_file(path: impl AsRef<Path>, input: Option<Value>) -> Result<Value> {
    RunBuilder::from_file(path)?.run(input).await
}

/// Builder for configuring and running a Juglans workflow.
pub struct RunBuilder {
    file_path: PathBuf,
    project_root: PathBuf,
    config: JuglansConfig,
    runtime: Option<Arc<dyn JuglansRuntime>>,
}

impl RunBuilder {
    /// Create a builder from a .jg file path. Automatically finds project root (juglans.toml).
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let absolute = std::fs::canonicalize(path.as_ref())
            .with_context(|| format!("Cannot resolve {:?}", path.as_ref()))?;
        let project_root = find_project_root(&absolute);

        let config = {
            let _guard = SetCwd::new(&project_root)?;
            JuglansConfig::load()?
        };

        Ok(Self {
            file_path: absolute,
            project_root,
            config,
            runtime: None,
        })
    }

    /// Override the runtime (default: auto-detect LocalRuntime or Jug0Client).
    pub fn runtime(mut self, rt: Arc<dyn JuglansRuntime>) -> Self {
        self.runtime = Some(rt);
        self
    }

    /// Override the config (default: loaded from juglans.toml).
    pub fn config(mut self, config: JuglansConfig) -> Self {
        self.config = config;
        self
    }

    /// Execute the workflow and return the final `output` value.
    pub async fn run(self, input: Option<Value>) -> Result<Value> {
        let ctx = self.run_context(input).await?;
        Ok(ctx.resolve_path("output")?.unwrap_or(Value::Null))
    }

    /// Execute the workflow and return the full WorkflowContext.
    pub async fn run_context(self, input: Option<Value>) -> Result<WorkflowContext> {
        let _guard = SetCwd::new(&self.project_root)?;

        let file_parent = self.file_path.parent().unwrap_or(Path::new("."));
        let base_dir = pathdiff::diff_paths(file_parent, &self.project_root)
            .unwrap_or_else(|| PathBuf::from("."));

        let at_base: Option<PathBuf> = self
            .config
            .paths
            .base
            .as_ref()
            .map(|b| self.project_root.join(b));

        // 1. Parse
        let source = std::fs::read_to_string(&self.file_path)?;
        let mut workflow = GraphParser::parse(&source)?;

        // 2. Resolve imports
        let mut import_stack = vec![self.file_path.clone()];
        resolver::resolve_lib_imports(
            &mut workflow,
            &base_dir,
            &mut import_stack,
            at_base.as_deref(),
        )?;
        import_stack = vec![self.file_path.clone()];
        resolver::resolve_flow_imports(
            &mut workflow,
            &base_dir,
            &mut import_stack,
            at_base.as_deref(),
        )?;

        // 3. Validate
        let validation = WorkflowValidator::validate(&workflow);
        if !validation.is_valid {
            return Err(anyhow!(
                "Validation failed:\n{}",
                validation.format_report(&self.file_path.display().to_string())
            ));
        }

        // 4. Load resources
        let mut prompt_registry = PromptRegistry::new();

        let resolve_patterns = |patterns: &[String]| -> Vec<String> {
            let expanded = resolver::expand_at_prefixes(patterns, at_base.as_deref());
            expanded
                .into_iter()
                .map(|p| {
                    if Path::new(&p).is_absolute() {
                        p
                    } else {
                        base_dir.join(&p).to_string_lossy().to_string()
                    }
                })
                .collect()
        };

        let p_patterns = resolve_patterns(&workflow.prompt_patterns);
        workflow.tool_patterns = resolve_patterns(&workflow.tool_patterns);

        if !p_patterns.is_empty() {
            prompt_registry.load_from_paths(&p_patterns)?;
        }

        let workflow = Arc::new(workflow);

        // 5. Build runtime + executor
        let runtime: Arc<dyn JuglansRuntime> = match self.runtime {
            Some(rt) => rt,
            None => {
                if has_local_llm_provider(&self.config) {
                    Arc::new(LocalRuntime::new_with_config(&self.config.ai))
                } else {
                    Arc::new(crate::services::jug0::Jug0Client::new(&self.config))
                }
            }
        };

        let mut executor = WorkflowExecutor::new_with_debug(
            Arc::new(prompt_registry),
            runtime,
            self.config.debug.clone(),
        )
        .await;
        executor.apply_limits(&self.config.limits);
        executor.load_tools(&workflow).await;
        if let Err(e) = executor.init_python_runtime(&workflow, self.config.limits.python_workers) {
            tracing::warn!("Failed to initialize Python runtime: {}", e);
        }

        let executor = Arc::new(executor);
        executor
            .get_registry()
            .set_executor(Arc::downgrade(&executor));

        // 6. Execute
        let context = executor
            .run_with_input(workflow, &self.config, input)
            .await?;

        Ok(context)
    }
}

fn find_project_root(start: &Path) -> PathBuf {
    let mut current = if start.is_file() {
        start.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        start.to_path_buf()
    };
    let fallback = current.clone();
    loop {
        if current.join("juglans.toml").exists() {
            return current;
        }
        if !current.pop() {
            return fallback;
        }
    }
}

fn has_local_llm_provider(config: &JuglansConfig) -> bool {
    if config.ai.has_providers() {
        return true;
    }
    [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "DEEPSEEK_API_KEY",
        "GEMINI_API_KEY",
        "QWEN_API_KEY",
        "ARK_API_KEY",
        "XAI_API_KEY",
    ]
    .iter()
    .any(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false))
}

/// RAII guard: sets CWD on creation, restores on drop.
struct SetCwd {
    previous: PathBuf,
}

impl SetCwd {
    fn new(dir: &Path) -> Result<Self> {
        let previous = std::env::current_dir()?;
        std::env::set_current_dir(dir)?;
        Ok(Self { previous })
    }
}

impl Drop for SetCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}
