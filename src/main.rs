// src/main.rs
#![cfg(not(target_arch = "wasm32"))]

mod builtins;
mod services;
mod templates;
mod core;

use std::path::{Path, PathBuf};
use std::fs;
use std::sync::Arc;
use std::env;
use std::io::{self, Write};
use std::collections::HashMap;
use anyhow::{Result, anyhow, Context};
use tracing::{info, error};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use services::prompt_loader::PromptRegistry;
use services::agent_loader::AgentRegistry;
use services::config::JuglansConfig;
use services::mcp::McpClient;
use services::jug0::Jug0Client;
use services::interface::JuglansRuntime;
use services::web_server; 
use core::parser::GraphParser;
use core::agent_parser::AgentParser;
use core::prompt_parser::PromptParser;
use core::executor::WorkflowExecutor;
use core::renderer::JwlRenderer;
use core::context::WorkflowContext;

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
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new project scaffold
    Init { name: String },
    /// Retrieve MCP tool schemas
    Install,
    /// Push resources to the server
    Apply { file: PathBuf },
    /// Start local web server for development 
    Web {
        // 【修改】改为 Option 以便检测是否传入了参数
        #[arg(short, long)]
        port: Option<u16>,
        // 【新增】支持 host 参数
        #[arg(long)]
        host: Option<String>,
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
    let source_file_path = cli.file.as_ref()
        .ok_or_else(|| anyhow!("Input missing: Please provide a valid file path."))?;
        
    let file_ext_name = source_file_path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    
    let absolute_target_path = fs::canonicalize(source_file_path)
        .with_context(|| format!("Physical file error: Cannot resolve {:?}", source_file_path))?;
        
    let project_root_path = find_project_root(&absolute_target_path)?;
    env::set_current_dir(&project_root_path)?;

    let file_parent_context = absolute_target_path.parent().unwrap_or(Path::new("."));
    let relative_base_offset = pathdiff::diff_paths(file_parent_context, &project_root_path)
        .unwrap_or(PathBuf::from("."));

    let source_raw_text = fs::read_to_string(&absolute_target_path)?;

    match file_ext_name {
        "jgflow" => {
            info!("🚀 Starting Workflow Graph Logic: {:?}", source_file_path);
            let local_config = JuglansConfig::load()?;
            let workflow_definition_obj = Arc::new(GraphParser::parse(&source_raw_text)?);
            
            let mut prompt_registry_inst = PromptRegistry::new();
            let mut agent_registry_inst = AgentRegistry::new();
            
            let resolved_p_patterns = resolve_import_patterns_verbose(&relative_base_offset, &workflow_definition_obj.prompt_patterns);
            let resolved_a_patterns = resolve_import_patterns_verbose(&relative_base_offset, &workflow_definition_obj.agent_patterns);
            
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
                runtime_impl
            ).await;
            
            executor_instance_obj.load_mcp_tools(&local_config).await;
            
            let shared_executor_engine = Arc::new(executor_instance_obj);
            shared_executor_engine.run(workflow_definition_obj, &local_config).await?;
        }
        
        "jgagent" => {
            let agent_meta_definition = AgentParser::parse(&source_raw_text)?;
            println!("🤖 Agent Active: {} ({})", agent_meta_definition.name, agent_meta_definition.slug);
            let global_system_config = JuglansConfig::load()?;
            
            let shared_runtime_ptr: Arc<dyn JuglansRuntime> = Arc::new(Jug0Client::new(&global_system_config));
            
            let mut local_p_store = PromptRegistry::new();
            let mut local_a_store = AgentRegistry::new();
            let mut active_workflow_ptr = None;

            if let Some(wf_path_string) = &agent_meta_definition.workflow {
                let wf_physical_path = relative_base_offset.join(wf_path_string);
                let wf_source_data_str = fs::read_to_string(&wf_physical_path)
                    .with_context(|| format!("Linked logic file missing: {:?}", wf_physical_path))?;
                    
                let workflow_parsed_data = GraphParser::parse(&wf_source_data_str)?;
                let wf_context_base_dir = wf_physical_path.parent().unwrap_or(Path::new("."));
                
                let p_import_list = resolve_import_patterns_verbose(wf_context_base_dir, &workflow_parsed_data.prompt_patterns);
                let a_import_list = resolve_import_patterns_verbose(wf_context_base_dir, &workflow_parsed_data.agent_patterns);
                
                local_p_store.load_from_paths(&p_import_list)?;
                local_a_store.load_from_paths(&a_import_list)?;
                
                active_workflow_ptr = Some(Arc::new(workflow_parsed_data));
            }

            let primary_executor_ptr = Arc::new(WorkflowExecutor::new(
                Arc::new(local_p_store), 
                Arc::new(local_a_store), 
                shared_runtime_ptr
            ).await);

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

                multi_turn_interaction_ctx.set("input.message".to_string(), json!(session_input_string))?;
                multi_turn_interaction_ctx.set("input.agent".to_string(), json!(agent_meta_definition))?;
                
                multi_turn_interaction_ctx.set("reply.output".to_string(), json!(""))?;
                multi_turn_interaction_ctx.set("reply.status".to_string(), json!("processing"))?;

                if let Some(target_flow_obj) = &active_workflow_ptr {
                    if let Err(logic_err) = primary_executor_ptr.clone().execute_graph(target_flow_obj.clone(), &multi_turn_interaction_ctx).await {
                        error!("Execution Engine Failure: {}", logic_err);
                    }
                    
                    let final_concatenated_answer = multi_turn_interaction_ctx.resolve_path("reply.output")?
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    
                    println!("\n--- [Agent Response Log] ---\n{}", final_concatenated_answer);
                } else {
                    let chat_result_raw = primary_executor_ptr.execute_tool_internal("chat", &HashMap::from([
                        ("agent".to_string(), agent_meta_definition.slug.clone()),
                        ("message".to_string(), session_input_string)
                    ]), &multi_turn_interaction_ctx).await?;
                    
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
            let final_text_output = renderer_instance.render(&prompt_resource_item.ast, &rendering_variables_ctx)?;
            println!("\n--- Rendered Content ---\n{}\n-----------------------", final_text_output);
        }
        
        _ => return Err(anyhow!("Unsupported JWL file type: .{}", file_ext_name)),
    }

    Ok(())
}

fn find_project_root(start_search_path: &Path) -> Result<PathBuf> {
    let mut current_ptr = start_search_path.to_path_buf();
    if current_ptr.is_file() { current_ptr.pop(); }
    loop {
        if current_ptr.join("juglans.toml").exists() { return Ok(current_ptr); }
        if !current_ptr.pop() { return Err(anyhow!("Fatal: Project root not found (missing juglans.toml).")); }
    }
}

fn handle_init(new_project_name: &str) -> Result<()> {
    let root_path_obj = Path::new(new_project_name);
    if root_path_obj.exists() { return Err(anyhow!("Directory exists: '{}'", new_project_name)); }
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

async fn handle_apply(file_to_apply: &Path) -> Result<()> {
    let local_config = JuglansConfig::load()?;
    let jug0_api_ptr = Jug0Client::new(&local_config);
    let raw_file_data = fs::read_to_string(file_to_apply)?;
    let ext_str = file_to_apply.extension().and_then(|s| s.to_str()).unwrap_or("");

    if ext_str == "jgagent" {
        println!("✅ {}", jug0_api_ptr.apply_agent(&AgentParser::parse(&raw_file_data)?).await?);
    } else if ext_str == "jgprompt" {
        println!("✅ {}", jug0_api_ptr.apply_prompt(&PromptParser::parse(&raw_file_data)?).await?);
    } else if ext_str == "jgflow" {
        let mut workflow = GraphParser::parse(&raw_file_data)?;

        // 如果没有 slug，从文件名生成
        if workflow.slug.is_empty() {
            workflow.slug = file_to_apply.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();
        }

        // 构建 workflow endpoint URL
        let endpoint_url = format!(
            "http://{}:{}/api/chat",
            local_config.server.host,
            local_config.server.port
        );

        println!("📦 Registering workflow '{}' with endpoint: {}", workflow.slug, endpoint_url);
        println!("✅ {}", jug0_api_ptr.apply_workflow(&workflow, &raw_file_data, &endpoint_url).await?);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("juglans=info,tower_http=info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    let application_cli = Cli::parse();

    if let Some(sub_command_enum) = &application_cli.command {
        match sub_command_enum {
            Commands::Init { name } => handle_init(name)?,
            Commands::Install => handle_install().await?,
            Commands::Apply { file } => handle_apply(file).await?,
            Commands::Web { port, host } => {
                let current_dir = env::current_dir()?;
                let root = find_project_root(&current_dir)?;
                
                // 1. 尝试加载配置
                let config = JuglansConfig::load().ok();
                
                // 2. 决定 host (CLI > Config > Default)
                let final_host = host.clone()
                    .or_else(|| config.as_ref().map(|c| c.server.host.clone()))
                    .unwrap_or_else(|| "127.0.0.1".to_string());

                // 3. 决定 port (CLI > Config > Default)
                let final_port = port.or_else(|| config.as_ref().map(|c| c.server.port)).unwrap_or(3000);

                web_server::start_web_server(final_host, final_port, root).await?;
            }
        }
    } else if application_cli.file.is_some() {
        handle_file_logic(&application_cli).await?;
    } else {
        println!("JWL Language Runtime (Multipurpose CLI)\nUse --help for command list.");
    }

    Ok(())
}