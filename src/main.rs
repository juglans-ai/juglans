// src/main.rs
#![cfg(not(target_arch = "wasm32"))]

mod adapters;
mod builtins;
mod core;
mod registry;
mod runtime;
mod services;
mod templates;
mod testing;
mod ui;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info, warn};

use core::agent_parser::AgentParser;
use core::context::WorkflowContext;
use core::executor::WorkflowExecutor;
use core::parser::GraphParser;
use core::prompt_parser::PromptParser;
use core::renderer::JwlRenderer;
use core::resolver;
use core::skill_parser;
use core::validator::{ProjectContext, WorkflowValidator};
use services::agent_loader::AgentRegistry;
use services::config::JuglansConfig;
use services::github;
use services::interface::JuglansRuntime;
use services::jug0::Jug0Client;
use services::mcp::McpClient;
use services::prompt_loader::PromptRegistry;
use services::web_server;
use ui::{render::render_markdown, render::show_shortcuts, render::show_welcome, MultilineInput};

#[derive(Parser)]
#[command(name = "juglans", author = "Juglans Team", version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    /// Target file path to process (.jg, .jgprompt, .jgagent)
    file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Direct input for prompt variables or agent messages
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

    /// Output format (text or json)
    #[arg(long, default_value = "text")]
    output_format: String,

    /// Show agent/prompt info without executing
    #[arg(long)]
    info: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new project scaffold
    Init { name: String },
    /// Retrieve MCP tool schemas
    Install,
    /// Push resources to the server
    Push {
        /// Files or directories to push (if empty, uses workspace config)
        paths: Vec<PathBuf>,
        /// Force overwrite if resource already exists
        #[arg(long)]
        force: bool,
        /// Preview changes without pushing
        #[arg(long)]
        dry_run: bool,
        /// Filter by resource type (workflow, agent, prompt, tool, all)
        #[arg(long, short = 't')]
        r#type: Option<String>,
        /// Recursively scan directories
        #[arg(long, short = 'r')]
        recursive: bool,
        /// Override workflow endpoint URL (e.g. https://agent.juglans.ai)
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Validate syntax of .jg/.jgflow, .jgagent, .jgprompt files (like cargo check)
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
    /// Pull resources from the server
    Pull {
        /// Resource slug to pull
        slug: String,
        /// Resource type (prompt, agent, workflow)
        #[arg(long, short = 't')]
        r#type: String,
        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// List resources on the server
    List {
        /// Resource type to list (prompt, agent, workflow)
        #[arg(long, short = 't')]
        r#type: Option<String>,
    },
    /// Delete a resource from the server
    Delete {
        /// Resource slug to delete
        slug: String,
        /// Resource type (prompt, agent, workflow)
        #[arg(long, short = 't')]
        r#type: String,
    },
    /// Show current account information
    Whoami {
        /// Show detailed information
        #[arg(long, short = 'v')]
        verbose: bool,
        /// Test connection to Jug0 server
        #[arg(long)]
        check_connection: bool,
    },
    /// Start bot adapter (telegram, feishu)
    Bot {
        /// Platform: telegram, feishu
        platform: String,
        /// Agent slug to use (overrides config default)
        #[arg(long)]
        agent: Option<String>,
        /// Port for webhook-based platforms (feishu)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Manage Agent Skills (fetch from GitHub, convert to .jgprompt)
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
        /// Agent file to load (.jgagent)
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
    /// Run tests — discovers and executes _test_ blocks in .jg files
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
}

#[derive(Subcommand)]
enum SkillsAction {
    /// Fetch skills from a GitHub repository and save as .jgprompt
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
        /// Output directory for .jgprompt files (default: ./prompts)
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

/// Build a configured WorkflowExecutor with registries, MCP tools, and Python runtime.
async fn build_executor(
    config: &JuglansConfig,
    prompt_registry: PromptRegistry,
    agent_registry: AgentRegistry,
    workflow: Option<&Arc<core::graph::WorkflowGraph>>,
) -> Result<Arc<WorkflowExecutor>> {
    let runtime: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(config));

    let mut executor = WorkflowExecutor::new_with_debug(
        Arc::new(prompt_registry),
        Arc::new(agent_registry),
        runtime,
        config.debug.clone(),
    )
    .await;

    executor.load_mcp_tools(config).await;
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

/// Run an interactive agent REPL loop (shared between local and remote agents).
async fn run_agent_loop(
    executor: Arc<WorkflowExecutor>,
    agent_name: &str,
    agent_slug: &str,
    context: &WorkflowContext,
    workflow: Option<&Arc<core::graph::WorkflowGraph>>,
    cli_input: Option<&str>,
    extra_chat_params: HashMap<String, String>,
) -> Result<()> {
    let mut input_widget = MultilineInput::new();

    loop {
        let session_input = if let Some(cmd_input) = cli_input {
            cmd_input.to_string()
        } else {
            let chat_id = context
                .resolve_path("reply.chat_id")
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()));

            match input_widget.prompt(agent_name, chat_id.as_deref())? {
                Some(input) => {
                    let trimmed = input.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed == "help" {
                        show_shortcuts();
                        continue;
                    }
                    if trimmed == "exit" || trimmed == "quit" {
                        println!("\nGoodbye!");
                        break;
                    }
                    trimmed.to_string()
                }
                None => {
                    println!("\nGoodbye!");
                    break;
                }
            }
        };

        if session_input.is_empty() {
            continue;
        }

        context.set("input.message".to_string(), json!(session_input.clone()))?;
        context.set("reply.output".to_string(), json!(""))?;
        context.set("reply.status".to_string(), json!("processing"))?;

        if let Some(target_flow) = workflow {
            println!("\n⚡ Workflow Execution...");
            if let Err(e) = executor
                .clone()
                .execute_graph(target_flow.clone(), context)
                .await
            {
                error!("❌ Execution Failed: {}\n", e);
            } else {
                println!("✓ Workflow Completed\n");
            }

            let answer = context
                .resolve_path("reply.output")?
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();

            if !answer.is_empty() {
                render_markdown(&answer);
                println!();
            }
        } else {
            let mut params = HashMap::from([
                ("agent".to_string(), agent_slug.to_string()),
                ("message".to_string(), session_input),
            ]);
            params.extend(extra_chat_params.clone());

            match executor
                .execute_tool_internal("chat", &params, context)
                .await
            {
                Ok(Some(result)) => {
                    if let Some(text) = result.as_str() {
                        render_markdown(text);
                        println!();
                    } else if let Some(obj) = result.as_object() {
                        if let Some(text) = obj.get("response").and_then(|v| v.as_str()) {
                            render_markdown(text);
                            println!();
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    error!("❌ Chat error: {}", e);
                }
            }
        }

        if cli_input.is_some() {
            break;
        }
    }

    Ok(())
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

            // 计算 @ 路径别名的基准目录
            let at_base: Option<PathBuf> = local_config
                .paths
                .base
                .as_ref()
                .map(|b| project_root_path.join(b));

            let mut workflow_definition_obj = if file_ext_name == "jgflow" {
                GraphParser::parse_manifest(&source_raw_text)?
            } else {
                GraphParser::parse(&source_raw_text)?
            };

            // .jg 文件：metadata keys 仅作注释，warn 用户
            if file_ext_name == "jg" {
                let has_meta = !workflow_definition_obj.slug.is_empty()
                    || !workflow_definition_obj.name.is_empty()
                    || !workflow_definition_obj.version.is_empty()
                    || !workflow_definition_obj.author.is_empty()
                    || !workflow_definition_obj.description.is_empty();
                if has_meta {
                    warn!("Metadata in .jg files has no effect (use a .jgflow manifest instead)");
                }
                workflow_definition_obj.slug = String::new();
                workflow_definition_obj.name = String::new();
                workflow_definition_obj.version = String::new();
                workflow_definition_obj.author = String::new();
                workflow_definition_obj.description = String::new();
            }

            // .jgflow 文件：纯清单，不允许节点/边定义
            if file_ext_name == "jgflow" {
                if workflow_definition_obj.graph.node_count() > 0 {
                    anyhow::bail!(".jgflow is a manifest file — node definitions [name] and edges are not allowed.\nPut logic in a .jg file and reference it with: source: \"./your_file.jg\"");
                }
                if workflow_definition_obj.source.is_empty() {
                    anyhow::bail!(".jgflow manifest requires a 'source' field pointing to a .jg file.\nExample: source: \"./main.jg\"");
                }
            }

            // .jgflow 文件：读取 source 指向的 .jg 文件，合并逻辑
            if file_ext_name == "jgflow" && !workflow_definition_obj.source.is_empty() {
                let source_path = file_parent_context.join(&workflow_definition_obj.source);
                let source_content = fs::read_to_string(&source_path).with_context(|| {
                    format!("Failed to read source: {}", workflow_definition_obj.source)
                })?;
                let source_wf = GraphParser::parse(&source_content)?;

                // 保存清单的 metadata
                let manifest = workflow_definition_obj.clone();

                // 用源文件的逻辑替换（graph, nodes, edges, functions 等）
                workflow_definition_obj = source_wf;

                // 清单 metadata 覆盖（非空字段）
                if !manifest.slug.is_empty() {
                    workflow_definition_obj.slug = manifest.slug;
                }
                if !manifest.name.is_empty() {
                    workflow_definition_obj.name = manifest.name;
                }
                if !manifest.version.is_empty() {
                    workflow_definition_obj.version = manifest.version;
                }
                if !manifest.author.is_empty() {
                    workflow_definition_obj.author = manifest.author;
                }
                if !manifest.description.is_empty() {
                    workflow_definition_obj.description = manifest.description;
                }
                if !manifest.libs.is_empty() {
                    workflow_definition_obj.libs = manifest.libs;
                    workflow_definition_obj.lib_imports = manifest.lib_imports;
                    workflow_definition_obj.lib_auto_namespaces = manifest.lib_auto_namespaces;
                }
                if !manifest.entry_node.is_empty() {
                    workflow_definition_obj.entry_node = manifest.entry_node;
                }
                if !manifest.prompt_patterns.is_empty() {
                    workflow_definition_obj.prompt_patterns = manifest.prompt_patterns;
                }
                if !manifest.agent_patterns.is_empty() {
                    workflow_definition_obj.agent_patterns = manifest.agent_patterns;
                }
                if !manifest.tool_patterns.is_empty() {
                    workflow_definition_obj.tool_patterns = manifest.tool_patterns;
                }

                // base_dir 切换到 source 文件所在目录
                let source_parent = source_path.parent().unwrap_or(Path::new("."));
                relative_base_offset = pathdiff::diff_paths(source_parent, &project_root_path)
                    .unwrap_or(PathBuf::from("."));
            }

            // 解析 lib imports（提取函数定义到命名空间）
            let mut import_stack = vec![absolute_target_path.clone()];
            resolver::resolve_lib_imports(
                &mut workflow_definition_obj,
                &relative_base_offset,
                &mut import_stack,
                at_base.as_deref(),
            )?;

            // 解析 flow imports 并合并子图（编译时图合并）
            import_stack = vec![absolute_target_path.clone()];
            resolver::resolve_flow_imports(
                &mut workflow_definition_obj,
                &relative_base_offset,
                &mut import_stack,
                at_base.as_deref(),
            )?;

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
            let mut agent_registry_inst = AgentRegistry::new();

            let resolved_p_patterns = resolve_import_patterns_verbose(
                &relative_base_offset,
                &workflow_definition_obj.prompt_patterns,
                at_base.as_deref(),
            );
            let resolved_a_patterns = resolve_import_patterns_verbose(
                &relative_base_offset,
                &workflow_definition_obj.agent_patterns,
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
            if !resolved_a_patterns.is_empty() {
                agent_registry_inst.load_from_paths(&resolved_a_patterns)?;
            }

            let shared_executor_engine = build_executor(
                &local_config,
                prompt_registry_inst,
                agent_registry_inst,
                Some(&workflow_definition_obj),
            )
            .await?;

            // 解析 CLI 输入
            let input_value: Option<serde_json::Value> =
                resolve_input_data(cli)?.and_then(|s| serde_json::from_str(&s).ok());

            shared_executor_engine
                .run_with_input(workflow_definition_obj, &local_config, input_value)
                .await?;
        }

        "jgagent" => {
            let agent_meta_definition = AgentParser::parse(&source_raw_text)?;

            // 显示欢迎信息
            show_welcome(
                &agent_meta_definition.name,
                &agent_meta_definition.slug,
                agent_meta_definition.workflow.is_some(),
            );

            let global_system_config = JuglansConfig::load()?;

            // 计算 @ 路径别名的基准目录
            let at_base: Option<PathBuf> = global_system_config
                .paths
                .base
                .as_ref()
                .map(|b| project_root_path.join(b));

            let mut local_p_store = PromptRegistry::new();
            let mut local_a_store = AgentRegistry::new();

            // 【修复】将当前 agent 注册到本地 registry，使 pure agent 也能正常工作
            local_a_store.register(agent_meta_definition.clone(), absolute_target_path.clone());

            let mut active_workflow_ptr = None;

            if let Some(wf_path_string) = &agent_meta_definition.workflow {
                let wf_physical_path = relative_base_offset.join(wf_path_string);
                let wf_source_data_str =
                    fs::read_to_string(&wf_physical_path).with_context(|| {
                        format!("Linked logic file missing: {:?}", wf_physical_path)
                    })?;

                let mut workflow_parsed_data = GraphParser::parse(&wf_source_data_str)?;
                let wf_context_base_dir = wf_physical_path.parent().unwrap_or(Path::new("."));

                // 解析 lib imports（提取函数定义到命名空间）
                let wf_canonical = fs::canonicalize(&wf_physical_path)
                    .unwrap_or_else(|_| wf_physical_path.clone());
                let mut import_stack = vec![wf_canonical.clone()];
                resolver::resolve_lib_imports(
                    &mut workflow_parsed_data,
                    wf_context_base_dir,
                    &mut import_stack,
                    at_base.as_deref(),
                )?;

                // 解析 flow imports 并合并子图（编译时图合并）
                import_stack = vec![wf_canonical];
                resolver::resolve_flow_imports(
                    &mut workflow_parsed_data,
                    wf_context_base_dir,
                    &mut import_stack,
                    at_base.as_deref(),
                )?;

                let p_import_list = resolve_import_patterns_verbose(
                    wf_context_base_dir,
                    &workflow_parsed_data.prompt_patterns,
                    at_base.as_deref(),
                );
                let a_import_list = resolve_import_patterns_verbose(
                    wf_context_base_dir,
                    &workflow_parsed_data.agent_patterns,
                    at_base.as_deref(),
                );
                let t_import_list = resolve_import_patterns_verbose(
                    wf_context_base_dir,
                    &workflow_parsed_data.tool_patterns,
                    at_base.as_deref(),
                );

                local_p_store.load_from_paths(&p_import_list)?;
                local_a_store.load_from_paths(&a_import_list)?;

                // Update workflow with resolved tool patterns
                workflow_parsed_data.tool_patterns = t_import_list;

                active_workflow_ptr = Some(Arc::new(workflow_parsed_data));
            }

            let primary_executor_ptr = build_executor(
                &global_system_config,
                local_p_store,
                local_a_store,
                active_workflow_ptr.as_ref(),
            )
            .await?;

            let multi_turn_interaction_ctx = WorkflowContext::new();
            let resolved_cli_input = resolve_input_data(cli)?;

            // Set agent definition in context (persists across REPL iterations)
            multi_turn_interaction_ctx
                .set("input.agent".to_string(), json!(agent_meta_definition))?;

            run_agent_loop(
                primary_executor_ptr,
                &agent_meta_definition.name,
                &agent_meta_definition.slug,
                &multi_turn_interaction_ctx,
                active_workflow_ptr.as_ref(),
                resolved_cli_input.as_deref(),
                HashMap::new(),
            )
            .await?;
        }

        "jgprompt" => {
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

/// Handle remote agent via @handle (e.g., `juglans @jarvis`)
async fn handle_remote_agent(cli: &Cli, handle: &str) -> Result<()> {
    info!("🌐 Connecting to remote agent: {}", handle);

    // Load config
    let config = JuglansConfig::load()?;
    let jug0_client = Jug0Client::new(&config);

    // Fetch agent from jug0
    let agent_detail = jug0_client.get_agent(handle).await?;

    let agent_name = agent_detail
        .name
        .clone()
        .unwrap_or_else(|| handle.to_string());
    let agent_slug = agent_detail.slug.clone();

    // Show welcome
    show_welcome(&agent_name, &agent_slug, false);

    // Build executor with empty registries (remote agent, no local files)
    let executor_ptr =
        build_executor(&config, PromptRegistry::new(), AgentRegistry::new(), None).await?;

    let context = WorkflowContext::new();
    let resolved_cli_input = resolve_input_data(cli)?;

    // Build extra chat parameters from remote agent config
    let mut extra_chat_params = HashMap::new();
    if let Some(ref model) = agent_detail.default_model {
        extra_chat_params.insert("model".to_string(), model.clone());
    }
    if let Some(ref sp) = agent_detail.system_prompt {
        extra_chat_params.insert("system_prompt".to_string(), sp.content.clone());
    }
    if let Some(temp) = agent_detail.temperature {
        extra_chat_params.insert("temperature".to_string(), temp.to_string());
    }

    run_agent_loop(
        executor_ptr,
        &agent_name,
        &agent_slug,
        &context,
        None,
        resolved_cli_input.as_deref(),
        extra_chat_params,
    )
    .await?;

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

    // 2. Fetch MCP tool schemas (existing behavior)
    let runtime_config = JuglansConfig::load()?;
    if !runtime_config.mcp_servers.is_empty() {
        let schema_client = McpClient::new();
        for server_item in runtime_config.mcp_servers {
            info!("Fetching schemas for [{}]...", server_item.name);
            let _ = schema_client.fetch_tools(&server_item).await;
        }
    }

    Ok(())
}

async fn handle_pull(slug: &str, resource_type: &str, output_dir: Option<&Path>) -> Result<()> {
    let local_config = JuglansConfig::load()?;
    let jug0_client = Jug0Client::new(&local_config);

    let (content, filename) = jug0_client.pull_resource(slug, resource_type).await?;

    let output_path = if let Some(dir) = output_dir {
        dir.join(&filename)
    } else {
        PathBuf::from(&filename)
    };

    fs::write(&output_path, &content)?;
    println!("✅ Pulled {} to {:?}", slug, output_path);
    Ok(())
}

async fn handle_list(resource_type: Option<&str>) -> Result<()> {
    let local_config = JuglansConfig::load()?;
    let jug0_client = Jug0Client::new(&local_config);

    let resources = jug0_client.list_resources(resource_type).await?;

    if resources.is_empty() {
        println!("No resources found.");
    } else {
        for resource in resources {
            println!("  {} ({})", resource.slug, resource.resource_type);
        }
    }
    Ok(())
}

async fn handle_delete(slug: &str, resource_type: &str) -> Result<()> {
    let local_config = JuglansConfig::load()?;
    let jug0_client = Jug0Client::new(&local_config);

    jug0_client.delete_resource(slug, resource_type).await?;
    println!("✅ Deleted {} ({})", slug, resource_type);
    Ok(())
}

async fn handle_whoami(verbose: bool, check_connection: bool) -> Result<()> {
    let config = JuglansConfig::load()?;
    let config_path = if Path::new("juglans.toml").exists() {
        "./juglans.toml"
    } else {
        "~/.config/juglans/juglans.toml or system default"
    };

    println!("\n📋 Account Information\n");

    // Try to get server user info
    let jug0_client = Jug0Client::new(&config);
    let server_user = if config.account.api_key.is_some()
        && !config.account.api_key.as_ref().unwrap().is_empty()
    {
        match jug0_client.get_current_user().await {
            Ok(user) => Some(user),
            Err(e) => {
                if verbose {
                    println!(
                        "\x1b[33m⚠️  Unable to fetch server user info: {}\x1b[0m\n",
                        e
                    );
                }
                None
            }
        }
    } else {
        None
    };

    // Display server user info if available
    if let Some(user) = &server_user {
        println!("\x1b[1m🌐 Server Account (from Jug0)\x1b[0m");
        println!("User ID:       {}", user.id);
        println!("Username:      {}", user.username);
        if let Some(email) = &user.email {
            println!("Email:         {}", email);
        }
        if let Some(role) = &user.role {
            println!("Role:          {}", role);
        }
        if let Some(org_id) = &user.org_id {
            println!(
                "Organization:  {} ({})",
                user.org_name.as_deref().unwrap_or(""),
                org_id
            );
        }
        println!();
    }

    // Local config info
    println!("\x1b[1m💻 Local Configuration\x1b[0m");
    println!("User ID:       {}", config.account.id);
    println!("Name:          {}", config.account.name);

    if let Some(role) = &config.account.role {
        println!("Role:          {}", role);
    }

    // API Key (masked)
    if let Some(api_key) = &config.account.api_key {
        if api_key.is_empty() {
            println!("API Key:       \x1b[33m⚠️  Not configured\x1b[0m");
        } else {
            let masked = mask_api_key(api_key);
            let status = if server_user.is_some() {
                "\x1b[32m✅ Valid\x1b[0m"
            } else {
                "\x1b[33m(not verified)\x1b[0m"
            };
            println!("API Key:       {} {}", masked, status);
        }
    } else {
        println!("API Key:       \x1b[33m⚠️  Not configured\x1b[0m");
    }

    println!();

    // Workspace info
    if let Some(workspace) = &config.workspace {
        println!("Workspace:     {} ({})", workspace.id, workspace.name);
        if let Some(members) = &workspace.members {
            println!("Members:       {} user(s)", members.len());
        }

        // Resource paths (verbose mode)
        if verbose {
            if !workspace.agents.is_empty() {
                println!("\nResource Paths:");
                println!("  Agents:      {}", workspace.agents.join(", "));
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

    // Server info
    println!("Jug0 Server:   {}", config.jug0.base_url);

    // Connection test
    if check_connection {
        print!("Status:        ");
        io::stdout().flush()?;

        match test_connection(&config).await {
            Ok(true) => println!("\x1b[32m✅ Connected\x1b[0m"),
            Ok(false) => println!("\x1b[33m⚠️  Server unreachable\x1b[0m"),
            Err(e) => println!("\x1b[31m❌ Error: {}\x1b[0m", e),
        }
    }

    println!();

    // Web server config (verbose)
    if verbose {
        println!(
            "Web Server:    {}:{}",
            config.server.host, config.server.port
        );
        println!();
    }

    // MCP servers
    if !config.mcp_servers.is_empty() {
        println!("MCP Servers:   {} configured", config.mcp_servers.len());
        if verbose {
            for server in &config.mcp_servers {
                let alias_str = server.alias.as_deref().unwrap_or("");
                let alias_display = if !alias_str.is_empty() {
                    format!(" (alias: {})", alias_str)
                } else {
                    String::new()
                };
                println!("  - {}{}: {}", server.name, alias_display, server.base_url);
            }
        }
        println!();
    }

    // Config file location
    println!("Config:        {}", config_path);
    println!();

    Ok(())
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 12 {
        return "***".to_string();
    }
    let prefix = &key[..8];
    let suffix = &key[key.len() - 3..];
    format!("{}...{}", prefix, suffix)
}

async fn test_connection(config: &JuglansConfig) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let health_url = format!("{}/health", config.jug0.base_url);

    match client.get(&health_url).send().await {
        Ok(response) => Ok(response.status().is_success()),
        Err(_) => Ok(false),
    }
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
            check_path
                .join("**/*.jgagent")
                .to_string_lossy()
                .to_string(),
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
    let mut agent_count = 0;
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
        println!("    \x1b[33mNo .jg/.jgflow, .jgagent, or .jgprompt files found\x1b[0m");
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
                "jgagent" => {
                    if let Ok(agent) = AgentParser::parse(&content) {
                        project_ctx.agent_slugs.insert(agent.slug);
                    }
                }
                "jgprompt" => {
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
                    let parse_result = if ext == "jgflow" {
                        GraphParser::parse_manifest(&content)
                    } else {
                        GraphParser::parse(&content)
                    };
                    match parse_result {
                        Ok(graph) => {
                            // .jgflow 清单校验：不允许节点定义，必须有 source
                            if ext == "jgflow" {
                                if graph.graph.node_count() > 0 {
                                    error_count += 1;
                                    println!("    \x1b[1;31merror\x1b[0m[manifest]: {} — .jgflow must not contain node definitions", relative_path);
                                    continue;
                                }
                                if graph.source.is_empty() {
                                    error_count += 1;
                                    println!("    \x1b[1;31merror\x1b[0m[manifest]: {} — missing 'source' field", relative_path);
                                    continue;
                                }
                                let source_path =
                                    entry.parent().unwrap_or(Path::new(".")).join(&graph.source);
                                if !source_path.exists() {
                                    error_count += 1;
                                    println!("    \x1b[1;31merror\x1b[0m[manifest]: {} — source file not found: {}", relative_path, graph.source);
                                    continue;
                                }
                                // 清单校验通过 — 跳过 workflow 逻辑校验（source .jg 会单独校验）
                                valid_count += 1;
                                if show_all {
                                    println!(
                                        "    \x1b[32mok\x1b[0m[manifest]: {} → {}",
                                        relative_path, graph.source
                                    );
                                }
                                continue;
                            }

                            project_ctx.file_dir =
                                entry.parent().unwrap_or(Path::new(".")).to_path_buf();
                            let validation =
                                WorkflowValidator::validate_with_project(&graph, &project_ctx);
                            let slug = if graph.slug.is_empty() {
                                file_name.to_string()
                            } else {
                                graph.slug.clone()
                            };

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "workflow",
                                    "slug": slug,
                                    "valid": validation.is_valid,
                                    "errors": validation.errors,
                                    "warnings": validation.warnings,
                                }));
                            }

                            if validation.is_valid {
                                valid_count += 1;
                                if validation.warning_count() > 0 {
                                    warning_count += validation.warning_count();
                                    if show_all {
                                        println!("    \x1b[33mwarning\x1b[0m[workflow]: {} ({} warning(s))", relative_path, validation.warning_count());
                                        for warn in &validation.warnings {
                                            println!(
                                                "      \x1b[33m-->\x1b[0m [{}] {}",
                                                warn.code, warn.message
                                            );
                                        }
                                    }
                                }
                            } else {
                                error_count += 1;
                                warning_count += validation.warning_count();
                                println!("    \x1b[1;31merror\x1b[0m[workflow]: {} ({} error(s), {} warning(s))",
                                        relative_path, validation.error_count(), validation.warning_count());
                                for err in &validation.errors {
                                    println!(
                                        "      \x1b[31m-->\x1b[0m [{}] {}",
                                        err.code, err.message
                                    );
                                }
                                if show_all {
                                    for warn in &validation.warnings {
                                        println!(
                                            "      \x1b[33m-->\x1b[0m [{}] {}",
                                            warn.code, warn.message
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // parse() 失败 → 尝试作为纯库文件解析
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

                            // 两种解析都失败，报原始错误
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
                "jgagent" => {
                    agent_count += 1;
                    match AgentParser::parse(&content) {
                        Ok(agent) => {
                            valid_count += 1;
                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "agent",
                                    "slug": agent.slug,
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
                                "    \x1b[1;31merror\x1b[0m[agent]: {} (parse failed)",
                                relative_path
                            );
                            for line in full_err.lines() {
                                println!("      \x1b[31m-->\x1b[0m {}", line);
                            }

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "agent",
                                    "slug": file_name,
                                    "valid": false,
                                    "errors": [{"code": "PARSE", "message": full_err}],
                                    "warnings": [],
                                }));
                            }
                        }
                    }
                }
                "jgprompt" => {
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
                    "agents": agent_count,
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
        if agent_count > 0 {
            parts.push(format!("{} agent(s)", agent_count));
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

async fn handle_push(
    paths: Vec<PathBuf>,
    force: bool,
    dry_run: bool,
    resource_type: Option<String>,
    recursive: bool,
    endpoint: Option<String>,
) -> Result<()> {
    let local_config = JuglansConfig::load()?;

    // 收集要处理的文件
    let mut files_to_push = Vec::new();

    if paths.is_empty() {
        // 无参数：使用 workspace 配置
        println!("📦 Using workspace configuration from juglans.toml");

        if let Some(ref workspace) = local_config.workspace {
            let patterns = match resource_type.as_deref() {
                Some("workflow") => workspace.workflows.clone(),
                Some("agent") => workspace.agents.clone(),
                Some("prompt") => workspace.prompts.clone(),
                Some("tool") => workspace.tools.clone(),
                Some("all") | None => {
                    let mut all = Vec::new();
                    all.extend(workspace.workflows.clone());
                    all.extend(workspace.agents.clone());
                    all.extend(workspace.prompts.clone());
                    all.extend(workspace.tools.clone());
                    all
                }
                _ => {
                    return Err(anyhow!(
                        "Invalid resource type. Use: workflow, agent, prompt, tool, all"
                    ))
                }
            };

            for pattern in patterns {
                for entry in glob::glob(&pattern)? {
                    let path = entry?;
                    if !should_exclude(&path, &workspace.exclude) {
                        files_to_push.push(path);
                    }
                }
            }
        } else {
            return Err(anyhow!("No workspace configuration found in juglans.toml"));
        }
    } else {
        // 有参数：扫描指定路径
        for path in paths {
            if path.is_file() {
                files_to_push.push(path);
            } else if path.is_dir() {
                scan_directory(&path, &mut files_to_push, recursive, &resource_type)?;
            } else {
                // Glob 模式
                for entry in glob::glob(path.to_str().unwrap_or(""))? {
                    files_to_push.push(entry?);
                }
            }
        }
    }

    if files_to_push.is_empty() {
        println!("⚠️  No files found to push.");
        return Ok(());
    }

    // Sort files by dependency order: workflows → prompts → agents
    // This ensures agents can reference workflows that were just created
    files_to_push.sort_by_key(|path| {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        match ext {
            "jg" | "jgflow" => 0, // Workflows first (no dependencies)
            "jgprompt" => 1,      // Prompts second (agents reference them)
            "jgagent" => 2,       // Agents last (depend on workflows and prompts)
            "json" => 3,          // Tool definitions last (local only)
            _ => 4,
        }
    });

    // 统计
    let mut stats = PushStats::default();
    for file in &files_to_push {
        if let Some(ext) = file.extension().and_then(|s| s.to_str()) {
            match ext {
                "jg" | "jgflow" => stats.workflows += 1,
                "jgagent" => stats.agents += 1,
                "jgprompt" => stats.prompts += 1,
                "json" => stats.tools += 1,
                _ => {}
            }
        }
    }

    println!("\n📂 Found resources:");
    if stats.workflows > 0 {
        println!("  📄 {} workflow(s)", stats.workflows);
    }
    if stats.agents > 0 {
        println!("  👤 {} agent(s)", stats.agents);
    }
    if stats.prompts > 0 {
        println!("  📝 {} prompt(s)", stats.prompts);
    }
    if stats.tools > 0 {
        println!("  🔧 {} tool definition(s)", stats.tools);
    }

    if dry_run {
        println!("\n🔍 Dry run mode - preview only:\n");
        for file in &files_to_push {
            println!("  ✓ {}", file.display());
        }
        println!("\n📊 Total: {} file(s)", files_to_push.len());
        println!("\nRun without --dry-run to push.");
        return Ok(());
    }

    // Pre-push validation for workflow files
    {
        let mut validation_failed = false;
        for file in &files_to_push {
            let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext == "jgflow" {
                if let Ok(content) = std::fs::read_to_string(file) {
                    if let Ok(manifest) = GraphParser::parse_manifest(&content) {
                        if !manifest.source.is_empty() {
                            let source_path = file
                                .parent()
                                .unwrap_or(Path::new("."))
                                .join(&manifest.source);
                            if let Ok(source_content) = std::fs::read_to_string(&source_path) {
                                if let Ok(graph) = GraphParser::parse(&source_content) {
                                    let result = WorkflowValidator::validate(&graph);
                                    if !result.is_valid {
                                        eprint!(
                                            "{}",
                                            result.format_report(&file.display().to_string())
                                        );
                                        validation_failed = true;
                                    } else if result.warning_count() > 0 {
                                        eprint!(
                                            "{}",
                                            result.format_report(&file.display().to_string())
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if validation_failed {
            anyhow::bail!("Push aborted due to validation errors.");
        }
    }

    // 实际执行
    println!("\n📤 Pushing resources...\n");

    let jug0_api_ptr = Jug0Client::new(&local_config);
    let mut success_count = 0;
    let mut skip_count = 0;
    let mut error_count = 0;
    let mut workflow_slug_map: HashMap<String, String> = HashMap::new();

    for file in &files_to_push {
        match push_single_file(
            file,
            &jug0_api_ptr,
            &local_config,
            force,
            &endpoint,
            &mut workflow_slug_map,
        )
        .await
        {
            Ok(PushResult::Success(msg)) => {
                println!("  ✅ {}", msg);
                success_count += 1;
            }
            Ok(PushResult::Skipped(msg)) => {
                println!("  ⚠️  {}", msg);
                skip_count += 1;
            }
            Err(e) => {
                println!("  ❌ {}: {}", file.display(), e);
                error_count += 1;
            }
        }
    }

    println!("\n📊 Summary:");
    println!("  ✅ {} succeeded", success_count);
    if skip_count > 0 {
        println!("  ⚠️  {} skipped", skip_count);
    }
    if error_count > 0 {
        println!("  ❌ {} failed", error_count);
    }

    if error_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Default)]
struct PushStats {
    workflows: usize,
    agents: usize,
    prompts: usize,
    tools: usize,
}

enum PushResult {
    Success(String),
    Skipped(String),
}

fn should_exclude(path: &Path, exclude_patterns: &[String]) -> bool {
    let path_str = path.to_str().unwrap_or("");
    for pattern in exclude_patterns {
        if glob::Pattern::new(pattern)
            .ok()
            .is_some_and(|p| p.matches(path_str))
        {
            return true;
        }
    }
    false
}

fn scan_directory(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    recursive: bool,
    resource_type: &Option<String>,
) -> Result<()> {
    let extensions = match resource_type.as_deref() {
        Some("workflow") => vec!["jgflow"],
        Some("agent") => vec!["jgagent"],
        Some("prompt") => vec!["jgprompt"],
        Some("tool") => vec!["json"],
        Some("all") | None => vec!["jgflow", "jgagent", "jgprompt", "json"],
        _ => vec![],
    };

    let pattern = if recursive {
        format!("{}/**/*", dir.display())
    } else {
        format!("{}/*", dir.display())
    };

    for entry in glob::glob(&pattern)? {
        let path = entry?;
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if extensions.contains(&ext) {
                    files.push(path);
                }
            }
        }
    }

    Ok(())
}

async fn push_single_file(
    file: &Path,
    jug0_client: &Jug0Client,
    config: &JuglansConfig,
    force: bool,
    endpoint: &Option<String>,
    workflow_slug_map: &mut HashMap<String, String>,
) -> Result<PushResult> {
    let raw_file_data = fs::read_to_string(file)?;
    let ext_str = file.extension().and_then(|s| s.to_str()).unwrap_or("");
    let filename = file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    match ext_str {
        "jgagent" => {
            let mut agent = AgentParser::parse(&raw_file_data)?;
            if let Some(ref wf_ref) = agent.workflow {
                let stem = Path::new(wf_ref.as_str())
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(wf_ref);
                if let Some(real_slug) = workflow_slug_map.get(stem) {
                    agent.workflow = Some(real_slug.clone());
                }
            }
            let msg = jug0_client.apply_agent(&agent, force).await?;
            Ok(PushResult::Success(format!(
                "agent: {} - {}",
                filename, msg
            )))
        }
        "jgprompt" => {
            let msg = jug0_client
                .apply_prompt(&PromptParser::parse(&raw_file_data)?, force)
                .await?;
            Ok(PushResult::Success(format!(
                "prompt: {} - {}",
                filename, msg
            )))
        }
        "jg" => {
            Ok(PushResult::Skipped(format!(
                "workflow: {} - .jg files cannot be pushed directly. Use a .jgflow manifest with: source: \"./{}\"",
                filename, filename
            )))
        }
        "jgflow" => {
            let mut workflow = GraphParser::parse_manifest(&raw_file_data)?;

            // .jgflow 是纯清单，不允许节点定义
            if workflow.graph.node_count() > 0 {
                anyhow::bail!("{}: .jgflow is a manifest file — node definitions are not allowed", filename);
            }

            // 读取 source 指向的 .jg 文件内容
            let definition = if !workflow.source.is_empty() {
                let source_path = file.parent().unwrap_or(Path::new(".")).join(&workflow.source);
                fs::read_to_string(&source_path)
                    .with_context(|| format!("Failed to read source: {}", workflow.source))?
            } else {
                raw_file_data.clone()
            };

            if workflow.slug.is_empty() {
                workflow.slug = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unnamed")
                    .to_string();
            }

            // Priority: CLI --endpoint > config endpoint_url > default host:port
            let endpoint_url = endpoint
                .clone()
                .or_else(|| config.server.endpoint_url.clone())
                .map(|url| {
                    let base = url.trim_end_matches('/');
                    if base.ends_with("/api/chat") {
                        base.to_string()
                    } else {
                        format!("{}/api/chat", base)
                    }
                })
                .unwrap_or_else(|| {
                    format!(
                        "http://{}:{}/api/chat",
                        config.server.host, config.server.port
                    )
                });

            let msg = jug0_client
                .apply_workflow(&workflow, &definition, &endpoint_url, force)
                .await?;

            // Record slug mapping so agents can resolve workflow refs
            if !workflow.source.is_empty() {
                if let Some(stem) = Path::new(&workflow.source)
                    .file_stem()
                    .and_then(|s| s.to_str())
                {
                    workflow_slug_map.insert(stem.to_string(), workflow.slug.clone());
                }
            }

            Ok(PushResult::Success(format!(
                "workflow: {} - {}",
                filename, msg
            )))
        }
        "json" => {
            // Tool definition files - skip for now as they don't need to be uploaded
            // They are loaded locally by workflows when needed
            Ok(PushResult::Skipped(format!(
                "tool: {} - Tool definitions are loaded locally, no upload needed",
                filename
            )))
        }
        _ => Err(anyhow!("Unsupported file type: {}", ext_str)),
    }
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
        _ => {
            return Err(anyhow!(
                "Unknown platform '{}'. Supported: telegram, feishu",
                platform
            ));
        }
    }
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
                        let jgprompt_content = skill_parser::skill_to_jgprompt(&skill);
                        let output_path = prompts_dir.join(format!("{}.jgprompt", skill.name));
                        fs::write(&output_path, &jgprompt_content)?;
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
                "Use 'juglans push {}/*.jgprompt' to push to jug0.",
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
                    if path.extension().and_then(|e| e.to_str()) == Some("jgprompt") {
                        let name = path.file_stem().unwrap().to_string_lossy();
                        println!("  - {}", name);
                        found = true;
                    }
                }
            }
            if !found {
                println!("No .jgprompt files found in prompts/");
            }
        }
        SkillsAction::Remove { name } => {
            let path = PathBuf::from(format!("./prompts/{}.jgprompt", name));
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

    let api_key = config
        .account
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow!("account.api_key is required for publishing"))?;

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

async fn handle_tui(_agent: Option<&Path>) -> Result<()> {
    let app = ui::tui::app::App::new();
    ui::tui::run(app).await
}

async fn handle_test(path: Option<&Path>, filter: Option<&str>, format: &str) -> Result<()> {
    use testing::{reporter, TestRunner};

    let search_path = path.unwrap_or_else(|| Path::new("./tests"));

    // Discover test files
    let test_files = TestRunner::discover_test_files(search_path)?;

    if test_files.is_empty() {
        println!("\n  No test files found in {}\n", search_path.display());
        println!("  Test files are .jg files containing [_test_*] function blocks.");
        println!("  Default search path: ./tests/\n");
        return Ok(());
    }

    // Create runtime dependencies
    let config = JuglansConfig::load()?;
    let runtime: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(&config));
    let prompt_registry = Arc::new(PromptRegistry::new());
    let agent_registry = Arc::new(AgentRegistry::new());

    let runner = TestRunner::new(runtime, prompt_registry, agent_registry);

    // Run all test files
    let mut all_results = Vec::new();
    for file in &test_files {
        match runner.run_file(file).await {
            Ok(mut file_result) => {
                // Apply filter if specified
                if let Some(filter_str) = filter {
                    file_result.results.retain(|r| r.name.contains(filter_str));
                }
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
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("juglans=info,tower_http=info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();

    let application_cli = Cli::parse();

    if let Some(sub_command_enum) = &application_cli.command {
        match sub_command_enum {
            Commands::Init { name } => handle_init(name)?,
            Commands::Install => handle_install().await?,
            Commands::Push {
                paths,
                force,
                dry_run,
                r#type,
                recursive,
                endpoint,
            } => {
                handle_push(
                    paths.clone(),
                    *force,
                    *dry_run,
                    r#type.clone(),
                    *recursive,
                    endpoint.clone(),
                )
                .await?
            }
            Commands::Check { path, all, format } => {
                handle_check(path.as_deref(), *all, format)?;
            }
            Commands::Web { port, host } => {
                let current_dir = env::current_dir()?;
                let root = find_project_root(&current_dir)?;

                // 1. 尝试加载配置
                let config = JuglansConfig::load().ok();

                // 2. 决定 host (CLI > Config > Default)
                let final_host = host
                    .clone()
                    .or_else(|| config.as_ref().map(|c| c.server.host.clone()))
                    .unwrap_or_else(|| "127.0.0.1".to_string());

                // 3. 决定 port (CLI > Config > Default: 8080)
                let final_port = port
                    .or_else(|| config.as_ref().map(|c| c.server.port))
                    .unwrap_or(8080);

                web_server::start_web_server(final_host, final_port, root).await?;
            }
            Commands::Pull {
                slug,
                r#type,
                output,
            } => {
                handle_pull(slug, r#type, output.as_deref()).await?;
            }
            Commands::List { r#type } => {
                handle_list(r#type.as_deref()).await?;
            }
            Commands::Delete { slug, r#type } => {
                handle_delete(slug, r#type).await?;
            }
            Commands::Whoami {
                verbose,
                check_connection,
            } => {
                handle_whoami(*verbose, *check_connection).await?;
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
            Commands::Test {
                path,
                filter,
                format,
            } => {
                handle_test(path.as_deref(), filter.as_deref(), format).await?;
            }
        }
    } else if let Some(ref file_path) = application_cli.file {
        // Check if it's a @handle (remote agent)
        let file_str = file_path.to_string_lossy();
        if file_str.starts_with('@') {
            handle_remote_agent(&application_cli, &file_str).await?;
        } else {
            handle_file_logic(&application_cli).await?;
        }
    } else {
        println!("JWL Language Runtime (Multipurpose CLI)\nUse --help for command list.");
    }

    Ok(())
}
