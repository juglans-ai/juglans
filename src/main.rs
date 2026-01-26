// src/main.rs
#![cfg(not(target_arch = "wasm32"))]

mod builtins;
mod core;
mod services;
mod templates;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info};

use core::agent_parser::AgentParser;
use core::context::WorkflowContext;
use core::executor::WorkflowExecutor;
use core::parser::GraphParser;
use core::prompt_parser::PromptParser;
use core::renderer::JwlRenderer;
use core::validator::WorkflowValidator;
use services::agent_loader::AgentRegistry;
use services::config::JuglansConfig;
use services::interface::JuglansRuntime;
use services::jug0::Jug0Client;
use services::mcp::McpClient;
use services::prompt_loader::PromptRegistry;
use services::web_server;

#[derive(Parser)]
#[command(name = "juglans", author = "Juglans Team", version = "1.1")]
struct Cli {
    /// Target file path to process (.jgflow, .jgprompt, .jgagent)
    file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Direct input for prompt variables or agent messages
    #[arg(short, long)]
    input: Option<String>,

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
    Apply { file: PathBuf },
    /// Validate syntax of .jgflow, .jgagent, .jgprompt files (like cargo check)
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
}

fn resolve_import_patterns_verbose(base_dir_ref: &Path, raw_patterns: &[String]) -> Vec<String> {
    let mut resolved_output_list = Vec::new();
    for pattern_str in raw_patterns {
        if pattern_str.starts_with("/") {
            resolved_output_list.push(pattern_str.clone());
        } else {
            let combined_path_obj = base_dir_ref.join(pattern_str);
            resolved_output_list.push(combined_path_obj.to_string_lossy().to_string());
        }
    }
    resolved_output_list
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
    let relative_base_offset =
        pathdiff::diff_paths(file_parent_context, &project_root_path).unwrap_or(PathBuf::from("."));

    let source_raw_text = fs::read_to_string(&absolute_target_path)?;

    match file_ext_name {
        "jgflow" => {
            info!("🚀 Starting Workflow Graph Logic: {:?}", source_file_path);
            let local_config = JuglansConfig::load()?;
            let workflow_definition_obj = Arc::new(GraphParser::parse(&source_raw_text)?);

            let mut prompt_registry_inst = PromptRegistry::new();
            let mut agent_registry_inst = AgentRegistry::new();

            let resolved_p_patterns = resolve_import_patterns_verbose(
                &relative_base_offset,
                &workflow_definition_obj.prompt_patterns,
            );
            let resolved_a_patterns = resolve_import_patterns_verbose(
                &relative_base_offset,
                &workflow_definition_obj.agent_patterns,
            );

            if !resolved_p_patterns.is_empty() {
                prompt_registry_inst.load_from_paths(&resolved_p_patterns)?;
            }
            if !resolved_a_patterns.is_empty() {
                agent_registry_inst.load_from_paths(&resolved_a_patterns)?;
            }

            let runtime_impl: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(&local_config));

            let mut executor_instance_obj = WorkflowExecutor::new(
                Arc::new(prompt_registry_inst),
                Arc::new(agent_registry_inst),
                runtime_impl,
            )
            .await;

            executor_instance_obj.load_mcp_tools(&local_config).await;

            let shared_executor_engine = Arc::new(executor_instance_obj);
            shared_executor_engine
                .run(workflow_definition_obj, &local_config)
                .await?;
        }

        "jgagent" => {
            let agent_meta_definition = AgentParser::parse(&source_raw_text)?;
            println!(
                "🤖 Agent Active: {} ({})",
                agent_meta_definition.name, agent_meta_definition.slug
            );
            let global_system_config = JuglansConfig::load()?;

            let shared_runtime_ptr: Arc<dyn JuglansRuntime> =
                Arc::new(Jug0Client::new(&global_system_config));

            let mut local_p_store = PromptRegistry::new();
            let mut local_a_store = AgentRegistry::new();
            let mut active_workflow_ptr = None;

            if let Some(wf_path_string) = &agent_meta_definition.workflow {
                let wf_physical_path = relative_base_offset.join(wf_path_string);
                let wf_source_data_str =
                    fs::read_to_string(&wf_physical_path).with_context(|| {
                        format!("Linked logic file missing: {:?}", wf_physical_path)
                    })?;

                let workflow_parsed_data = GraphParser::parse(&wf_source_data_str)?;
                let wf_context_base_dir = wf_physical_path.parent().unwrap_or(Path::new("."));

                let p_import_list = resolve_import_patterns_verbose(
                    wf_context_base_dir,
                    &workflow_parsed_data.prompt_patterns,
                );
                let a_import_list = resolve_import_patterns_verbose(
                    wf_context_base_dir,
                    &workflow_parsed_data.agent_patterns,
                );

                local_p_store.load_from_paths(&p_import_list)?;
                local_a_store.load_from_paths(&a_import_list)?;

                active_workflow_ptr = Some(Arc::new(workflow_parsed_data));
            }

            let primary_executor_ptr = Arc::new(
                WorkflowExecutor::new(
                    Arc::new(local_p_store),
                    Arc::new(local_a_store),
                    shared_runtime_ptr,
                )
                .await,
            );

            let multi_turn_interaction_ctx = WorkflowContext::new();

            loop {
                let session_input_string = if let Some(cmd_input) = &cli.input {
                    cmd_input.clone()
                } else {
                    print!("\nUser > ");
                    io::stdout().flush()?;
                    let mut input_buffer_str = String::new();
                    io::stdin().read_line(&mut input_buffer_str)?;
                    let sanitized_input = input_buffer_str.trim().to_string();

                    if sanitized_input == "exit" || sanitized_input == "quit" {
                        println!("Session terminated. Finalizing...");
                        break;
                    }
                    sanitized_input
                };

                if session_input_string.is_empty() {
                    continue;
                }

                multi_turn_interaction_ctx
                    .set("input.message".to_string(), json!(session_input_string))?;
                multi_turn_interaction_ctx
                    .set("input.agent".to_string(), json!(agent_meta_definition))?;

                multi_turn_interaction_ctx.set("reply.output".to_string(), json!(""))?;
                multi_turn_interaction_ctx.set("reply.status".to_string(), json!("processing"))?;

                if let Some(target_flow_obj) = &active_workflow_ptr {
                    if let Err(logic_err) = primary_executor_ptr
                        .clone()
                        .execute_graph(target_flow_obj.clone(), &multi_turn_interaction_ctx)
                        .await
                    {
                        error!("Execution Engine Failure: {}", logic_err);
                    }

                    let final_concatenated_answer = multi_turn_interaction_ctx
                        .resolve_path("reply.output")?
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();

                    println!(
                        "\n--- [Agent Response Log] ---\n{}",
                        final_concatenated_answer
                    );
                } else {
                    let chat_result_raw = primary_executor_ptr
                        .execute_tool_internal(
                            "chat",
                            &HashMap::from([
                                ("agent".to_string(), agent_meta_definition.slug.clone()),
                                ("message".to_string(), session_input_string),
                            ]),
                            &multi_turn_interaction_ctx,
                        )
                        .await?;

                    if let Some(Value::Object(map)) = chat_result_raw {
                        if let Some(txt_content) = map.get("response").and_then(|v| v.as_str()) {
                            println!("\nAssistant > {}", txt_content);
                        }
                    }
                }

                if cli.input.is_some() {
                    break;
                }
            }
        }

        "jgprompt" => {
            println!("🔍 Executing Local Render: {:?}", source_file_path);
            let prompt_resource_item = PromptParser::parse(&source_raw_text)?;
            let mut rendering_variables_ctx = prompt_resource_item.inputs.clone();

            if let Some(ext_input_json) = &cli.input {
                let parsed_input_data: Value = serde_json::from_str(ext_input_json)?;
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
    loop {
        if current_ptr.join("juglans.toml").exists() {
            return Ok(current_ptr);
        }
        if !current_ptr.pop() {
            return Err(anyhow!(
                "Fatal: Project root not found (missing juglans.toml)."
            ));
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
    println!("✅ Initialized: {:?}", root_path_obj);
    Ok(())
}

async fn handle_install() -> Result<()> {
    let runtime_config = JuglansConfig::load()?;
    let schema_client = McpClient::new();
    for server_item in runtime_config.mcp_servers {
        info!("🔄 Schemas for [{}]...", server_item.name);
        let _ = schema_client.fetch_tools(&server_item).await;
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
        println!("    \x1b[33mNo .jgflow, .jgagent, or .jgprompt files found\x1b[0m");
        return Ok(());
    }

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
                "jgflow" => {
                    workflow_count += 1;
                    match GraphParser::parse(&content) {
                        Ok(graph) => {
                            let validation = WorkflowValidator::validate(&graph);
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
                            error_count += 1;
                            let err_msg = e
                                .to_string()
                                .lines()
                                .next()
                                .unwrap_or("Parse error")
                                .to_string();
                            println!(
                                "    \x1b[1;31merror\x1b[0m[workflow]: {} (parse failed)",
                                relative_path
                            );
                            println!("      \x1b[31m-->\x1b[0m {}", err_msg);

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "workflow",
                                    "slug": file_name,
                                    "valid": false,
                                    "errors": [{"code": "PARSE", "message": err_msg}],
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
                            let err_msg = e
                                .to_string()
                                .lines()
                                .next()
                                .unwrap_or("Parse error")
                                .to_string();
                            println!(
                                "    \x1b[1;31merror\x1b[0m[agent]: {} (parse failed)",
                                relative_path
                            );
                            println!("      \x1b[31m-->\x1b[0m {}", err_msg);

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "agent",
                                    "slug": file_name,
                                    "valid": false,
                                    "errors": [{"code": "PARSE", "message": err_msg}],
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
                            let err_msg = e
                                .to_string()
                                .lines()
                                .next()
                                .unwrap_or("Parse error")
                                .to_string();
                            println!(
                                "    \x1b[1;31merror\x1b[0m[prompt]: {} (parse failed)",
                                relative_path
                            );
                            println!("      \x1b[31m-->\x1b[0m {}", err_msg);

                            if output_format == "json" {
                                results.push(serde_json::json!({
                                    "file": relative_path,
                                    "type": "prompt",
                                    "slug": file_name,
                                    "valid": false,
                                    "errors": [{"code": "PARSE", "message": err_msg}],
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

async fn handle_apply(file_to_apply: &Path) -> Result<()> {
    let local_config = JuglansConfig::load()?;
    let jug0_api_ptr = Jug0Client::new(&local_config);
    let raw_file_data = fs::read_to_string(file_to_apply)?;
    let ext_str = file_to_apply
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    if ext_str == "jgagent" {
        println!(
            "✅ {}",
            jug0_api_ptr
                .apply_agent(&AgentParser::parse(&raw_file_data)?)
                .await?
        );
    } else if ext_str == "jgprompt" {
        println!(
            "✅ {}",
            jug0_api_ptr
                .apply_prompt(&PromptParser::parse(&raw_file_data)?)
                .await?
        );
    } else if ext_str == "jgflow" {
        let mut workflow = GraphParser::parse(&raw_file_data)?;

        // 如果没有 slug，从文件名生成
        if workflow.slug.is_empty() {
            workflow.slug = file_to_apply
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();
        }

        // 构建 workflow endpoint URL
        let endpoint_url = format!(
            "http://{}:{}/api/chat",
            local_config.server.host, local_config.server.port
        );

        println!(
            "📦 Registering workflow '{}' with endpoint: {}",
            workflow.slug, endpoint_url
        );
        println!(
            "✅ {}",
            jug0_api_ptr
                .apply_workflow(&workflow, &raw_file_data, &endpoint_url)
                .await?
        );
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
            Commands::Apply { file } => handle_apply(file).await?,
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
        }
    } else if application_cli.file.is_some() {
        handle_file_logic(&application_cli).await?;
    } else {
        println!("JWL Language Runtime (Multipurpose CLI)\nUse --help for command list.");
    }

    Ok(())
}
