// src/providers/chatgpt.rs
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
use anyhow::Result;
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestMessageContentPart, ChatCompletionRequestMessageContentPartImageArgs,
        ChatCompletionRequestMessageContentPartTextArgs, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
        ChatCompletionRequestUserMessageContent, ChatCompletionStreamOptions, ChatCompletionTool,
        CreateChatCompletionRequestArgs, ImageUrlArgs,
    },
    Client,
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::time::Duration;

pub struct ChatGPTProvider {
    client: Client<OpenAIConfig>,
}

impl ChatGPTProvider {
    pub fn new() -> Self {
        let raw_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let api_key = raw_key.trim().to_string();
        let api_base = std::env::var("OPENAI_API_BASE")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

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

    fn build_user_content(
        &self,
        parts_json: &serde_json::Value,
    ) -> ChatCompletionRequestUserMessageContent {
        let mut content_parts = Vec::new();

        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            for part in parts {
                match part.part_type.as_str() {
                    "text" => {
                        if let Some(text) = part.content {
                            if !text.is_empty() {
                                content_parts.push(ChatCompletionRequestMessageContentPart::Text(
                                    ChatCompletionRequestMessageContentPartTextArgs::default()
                                        .text(text)
                                        .build()
                                        .unwrap(),
                                ));
                            }
                        }
                    }
                    "image" => {
                        if let Some(url_val) = part.data {
                            if let Some(url_str) = url_val.as_str() {
                                content_parts.push(
                                    ChatCompletionRequestMessageContentPart::ImageUrl(
                                        ChatCompletionRequestMessageContentPartImageArgs::default()
                                            .image_url(
                                                ImageUrlArgs::default()
                                                    .url(url_str)
                                                    .build()
                                                    .unwrap(),
                                            )
                                            .build()
                                            .unwrap(),
                                    ),
                                );
                            }
                        }
                    }
                    "kline" | "position" | "balance" | "news" | "order" | "symbolInfo" => {
                        if let Some(data) = part.data {
                            let label = match part.part_type.as_str() {
                                "kline" => "Market Chart Data",
                                "position" => "User Positions",
                                "balance" => "Wallet Balance",
                                "news" => "News Article",
                                _ => "Attached Data",
                            };
                            let text_block = format!(
                                "\n--- {} ---\n{}\n--- End {} ---\n",
                                label,
                                data.to_string(),
                                label
                            );
                            content_parts.push(ChatCompletionRequestMessageContentPart::Text(
                                ChatCompletionRequestMessageContentPartTextArgs::default()
                                    .text(text_block)
                                    .build()
                                    .unwrap(),
                            ));
                        }
                    }
                    "tool_result" => {
                        if let Some(text) = part.content {
                            content_parts.push(ChatCompletionRequestMessageContentPart::Text(
                                ChatCompletionRequestMessageContentPartTextArgs::default()
                                    .text(text)
                                    .build()
                                    .unwrap(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        if content_parts.is_empty() {
            ChatCompletionRequestUserMessageContent::Text("".to_string())
        } else {
            ChatCompletionRequestUserMessageContent::Array(content_parts)
        }
    }
}

#[async_trait]
impl LlmProvider for ChatGPTProvider {
    async fn stream_chat(
        &self,
        model: &str,
        // 【修复】添加 system_prompt 参数以匹配 trait
        system_prompt: Option<String>,
        history: Vec<Message>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        let mut request_messages: Vec<ChatCompletionRequestMessage> = Vec::new();
        let history_len = history.len();

        // 【修复】注入 System Prompt (如果存在)
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
            match msg.role.as_str() {
                // 如果数据库里偶尔还有 system 消息，可以选择保留或忽略，这里保留以兼容旧数据
                "system" => {
                    let text = if let Ok(parts) =
                        serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                    {
                        parts
                            .first()
                            .and_then(|p| p.content.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    request_messages.push(
                        ChatCompletionRequestSystemMessageArgs::default()
                            .content(text)
                            .build()?
                            .into(),
                    );
                }
                "user" => {
                    let content = self.build_user_content(&msg.parts);
                    request_messages.push(
                        ChatCompletionRequestUserMessageArgs::default()
                            .content(content)
                            .build()?
                            .into(),
                    );
                }
                "assistant" => {
                    let text = if let Ok(parts) =
                        serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                    {
                        parts
                            .first()
                            .and_then(|p| p.content.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    let mut builder = ChatCompletionRequestAssistantMessageArgs::default();
                    let mut has_content = false;
                    if !text.is_empty() {
                        builder.content(text);
                        has_content = true;
                    }

                    // 检测并丢弃无效消息
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
                        request_messages.push(builder.build()?.into());
                    }
                }
                "tool" => {
                    let tool_call_id = msg
                        .tool_call_id
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("Tool message missing ID"))?;
                    let text = if let Ok(parts) =
                        serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                    {
                        parts
                            .first()
                            .and_then(|p| p.content.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    request_messages.push(
                        ChatCompletionRequestToolMessageArgs::default()
                            .content(text)
                            .tool_call_id(tool_call_id)
                            .build()?
                            .into(),
                    );
                }
                _ => {}
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
        if let Some(t) = request_tools {
            args.tools(t);
        }
        let request = args.build()?;

        let stream = self.client.chat().create_stream(request).await?;

        let mapped_stream = stream.map(|item| {
            match item {
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
                                    arguments: tc
                                        .function
                                        .as_ref()
                                        .and_then(|f| f.arguments.clone()),
                                    signature: None,
                                });
                            }
                        }
                    }

                    // Extract usage from final chunk (when include_usage is enabled)
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
                Err(e) => Err(anyhow::anyhow!("ChatGPT Provider Error: {}", e)),
            }
        });

        Ok(Box::pin(mapped_stream))
    }
}
