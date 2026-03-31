// src/services/local_runtime.rs
//
// LocalRuntime: calls LLM providers directly via jug0's provider layer.
// No jug0 server needed — uses API keys configured locally.

use crate::services::config::AiConfig;
use crate::services::interface::{ChatRequest, JuglansRuntime};
use crate::services::jug0::ChatOutput;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use jug0::providers::llm::{Message, ToolCallChunk};
use jug0::providers::ProviderFactory;
use serde_json::{json, Value};

pub struct LocalRuntime {
    factory: ProviderFactory,
    default_model: String,
}

impl Default for LocalRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalRuntime {
    pub fn new() -> Self {
        Self {
            factory: ProviderFactory::new(),
            default_model: "gpt-4o-mini".to_string(),
        }
    }

    pub fn new_with_config(ai: &AiConfig) -> Self {
        use jug0::providers::llm::factory::LlmProviderConfig;
        let configs: std::collections::HashMap<String, LlmProviderConfig> = ai
            .providers
            .iter()
            .map(|(name, cfg)| {
                (
                    name.clone(),
                    LlmProviderConfig {
                        api_key: cfg.api_key.clone(),
                        base_url: cfg.base_url.clone(),
                    },
                )
            })
            .collect();
        Self {
            factory: ProviderFactory::new_with_config(&configs),
            default_model: ai
                .default_model
                .clone()
                .unwrap_or_else(|| "gpt-4o-mini".to_string()),
        }
    }
}

/// Accumulate tool call chunks into complete tool calls
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

fn accumulate_tool_chunks(accumulators: &mut Vec<ToolCallAccumulator>, chunks: &[ToolCallChunk]) {
    for chunk in chunks {
        let idx = chunk.index as usize;
        // Grow the accumulator list if needed
        while accumulators.len() <= idx {
            accumulators.push(ToolCallAccumulator {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
        }
        if let Some(id) = &chunk.id {
            accumulators[idx].id = id.clone();
        }
        if let Some(name) = &chunk.name {
            accumulators[idx].name = name.clone();
        }
        if let Some(args) = &chunk.arguments {
            accumulators[idx].arguments.push_str(args);
        }
    }
}

fn accumulators_to_tool_calls(accs: &[ToolCallAccumulator]) -> Vec<Value> {
    accs.iter()
        .filter(|a| !a.name.is_empty())
        .map(|a| {
            json!({
                "id": a.id,
                "type": "function",
                "function": {
                    "name": a.name,
                    "arguments": a.arguments,
                }
            })
        })
        .collect()
}

#[async_trait]
impl JuglansRuntime for LocalRuntime {
    async fn chat(&self, req: ChatRequest) -> Result<ChatOutput> {
        // Extract model from agent_config
        let model = req
            .agent_config
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model)
            .to_string();

        // Extract system_prompt
        let system_prompt = req
            .agent_config
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Convert messages (Vec<Value>) → Vec<Message>
        // Handles both formats: {"role","parts":[...]} and {"role","content":"..."}
        let mut history: Vec<Message> = req
            .messages
            .iter()
            .filter_map(|v| {
                serde_json::from_value(v.clone()).ok().or_else(|| {
                    let role = v.get("role")?.as_str()?;
                    let content = v.get("content")?.as_str()?;
                    Some(Message {
                        role: role.to_string(),
                        parts: json!([{"type": "text", "content": content}]),
                        tool_calls: None,
                        tool_call_id: None,
                    })
                })
            })
            .collect();

        let mut tools = req.tools.clone();

        // Tool call loop (max 50 iterations, same as jug0)
        for _ in 0..50 {
            let (provider, actual_model) = self.factory.get_provider(&model);

            let mut stream = provider
                .stream_chat(
                    &actual_model,
                    system_prompt.clone(),
                    history.clone(),
                    tools.clone(),
                )
                .await?;

            let mut text_acc = String::new();
            let mut tool_accs: Vec<ToolCallAccumulator> = Vec::new();
            let mut has_tool_finish = false;

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;

                // Stream text tokens to caller
                if let Some(content) = &chunk.content {
                    text_acc.push_str(content);
                    if let Some(ref sender) = req.token_sender {
                        let _ = sender.send(content.clone());
                    }
                }

                // Accumulate tool call chunks
                if !chunk.tool_calls.is_empty() {
                    accumulate_tool_chunks(&mut tool_accs, &chunk.tool_calls);
                }

                // Check finish reason
                if let Some(ref reason) = chunk.finish_reason {
                    let r = reason.to_lowercase();
                    if (r.contains("tool") || r.contains("end_turn")) && !tool_accs.is_empty() {
                        has_tool_finish = true;
                    }
                }
            }

            // No tool calls → return final text
            if !has_tool_finish || tool_accs.is_empty() {
                return Ok(ChatOutput::Final {
                    text: text_acc,
                    chat_id: String::new(),
                });
            }

            let assembled_calls = accumulators_to_tool_calls(&tool_accs);

            // No tool_handler → return tool calls to caller
            let handler = match &req.tool_handler {
                Some(h) => h,
                None => {
                    return Ok(ChatOutput::ToolCalls {
                        _calls: assembled_calls,
                        _chat_id: String::new(),
                    });
                }
            };

            // Execute tool calls via handler, build tool result messages
            // First, add assistant message with tool_calls to history
            history.push(Message {
                role: "assistant".to_string(),
                parts: if text_acc.is_empty() {
                    json!([])
                } else {
                    json!([{"type": "text", "content": text_acc}])
                },
                tool_calls: Some(json!(assembled_calls)),
                tool_call_id: None,
            });

            for call in &assembled_calls {
                let tool_name = call
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let arguments = call
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let call_id = call.get("id").and_then(|v| v.as_str()).unwrap_or("");

                let result = handler.handle_tool_call(tool_name, arguments).await?;

                history.push(Message {
                    role: "tool".to_string(),
                    parts: json!([{"type": "text", "content": result}]),
                    tool_calls: None,
                    tool_call_id: Some(call_id.to_string()),
                });
            }

            // Check for dynamic tools from handler
            if let Some(new_tools) = handler.take_pending_tools() {
                tools = Some(new_tools);
            }

            // Loop: re-invoke LLM with updated history
        }

        Err(anyhow::anyhow!(
            "LocalRuntime: exceeded maximum tool call iterations (50)"
        ))
    }

    async fn fetch_prompt(&self, _slug: &str) -> Result<String> {
        Err(anyhow::anyhow!(
            "fetch_prompt not supported in local runtime (use file-based prompts)"
        ))
    }

    async fn search_memories(&self, _query: &str, _limit: u64) -> Result<Vec<Value>> {
        Err(anyhow::anyhow!(
            "search_memories not supported in local runtime"
        ))
    }

    async fn fetch_chat_history(&self, _chat_id: &str, _include_all: bool) -> Result<Vec<Value>> {
        Err(anyhow::anyhow!(
            "fetch_chat_history not supported in local runtime"
        ))
    }

    async fn create_message(
        &self,
        _chat_id: &str,
        _role: &str,
        _content: &str,
        _state: &str,
    ) -> Result<()> {
        // No-op in local runtime (no persistence)
        Ok(())
    }

    async fn update_message_state(
        &self,
        _chat_id: &str,
        _message_id: i32,
        _state: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn vector_create_space(
        &self,
        _space: &str,
        _model: Option<&str>,
        _public: bool,
    ) -> Result<Value> {
        Err(anyhow::anyhow!(
            "vector operations not supported in local runtime"
        ))
    }

    async fn vector_upsert(
        &self,
        _space: &str,
        _points: Vec<Value>,
        _model: Option<&str>,
    ) -> Result<Value> {
        Err(anyhow::anyhow!(
            "vector operations not supported in local runtime"
        ))
    }

    async fn vector_search(
        &self,
        _space: &str,
        _query: &str,
        _limit: u64,
        _model: Option<&str>,
    ) -> Result<Vec<Value>> {
        Err(anyhow::anyhow!(
            "vector operations not supported in local runtime"
        ))
    }

    async fn vector_list_spaces(&self) -> Result<Vec<Value>> {
        Err(anyhow::anyhow!(
            "vector operations not supported in local runtime"
        ))
    }

    async fn vector_delete_space(&self, _space: &str) -> Result<Value> {
        Err(anyhow::anyhow!(
            "vector operations not supported in local runtime"
        ))
    }

    async fn vector_delete(&self, _space: &str, _ids: Vec<String>) -> Result<Value> {
        Err(anyhow::anyhow!(
            "vector operations not supported in local runtime"
        ))
    }
}
