// src/builtins/ai.rs
use super::Tool;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tracing::{debug, error, info, trace, warn};

use crate::core::context::WorkflowContext;
use crate::core::prompt_parser::PromptParser;
use crate::services::agent_loader::AgentRegistry;
use crate::services::interface::JuglansRuntime;
use crate::services::jug0::ChatOutput;
use crate::services::prompt_loader::PromptRegistry;

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
        runtime: Arc<dyn JuglansRuntime>,
    ) -> Self {
        Self {
            agent_registry,
            prompt_registry,
            runtime,
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }

    fn clean_json_output_verbose(&self, raw_content: &str) -> String {
        let trimmed_content = raw_content.trim();
        if trimmed_content.starts_with("```json") {
            if let Some(end_index) = trimmed_content.rfind("```") {
                if end_index > 7 {
                    return trimmed_content[7..end_index].trim().to_string();
                }
            }
        }
        if trimmed_content.starts_with("```") {
            if let Some(end_index) = trimmed_content.rfind("```") {
                if end_index > 3 {
                    return trimmed_content[3..end_index].trim().to_string();
                }
            }
        }
        trimmed_content.to_string()
    }

    /// 尝试在 BuiltinRegistry 中执行 tool，返回 None 表示未找到
    async fn try_execute_builtin(
        &self,
        tool_name: &str,
        args_str: &str,
        ctx: &WorkflowContext,
    ) -> Option<String> {
        let weak_registry = self.builtin_registry.as_ref()?;
        let registry_strong = weak_registry.upgrade()?;
        let tool_instance = registry_strong.get(tool_name)?;

        let args_map: HashMap<String, String> = match serde_json::from_str(args_str) {
            Ok(map) => map,
            Err(_) => HashMap::new(),
        };

        info!("  🔧 [Builtin Tool] Executing: {} ...", tool_name);

        let result = match tool_instance.execute(&args_map, ctx).await {
            Ok(Some(output_val)) => {
                let s = match output_val {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                info!("  ✅ [Builtin Tool] Result: {:.80}...", s.replace("\n", " "));
                s
            }
            Ok(None) => {
                info!("  ✅ [Builtin Tool] Finished (No Output)");
                "Tool executed successfully.".to_string()
            }
            Err(e) => {
                error!("  ❌ [Builtin Tool] Error: {}", e);
                format!("Error during tool execution: {}", e)
            }
        };
        Some(result)
    }

    /// 尝试通过 Executor → MCP 执行 tool，返回 None 表示未找到
    async fn try_execute_mcp(&self, tool_name: &str, args_str: &str) -> Option<String> {
        let weak_registry = self.builtin_registry.as_ref()?;
        let registry_strong = weak_registry.upgrade()?;
        let executor = registry_strong.get_executor()?;

        info!("  🔧 [MCP Tool] Attempting: {} ...", tool_name);
        let result = executor.execute_mcp_tool(tool_name, args_str).await?;
        info!("  ✅ [MCP Tool] Result: {:.80}...", result.replace("\n", " "));
        Some(result)
    }

    /// 兼容旧调用：依次尝试 builtin → MCP，都失败则返回错误信息
    async fn execute_local_tool(
        &self,
        tool_name: &str,
        args_str: &str,
        ctx: &WorkflowContext,
    ) -> String {
        if let Some(result) = self.try_execute_builtin(tool_name, args_str, ctx).await {
            return result;
        }
        if let Some(result) = self.try_execute_mcp(tool_name, args_str).await {
            return result;
        }
        format!(
            "Error: Tool '{}' is not registered (checked builtin and MCP).",
            tool_name
        )
    }
}

#[async_trait]
impl Tool for Chat {
    fn name(&self) -> &str {
        "chat"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let agent_slug_str = params.get("agent").map(|s| s.as_str()).unwrap_or("default");
        let user_message_body = params
            .get("message")
            .ok_or_else(|| anyhow!("Chat Tool Error: Mandatory parameter 'message' is missing."))?;

        // 消息状态：支持组合语法 input:output
        // 单值: state="silent" → input=silent, output=silent
        // 组合: state="context_hidden:context_visible" → input=hidden, output=visible
        let state_raw = params.get("state").cloned()
            .unwrap_or_else(|| "context_visible".to_string());
        let (input_state, output_state) = match state_raw.split_once(':') {
            Some((i, o)) => (i.to_string(), o.to_string()),
            None => (state_raw.clone(), state_raw.clone()),
        };
        // should_stream 基于 output_state（AI 回复是否对用户可见）
        let should_stream = output_state == "context_visible" || output_state == "display_only";
        // should_persist 基于 input_state（是否继承 chat_id）
        let should_persist = input_state == "context_visible" || input_state == "context_hidden";
        let system_prompt_manual_override = params.get("system_prompt").cloned();
        let requested_format_mode = params
            .get("format")
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "text".to_string());

        // 【修改】支持从 agent 获取默认 tools，并支持引用解析
        let tools_json_str = params.get("tools")
            .or_else(|| {
                // 如果 chat 没有指定 tools，尝试从 agent 获取默认 tools
                self.agent_registry.get(agent_slug_str)
                    .and_then(|agent| agent.tools.as_ref())
            });

        let custom_tools_json_schema = if let Some(schema_raw) = tools_json_str {
            // 解析 tools：支持内联 JSON、单个引用(@slug)、多个引用([slugs])
            let parsed: Vec<Value> = if schema_raw.starts_with('@') {
                // 单个引用：@web-tools
                let slug = &schema_raw[1..];
                debug!("Resolving tool reference: {}", slug);

                // 从 BuiltinRegistry 获取 ToolRegistry
                if let Some(builtin_reg_weak) = &self.builtin_registry {
                    if let Some(builtin_reg) = builtin_reg_weak.upgrade() {
                        if let Some(executor) = builtin_reg.get_executor() {
                            let tool_registry = executor.get_tool_registry();
                            if let Some(tool_resource) = tool_registry.get(slug) {
                                tool_resource.tools.clone()
                            } else {
                                return Err(anyhow!("Tool resource '{}' not found", slug));
                            }
                        } else {
                            return Err(anyhow!("Executor not available for tool resolution"));
                        }
                    } else {
                        return Err(anyhow!("BuiltinRegistry not available"));
                    }
                } else {
                    return Err(anyhow!("BuiltinRegistry not set for Chat builtin"));
                }
            } else if let Ok(slugs) = serde_json::from_str::<Vec<String>>(schema_raw) {
                // 多个引用：["devtools", "web-tools", "data-tools"]
                debug!("Resolving tool references: {:?}", slugs);

                if let Some(builtin_reg_weak) = &self.builtin_registry {
                    if let Some(builtin_reg) = builtin_reg_weak.upgrade() {
                        // 尝试通过 ToolRegistry 解析
                        let resolve_result = if let Some(executor) = builtin_reg.get_executor() {
                            let tool_registry = executor.get_tool_registry();
                            tool_registry.resolve_tools(&slugs).ok()
                        } else {
                            None
                        };

                        if let Some(tools) = resolve_result {
                            tools
                        } else {
                            // Fallback: 逐个解析 slug，支持 "devtools" 从 builtin schemas 获取
                            let mut all_tools = Vec::new();
                            let tool_registry_opt = builtin_reg.get_executor()
                                .map(|e| e.get_tool_registry().clone());

                            for slug in &slugs {
                                // 先尝试 ToolRegistry
                                if let Some(ref registry) = tool_registry_opt {
                                    if let Some(resource) = registry.get(slug) {
                                        all_tools.extend(resource.tools.clone());
                                        continue;
                                    }
                                }
                                // Fallback: "devtools" → builtin schemas
                                if slug == "devtools" {
                                    all_tools.extend(builtin_reg.list_schemas());
                                } else {
                                    return Err(anyhow!("Tool resource '{}' not found", slug));
                                }
                            }
                            all_tools
                        }
                    } else {
                        return Err(anyhow!("BuiltinRegistry not available"));
                    }
                } else {
                    return Err(anyhow!("BuiltinRegistry not set for Chat builtin"));
                }
            } else {
                // 内联 JSON：[{...}, {...}]
                serde_json::from_str(schema_raw).with_context(|| {
                    format!(
                        "Failed to parse 'tools' parameter as JSON array. Input was: {}",
                        schema_raw
                    )
                })?
            };

            if !parsed.is_empty() {
                info!("🛠️ Attaching {} custom tools to the request.", parsed.len());
                debug!("🛠️ Tools: {:?}", parsed);
                Some(parsed)
            } else {
                None
            }
        } else {
            None
        };

        info!("│   Message content: {}", user_message_body);

        let history_param = params.get("history").map(|s| s.as_str());

        let mut chat_messages_buffer = vec![json!({
            "type": "text",
            "role": "user",
            "content": user_message_body
        })];

        let mut active_session_id = if let Some(explicit_id) = params.get("chat_id") {
            if explicit_id.starts_with("[Missing:") || explicit_id.trim().is_empty() {
                debug!("Explicit chat_id parameter invalid or empty, treating as None.");
                None
            } else {
                debug!("Using explicit chat_id from parameters: {}", explicit_id);
                Some(explicit_id.clone())
            }
        } else if should_persist {
            if let Ok(Some(ctx_val)) = context.resolve_path("reply.chat_id") {
                if let Some(ctx_str) = ctx_val.as_str() {
                    debug!("Inheriting chat_id from context: {}", ctx_str);
                    Some(ctx_str.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            debug!("Non-persist state ({}): Starting fresh session.", input_state);
            None
        };

        let final_agent_config = if let Some(local_res) = self.agent_registry.get(agent_slug_str) {
            info!("│   Using local agent: {} (has_workflow: {})", agent_slug_str, local_res.workflow.is_some());

            // 【新增】检查 agent 是否有 workflow，如果有则执行嵌套 workflow
            if let Some(ref workflow_path) = local_res.workflow {
                if let Some(registry_weak) = &self.builtin_registry {
                    if let Some(registry) = registry_weak.upgrade() {
                        // 获取 agent 文件的基准目录
                        let agent_base_dir = if let Some((_, path)) = self.agent_registry.get_with_path(agent_slug_str) {
                            path.parent().unwrap_or(std::path::Path::new("."))
                        } else {
                            std::path::Path::new(".")
                        };

                        // 构建 identifier 用于递归检查
                        let identifier = format!("{}:{}", agent_slug_str, workflow_path);

                        // 获取超时配置（可选参数，默认无限制）
                        let timeout = params.get("workflow_timeout")
                            .and_then(|t| t.parse::<u64>().ok())
                            .map(std::time::Duration::from_secs);

                        if let Some(timeout_duration) = timeout {
                            info!("│   ⚡ Executing workflow: {} (timeout: {:?})", workflow_path, timeout_duration);
                        } else {
                            info!("│   ⚡ Executing workflow: {} (no timeout)", workflow_path);
                        }

                        // 【修复】保存原始 input.message，执行后恢复
                        let original_input_message = context.resolve_path("input.message").ok().flatten();

                        // 设置 input.message 到 context（workflow 需要）
                        context.set("input.message".to_string(), serde_json::json!(user_message_body))?;

                        // 执行嵌套 workflow（带超时控制）
                        let workflow_future = registry.execute_nested_workflow(
                            workflow_path,
                            agent_base_dir,
                            context,
                            identifier,
                        );

                        let execution_result = if let Some(timeout_duration) = timeout {
                            // 带超时执行
                            match tokio::time::timeout(timeout_duration, workflow_future).await {
                                Ok(result) => result,
                                Err(_) => {
                                    return Err(anyhow::anyhow!(
                                        "Workflow execution timeout after {:?}. Consider increasing workflow_timeout parameter.",
                                        timeout_duration
                                    ));
                                }
                            }
                        } else {
                            // 无超时限制
                            workflow_future.await
                        };

                        let result = match execution_result {
                            Ok(_) => {
                                // 从 context 获取 workflow 的输出
                                let output = context
                                    .resolve_path("reply.output")?
                                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                                    .unwrap_or_default();

                                if requested_format_mode == "json" {
                                    Ok(Some(
                                        serde_json::from_str::<Value>(&output).unwrap_or(json!(output)),
                                    ))
                                } else {
                                    Ok(Some(json!(output)))
                                }
                            }
                            Err(e) => {
                                Err(anyhow::anyhow!("Nested workflow execution failed: {}", e))
                            }
                        };

                        // 【修复】恢复原始 input.message
                        if let Some(original) = original_input_message {
                            context.set("input.message".to_string(), original)?;
                        }

                        return result;
                    }
                }
            }

            let mut resolved_sys_prompt = String::new();
            if let Some(override_val) = system_prompt_manual_override {
                resolved_sys_prompt = override_val;
            } else if let Some(slug_ref) = &local_res.system_prompt_slug {
                if let Some(template_content) = self.prompt_registry.get(slug_ref) {
                    match PromptParser::parse(template_content) {
                        Ok(parsed_resource) => {
                            resolved_sys_prompt = parsed_resource.content;
                        }
                        Err(_) => {
                            resolved_sys_prompt = template_content.clone();
                        }
                    }
                } else {
                    warn!("Warning: Linked prompt '{}' not found locally.", slug_ref);
                    resolved_sys_prompt = local_res.system_prompt.clone();
                }
            } else {
                resolved_sys_prompt = local_res.system_prompt.clone();
            }

            info!("│   System prompt: {}...", &resolved_sys_prompt.chars().take(100).collect::<String>());

            json!({
                "slug": local_res.slug,
                "model": local_res.model,
                "system_prompt": resolved_sys_prompt,
                "temperature": local_res.temperature,
            })
        } else {
            debug!("│   Using remote agent: {}", agent_slug_str);
            let mut base_config = json!({ "slug": agent_slug_str });
            if let Some(map) = base_config.as_object_mut() {
                if let Some(override_val) = system_prompt_manual_override {
                    map.insert("system_prompt".to_string(), json!(override_val));
                }
                if let Some(model) = params.get("model") {
                    map.insert("model".to_string(), json!(model));
                }
                if let Some(temp) = params.get("temperature") {
                    if let Ok(t) = temp.parse::<f64>() {
                        map.insert("temperature".to_string(), json!(t));
                    }
                }
            }
            base_config
        };

        let mut current_loop_session_id = active_session_id.clone();

        // 从 context 获取 Token 适配器（根据 state 决定是否 SSE 输出）
        let token_sender = context.get_token_sender_adapter();
        let effective_token_sender = if should_stream { token_sender } else { None };

        loop {
            let api_execution_result = self
                .runtime
                .chat(
                    final_agent_config.clone(),
                    chat_messages_buffer.clone(),
                    custom_tools_json_schema.clone(),
                    current_loop_session_id.as_deref(),
                    effective_token_sender.clone(),
                    Some(&state_raw),
                    history_param,
                )
                .await?;

            match api_execution_result {
                ChatOutput::Final { text, chat_id } => {
                    debug!("│   ✓ Response completed (session: {})", chat_id);

                    if should_persist {
                        context.set("reply.chat_id".to_string(), json!(chat_id))?;

                        let current_display_buffer = context
                            .resolve_path("reply.output")
                            .ok()
                            .flatten()
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                        let new_display_buffer = format!("{}{}", current_display_buffer, text);
                        context.set("reply.output".to_string(), json!(new_display_buffer))?;
                    }

                    if requested_format_mode == "json" {
                        let clean_json_str = self.clean_json_output_verbose(&text);
                        return Ok(Some(
                            serde_json::from_str::<Value>(&clean_json_str).unwrap_or(json!(text)),
                        ));
                    }
                    return Ok(Some(json!(text)));
                }

                ChatOutput::ToolCalls { calls, chat_id } => {
                    // 提取所有工具名称用于日志显示
                    let tool_names: Vec<&str> = calls.iter()
                        .map(|call| {
                            call["name"]
                                .as_str()
                                .or(call.pointer("/function/name").and_then(|v| v.as_str()))
                                .unwrap_or("unknown_tool")
                        })
                        .collect();

                    info!("│   🔧 Tool calls requested: {} - [{}]", calls.len(), tool_names.join(", "));
                    current_loop_session_id = Some(chat_id.clone());

                    chat_messages_buffer.clear();

                    // 收集无法本地执行的 client tools
                    let mut client_tools: Vec<Value> = Vec::new();

                    for call_request in &calls {
                        let call_id = call_request["id"].as_str().unwrap_or("unknown_id");

                        let tool_function_name = call_request["name"]
                            .as_str()
                            .or(call_request
                                .pointer("/function/name")
                                .and_then(|v| v.as_str()))
                            .unwrap_or("unknown_tool");

                        let arguments_json_str = call_request["arguments"]
                            .as_str()
                            .or(call_request
                                .pointer("/function/arguments")
                                .and_then(|v| v.as_str()))
                            .unwrap_or("{}");

                        // 1. 尝试 builtin tool
                        if let Some(result) = self.try_execute_builtin(tool_function_name, arguments_json_str, context).await {
                            chat_messages_buffer.push(json!({
                                "type": "tool_result",
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": result
                            }));
                            continue;
                        }

                        // 2. 尝试 MCP tool
                        if let Some(result) = self.try_execute_mcp(tool_function_name, arguments_json_str).await {
                            chat_messages_buffer.push(json!({
                                "type": "tool_result",
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": result
                            }));
                            continue;
                        }

                        // 3. 都没有 → client tool，收集起来
                        info!("│   ├─ [Client Tool Bridge] Queuing: {} for frontend execution", tool_function_name);
                        client_tools.push(call_request.clone());
                    }

                    // 如果有 client tools，通过 SSE 桥接发给前端并等待结果
                    if !client_tools.is_empty() {
                        // 去重：name + arguments 完全相同的调用只保留一个
                        let mut seen = std::collections::HashSet::new();
                        let deduped_tools: Vec<Value> = client_tools.into_iter().filter(|t| {
                            let key = format!(
                                "{}:{}",
                                t["name"].as_str().unwrap_or(""),
                                t["arguments"].as_str().unwrap_or("")
                            );
                            seen.insert(key)
                        }).collect();
                        let client_tools = deduped_tools;

                        let client_tool_names: Vec<&str> = client_tools.iter()
                            .filter_map(|c| c["name"].as_str())
                            .collect();
                        info!("│   🌉 [Client Tool Bridge] Waiting for frontend: [{}]", client_tool_names.join(", "));

                        let bridge_call_id = uuid::Uuid::new_v4().to_string();
                        match context.emit_tool_call_and_wait(
                            bridge_call_id,
                            client_tools,
                            120, // 120 秒超时
                        ).await {
                            Ok(results) => {
                                info!("│   ✅ [Client Tool Bridge] Received {} results from frontend", results.len());
                                for r in &results {
                                    info!("│   📦 [Client Tool Bridge] tool_call_id={}, content={}", r.tool_call_id, r.content);
                                    let parsed = serde_json::from_str::<Value>(&r.content);
                                    info!("│   📦 [Client Tool Bridge] parsed={:?}, executed_on_client={:?}",
                                        parsed.is_ok(),
                                        parsed.as_ref().ok().and_then(|v| v.get("executed_on_client"))
                                    );
                                }

                                // 检查是否所有结果都是 terminal（前端已渲染，无需继续 LLM loop）
                                let all_terminal = results.iter().all(|r| {
                                    serde_json::from_str::<Value>(&r.content)
                                        .ok()
                                        .and_then(|v| v.get("executed_on_client")?.as_bool())
                                        .unwrap_or(false)
                                });

                                info!("│   📦 [Client Tool Bridge] all_terminal={}", all_terminal);
                                if all_terminal {
                                    info!("│   🏁 [Client Tool Bridge] All client tools are terminal, ending loop");
                                    // Terminal tools: 前端已渲染（如交易卡片），无需再问 LLM
                                    return Ok(Some(json!("Client tools executed on frontend.")));
                                }

                                for result in results {
                                    chat_messages_buffer.push(json!({
                                        "type": "tool_result",
                                        "role": "tool",
                                        "tool_call_id": result.tool_call_id,
                                        "content": result.content
                                    }));
                                }
                            }
                            Err(e) => {
                                error!("│   ❌ [Client Tool Bridge] Error: {}", e);
                                // 为所有 client tools 生成错误结果，让 LLM 知道
                                for tool in &calls {
                                    let cid = tool["id"].as_str().unwrap_or("unknown");
                                    // 只为未处理的 client tools 添加错误
                                    if !chat_messages_buffer.iter().any(|m| m["tool_call_id"].as_str() == Some(cid)) {
                                        chat_messages_buffer.push(json!({
                                            "type": "tool_result",
                                            "role": "tool",
                                            "tool_call_id": cid,
                                            "content": format!("Error: {}", e)
                                        }));
                                    }
                                }
                            }
                        }
                    }

                    debug!("│   └─ Sending tool results back to LLM");
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
    fn name(&self) -> &str {
        "memory_search"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let query_text = params
            .get("query")
            .ok_or_else(|| anyhow!("MemorySearch: 'query' parameter is required."))?;

        let limit_val: u64 = params
            .get("limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(5);

        info!(
            "🧠 Executing Semantic Memory Search: '{}' (limit: {})",
            query_text, limit_val
        );

        let search_results = self.runtime.search_memories(query_text, limit_val).await?;

        Ok(Some(json!(search_results)))
    }
}

pub struct History {
    runtime: Arc<dyn JuglansRuntime>,
}

impl History {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for History {
    fn name(&self) -> &str {
        "history"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params
            .get("chat_id")
            .ok_or_else(|| anyhow!("history() requires 'chat_id' parameter"))?;

        let include_all = params
            .get("include_all")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        info!("📚 Fetching chat history for: {} (include_all: {})", chat_id, include_all);

        let messages = self.runtime
            .fetch_chat_history(chat_id, include_all)
            .await?;

        info!("📚 Retrieved {} messages", messages.len());

        Ok(Some(json!(messages)))
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

    fn render_template_verbose(
        &self,
        raw_body: &str,
        node_params: &HashMap<String, String>,
        flow_ctx: &WorkflowContext,
    ) -> String {
        TEMPLATE_VAR_RE
            .replace_all(raw_body, |caps: &regex::Captures| {
                let variable_name = &caps[1];
                if let Some(explicit_value) = node_params.get(variable_name) {
                    return explicit_value.clone();
                }
                match flow_ctx.resolve_path(variable_name) {
                    Ok(Some(ctx_value)) => ctx_value
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| ctx_value.to_string()),
                    _ => {
                        format!("{{{{{}}}}}", variable_name)
                    }
                }
            })
            .to_string()
    }
}

#[async_trait]
impl Tool for Prompt {
    fn name(&self) -> &str {
        "p"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let target_slug = params
            .get("slug")
            .or_else(|| params.get("file"))
            .ok_or_else(|| anyhow!("Prompt Tool: 'slug' parameter is required."))?;

        let template_raw_string = if let Some(local_content) = self.registry.get(target_slug) {
            local_content.clone()
        } else {
            self.runtime.fetch_prompt(target_slug).await?
        };

        let template_body_content = match PromptParser::parse(&template_raw_string) {
            Ok(parsed_resource) => parsed_resource.content,
            Err(_) => template_raw_string,
        };

        let finalized_output =
            self.render_template_verbose(&template_body_content, params, context);

        Ok(Some(Value::String(finalized_output)))
    }
}
