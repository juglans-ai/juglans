// src/providers/qwen.rs
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
use anyhow::Result;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::pin::Pin;
use std::time::Duration;

pub struct QwenProvider {
    client: Client,
    api_key: String,
}

#[derive(Serialize)]
struct QwenChatRequest {
    model: String,
    input: QwenChatInput,
    parameters: QwenChatParameters,
}

#[derive(Serialize)]
struct QwenChatInput {
    messages: Vec<QwenMessage>,
}

#[derive(Serialize, Deserialize)]
struct QwenMessage {
    role: String,
    content: serde_json::Value, // String for text-only, Array for multimodal
}

#[derive(Serialize)]
struct QwenChatParameters {
    result_format: String, // 设为 "message"
    incremental_output: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct QwenResponseChunk {
    output: QwenOutput,
    usage: Option<QwenUsage>,
}

#[derive(Deserialize)]
struct QwenUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

#[derive(Deserialize)]
struct QwenOutput {
    choices: Vec<QwenChoice>,
}

#[derive(Deserialize)]
struct QwenChoice {
    message: QwenMessage,
    // finish_reason: String,
}

impl QwenProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default();
        Self {
            client,
            api_key: env::var("QWEN_API_KEY").unwrap_or_default(),
        }
    }

    /// Build DashScope-native content for a user message.
    /// Returns (content_value, has_image).
    /// - Text-only: returns (Value::String, false)
    /// - With images: returns (Value::Array([{image: url}, {text: ...}]), true)
    fn build_qwen_content(&self, parts_json: &serde_json::Value) -> (serde_json::Value, bool) {
        let mut has_image = false;
        let mut content_items: Vec<serde_json::Value> = Vec::new();

        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            for part in &parts {
                match part.part_type.as_str() {
                    "image" => {
                        if let Some(url_val) = &part.data {
                            if let Some(url_str) = url_val.as_str() {
                                has_image = true;
                                content_items.push(serde_json::json!({"image": url_str}));
                            }
                        }
                    }
                    "text" => {
                        if let Some(text) = &part.content {
                            if !text.is_empty() {
                                content_items.push(serde_json::json!({"text": text}));
                            }
                        }
                    }
                    "kline" | "position" | "balance" | "news" | "order" | "symbolInfo" => {
                        if let Some(data) = &part.data {
                            let label = match part.part_type.as_str() {
                                "kline" => "Market Chart Data",
                                "position" => "User Positions",
                                "balance" => "Wallet Balance",
                                "news" => "News Article",
                                _ => "Attached Data",
                            };
                            let text_block =
                                format!("\n--- {} ---\n{}\n--- End {} ---\n", label, data, label);
                            content_items.push(serde_json::json!({"text": text_block}));
                        }
                    }
                    _ => {
                        if let Some(text) = &part.content {
                            if !text.is_empty() {
                                content_items.push(serde_json::json!({"text": text}));
                            }
                        }
                    }
                }
            }
        }

        if has_image {
            (serde_json::Value::Array(content_items), true)
        } else {
            // Text-only: flatten to a single string
            let text = content_items
                .iter()
                .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            (serde_json::Value::String(text), false)
        }
    }

    fn extract_text(&self, parts_json: &serde_json::Value) -> String {
        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            parts
                .iter()
                .filter_map(|p| p.content.clone())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        }
    }
}

#[async_trait]
impl LlmProvider for QwenProvider {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>,
        history: Vec<Message>,
        _tools: Option<Vec<serde_json::Value>>, // 原生封装暂不处理复杂工具调用逻辑
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        let mut qwen_messages = Vec::new();
        let mut use_multimodal = false;

        if let Some(sp) = system_prompt {
            qwen_messages.push(QwenMessage {
                role: "system".to_string(),
                content: serde_json::Value::String(sp),
            });
        }

        for msg in history {
            if msg.role == "user" {
                let (content, has_image) = self.build_qwen_content(&msg.parts);
                if has_image {
                    use_multimodal = true;
                }
                qwen_messages.push(QwenMessage {
                    role: msg.role,
                    content,
                });
            } else {
                let text = self.extract_text(&msg.parts);
                qwen_messages.push(QwenMessage {
                    role: msg.role,
                    content: serde_json::Value::String(text),
                });
            }
        }

        let request = QwenChatRequest {
            model: model.to_string(),
            input: QwenChatInput {
                messages: qwen_messages,
            },
            parameters: QwenChatParameters {
                result_format: "message".to_string(),
                incremental_output: true,
                temperature: Some(0.7),
            },
        };

        let endpoint = if use_multimodal {
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
        } else {
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation"
        };

        let response = self
            .client
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("X-DashScope-SSE", "enable") // 关键：开启 SSE
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let txt = response.text().await?;
            return Err(anyhow::anyhow!("DashScope Chat Error: {}", txt));
        }

        let stream = response.bytes_stream().eventsource();
        let mapped = stream.map(|event_res| {
            let event = event_res.map_err(|e| anyhow::anyhow!("SSE Error: {}", e))?;

            // DashScope SSE 结束后通常不发送 [DONE]，直接关闭连接
            let chunk: QwenResponseChunk = serde_json::from_str(&event.data)?;
            let text = chunk
                .output
                .choices
                .first()
                .and_then(|c| match &c.message.content {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Array(arr) => {
                        // Multimodal response: extract text from content array
                        let texts: Vec<&str> = arr
                            .iter()
                            .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
                            .collect();
                        if texts.is_empty() {
                            None
                        } else {
                            Some(texts.join(""))
                        }
                    }
                    _ => None,
                });

            // Extract usage from response
            let usage = chunk.usage.map(|u| TokenUsage {
                input_tokens: u.input_tokens.unwrap_or(0),
                output_tokens: u.output_tokens.unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
            });

            Ok(ChatStreamChunk {
                content: text,
                tool_calls: vec![],
                usage,
                finish_reason: None,
            })
        });

        Ok(Box::pin(mapped))
    }
}
