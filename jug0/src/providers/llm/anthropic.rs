// src/providers/llm/anthropic.rs
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
use anyhow::Result;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::pin::Pin;
use std::time::Duration;

// --- Request DTOs ---

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    stream: bool,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Serialize, Clone)]
struct AnthropicMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Serialize, Clone)]
struct ImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Serialize)]
struct AnthropicTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: Value,
}

// --- Response DTOs (SSE events) ---

#[derive(Deserialize, Debug)]
struct MessageStartBody {
    message: Option<MessageMeta>,
}

#[derive(Deserialize, Debug)]
struct MessageMeta {
    usage: Option<UsageMeta>,
}

#[derive(Deserialize, Debug)]
struct UsageMeta {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

#[derive(Deserialize, Debug)]
struct ContentBlockStartBody {
    index: u32,
    content_block: Option<ContentBlockMeta>,
}

#[derive(Deserialize, Debug)]
struct ContentBlockMeta {
    #[serde(rename = "type")]
    block_type: String,
    // tool_use fields
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ContentBlockDeltaBody {
    index: u32,
    delta: Option<DeltaMeta>,
}

#[derive(Deserialize, Debug)]
struct DeltaMeta {
    #[serde(rename = "type")]
    delta_type: String,
    // text_delta
    text: Option<String>,
    // input_json_delta
    partial_json: Option<String>,
}

#[derive(Deserialize, Debug)]
struct MessageDeltaBody {
    delta: Option<MessageDeltaMeta>,
    usage: Option<UsageMeta>,
}

#[derive(Deserialize, Debug)]
struct MessageDeltaMeta {
    stop_reason: Option<String>,
}

// --- Provider ---

pub struct AnthropicProvider {
    client: Client,
    base_url: String,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url,
            api_key,
        }
    }

    fn build_content_blocks(&self, parts_json: &Value) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();
        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            for part in parts {
                match part.part_type.as_str() {
                    "text" | "tool_result" => {
                        if let Some(t) = part.content {
                            if !t.is_empty() {
                                blocks.push(ContentBlock::Text { text: t });
                            }
                        }
                    }
                    "image" => {
                        if let Some(data_val) = part.data {
                            if let Some(data_str) = data_val.as_str() {
                                if data_str.starts_with("data:") {
                                    let split: Vec<&str> = data_str.split(',').collect();
                                    if split.len() == 2 {
                                        let meta = split[0];
                                        let mime = meta
                                            .split(';')
                                            .next()
                                            .unwrap_or("")
                                            .replace("data:", "");
                                        let b64 = split[1];
                                        blocks.push(ContentBlock::Image {
                                            source: ImageSource {
                                                source_type: "base64".to_string(),
                                                media_type: mime,
                                                data: b64.to_string(),
                                            },
                                        });
                                    }
                                } else {
                                    blocks.push(ContentBlock::Text {
                                        text: format!("[Image URL: {}]", data_str),
                                    });
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(data) = part.data {
                            let label = part.part_type;
                            let text = format!("--- {} ---\n{}\n", label, data);
                            blocks.push(ContentBlock::Text { text });
                        }
                    }
                }
            }
        }
        if blocks.is_empty() {
            blocks.push(ContentBlock::Text {
                text: String::new(),
            });
        }
        blocks
    }

    fn convert_tools(&self, tools: Vec<Value>) -> Vec<AnthropicTool> {
        tools
            .into_iter()
            .filter_map(|t| {
                // OpenAI format: { type: "function", function: { name, description, parameters } }
                let func = t.get("function")?;
                let name = func.get("name")?.as_str()?.to_string();
                let description = func
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string());
                let input_schema = func
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
                Some(AnthropicTool {
                    name,
                    description,
                    input_schema,
                })
            })
            .collect()
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>,
        history: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        // Build messages
        let mut messages = Vec::new();

        let history_len = history.len();
        for (i, msg) in history.iter().enumerate() {
            match msg.role.as_str() {
                "system" => {} // system handled via top-level `system` field
                "user" => {
                    let blocks = self.build_content_blocks(&msg.parts);
                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: blocks,
                    });
                }
                "assistant" => {
                    let mut blocks = Vec::new();

                    // Text content
                    let text_content = if let Ok(p) =
                        serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                    {
                        p.first()
                            .and_then(|pp| pp.content.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    if !text_content.is_empty() {
                        blocks.push(ContentBlock::Text { text: text_content });
                    }

                    // Tool calls → tool_use blocks (only if next message is tool result)
                    if let Some(tc_json) = &msg.tool_calls {
                        let is_next_tool = if i + 1 < history_len {
                            history[i + 1].role == "tool"
                        } else {
                            false
                        };
                        if is_next_tool {
                            if let Ok(calls) = serde_json::from_value::<Vec<Value>>(tc_json.clone())
                            {
                                for call in calls {
                                    if let Some(func) = call.get("function") {
                                        let id = call
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let name =
                                            func["name"].as_str().unwrap_or("unknown").to_string();
                                        let args_str = func["arguments"].as_str().unwrap_or("{}");
                                        let input: Value =
                                            serde_json::from_str(args_str).unwrap_or(json!({}));
                                        blocks.push(ContentBlock::ToolUse { id, name, input });
                                    }
                                }
                            }
                        }
                    }

                    if !blocks.is_empty() {
                        messages.push(AnthropicMessage {
                            role: "assistant".to_string(),
                            content: blocks,
                        });
                    }
                }
                "tool" => {
                    // Tool results → user message with tool_result content block
                    let tool_id = msg.tool_call_id.clone().unwrap_or_default();
                    let content_str = if let Ok(p) =
                        serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                    {
                        p.first()
                            .and_then(|pp| pp.content.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: tool_id,
                            content: content_str,
                        }],
                    });
                }
                _ => {}
            }
        }

        // Convert tools
        let anthropic_tools = tools.map(|ts| self.convert_tools(ts));

        // System prompt
        let system = system_prompt.filter(|s| !s.trim().is_empty());

        let request_body = AnthropicRequest {
            model: model.to_string(),
            max_tokens: 8192,
            stream: true,
            messages,
            system,
            tools: anthropic_tools,
        };

        let url = format!("{}/v1/messages", self.base_url);

        let res = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Anthropic HTTP Error: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Anthropic API Error ({}): {}",
                status,
                err_text
            ));
        }

        let stream = res.bytes_stream().eventsource();

        // Track input_tokens from message_start for final usage
        let mut input_tokens_acc: i64 = 0;

        let mapped_stream = stream.map(move |item| {
            match item {
                Ok(event) => {
                    let event_type = event.event.as_str();

                    match event_type {
                        "message_start" => {
                            // Extract input_tokens
                            if let Ok(body) = serde_json::from_str::<MessageStartBody>(&event.data)
                            {
                                if let Some(msg) = body.message {
                                    if let Some(usage) = msg.usage {
                                        input_tokens_acc = usage.input_tokens.unwrap_or(0);
                                    }
                                }
                            }
                            Ok(ChatStreamChunk {
                                content: None,
                                tool_calls: vec![],
                                usage: None,
                                finish_reason: None,
                            })
                        }

                        "content_block_start" => {
                            if let Ok(body) =
                                serde_json::from_str::<ContentBlockStartBody>(&event.data)
                            {
                                if let Some(block) = body.content_block {
                                    if block.block_type == "tool_use" {
                                        return Ok(ChatStreamChunk {
                                            content: None,
                                            tool_calls: vec![ToolCallChunk {
                                                index: body.index as i32,
                                                id: block.id,
                                                name: block.name,
                                                arguments: None,
                                                signature: None,
                                            }],
                                            usage: None,
                                            finish_reason: None,
                                        });
                                    }
                                }
                            }
                            Ok(ChatStreamChunk {
                                content: None,
                                tool_calls: vec![],
                                usage: None,
                                finish_reason: None,
                            })
                        }

                        "content_block_delta" => {
                            if let Ok(body) =
                                serde_json::from_str::<ContentBlockDeltaBody>(&event.data)
                            {
                                if let Some(delta) = body.delta {
                                    match delta.delta_type.as_str() {
                                        "text_delta" => {
                                            return Ok(ChatStreamChunk {
                                                content: delta.text,
                                                tool_calls: vec![],
                                                usage: None,
                                                finish_reason: None,
                                            });
                                        }
                                        "input_json_delta" => {
                                            return Ok(ChatStreamChunk {
                                                content: None,
                                                tool_calls: vec![ToolCallChunk {
                                                    index: body.index as i32,
                                                    id: None,
                                                    name: None,
                                                    arguments: delta.partial_json,
                                                    signature: None,
                                                }],
                                                usage: None,
                                                finish_reason: None,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(ChatStreamChunk {
                                content: None,
                                tool_calls: vec![],
                                usage: None,
                                finish_reason: None,
                            })
                        }

                        "message_delta" => {
                            if let Ok(body) = serde_json::from_str::<MessageDeltaBody>(&event.data)
                            {
                                let finish_reason = body.delta.and_then(|d| d.stop_reason);
                                let usage = body.usage.map(|u| {
                                    let output = u.output_tokens.unwrap_or(0);
                                    TokenUsage {
                                        input_tokens: input_tokens_acc,
                                        output_tokens: output,
                                        total_tokens: input_tokens_acc + output,
                                    }
                                });
                                return Ok(ChatStreamChunk {
                                    content: None,
                                    tool_calls: vec![],
                                    usage,
                                    finish_reason,
                                });
                            }
                            Ok(ChatStreamChunk {
                                content: None,
                                tool_calls: vec![],
                                usage: None,
                                finish_reason: None,
                            })
                        }

                        "message_stop" | "ping" | "content_block_stop" => Ok(ChatStreamChunk {
                            content: None,
                            tool_calls: vec![],
                            usage: None,
                            finish_reason: None,
                        }),

                        _ => Ok(ChatStreamChunk {
                            content: None,
                            tool_calls: vec![],
                            usage: None,
                            finish_reason: None,
                        }),
                    }
                }
                Err(e) => Err(anyhow::anyhow!("SSE Stream Error: {}", e)),
            }
        });

        Ok(Box::pin(mapped_stream))
    }
}
