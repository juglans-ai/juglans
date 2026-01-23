// src/builtins/ai.rs
use super::Tool;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use anyhow::{Result, anyhow, Context};
use serde_json::{Value, json};
use async_trait::async_trait;
use regex::Regex;
use lazy_static::lazy_static;
use tracing::{info, warn, debug, error, trace};

use crate::services::prompt_loader::PromptRegistry;
use crate::services::agent_loader::AgentRegistry;
use crate::services::interface::JuglansRuntime;
use crate::services::jug0::ChatOutput;
use crate::core::context::WorkflowContext;
use crate::core::prompt_parser::PromptParser;

lazy_static! {
    static ref TEMPLATE_VAR_RE: Regex = Regex::new(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}").unwrap();
}

pub struct Chat {
    agent_registry: Arc<AgentRegistry>,
    prompt_registry: Arc<PromptRegistry>,
    runtime: Arc<dyn JuglansRuntime>,
    builtin_registry: Option<Weak<super::BuiltinRegistry>>, 
}

impl Chat {
    pub fn new(
        agent_registry: Arc<AgentRegistry>, 
        prompt_registry: Arc<PromptRegistry>, 
        runtime: Arc<dyn JuglansRuntime>
    ) -> Self { 
        Self { 
            agent_registry, 
            prompt_registry, 
            runtime, 
            builtin_registry: None 
        } 
    }

    pub fn set_registry(&mut self, registry: Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }

    fn clean_json_output_verbose(&self, raw_content: &str) -> String {
        let trimmed_content = raw_content.trim();
        if trimmed_content.starts_with("```json") {
            if let Some(end_index) = trimmed_content.rfind("```") {
                if end_index > 7 { return trimmed_content[7..end_index].trim().to_string(); }
            }
        }
        if trimmed_content.starts_with("```") {
            if let Some(end_index) = trimmed_content.rfind("```") {
                if end_index > 3 { return trimmed_content[3..end_index].trim().to_string(); }
            }
        }
        trimmed_content.to_string()
    }

    async fn execute_local_tool(&self, tool_name: &str, args_str: &str, ctx: &WorkflowContext) -> String {
        if let Some(weak_registry) = &self.builtin_registry {
            if let Some(registry_strong) = weak_registry.upgrade() {
                if let Some(tool_instance) = registry_strong.get(tool_name) {
                    let args_map: HashMap<String, String> = match serde_json::from_str(args_str) {
                        Ok(map) => map,
                        Err(_) => HashMap::new()
                    };
                    
                    println!("  🔧 [Local Tool] Executing: {} ...", tool_name);
                    
                    match tool_instance.execute(&args_map, ctx).await {
                        Ok(Some(output_val)) => {
                            let s = match output_val {
                                Value::String(s) => s,
                                other => other.to_string()
                            };
                            println!("  ✅ [Local Tool] Result: {:.80}...", s.replace("\n", " "));
                            s
                        },
                        Ok(None) => {
                            println!("  ✅ [Local Tool] Finished (No Output)");
                            "Tool executed successfully.".to_string()
                        },
                        Err(e) => {
                            println!("  ❌ [Local Tool] Error: {}", e);
                            format!("Error during tool execution: {}", e)
                        }
                    }
                } else {
                    format!("Error: Tool '{}' is not registered in the local environment.", tool_name)
                }
            } else {
                "Critical Error: Tool registry has been dropped from memory.".to_string()
            }
        } else {
            "Configuration Error: Chat tool was not properly initialized with a registry reference.".to_string()
        }
    }
}

#[async_trait]
impl Tool for Chat {
    fn name(&self) -> &str { "chat" }

    async fn execute(&self, params: &HashMap<String, String>, context: &WorkflowContext) -> Result<Option<Value>> {
        let agent_slug_str = params.get("agent").map(|s| s.as_str()).unwrap_or("default");
        let user_message_body = params.get("message")
            .ok_or_else(|| anyhow!("Chat Tool Error: Mandatory parameter 'message' is missing."))?;

        let is_stateless_mode = params.get("stateless").map(|s| s.to_lowercase() == "true").unwrap_or(false);
        let system_prompt_manual_override = params.get("system_prompt").cloned();
        let requested_format_mode = params.get("format").map(|s| s.to_lowercase()).unwrap_or_else(|| "text".to_string());
        
        let custom_tools_json_schema = if let Some(schema_raw) = params.get("tools") {
            let parsed: Vec<Value> = serde_json::from_str(schema_raw)
                .with_context(|| format!("Failed to parse 'tools' parameter as JSON array. Input was: {}", schema_raw))?;
            info!("🛠️ Attaching {} custom tools to the request.", parsed.len());
            Some(parsed)
        } else {
            None
        };

        let mut chat_messages_buffer = vec![
            json!({ 
                "type": "text", 
                "role": "user", 
                "content": user_message_body 
            })
        ];
        
        let mut active_session_id = if let Some(explicit_id) = params.get("chat_id") {
            if explicit_id.starts_with("[Missing:") || explicit_id.trim().is_empty() {
                debug!("Explicit chat_id parameter invalid or empty, treating as None.");
                None
            } else {
                debug!("Using explicit chat_id from parameters: {}", explicit_id);
                Some(explicit_id.clone())
            }
        } else if !is_stateless_mode {
            if let Ok(Some(ctx_val)) = context.resolve_path("reply.chat_id") {
                if let Some(ctx_str) = ctx_val.as_str() {
                    debug!("Inheriting chat_id from context: {}", ctx_str);
                    Some(ctx_str.to_string())
                } else { None }
            } else { None }
        } else {
            debug!("Stateless mode active: Starting fresh session.");
            None
        };
        
        let final_agent_config = if let Some(local_res) = self.agent_registry.get(agent_slug_str) {
            info!("🤖 Resolving Local Agent Definition: [{}]", agent_slug_str);
            let mut resolved_sys_prompt = String::new();
            if let Some(override_val) = system_prompt_manual_override {
                resolved_sys_prompt = override_val;
            } else if let Some(slug_ref) = &local_res.system_prompt_slug {
                if let Some(template_content) = self.prompt_registry.get(slug_ref) {
                    match PromptParser::parse(template_content) {
                        Ok(parsed_resource) => { resolved_sys_prompt = parsed_resource.content; },
                        Err(_) => { resolved_sys_prompt = template_content.clone(); }
                    }
                } else {
                    warn!("Warning: Linked prompt '{}' not found locally.", slug_ref);
                    resolved_sys_prompt = local_res.system_prompt.clone();
                }
            } else {
                resolved_sys_prompt = local_res.system_prompt.clone();
            }

            json!({
                "slug": local_res.slug,
                "model": local_res.model,
                "system_prompt": resolved_sys_prompt,
                "temperature": local_res.temperature,
            })
        } else {
            info!("🤖 Using Remote Agent Configuration: [{}]", agent_slug_str);
            let mut base_config = json!({ "slug": agent_slug_str });
            if let Some(override_val) = system_prompt_manual_override {
                if let Some(map) = base_config.as_object_mut() {
                    map.insert("system_prompt".to_string(), json!(override_val));
                }
            }
            base_config
        };
        
        let mut current_loop_session_id = active_session_id.clone();
        
        // 【新增】从 context 获取 Token 适配器
        let token_sender = context.get_token_sender_adapter();

        loop {
            let api_execution_result = self.runtime.chat(
                final_agent_config.clone(), 
                chat_messages_buffer.clone(), 
                custom_tools_json_schema.clone(), 
                current_loop_session_id.as_deref(),
                token_sender.clone() // 【修改】透传 Sender
            ).await?;
            
            match api_execution_result {
                ChatOutput::Final { text, chat_id } => {
                    info!("✅ AI Response Generation Completed. Session ID: {}", chat_id);
                    
                    if !is_stateless_mode {
                        context.set("reply.chat_id".to_string(), json!(chat_id))?;
                        
                        let current_display_buffer = context.resolve_path("reply.output")
                            .ok().flatten().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default();
                        let new_display_buffer = format!("{}{}", current_display_buffer, text);
                        context.set("reply.output".to_string(), json!(new_display_buffer))?;
                    }

                    if requested_format_mode == "json" {
                        let clean_json_str = self.clean_json_output_verbose(&text);
                        return Ok(Some(serde_json::from_str::<Value>(&clean_json_str).unwrap_or(json!(text))));
                    }
                    return Ok(Some(json!(text)));
                },
                
                ChatOutput::ToolCalls { calls, chat_id } => {
                    info!("🛠️ AI requested tool execution. Pending calls: {}", calls.len());
                    current_loop_session_id = Some(chat_id.clone());
                    
                    chat_messages_buffer.clear();

                    for call_request in calls {
                        let call_id = call_request["id"].as_str().unwrap_or("unknown_id");
                        
                        let tool_function_name = call_request["name"].as_str()
                            .or(call_request.pointer("/function/name").and_then(|v| v.as_str()))
                            .unwrap_or("unknown_tool");
                            
                        let arguments_json_str = call_request["arguments"].as_str()
                            .or(call_request.pointer("/function/arguments").and_then(|v| v.as_str()))
                            .unwrap_or("{}");
                        
                        info!("  -> Invoking Local Tool: [{}] Args: {}", tool_function_name, arguments_json_str);
                        
                        let execution_result_payload = self.execute_local_tool(tool_function_name, arguments_json_str, context).await;

                        chat_messages_buffer.push(json!({
                            "type": "tool_result",
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": execution_result_payload
                        }));
                    }
                    info!("🔄 Feedback Loop: Sending tool execution results back to AI...");
                }
            }
        }
    }
}

pub struct MemorySearch {
    runtime: Arc<dyn JuglansRuntime>,
}

impl MemorySearch {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for MemorySearch {
    fn name(&self) -> &str { "memory_search" }

    async fn execute(&self, params: &HashMap<String, String>, _context: &WorkflowContext) -> Result<Option<Value>> {
        let query_text = params.get("query")
            .ok_or_else(|| anyhow!("MemorySearch: 'query' parameter is required."))?;
            
        let limit_val: u64 = params.get("limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(5);

        info!("🧠 Executing Semantic Memory Search: '{}' (limit: {})", query_text, limit_val);
        
        let search_results = self.runtime.search_memories(query_text, limit_val).await?;

        Ok(Some(json!(search_results)))
    }
}

pub struct Prompt {
    registry: Arc<PromptRegistry>,
    runtime: Arc<dyn JuglansRuntime>,
}

impl Prompt {
    pub fn new(registry: Arc<PromptRegistry>, runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { registry, runtime }
    }

    fn render_template_verbose(&self, raw_body: &str, node_params: &HashMap<String, String>, flow_ctx: &WorkflowContext) -> String {
        TEMPLATE_VAR_RE.replace_all(raw_body, |caps: &regex::Captures| {
            let variable_name = &caps[1];
            if let Some(explicit_value) = node_params.get(variable_name) {
                return explicit_value.clone();
            }
            match flow_ctx.resolve_path(variable_name) {
                Ok(Some(ctx_value)) => {
                    ctx_value.as_str().map(|s| s.to_string()).unwrap_or_else(|| ctx_value.to_string())
                },
                _ => {
                    format!("{{{{{}}}}}", variable_name)
                }
            }
        }).to_string()
    }
}

#[async_trait]
impl Tool for Prompt {
    fn name(&self) -> &str { "p" }

    async fn execute(&self, params: &HashMap<String, String>, context: &WorkflowContext) -> Result<Option<Value>> {
        let target_slug = params.get("slug").or_else(|| params.get("file")).ok_or_else(|| anyhow!("Prompt Tool: 'slug' parameter is required."))?;

        let template_raw_string = if let Some(local_content) = self.registry.get(target_slug) {
            local_content.clone()
        } else {
            self.runtime.fetch_prompt(target_slug).await?
        };

        let template_body_content = match PromptParser::parse(&template_raw_string) {
            Ok(parsed_resource) => parsed_resource.content,
            Err(_) => template_raw_string
        };

        let finalized_output = self.render_template_verbose(&template_body_content, params, context);

        Ok(Some(Value::String(finalized_output)))
    }
}