// src/providers/openai.rs
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, ToolCallChunk};
use anyhow::Result;
use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestMessageContentPart, ChatCompletionRequestMessageContentPartImageArgs,
        ChatCompletionRequestMessageContentPartTextArgs, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestToolMessageArgs, ChatCompletionRequestUserMessageArgs,
        ChatCompletionRequestUserMessageContent, ChatCompletionTool,
        CreateChatCompletionRequestArgs, ImageUrlArgs,
    },
    Client,
};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::time::Duration;

pub struct OpenAIProvider {
    client: Client<OpenAIConfig>,
}

impl OpenAIProvider {
    pub fn new() -> Self {
        let raw_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let api_key = raw_key.trim().to_string();
        let raw_base = std::env::var("OPENAI_API_BASE")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        let api_base = raw_base.trim().trim_end_matches('/').to_string();
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
        // (保持 build_user_content 逻辑不变，为了节省篇幅简略显示)
        // ... 原来的逻辑 ...
        let mut content_parts = Vec::new();
        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            for part in parts {
                if let Some(text) = part.content {
                    content_parts.push(ChatCompletionRequestMessageContentPart::Text(
                        ChatCompletionRequestMessageContentPartTextArgs::default()
                            .text(text)
                            .build()
                            .unwrap(),
                    ));
                }
                // Image 等逻辑保持不变
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
impl LlmProvider for OpenAIProvider {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>, // 新增参数
        history: Vec<Message>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        let mut request_messages: Vec<ChatCompletionRequestMessage> = Vec::new();

        // 1. 注入 System Prompt (放到最前)
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

        // 2. 转换历史记录
        for msg in history {
            // 注意：因为数据库不再存 system 消息，这里主要是 user/assistant/tool
            match msg.role.as_str() {
                // 如果数据库里偶尔还有旧的 system 消息，可以选择忽略或兼容
                "system" => {
                    // 兼容旧数据：如果历史记录里有，也加上去
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

                    if let Some(tc_json) = &msg.tool_calls {
                        if let Ok(tc_vec) = serde_json::from_value::<
                            Vec<async_openai::types::ChatCompletionMessageToolCall>,
                        >(tc_json.clone())
                        {
                            if !tc_vec.is_empty() {
                                builder.tool_calls(tc_vec);
                                has_content = true; // 有工具调用也算有效消息
                            }
                        }
                    }

                    if has_content {
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

        // 3. 处理 Tools
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

        // 4. 构建请求
        let mut args = CreateChatCompletionRequestArgs::default();
        args.model(model).messages(request_messages).stream(true);
        if let Some(t) = request_tools {
            args.tools(t);
        }
        let request = args.build()?;

        let stream = self.client.chat().create_stream(request).await?;

        let mapped_stream = stream.map(|item| match item {
            Ok(resp) => {
                let choice = resp.choices.first();
                let content = choice.and_then(|c| c.delta.content.clone());
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
                Ok(ChatStreamChunk {
                    content,
                    tool_calls: tool_chunks,
                    usage: None,
                    finish_reason: None,
                })
            }
            Err(e) => Err(anyhow::anyhow!("ChatGPT Provider Error: {}", e)),
        });

        Ok(Box::pin(mapped_stream))
    }
}
