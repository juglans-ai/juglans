// src/main.rs
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]

mod adapters;
mod builtins;
mod core;
mod lsp;
mod providers;
mod registry;
mod runtime;
mod services;
mod templates;
mod testing;
mod ui;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

use core::executor::WorkflowExecutor;
use core::parser::GraphParser;
use core::prompt_parser::PromptParser;
use core::renderer::JwlRenderer;
use core::resolver;
use core::skill_parser;
use core::type_checker::TypeChecker;
use core::validator::{ProjectContext, WorkflowValidator};
use services::config::JuglansConfig;
use services::github;
use services::local_runtime::LocalRuntime;
use services::prompt_loader::PromptRegistry;
use services::web_server;

#[derive(Parser)]
#[command(name = "juglans", author = "Juglans Team", version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Target file path to process (.jg, .jgx)
    file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Direct input for prompt variables
    #[arg(short, long)]
    input: Option<String>,

    /// Read input from a JSON file
    #[arg(long)]
    input_file: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Parse only, do not execute
    #[arg(long)]
    dry_run: bool,

    /// Output result to file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format (text, json, or sse)
    #[arg(long, default_value = "text")]
    output_format: String,

    /// Chat session ID for multi-turn conversation
    #[arg(long)]
    chat_id: Option<String>,

    /// Show prompt info without executing
    #[arg(long)]
    info: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new project scaffold
    Init { name: String },
    /// Retrieve MCP tool schemas
    Install,
    /// Validate syntax of .jg/.jgflow, .jgx files (like cargo check)
    Check {
        /// Path to check (file or directory, defaults to current directory)
        path: Option<PathBuf>,
        /// Show all issues including warnings
        #[arg(long)]
        all: bool,
        /// Output format (text or json)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Start local web server for development
    Web {
        #[arg(short, long)]
        port: Option<u16>,
        #[arg(long)]
        host: Option<String>,
    },
    /// Show current account information
    Whoami {
        /// Show detailed information
        #[arg(long, short = 'v')]
        verbose: bool,
    },
    /// Start unified server: web API + all configured bot adapters
    Serve {
        /// Port for the web server
        #[arg(short, long)]
        port: Option<u16>,
        /// Host address to bind
        #[arg(long)]
        host: Option<String>,
        /// Workflow entry file (default: main.jg in project root)
        #[arg(long)]
        entry: Option<PathBuf>,
    },
    /// Start bot adapter (telegram, feishu, wechat, discord)
    Bot {
        /// Platform: telegram, feishu, wechat, discord
        platform: String,
        /// Agent slug to use (overrides config default)
        #[arg(long)]
        agent: Option<String>,
        /// Port for webhook-based platforms (feishu)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Manage Agent Skills (fetch from GitHub, convert to .jgx)
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Pack a package directory into a .tar.gz archive
    Pack {
        /// Path to the package directory (default: current directory)
        path: Option<PathBuf>,
        /// Output directory for the archive (default: same as package directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Publish a package to the registry
    Publish {
        /// Path to the package directory (default: current directory)
        path: Option<PathBuf>,
    },
    /// Add a package dependency from the registry
    Add {
        /// Package name or name@version (e.g., "sqlite-tools" or "sqlite-tools@^1.2.0")
        package: String,
    },
    /// Remove a package dependency
    Remove {
        /// Package name to remove
        package: String,
    },
    /// Launch interactive chat UI
    Chat {
        /// Agent file to load
        #[arg(short, long)]
        agent: Option<PathBuf>,
    },
    /// Deploy project to Docker container
    Deploy {
        /// Custom image tag (default: juglans-{project}:latest)
        #[arg(long)]
        tag: Option<String>,
        /// Host port to bind (default: 8080)
        #[arg(short, long)]
        port: Option<u16>,
        /// Only build the image, don't start a container
        #[arg(long)]
        build_only: bool,
        /// Push image to registry after build
        #[arg(long)]
        push: bool,
        /// Stop and remove the running container
        #[arg(long)]
        stop: bool,
        /// Show container status
        #[arg(long)]
        status: bool,
        /// Environment variables (can be repeated: -e KEY=VAL)
        #[arg(long = "env", short = 'e')]
        env_vars: Vec<String>,
        /// Project path (default: current directory)
        path: Option<PathBuf>,
    },
    /// Run a workflow on a cron schedule (local dev scheduler)
    Cron {
        /// Workflow file (.jg or .jgflow) to schedule
        file: PathBuf,
        /// Cron expression (overrides schedule in file metadata)
        #[arg(long, short = 's')]
        schedule: Option<String>,
    },
    /// Start Language Server Protocol (LSP) server
    Lsp,
    /// Run tests — discovers and executes test_* nodes in .jg files
    Test {
        /// Path to test file or directory (default: ./tests/)
        path: Option<PathBuf>,
        /// Filter tests by name substring
        #[arg(long)]
        filter: Option<String>,
        /// Output format: text, json, junit
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Validate code snippets in markdown documentation
    Doctest {
        /// Path to markdown file or directory (default: ./docs/)
        path: Option<PathBuf>,
        /// Output format: text, json
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
enum SkillsAction {
    /// Fetch skills from a GitHub repository and save as .jgx
    Add {
        /// GitHub repository (owner/repo), e.g. "anthropics/skills"
        repo: String,
        /// Specific skill(s) to fetch (can be repeated)
        #[arg(long = "skill")]
        skills: Vec<String>,
        /// Fetch all available skills
        #[arg(long)]
        all: bool,
        /// List available skills without downloading
        #[arg(long)]
        list: bool,
        /// Output directory for .jgx files (default: ./prompts)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// List locally installed skills
    List,
    /// Remove a locally installed skill
    Remove {
        /// Skill name to remove
        name: String,
    },
}

/// Resolve input data from --input or --input-file
fn resolve_input_data(cli: &Cli) -> Result<Option<String>> {
    if let Some(input_file_path) = &cli.input_file {
        let content = fs::read_to_string(input_file_path)
            .with_context(|| format!("Failed to read input file: {:?}", input_file_path))?;
        Ok(Some(content))
    } else {
        Ok(cli.input.clone())
    }
}

fn resolve_import_patterns_verbose(
    base_dir_ref: &Path,
    raw_patterns: &[String],
    at_base: Option<&Path>,
) -> Vec<String> {
    let expanded = resolver::expand_at_prefixes(raw_patterns, at_base);
    let mut resolved_output_list = Vec::new();
    for pattern_str in &expanded {
        if Path::new(pattern_str).is_absolute() {
            resolved_output_list.push(pattern_str.clone());
        } else {
            let combined_path_obj = base_dir_ref.join(pattern_str);
            resolved_output_list.push(combined_path_obj.to_string_lossy().to_string());
        }
    }
    resolved_output_list
}

/// Check if local LLM providers are available.
/// Priority: [ai.providers] in juglans.toml > environment variables (fallback).
fn has_local_llm_provider(config: &JuglansConfig) -> bool {
    // 1. Check [ai.providers] in juglans.toml
    if config.ai.has_providers() {
        return true;
    }
    // 2. Fallback: check environment variables (compat with projects without [ai] section)
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

/// Build a configured WorkflowExecutor with registries, MCP tools, and Python runtime.
async fn build_executor(
    config: &JuglansConfig,
    prompt_registry: PromptRegistry,
    workflow: Option<&Arc<core::graph::WorkflowGraph>>,
) -> Result<Arc<WorkflowExecutor>> {
    if has_local_llm_provider(config) {
        tracing::info!("Using local LLM provider (direct API)");
    }
    let runtime: Arc<LocalRuntime> = Arc::new(LocalRuntime::new_with_config(&config.ai));

    let mut executor =
        WorkflowExecutor::new_with_debug(Arc::new(prompt_registry), runtime, config.debug.clone())
            .await;

    executor.apply_limits(&config.limits);

    if let Some(wf) = workflow {
        executor.load_tools(wf).await;
        if let Err(e) = executor.init_python_runtime(wf, config.limits.python_workers) {
            warn!("Failed to initialize Python runtime: {}", e);
        }
    }

    let shared = Arc::new(executor);
    shared.get_registry().set_executor(Arc::downgrade(&shared));

    Ok(shared)
}

async fn handle_file_logic(cli: &Cli) -> Result<()> {
    let source_file_path = cli
        .file
        .as_ref()
        .ok_or_else(|| anyhow!("Input missing: Please provide a valid file path."))?;

    let file_ext_name = source_file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let absolute_target_path = fs::canonicalize(source_file_path)
        .with_context(|| format!("Physical file error: Cannot resolve {:?}", source_file_path))?;

    let project_root_path = find_project_root(&absolute_target_path)?;
    env::set_current_dir(&project_root_path)?;

    let file_parent_context = absolute_target_path.parent().unwrap_or(Path::new("."));
    let mut relative_base_offset =
        pathdiff::diff_paths(file_parent_context, &project_root_path).unwrap_or(PathBuf::from("."));

    let source_raw_text = fs::read_to_string(&absolute_target_path)?;

    match file_ext_name {
        "jg" | "jgflow" => {
            info!("🚀 Starting Workflow Graph Logic: {:?}", source_file_path);
            let local_config = JuglansConfig::load()?;

            // Initialize global conversation-history store (idempotent).
            // Without this, the file-exec path runs every chat() stateless,
            // even when chat_id resolves and [history] is enabled.
            if let Err(e) = crate::services::history::init_global(&local_config.history) {
                tracing::warn!("[history] init_global failed: {}", e);
            }

            // Compute base directory for @ path alias
            let at_base: Option<PathBuf> = local_config
                .paths
                .base
                .as_ref()
                .map(|b| project_root_path.join(b));

            let mut workflow_definition_obj = if file_ext_name == "jgflow" {
                let manifest = GraphParser::parse_manifest(&source_raw_text)?;
                if manifest.source.is_empty() {
                    anyhow::bail!(".jgflow manifest requires a 'source' field pointing to a .jg file.\nExample: source: \"./main.jg\"");
                }
                let source_path = file_parent_context.join(&manifest.source);
                let source_content = fs::read_to_string(&source_path)
                    .with_context(|| format!("Failed to read source: {}", manifest.source))?;
                let mut wf = GraphParser::parse(&source_content)?;
                manifest.apply_to(&mut wf);

                // Switch base_dir to the directory of the source file
                let source_parent = source_path.parent().unwrap_or(Path::new("."));
                relative_base_offset = pathdiff::diff_paths(source_parent, &project_root_path)
                    .unwrap_or(PathBuf::from("."));
                wf
            } else {
                GraphParser::parse(&source_raw_text)?
            };

            // Resolve lib imports (extract function definitions into namespace)
            let mut import_stack = vec![absolute_target_path.clone()];
            resolver::resolve_lib_imports(
                &mut workflow_definition_obj,
                &relative_base_offset,
                &mut import_stack,
                at_base.as_deref(),
            )?;

            // Resolve flow imports and merge subgraphs (compile-time graph merging)
            import_stack = vec![absolute_target_path.clone()];
            resolver::resolve_flow_imports(
                &mut workflow_definition_obj,
                &relative_base_offset,
                &mut import_stack,
                at_base.as_deref(),
            )?;

            // Macro expand: process @decorator applications
            core::macro_expand::expand_decorators(&mut workflow_definition_obj)?;

            // Pre-flight validation (after imports resolved)
            let validation = WorkflowValidator::validate(&workflow_definition_obj);
            if !validation.is_valid {
                eprint!(
                    "{}",
                    validation.format_report(&source_file_path.display().to_string())
                );
                anyhow::bail!("Validation failed. Run `juglans check` for details.");
            }
            if validation.warning_count() > 0 {
                eprint!(
                    "{}",
                    validation.format_report(&source_file_path.display().to_string())
                );
            }

            let mut prompt_registry_inst = PromptRegistry::new();

            let resolved_p_patterns = resolve_import_patterns_verbose(
                &relative_base_offset,
                &workflow_definition_obj.prompt_patterns,
                at_base.as_deref(),
            );
            let resolved_t_patterns = resolve_import_patterns_verbose(
                &relative_base_offset,
                &workflow_definition_obj.tool_patterns,
                at_base.as_deref(),
            );

            // Update workflow with resolved tool patterns
            workflow_definition_obj.tool_patterns = resolved_t_patterns;
            let workflow_definition_obj = Arc::new(workflow_definition_obj);

            if !resolved_p_patterns.is_empty() {
                prompt_registry_inst.load_from_paths(&resolved_p_patterns)?;
            }

            let shared_executor_engine = build_executor(
                &local_config,
                prompt_registry_inst,
                Some(&workflow_definition_obj),
            )
            .await?;

            // Parse CLI input
            let input_value: Option<serde_json::Value> =
                resolve_input_data(cli)?.and_then(|s| serde_json::from_str(&s).ok());

            let context = shared_executor_engine
                .run_with_input(workflow_definition_obj, &local_config, input_value)
                .await?;

            // Output result based on format
            match cli.output_format.as_str() {
                "json" => {
                    let output = context
                        .resolve_path("output")
                        .ok()
                        .flatten()
                        .unwrap_or(serde_json::Value::Null);
                    println!("{}", serde_json::to_string(&output)?);
                }
                "sse" => {
                    let output = context
                        .resolve_path("output")
                        .ok()
                        .flatten()
                        .unwrap_or(serde_json::Value::Null);
                    println!(
                        "data: {}\n",
                        serde_json::to_string(&serde_json::json!({
                            "event": "done",
                            "output": output
                        }))?
                    );
                }
                _ => {} // text: already printed by notify/chat during execution
            }
        }

        "jgx" | "jgprompt" => {
            println!("🔍 Executing Local Render: {:?}", source_file_path);
            let prompt_resource_item = PromptParser::parse(&source_raw_text)?;
            let mut rendering_variables_ctx = prompt_resource_item.inputs.clone();

            if let Some(ext_input_json) = resolve_input_data(cli)? {
                let parsed_input_data: Value = serde_json::from_str(&ext_input_json)?;
                if let Some(data_obj) = parsed_input_data.as_object() {
                    for (k, v) in data_obj {
                        rendering_variables_ctx[k] = v.clone();
                    }
                }
            }

            let renderer_instance = JwlRenderer::new();
            let final_text_output =
                renderer_instance.render(&prompt_resource_item.ast, &rendering_variables_ctx)?;
            println!(
                "\n--- Rendered Content ---\n{}\n-----------------------",
                final_text_output
            );
        }

        _ => return Err(anyhow!("Unsupported JWL file type: .{}", file_ext_name)),
    }

    Ok(())
}

fn find_project_root(start_search_path: &Path) -> Result<PathBuf> {
    let mut current_ptr = start_search_path.to_path_buf();
    if current_ptr.is_file() {
        current_ptr.pop();
    }
    let fallback = current_ptr.clone();
    loop {
        if current_ptr.join("juglans.toml").exists() {
            return Ok(current_ptr);
        }
        if !current_ptr.pop() {
            return Ok(fallback);
        }
    }
}

fn handle_init(new_project_name: &str) -> Result<()> {
    let root_path_obj = Path::new(new_project_name);
    if root_path_obj.exists() {
        return Err(anyhow!("Directory exists: '{}'", new_project_name));
    }
    fs::create_dir_all(root_path_obj)?;
    fs::write(root_path_obj.join("juglans.toml"), templates::TPL_TOML)?;
    templates::PROJECT_TEMPLATE_DIR.extract(root_path_obj)?;

    let docs_path = root_path_obj.join("docs");
    fs::create_dir_all(&docs_path)?;
    templates::DOCS_DIR.extract(&docs_path)?;

    println!("✅ Initialized: {:?}", root_path_obj);
    Ok(())
}

async fn handle_install() -> Result<()> {
    let project_dir = env::current_dir()?;

    // 1. Install package dependencies from jgpackage.toml
    let manifest_path = project_dir.join("jgpackage.toml");
    if manifest_path.exists() {
        let config = JuglansConfig::load()?;
        let registry_url = config
            .registry
            .as_ref()
            .map(|r| r.url.as_str())
            .unwrap_or("https://jgr.juglans.ai");

        let installer = registry::installer::PackageInstaller::with_defaults(registry_url)?;
        let installed = installer.install_all(&project_dir).await?;

        if installed.is_empty() {
            println!("No package dependencies to install.");
        } else {
            for pkg in &installed {
                println!("  Installed {}@{}", pkg.name, pkg.version);
            }
            println!("Installed {} package(s).", installed.len());
        }
    }

    Ok(())
}

async fn handle_whoami(verbose: bool) -> Result<()> {
    let config = JuglansConfig::load()?;
    let config_path = if Path::new("juglans.toml").exists() {
        "./juglans.toml"
    } else {
        "~/.config/juglans/juglans.toml or system default"
    };

    println!("\n📋 Account Information\n");

    println!("\x1b[1m💻 Local Configuration\x1b[0m");
    println!("User ID:       {}", config.account.id);
    println!("Name:          {}", config.account.name);
    if let Some(role) = &config.account.role {
        println!("Role:          {}", role);
    }
    println!();

    if let Some(workspace) = &config.workspace {
        println!("Workspace:     {} ({})", workspace.id, workspace.name);
        if let Some(members) = &workspace.members {
            println!("Members:       {} user(s)", members.len());
        }
        if verbose {
            if !workspace.workflows.is_empty() || !workspace.prompts.is_empty() {
                println!("\nResource Paths:");
                println!("  Workflows:   {}", workspace.workflows.join(", "));
                println!("  Prompts:     {}", workspace.prompts.join(", "));
                if !workspace.tools.is_empty() {
                    println!("  Tools:       {}", workspace.tools.join(", "));
                }
            }
            if !workspace.exclude.is_empty() {
                println!("\nExclude:       {}", workspace.exclude.join(", "));
            }
        }
        println!();
    }

    if verbose {
        println!(
            "Web Server:    {}:{}",
            config.server.host, config.server.port
        );
        println!();
    }

    println!("Config:        {}", config_path);
    println!();
    Ok(())
}

fn handle_check(path: Option<&Path>, show_all: bool, output_format: &str) -> Result<()> {
    use glob::glob;

    let check_path = path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Determine patterns based on input
    let patterns: Vec<String> = if check_path.is_file() {
        vec![check_path.to_string_lossy().to_string()]
    } else {
        vec![
            check_path.join("**/*.jg").to_string_lossy().to_string(),
            check_path.join("**/*.jgflow").to_string_lossy().to_string(),
            check_path.join("**/*.jgx").to_string_lossy().to_string(),
            check_path
                .join("**/*.jgprompt")
                .to_string_lossy()
                .to_string(),
        ]
    };

    let mut total_files = 0;
    let mut valid_count = 0;
    let mut error_count = 0;
    let mut warning_count = 0;
    let mut results: Vec<serde_json::Value> = Vec::new();

    // Collect stats by type
    let mut workflow_count = 0;
    let mut prompt_count = 0;

    println!(
        "    \x1b[1;32mChecking\x1b[0m juglans files in {:?}\n",
        check_path
    );

    // Collect all matching files
    let mut all_paths: Vec<PathBuf> = Vec::new();
    for pattern in &patterns {
        if let Ok(paths) = glob(pattern) {
            all_paths.extend(paths.flatten());
        }
    }

    if all_paths.is_empty() {
        println!("    \x1b[33mNo .jg/.jgflow or .jgx files found\x1b[0m");
        return Ok(());
    }

    // --- Pass 1: Build ProjectContext (collect all agent/prompt slugs & flow paths) ---
    let mut project_ctx = ProjectContext {
        base_dir: check_path.clone(),
        ..Default::default()
    };
    for entry in &all_paths {
        if let Ok(content) = fs::read_to_string(entry) {
            let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");
            match ext {
                "jgx" | "jgprompt" => {
                    if let Ok(prompt) = PromptParser::parse(&content) {
                        project_ctx.prompt_slugs.insert(prompt.slug);
                    }
                }
                "jg" | "jgflow" => {
                    if let Ok(canonical) = entry.canonicalize() {
                        project_ctx.flow_paths.insert(canonical);
                    } else {
                        project_ctx.flow_paths.insert(entry.clone());
                    }
                }
                _ => {}
            }
        }
    }

    // --- Pass 2: Validate all files ---
    for entry in all_paths {
        total_files += 1;
        let file_name = entry
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");

        let relative_path = entry
            .strip_prefix(&check_path)
            .unwrap_or(&entry)
            .display()
            .to_string();

        let relative_path = if relative_path.is_empty() {
            entry
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            relative_path
        };

        match fs::read_to_string(&entry) {
            Ok(content) => match ext {
                "jg" | "jgflow" => {
                    workflow_count += 1;
                    // .jgflow manifest validation (separate path)
                    if ext == "jgflow" {
                        match GraphParser::parse_manifest(&content) {
                            Ok(manifest) => {
                                if manifest.source.is_empty() {
                                    error_count += 1;
                                    println!("    \x1b[1;31merror\x1b[0m[manifest]: {} — missing 'source' field", relative_path);
                                    continue;
                                }
                                let source_path = entry
                                    .parent()
                                    .unwrap_or(Path::new("."))
                                    .join(&manifest.source);
                                if !source_path.exists() {
                                    error_count += 1;
                                    println!("    \x1b[1;31merror\x1b[0m[manifest]: {} — source file not found: {}", relative_path, manifest.source);
                                    continue;
                                }
                                valid_count += 1;
                                if show_all {
                                    println!(
                                        "    \x1b[32mok\x1b[0m[manifest]: {} → {}",
                                        relative_path, manifest.source
                                    );
                                }
                            }
                            Err(e) => {
                                error_count += 1;
                                println!(
                                    "    \x1b[1;31merror\x1b[0m[parse]: {} — {}",
                                    relative_path, e
                                );
                            }
                        }
                        continue;
                    }

                    let parse_result = GraphParser::parse(&content);
                    match parse_result {
                        Ok(graph) => {
                            project_ctx.file_dir =
                                entry.parent().unwrap_or(Path::new(".")).to_path_buf();

                            // Resolve imports before validation (merge subgraph nodes/edges + lib functions)
                            let mut graph = graph;
                            if let Err(e) = resolver::resolve_lib_imports(
                                &mut graph,
                                &project_ctx.file_dir.clone(),
                                &mut vec![],
                                None,
                            ) {
                                debug!("Lib import resolution warning: {}", e);
                            }
                            let _ = resolver::resolve_flow_imports(
                                &mut graph,
                                &project_ctx.file_dir.clone(),
                                &mut vec![],
                                None,
                            );

                            let validation =
                                WorkflowValidator::validate_with_project(&graph, &project_ctx);

                            // Phase A: Type checking (class field type annotations + assignment compatibility)
                            let type_result = TypeChecker::new().check(&graph);

                            let slug = if graph.slug.is_empty() {
                                file_name.to_string()
                            } else {
                                graph.slug.clone()
                            };

                            let type_warn_count = type_result.warnings.len();
                            let type_err_count = type_result.errors.len();

                            if output_format == "json" {
                                let mut all_warnings: Vec<serde_json::Value> = validation.warnings.iter().map(|w| {
                                    serde_json::json!({"code": w.code, "message": w.message})
                                }).collect();
                                for tw in &type_result.warnings {
                                    all_warnings.push(serde_json::json!({
                                        "code": "TYPE_WARN",
                                        "message": format!("class '{}' field '{}': {}", tw.class_name, tw.field_or_method, tw.message)
                                    }));
                                }
                                let mut all_errors: Vec<serde_json::Value> = validation.errors.iter().map(|e| {
                                    serde_json::json!({"code": e.code, "message": e.message})
                                }).collect();
                                for te in &type_result.errors {
                                    all_errors.push(serde_json::json!({
                                        "code": "TYPE_ERR",
                                        "message": format!("class '{}' {}: {}", te.class_name, te.field_or_method, te.message)
                                    }));
                                }
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "workflow",
                                    "slug": slug,
                                    "valid": validation.is_valid && type_err_count == 0,
                                    "errors": all_errors,
                                    "warnings": all_warnings,
                                }));
                            }

                            if validation.is_valid && type_err_count == 0 {
                                valid_count += 1;
                                let total_warns = validation.warning_count() + type_warn_count;
                                if total_warns > 0 {
                                    warning_count += total_warns;
                                    if show_all {
                                        println!("    \x1b[33mwarning\x1b[0m[workflow]: {} ({} warning(s))", relative_path, total_warns);
                                        for warn in &validation.warnings {
                                            println!(
                                                "      \x1b[33m-->\x1b[0m [{}] {}",
                                                warn.code, warn.message
                                            );
                                        }
                                        for tw in &type_result.warnings {
                                            println!(
                                                "      \x1b[33m-->\x1b[0m [TYPE] class '{}' field '{}': {}",
                                                tw.class_name, tw.field_or_method, tw.message
                                            );
                                        }
                                    }
                                }
                            } else {
                                error_count += 1;
                                let total_warns = validation.warning_count() + type_warn_count;
                                let total_errs = validation.error_count() + type_err_count;
                                warning_count += total_warns;
                                println!("    \x1b[1;31merror\x1b[0m[workflow]: {} ({} error(s), {} warning(s))",
                                        relative_path, total_errs, total_warns);
                                for err in &validation.errors {
                                    println!(
                                        "      \x1b[31m-->\x1b[0m [{}] {}",
                                        err.code, err.message
                                    );
                                }
                                for te in &type_result.errors {
                                    println!(
                                        "      \x1b[31m-->\x1b[0m [TYPE] class '{}' {}: {}",
                                        te.class_name, te.field_or_method, te.message
                                    );
                                }
                                if show_all {
                                    for warn in &validation.warnings {
                                        println!(
                                            "      \x1b[33m-->\x1b[0m [{}] {}",
                                            warn.code, warn.message
                                        );
                                    }
                                    for tw in &type_result.warnings {
                                        println!(
                                            "      \x1b[33m-->\x1b[0m [TYPE] class '{}' field '{}': {}",
                                            tw.class_name, tw.field_or_method, tw.message
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // parse() failed -> try parsing as a library-only file
                            if let Ok(lib_graph) = GraphParser::parse_lib(&content) {
                                valid_count += 1;
                                let slug = if lib_graph.slug.is_empty() {
                                    file_name.to_string()
                                } else {
                                    lib_graph.slug.clone()
                                };
                                let func_count = lib_graph.functions.len();
                                if show_all {
                                    println!(
                                        "    \x1b[32mok\x1b[0m[library]: {} — {} ({} function(s))",
                                        relative_path, slug, func_count
                                    );
                                }
                                if output_format == "json" {
                                    results.push(serde_json::json!({
                                        "file": relative_path,
                                        "type": "library",
                                        "slug": slug,
                                        "valid": true,
                                        "functions": func_count,
                                    }));
                                }
                                continue;
                            }

                            // Both parse attempts failed, report original error
                            error_count += 1;
                            let full_err = e.to_string();
                            println!(
                                "    \x1b[1;31merror\x1b[0m[workflow]: {} (parse failed)",
                                relative_path
                            );
                            for line in full_err.lines() {
                                println!("      \x1b[31m-->\x1b[0m {}", line);
                            }

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "workflow",
                                    "slug": file_name,
                                    "valid": false,
                                    "errors": [{"code": "PARSE", "message": full_err}],
                                    "warnings": [],
                                }));
                            }
                        }
                    }
                }
                "jgx" | "jgprompt" => {
                    prompt_count += 1;
                    match PromptParser::parse(&content) {
                        Ok(prompt) => {
                            valid_count += 1;
                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "prompt",
                                    "slug": prompt.slug,
                                    "valid": true,
                                    "errors": [],
                                    "warnings": [],
                                }));
                            }
                        }
                        Err(e) => {
                            error_count += 1;
                            let full_err = e.to_string();
                            println!(
                                "    \x1b[1;31merror\x1b[0m[prompt]: {} (parse failed)",
                                relative_path
                            );
                            for line in full_err.lines() {
                                println!("      \x1b[31m-->\x1b[0m {}", line);
                            }

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "prompt",
                                    "slug": file_name,
                                    "valid": false,
                                    "errors": [{"code": "PARSE", "message": full_err}],
                                    "warnings": [],
                                }));
                            }
                        }
                    }
                }
                _ => {}
            },
            Err(e) => {
                error_count += 1;
                println!(
                    "    \x1b[1;31merror\x1b[0m: {} (read failed: {})",
                    relative_path, e
                );
            }
        }
    }

    println!();

    if output_format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "total": total_files,
                "valid": valid_count,
                "errors": error_count,
                "warnings": warning_count,
                "by_type": {
                    "workflows": workflow_count,
                    "prompts": prompt_count,
                },
                "results": results,
            }))?
        );
    } else {
        // Summary line like cargo check
        if error_count > 0 {
            println!(
                "\x1b[1;31merror\x1b[0m: could not validate {} file(s) due to {} previous error(s)",
                error_count, error_count
            );
        }

        if warning_count > 0 && error_count == 0 {
            println!(
                "\x1b[1;33mwarning\x1b[0m: {} warning(s) generated",
                warning_count
            );
        }

        // Build summary parts
        let mut parts = Vec::new();
        if workflow_count > 0 {
            parts.push(format!("{} workflow(s)", workflow_count));
        }
        if prompt_count > 0 {
            parts.push(format!("{} prompt(s)", prompt_count));
        }
        let summary = parts.join(", ");

        if error_count == 0 && warning_count == 0 {
            println!(
                "    \x1b[1;32mFinished\x1b[0m checking {} - all valid",
                summary
            );
        } else if error_count == 0 {
            println!(
                "    \x1b[1;32mFinished\x1b[0m checking {} - {} valid with warnings",
                summary, valid_count
            );
        }
    }

    if error_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

async fn handle_bot(
    platform: &str,
    agent_override: Option<String>,
    port: Option<u16>,
) -> Result<()> {
    let config = JuglansConfig::load()?;
    let current_dir = env::current_dir()?;
    let project_root = find_project_root(&current_dir)?;

    match platform {
        "telegram" => {
            let agent_slug = agent_override
                .or_else(|| {
                    config
                        .bot
                        .as_ref()
                        .and_then(|b| b.telegram.as_ref())
                        .map(|t| t.agent.clone())
                })
                .unwrap_or_else(|| "default".to_string());
            adapters::telegram::start(config, project_root, agent_slug).await?;
        }
        "feishu" | "lark" => {
            let agent_slug = agent_override
                .or_else(|| {
                    config
                        .bot
                        .as_ref()
                        .and_then(|b| b.feishu.as_ref())
                        .map(|f| f.agent.clone())
                })
                .unwrap_or_else(|| "default".to_string());
            let feishu_port = port.unwrap_or_else(|| {
                config
                    .bot
                    .as_ref()
                    .and_then(|b| b.feishu.as_ref())
                    .map(|f| f.port)
                    .unwrap_or(9000)
            });
            adapters::feishu::start(config, project_root, agent_slug, feishu_port).await?;
        }
        "wechat" | "weixin" => {
            adapters::wechat::start(config, project_root, agent_override).await?;
        }
        "discord" => {
            let agent_slug = agent_override
                .or_else(|| {
                    config
                        .bot
                        .as_ref()
                        .and_then(|b| b.discord.as_ref())
                        .map(|d| d.agent.clone())
                })
                .unwrap_or_else(|| "default".to_string());
            adapters::discord::start(config, project_root, agent_slug).await?;
        }
        _ => {
            return Err(anyhow!(
                "Unknown platform '{}'. Supported: telegram, feishu, wechat, discord",
                platform
            ));
        }
    }
    Ok(())
}

async fn handle_serve(
    host: Option<String>,
    port: Option<u16>,
    entry: Option<PathBuf>,
) -> Result<()> {
    let config = JuglansConfig::load()?;
    let current_dir = env::current_dir()?;
    let project_root = find_project_root(&current_dir)?;

    // Resolve entry workflow
    let entry_file = entry.unwrap_or_else(|| project_root.join("main.jg"));
    if !entry_file.exists() {
        return Err(anyhow!(
            "Entry workflow not found: {:?}\nCreate a main.jg in your project root, or specify --entry <file>",
            entry_file
        ));
    }

    let entry_slug = entry_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string();

    // Determine host/port
    let final_host = host.unwrap_or_else(|| config.server.host.clone());
    let final_port = port.unwrap_or(config.server.port);

    let mut active_platforms: Vec<String> = vec![];

    // Start bot adapters in background based on config
    if let Some(ref bot) = config.bot {
        // Telegram long-poll
        if bot.telegram.is_some() && config.server.endpoint_url.is_none() {
            let tg_config = config.clone();
            let tg_root = project_root.clone();
            let tg_slug = entry_slug.clone();
            active_platforms.push("telegram (polling)".into());
            tokio::spawn(async move {
                if let Err(e) = adapters::telegram::start(tg_config, tg_root, tg_slug).await {
                    tracing::error!("Telegram bot error: {}", e);
                }
            });
        }

        // WeChat long-poll
        if bot.wechat.is_some() {
            let wx_config = config.clone();
            let wx_root = project_root.clone();
            let wx_slug = Some(entry_slug.clone());
            active_platforms.push("wechat (polling)".into());
            tokio::spawn(async move {
                if let Err(e) = adapters::wechat::start(wx_config, wx_root, wx_slug).await {
                    tracing::error!("WeChat bot error: {}", e);
                }
            });
        }

        // Discord Gateway (WebSocket — no webhook alternative).
        // Requires a long-lived process; will error repeatedly on serverless
        // platforms that suspend idle containers.
        if bot.discord.is_some() {
            let dc_config = config.clone();
            let dc_root = project_root.clone();
            let dc_slug = entry_slug.clone();
            active_platforms.push("discord (gateway)".into());
            tokio::spawn(async move {
                if let Err(e) = adapters::discord::start(dc_config, dc_root, dc_slug).await {
                    tracing::error!("Discord bot error: {}", e);
                }
            });
        }
    }

    // Print banner
    println!("──────────────────────────────────────────────────");
    println!("  Juglans Serve");
    println!("  Listening on: http://{}:{}", final_host, final_port);
    println!("  Project: {}", project_root.display());
    println!("  Entry: {}", entry_file.display());
    println!("  Endpoints:");
    println!("    POST /api/chat");
    if !active_platforms.is_empty() {
        println!("  Platforms: {}", active_platforms.join(", "));
    }
    println!("──────────────────────────────────────────────────");

    // Start web server (blocks)
    web_server::start_web_server(final_host, final_port, project_root).await?;

    Ok(())
}

async fn handle_skills(action: &SkillsAction) -> Result<()> {
    match action {
        SkillsAction::Add {
            repo,
            skills,
            all,
            list,
            output,
        } => {
            // --list: show available skills and exit
            if *list {
                println!("Fetching skill list from {}...", repo);
                let names = github::list_remote_skills(repo).await?;
                if names.is_empty() {
                    println!("No skills found in {}", repo);
                } else {
                    println!("Available skills in {}:", repo);
                    for name in &names {
                        println!("  - {}", name);
                    }
                    println!("\nUse: juglans skills add {} --skill <name>", repo);
                }
                return Ok(());
            }

            // Determine which skills to fetch
            let skill_names = if *all {
                println!("Fetching all skills from {}...", repo);
                github::list_remote_skills(repo).await?
            } else if skills.is_empty() {
                return Err(anyhow!(
                    "No skill specified. Use --skill <name>, --all, or --list.\n\
                     Example: juglans skills add {} --skill pdf",
                    repo
                ));
            } else {
                skills.clone()
            };

            // Output directory
            let prompts_dir = output.clone().unwrap_or_else(|| PathBuf::from("./prompts"));
            fs::create_dir_all(&prompts_dir)?;

            // Create temp dir for fetching
            let temp_dir = env::temp_dir().join(format!("juglans-skills-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&temp_dir)?;

            // Fetch from GitHub
            let fetched = github::fetch_skills(repo, &skill_names, &temp_dir).await?;

            let mut success_count = 0;
            for skill_entry in &fetched {
                match skill_parser::load_skill_dir(&skill_entry.local_dir) {
                    Ok(skill) => {
                        let jgx_content = skill_parser::skill_to_jgx(&skill);
                        let output_path = prompts_dir.join(format!("{}.jgx", skill.name));
                        fs::write(&output_path, &jgx_content)?;
                        println!("  ✓ Saved: {}", output_path.display());
                        success_count += 1;
                    }
                    Err(e) => {
                        eprintln!("  ✗ Failed to parse skill '{}': {}", skill_entry.name, e);
                    }
                }
            }

            // Cleanup temp dir
            let _ = fs::remove_dir_all(&temp_dir);

            println!(
                "\nDone! {} skill(s) saved to {}/",
                success_count,
                prompts_dir.display()
            );
            println!(
                "Prompts in {} are loaded automatically by workflows.",
                prompts_dir.display()
            );
        }
        SkillsAction::List => {
            let prompts_dir = PathBuf::from("./prompts");
            if !prompts_dir.is_dir() {
                println!("No prompts/ directory found. No skills installed.");
                return Ok(());
            }
            let mut found = false;
            if let Ok(entries) = fs::read_dir(&prompts_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if matches!(
                        path.extension().and_then(|e| e.to_str()),
                        Some("jgx" | "jgprompt")
                    ) {
                        let name = path.file_stem().unwrap().to_string_lossy();
                        println!("  - {}", name);
                        found = true;
                    }
                }
            }
            if !found {
                println!("No .jgx files found in prompts/");
            }
        }
        SkillsAction::Remove { name } => {
            let path = PathBuf::from(format!("./prompts/{}.jgx", name));
            if path.exists() {
                fs::remove_file(&path)?;
                println!("Removed: {}", path.display());
            } else {
                println!("Skill '{}' not found at {}", name, path.display());
            }
        }
    }
    Ok(())
}

// ── Registry: pack & publish ─────────────────────────────────────────

fn handle_pack(path: Option<&Path>, output: Option<&Path>) -> Result<()> {
    let dir = path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let manifest_path = dir.join("jgpackage.toml");
    if !manifest_path.exists() {
        return Err(anyhow!("jgpackage.toml not found in {}", dir.display()));
    }

    let manifest = registry::package::PackageManifest::load(&manifest_path)?;
    println!(
        "Packing {} v{} ...",
        manifest.package.name, manifest.package.version
    );

    let archive = registry::package::pack(&dir, output)?;
    println!("  Created {}", archive.display());
    Ok(())
}

async fn handle_publish(path: Option<&Path>) -> Result<()> {
    let dir = path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let manifest_path = dir.join("jgpackage.toml");
    if !manifest_path.exists() {
        return Err(anyhow!("jgpackage.toml not found in {}", dir.display()));
    }

    let manifest = registry::package::PackageManifest::load(&manifest_path)?;
    println!(
        "Publishing {} v{} ...",
        manifest.package.name, manifest.package.version
    );

    // Pack first
    let archive = registry::package::pack(&dir, None)?;

    // Load config for registry URL and auth
    let config = JuglansConfig::load()?;
    let registry_url = config
        .registry
        .as_ref()
        .map(|r| r.url.as_str())
        .unwrap_or("https://jgr.juglans.ai");

    let api_key = std::env::var("JUGLANS_REGISTRY_API_KEY")
        .or_else(|_| std::env::var("REGISTRY_API_KEY"))
        .map_err(|_| {
            anyhow!(
                "JUGLANS_REGISTRY_API_KEY (or REGISTRY_API_KEY) env var is required for publishing"
            )
        })?;
    let api_key = api_key.as_str();

    let url = format!(
        "{}/api/v1/packages/{}/{}",
        registry_url.trim_end_matches('/'),
        manifest.package.name,
        manifest.package.version
    );

    let file_bytes = fs::read(&archive)?;
    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(archive.file_name().unwrap().to_string_lossy().to_string())
        .mime_str("application/gzip")?;

    let metadata = serde_json::json!({
        "name": manifest.package.name,
        "version": manifest.package.version,
        "slug": manifest.slug(),
        "description": manifest.package.description,
        "author": manifest.package.author,
        "license": manifest.package.license,
        "entry": manifest.package.entry,
        "dependencies": manifest.dependencies,
    });

    let meta_part =
        reqwest::multipart::Part::text(metadata.to_string()).mime_str("application/json")?;

    let form = reqwest::multipart::Form::new()
        .part("metadata", meta_part)
        .part("package", part);

    let client = reqwest::Client::new();
    let resp = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await?;

    if resp.status().is_success() {
        println!(
            "  Published {}-{} to {}",
            manifest.package.name, manifest.package.version, registry_url
        );
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Publish failed ({}): {}", status, body));
    }

    // Clean up local archive
    let _ = fs::remove_file(&archive);

    Ok(())
}

async fn handle_add(package: &str) -> Result<()> {
    let config = JuglansConfig::load()?;
    let registry_url = config
        .registry
        .as_ref()
        .map(|r| r.url.as_str())
        .unwrap_or("https://jgr.juglans.ai");

    let installer = registry::installer::PackageInstaller::with_defaults(registry_url)?;

    let project_dir = env::current_dir()?;
    let installed = installer.install_from_import(package, &project_dir).await?;

    // Update jgpackage.toml [dependencies] if it exists
    let manifest_path = project_dir.join("jgpackage.toml");
    if manifest_path.exists() {
        let content = fs::read_to_string(&manifest_path)?;
        let mut doc: toml::Table = toml::from_str(&content)?;

        let deps = doc
            .entry("dependencies")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let toml::Value::Table(deps_table) = deps {
            // Store as ^version constraint
            let constraint = format!("^{}", installed.version);
            deps_table.insert(installed.name.clone(), toml::Value::String(constraint));
        }

        fs::write(&manifest_path, toml::to_string_pretty(&doc)?)?;
    }

    println!(
        "Added {}@{} → jg_modules/{}",
        installed.name, installed.version, installed.name
    );
    Ok(())
}

fn handle_remove(package: &str) -> Result<()> {
    let project_dir = env::current_dir()?;

    // Remove from jgpackage.toml
    let manifest_path = project_dir.join("jgpackage.toml");
    if manifest_path.exists() {
        let content = fs::read_to_string(&manifest_path)?;
        let mut doc: toml::Table = toml::from_str(&content)?;

        if let Some(toml::Value::Table(deps)) = doc.get_mut("dependencies") {
            deps.remove(package);
        }

        fs::write(&manifest_path, toml::to_string_pretty(&doc)?)?;
    }

    // Remove symlink
    let config = JuglansConfig::load().ok();
    let registry_url = config
        .as_ref()
        .and_then(|c| c.registry.as_ref())
        .map(|r| r.url.as_str())
        .unwrap_or("https://jgr.juglans.ai");

    let installer = registry::installer::PackageInstaller::with_defaults(registry_url)?;
    installer.unlink(package, &project_dir)?;

    // Remove from lock file
    let mut lock = registry::lock::LockFile::load(&project_dir).unwrap_or_default();
    lock.remove(package);
    let _ = lock.save(&project_dir);

    println!("Removed {}", package);
    Ok(())
}

async fn handle_tui(agent: Option<&Path>) -> Result<()> {
    let mut app = ui::tui::app::App::new();
    if let Some(path) = agent {
        app.pending_agent_load = Some(path.to_path_buf());
    }
    ui::tui::run(app).await
}

async fn handle_cron(file: &PathBuf, schedule_override: Option<&str>) -> Result<()> {
    use cron::Schedule;
    use std::str::FromStr;

    let absolute_path =
        fs::canonicalize(file).with_context(|| format!("Cannot resolve {:?}", file))?;
    let source_raw = fs::read_to_string(&absolute_path)?;
    let ext = absolute_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // Parse file to extract schedule metadata
    let manifest_schedule = if ext == "jgflow" {
        GraphParser::parse_manifest(&source_raw)?.schedule.clone()
    } else {
        None
    };

    let cron_expr = schedule_override
        .map(|s| s.to_string())
        .or(manifest_schedule)
        .ok_or_else(|| {
            anyhow!(
                "No schedule found. Add `schedule: \"0 * * * *\"` to your .jgflow \
                 or use --schedule \"0 * * * *\""
            )
        })?;

    // cron crate requires 6-field (with seconds) or 7-field expressions.
    // If user provides standard 5-field cron, prepend "0" for seconds.
    let full_expr = if cron_expr.split_whitespace().count() == 5 {
        format!("0 {}", cron_expr)
    } else {
        cron_expr.clone()
    };

    let schedule = Schedule::from_str(&full_expr)
        .map_err(|e| anyhow!("Invalid cron expression '{}': {}", cron_expr, e))?;

    println!("⏰ Cron scheduler started for: {}", absolute_path.display());
    println!("   Schedule: {}", cron_expr);

    // Show next few fire times
    let upcoming: Vec<_> = schedule.upcoming(chrono::Utc).take(3).collect();
    for (i, t) in upcoming.iter().enumerate() {
        println!("   Next {}: {}", i + 1, t.format("%Y-%m-%d %H:%M:%S UTC"));
    }
    println!("   Press Ctrl+C to stop.\n");

    loop {
        let now = chrono::Utc::now();
        let next = schedule
            .upcoming(chrono::Utc)
            .next()
            .ok_or_else(|| anyhow!("No upcoming schedule"))?;

        let wait = (next - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(1));
        println!(
            "⏳ Waiting until {} ({:.0}s)...",
            next.format("%H:%M:%S UTC"),
            wait.as_secs_f64()
        );

        tokio::time::sleep(wait).await;

        println!(
            "🚀 Executing workflow at {}...",
            chrono::Utc::now().format("%H:%M:%S UTC")
        );

        // Build a synthetic CLI to reuse handle_file_logic
        let cli = Cli {
            file: Some(absolute_path.clone()),
            command: None,
            input: None,
            input_file: None,
            dry_run: false,
            output: None,
            output_format: "text".to_string(),
            chat_id: None,
            verbose: false,
            info: false,
        };

        match handle_file_logic(&cli).await {
            Ok(_) => println!("✅ Execution completed.\n"),
            Err(e) => println!("❌ Execution failed: {}\n", e),
        }
    }
}

async fn handle_test(path: Option<&Path>, filter: Option<&str>, format: &str) -> Result<()> {
    use testing::{reporter, TestRunner};

    let search_path = path.unwrap_or_else(|| Path::new("./tests"));

    // Discover test files
    let test_files = TestRunner::discover_test_files(search_path)?;

    if test_files.is_empty() {
        println!("\n  No test files found in {}\n", search_path.display());
        println!("  Test files are .jg files containing [test_*] nodes.");
        println!("  Default search path: ./tests/\n");
        return Ok(());
    }

    // Create runtime dependencies
    let config = JuglansConfig::load()?;
    let runtime: Arc<LocalRuntime> = Arc::new(LocalRuntime::new_with_config(&config.ai));
    let prompt_registry = Arc::new(PromptRegistry::new());

    let runner = TestRunner::new(runtime, prompt_registry);

    // Run all test files
    let mut all_results = Vec::new();
    for file in &test_files {
        match runner.run_file_filtered(file, filter).await {
            Ok(file_result) => {
                all_results.push(file_result);
            }
            Err(e) => {
                eprintln!("  Error running {}: {}", file.display(), e);
            }
        }
    }

    // Output results
    match format {
        "json" => reporter::print_json(&all_results),
        "junit" => reporter::print_junit(&all_results),
        _ => reporter::print_text(&all_results),
    }

    // Exit with code 1 if any failures
    let total_failed: usize = all_results.iter().map(|f| f.failed_count()).sum();
    if total_failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let application_cli = Cli::parse();

    // Disable logging completely in TUI mode to avoid corrupting ratatui rendering
    let is_tui = matches!(application_cli.command, Some(Commands::Chat { .. }));
    let default_filter = if is_tui {
        "off"
    } else {
        "juglans=info,tower_http=info"
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_filter));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    if let Some(sub_command_enum) = &application_cli.command {
        match sub_command_enum {
            Commands::Init { name } => handle_init(name)?,
            Commands::Install => handle_install().await?,
            Commands::Check { path, all, format } => {
                handle_check(path.as_deref(), *all, format)?;
            }
            Commands::Web { port, host } => {
                let current_dir = env::current_dir()?;
                let root = find_project_root(&current_dir)?;

                // 1. Try to load configuration
                let config = JuglansConfig::load().ok();

                // 2. Determine host (CLI > Config > Default)
                let final_host = host
                    .clone()
                    .or_else(|| config.as_ref().map(|c| c.server.host.clone()))
                    .unwrap_or_else(|| "127.0.0.1".to_string());

                // 3. Determine port (CLI > Config > Default: 8080)
                let final_port = port
                    .or_else(|| config.as_ref().map(|c| c.server.port))
                    .unwrap_or(8080);

                web_server::start_web_server(final_host, final_port, root).await?;
            }
            Commands::Whoami { verbose } => {
                handle_whoami(*verbose).await?;
            }
            Commands::Serve { port, host, entry } => {
                handle_serve(host.clone(), *port, entry.clone()).await?;
            }
            Commands::Bot {
                platform,
                agent,
                port,
            } => {
                handle_bot(platform, agent.clone(), *port).await?;
            }
            Commands::Skills { action } => {
                handle_skills(action).await?;
            }
            Commands::Pack { path, output } => {
                handle_pack(path.as_deref(), output.as_deref())?;
            }
            Commands::Publish { path } => {
                handle_publish(path.as_deref()).await?;
            }
            Commands::Add { package } => {
                handle_add(package).await?;
            }
            Commands::Remove { package } => {
                handle_remove(package)?;
            }
            Commands::Chat { agent } => {
                handle_tui(agent.as_deref()).await?;
            }
            Commands::Deploy {
                tag,
                port,
                build_only,
                push,
                stop,
                status,
                env_vars,
                path,
            } => {
                services::deploy::handle_deploy(services::deploy::DeployConfig {
                    tag: tag.clone(),
                    port: *port,
                    build_only: *build_only,
                    push: *push,
                    stop: *stop,
                    status: *status,
                    env_vars: env_vars.clone(),
                    path: path.clone(),
                })?;
            }
            Commands::Cron { file, schedule } => {
                handle_cron(file, schedule.as_deref()).await?;
            }
            Commands::Lsp => {
                lsp::run_server().await?;
            }
            Commands::Test {
                path,
                filter,
                format,
            } => {
                handle_test(path.as_deref(), filter.as_deref(), format).await?;
            }
            Commands::Doctest { path, format } => {
                let target = path.as_deref().unwrap_or(Path::new("./docs"));
                juglans::doctest::run_doctest(target, format)?;
            }
        }
    } else if application_cli.file.is_some() {
        handle_file_logic(&application_cli).await?;
    } else {
        println!("JWL Language Runtime (Multipurpose CLI)\nUse --help for command list.");
    }

    Ok(())
}
