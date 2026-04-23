// src/providers/llm/juglans.rs
//
// The `juglans/` model provider routes LLM requests through the
// juglans-wallet platform proxy. Structurally identical to
// `deepseek.rs` — uses `async_openai::Client<OpenAIConfig>` with a
// different base URL (the proxy) and the agent's API key.
//
// Credentials come from the agent's juglans.toml `[ai.providers.juglans]`
// block (api_key + base_url), passed in as a `LlmProviderConfig`. The
// wallet proxy authenticates the agent via that key and forwards the
// request to the real upstream (DeepSeek etc.), billing/auditing per
// agent.

use super::factory::LlmProviderConfig;
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
use anyhow::Result;
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestToolMessageArgs,
        ChatCompletionRequestUserMessageArgs, ChatCompletionStreamOptions, ChatCompletionTool,
        ChatCompletionToolChoiceOption, CreateChatCompletionRequestArgs,
    },
    Client,
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::time::Duration;

pub struct JuglansProvider {
    client: Client<OpenAIConfig>,
}

impl JuglansProvider {
    pub fn new(cfg: &LlmProviderConfig) -> Self {
        let api_key = cfg.api_key.clone().unwrap_or_default();
        let api_base = cfg
            .base_url
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http://127.0.0.1:3002/v1/llm".to_string());
        let config = OpenAIConfig::new()
            .with_api_key(api_key)
            .with_api_base(api_base);
        let http_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default();
        Self {
            client: Client::with_config(config).with_http_client(http_client),
        }
    }

    fn build_flattened_content(&self, parts_json: &serde_json::Value) -> String {
        let mut buffer = String::new();
        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            for part in parts {
                if let Some(c) = part.content {
                    buffer.push_str(&c);
                    buffer.push('\n');
                }
            }
        }
        buffer
    }
}

#[async_trait]
impl LlmProvider for JuglansProvider {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>,
        history: Vec<Message>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        let mut request_messages: Vec<ChatCompletionRequestMessage> = Vec::new();
        let history_len = history.len();

        if let Some(sp) = system_prompt {
            if !sp.is_empty() {
                request_messages.push(
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content(sp)
                        .build()?
                        .into(),
                );
            }
        }

        for (i, msg) in history.iter().enumerate() {
            let content_str = self.build_flattened_content(&msg.parts);

            let api_msg: Option<ChatCompletionRequestMessage> = match msg.role.as_str() {
                "system" => Some(
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content(content_str)
                        .build()?
                        .into(),
                ),
                "user" => Some(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(content_str)
                        .build()?
                        .into(),
                ),
                "assistant" => {
                    let mut builder = ChatCompletionRequestAssistantMessageArgs::default();
                    let mut has_content = false;

                    if !content_str.is_empty() {
                        builder.content(content_str);
                        has_content = true;
                    }

                    let mut has_tools = false;
                    if let Some(tc_json) = &msg.tool_calls {
                        let is_next_tool = if i + 1 < history_len {
                            history[i + 1].role == "tool"
                        } else {
                            false
                        };
                        if is_next_tool {
                            if let Ok(tc_vec) = serde_json::from_value::<
                                Vec<async_openai::types::ChatCompletionMessageToolCall>,
                            >(tc_json.clone())
                            {
                                builder.tool_calls(tc_vec);
                                has_tools = true;
                            }
                        }
                    }

                    if has_content || has_tools {
                        Some(builder.build()?.into())
                    } else {
                        None
                    }
                }
                "tool" => {
                    let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
                    Some(
                        ChatCompletionRequestToolMessageArgs::default()
                            .content(content_str)
                            .tool_call_id(tool_call_id)
                            .build()?
                            .into(),
                    )
                }
                _ => None,
            };
            if let Some(m) = api_msg {
                request_messages.push(m);
            }
        }

        let mut request_tools: Option<Vec<ChatCompletionTool>> = None;
        if let Some(t) = tools {
            let mut converted_tools = Vec::new();
            for tool_json in t {
                if let Ok(tool) = serde_json::from_value::<ChatCompletionTool>(tool_json) {
                    converted_tools.push(tool);
                }
            }
            if !converted_tools.is_empty() {
                request_tools = Some(converted_tools);
            }
        }

        let mut args = CreateChatCompletionRequestArgs::default();
        args.model(model)
            .messages(request_messages)
            .stream(true)
            .stream_options(ChatCompletionStreamOptions {
                include_usage: true,
            });
        if let Some(ref t) = request_tools {
            tracing::debug!("[Juglans] Sending {} tools", t.len());
            args.tools(t.clone());
            args.tool_choice(ChatCompletionToolChoiceOption::Auto);
        }
        let request = args.build()?;

        tracing::debug!(
            "[Juglans] Request model: {}, messages: {}, has_tools: {}",
            model,
            request.messages.len(),
            request_tools.is_some()
        );

        let stream = self
            .client
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| {
                tracing::error!("[Juglans] API Error: {:?}", e);
                e
            })?;

        let mapped_stream = stream.map(|item| match item {
            Ok(resp) => {
                let choice = resp.choices.first();
                let content = choice.and_then(|c| c.delta.content.clone());
                let finish_reason = choice
                    .and_then(|c| c.finish_reason.clone())
                    .map(|r| format!("{:?}", r));

                let mut tool_chunks = Vec::new();
                if let Some(c) = choice {
                    if let Some(tool_calls) = &c.delta.tool_calls {
                        for tc in tool_calls {
                            tool_chunks.push(ToolCallChunk {
                                index: tc.index,
                                id: tc.id.clone(),
                                name: tc.function.as_ref().and_then(|f| f.name.clone()),
                                arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
                                signature: None,
                            });
                        }
                    }
                }

                let usage = resp.usage.map(|u| TokenUsage {
                    input_tokens: u.prompt_tokens as i64,
                    output_tokens: u.completion_tokens as i64,
                    total_tokens: u.total_tokens as i64,
                });

                Ok(ChatStreamChunk {
                    content,
                    tool_calls: tool_chunks,
                    usage,
                    finish_reason,
                })
            }
            Err(e) => Err(anyhow::anyhow!("Juglans Provider Error: {}", e)),
        });

        Ok(Box::pin(mapped_stream))
    }
}
