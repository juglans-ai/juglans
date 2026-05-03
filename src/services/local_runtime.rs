// src/services/local_runtime.rs
//
// LocalRuntime: the only juglans runtime. Calls LLM providers directly via
// the providers layer using API keys configured locally. juglans is local-first;
// there is no remote backend dependency.

use crate::providers::llm::{Message, ToolCallChunk};
use crate::providers::ProviderFactory;
use crate::services::config::AiConfig;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

// ─── Public types (moved from the former services::interface module) ────────

/// Chat output type, distinguishing final text from tool call requests.
#[derive(Debug)]
pub enum ChatOutput {
    /// Final reply text
    Final { text: String, chat_id: String },
    /// Tool call request initiated by AI
    ToolCalls {
        _calls: Vec<Value>,
        _chat_id: String,
    },
}

/// Tool execution callback — provided by the caller, invoked inline when
/// `LocalRuntime::chat()` receives a `tool_call` event.
#[async_trait]
pub trait ChatToolHandler: Send + Sync {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String>;
    /// Take pending dynamic tool definitions injected by the frontend via
    /// tool-result. Returns `None` if no tools were injected. Clears the
    /// pending state after taking.
    fn take_pending_tools(&self) -> Option<Vec<Value>> {
        None
    }
}

/// Chat request parameters for `LocalRuntime::chat()`.
pub struct ChatRequest {
    pub agent_config: Value,
    pub messages: Vec<Value>,
    pub tools: Option<Vec<Value>>,
    pub token_sender: Option<UnboundedSender<String>>,
    pub tool_handler: Option<Arc<dyn ChatToolHandler>>,
}

// ─── LocalRuntime ───────────────────────────────────────────────────────────

pub struct LocalRuntime {
    factory: ProviderFactory,
    default_model: String,
    /// Fallback `ChatToolHandler` consulted when a per-call `req.tool_handler`
    /// is `None`. Lets a Rust host (e.g. an embedded app) inject one handler
    /// once at runtime construction and have every `.jg` `chat(...)` call
    /// use it without each workflow having to declare `on_tool` / `tools={..}`.
    /// Per-call handlers (set inside `.jg`) still take precedence.
    default_tool_handler: Option<Arc<dyn ChatToolHandler>>,
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
            default_tool_handler: None,
        }
    }

    pub fn new_with_config(ai: &AiConfig) -> Self {
        use crate::providers::llm::factory::LlmProviderConfig;
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
            default_tool_handler: None,
        }
    }

    /// Builder: install a default `ChatToolHandler` that handles tool_call
    /// events whose `.jg` did not declare a `on_tool` / `tools={..}` mapping.
    /// Returns `self` so callers can write
    /// `LocalRuntime::new_with_config(&cfg).with_default_tool_handler(h)`.
    #[allow(dead_code)] // pub API for external Rust hosts (e.g. embedding apps); main.rs reincludes src/ via `mod`, so the bin build doesn't see a caller
    pub fn with_default_tool_handler(mut self, handler: Arc<dyn ChatToolHandler>) -> Self {
        self.default_tool_handler = Some(handler);
        self
    }

    /// Read accessor used by `Chat::execute` to fall back to the runtime's
    /// default handler when the per-call request has none.
    pub fn default_tool_handler(&self) -> Option<Arc<dyn ChatToolHandler>> {
        self.default_tool_handler.clone()
    }

    /// Core chat capability: streams a chat completion against the configured
    /// LLM provider, handling multi-round tool calls (up to 50 iterations).
    ///
    /// When `req.tool_handler` is `Some`, tool_call events are executed inline
    /// and the loop continues until the model returns final text. When `None`,
    /// the first tool_call breaks the loop and returns `ChatOutput::ToolCalls`
    /// for the caller to handle.
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatOutput> {
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

        // Tool call loop (max 50 iterations)
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

            // No tool_handler on the per-call request → consult the runtime's
            // default tool handler (set via `with_default_tool_handler`). If
            // both are absent, we surface the tool_calls to the caller as
            // before so non-Rust-host paths (CLI, tests) keep working.
            let fallback = self.default_tool_handler.clone();
            let handler: &Arc<dyn ChatToolHandler> = match (&req.tool_handler, &fallback) {
                (Some(h), _) => h,
                (None, Some(h)) => h,
                (None, None) => {
                    return Ok(ChatOutput::ToolCalls {
                        _calls: assembled_calls,
                        _chat_id: String::new(),
                    });
                }
            };

            // Execute tool calls via handler, build tool result messages.
            // First, add assistant message with tool_calls to history.
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
}

// ─── Tool call accumulator helpers ──────────────────────────────────────────

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
