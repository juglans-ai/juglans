// src/providers/gemini.rs
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
use anyhow::Result;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::pin::Pin;
use std::time::Duration;
use uuid::Uuid;

// --- DTOs ---
#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolWrapper>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
}

#[derive(Serialize, Clone)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
enum Part {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: InlineData,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: serde_json::Value,
        #[serde(rename = "thoughtSignature")]
        #[serde(skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponseData,
    },
}

#[derive(Serialize, Clone)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Serialize, Clone)]
struct FunctionResponseData {
    name: String,
    response: serde_json::Value,
}

#[derive(Serialize)]
struct ToolWrapper {
    function_declarations: Vec<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize, Debug)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<i64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<i64>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<i64>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    content: Option<ResponseContent>,
}

#[derive(Deserialize, Debug)]
struct ResponseContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ResponsePart {
    Text {
        text: String,
    },
    FunctionCallWrapper {
        #[serde(rename = "functionCall")]
        function_call: serde_json::Value,
        #[serde(rename = "thoughtSignature")]
        thought_signature: Option<String>,
    },
}

pub struct GeminiProvider {
    client: Client,
    base_url: String,
    api_key: String,
}

impl GeminiProvider {
    pub fn new() -> Self {
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: "https://generativelanguage.googleapis.com/v1beta/models".to_string(),
            api_key,
        }
    }

    fn find_function_name_by_id(history: &[Message], target_id: &str) -> Option<String> {
        for msg in history.iter().rev() {
            if msg.role == "assistant" {
                if let Some(tools_json) = &msg.tool_calls {
                    if let Ok(calls) =
                        serde_json::from_value::<Vec<serde_json::Value>>(tools_json.clone())
                    {
                        for call in calls {
                            if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                                if id == target_id {
                                    return call
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|n| n.as_str())
                                        .map(|s| s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn build_gemini_parts(&self, parts_json: &serde_json::Value) -> Vec<Part> {
        let mut gemini_parts = Vec::new();
        if let Ok(parts) = serde_json::from_value::<Vec<MessagePart>>(parts_json.clone()) {
            for part in parts {
                match part.part_type.as_str() {
                    "text" | "tool_result" => {
                        if let Some(t) = part.content {
                            if !t.is_empty() {
                                gemini_parts.push(Part::Text { text: t });
                            }
                        }
                    }
                    "image" => {
                        if let Some(data_val) = part.data {
                            if let Some(data_str) = data_val.as_str() {
                                if data_str.starts_with("data:") {
                                    let parts: Vec<&str> = data_str.split(',').collect();
                                    if parts.len() == 2 {
                                        let meta = parts[0];
                                        let mime = meta
                                            .split(';')
                                            .next()
                                            .unwrap_or("")
                                            .replace("data:", "");
                                        let b64 = parts[1];
                                        gemini_parts.push(Part::InlineData {
                                            inline_data: InlineData {
                                                mime_type: mime,
                                                data: b64.to_string(),
                                            },
                                        });
                                    }
                                } else {
                                    gemini_parts.push(Part::Text {
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
                            gemini_parts.push(Part::Text { text });
                        }
                    }
                }
            }
        }
        if gemini_parts.is_empty() {
            gemini_parts.push(Part::Text {
                text: "".to_string(),
            });
        }
        gemini_parts
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>,
        history: Vec<Message>,
        tools: Option<Vec<serde_json::Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        let system_instruction_obj = if let Some(sp) = system_prompt {
            if !sp.trim().is_empty() {
                Some(Content {
                    role: "user".to_string(),
                    parts: vec![Part::Text { text: sp }],
                })
            } else {
                None
            }
        } else {
            None
        };

        let mut gemini_contents = Vec::new();
        let history_len = history.len();

        for (i, msg) in history.iter().enumerate() {
            match msg.role.as_str() {
                "system" => {} // 忽略历史记录中的 system，因为已通过 instruction 注入
                "user" => {
                    let parts = self.build_gemini_parts(&msg.parts);
                    gemini_contents.push(Content {
                        role: "user".to_string(),
                        parts,
                    });
                }
                "assistant" => {
                    let mut parts = Vec::new();
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
                        parts.push(Part::Text { text: text_content });
                    }

                    if let Some(tc_json) = &msg.tool_calls {
                        let is_next_tool = if i + 1 < history_len {
                            history[i + 1].role == "tool"
                        } else {
                            false
                        };
                        if is_next_tool {
                            if let Ok(calls) =
                                serde_json::from_value::<Vec<serde_json::Value>>(tc_json.clone())
                            {
                                for call in calls {
                                    if let Some(func) = call.get("function") {
                                        let name =
                                            func["name"].as_str().unwrap_or("unknown").to_string();
                                        let args_str = func["arguments"].as_str().unwrap_or("{}");
                                        let args_json: serde_json::Value =
                                            serde_json::from_str(args_str).unwrap_or(json!({}));

                                        let sig = call
                                            .get("id")
                                            .and_then(|s| s.as_str())
                                            .map(|s| s.to_string());

                                        parts.push(Part::FunctionCall { 
                                            function_call: json!({ "name": name, "args": args_json }),
                                            thought_signature: sig 
                                        });
                                    }
                                }
                            }
                        }
                    }

                    if !parts.is_empty() {
                        gemini_contents.push(Content {
                            role: "model".to_string(),
                            parts,
                        });
                    }
                }
                "tool" => {
                    let tool_id = msg.tool_call_id.clone().unwrap_or_default();
                    let func_name = Self::find_function_name_by_id(&history, &tool_id)
                        .unwrap_or_else(|| "unknown_tool".to_string());

                    let content_str = if let Ok(p) =
                        serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                    {
                        p.first()
                            .and_then(|pp| pp.content.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    let response_json: serde_json::Value = serde_json::from_str(&content_str)
                        .unwrap_or(json!({ "result": content_str }));

                    gemini_contents.push(Content {
                        role: "function".to_string(),
                        parts: vec![Part::FunctionResponse {
                            function_response: FunctionResponseData {
                                name: func_name,
                                response: response_json,
                            },
                        }],
                    });
                }
                _ => {}
            }
        }

        let gemini_tools = tools.map(|ts| {
            let mut decls = Vec::new();
            for t in ts {
                if let Some(func) = t.get("function") {
                    decls.push(func.clone());
                }
            }
            vec![ToolWrapper {
                function_declarations: decls,
            }]
        });

        let request_body = GeminiRequest {
            contents: gemini_contents,
            tools: gemini_tools,
            system_instruction: system_instruction_obj,
        };

        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, model, self.api_key
        );

        // 发送请求
        let res = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Gemini HTTP Error: {}", e))?;

        // 【修复】先获取 status，再获取 text，避免所有权问题
        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gemini API Error ({}): {}",
                status,
                err_text
            ));
        }

        let stream = res.bytes_stream().eventsource();

        let mapped_stream = stream.map(|item| {
            match item {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        return Ok(ChatStreamChunk {
                            content: None,
                            tool_calls: vec![],
                            usage: None,
                            finish_reason: None,
                        });
                    }

                    let resp: GeminiResponse = match serde_json::from_str(&event.data) {
                        Ok(r) => r,
                        Err(_) => {
                            return Ok(ChatStreamChunk {
                                content: None,
                                tool_calls: vec![],
                                usage: None,
                                finish_reason: None,
                            })
                        }
                    };

                    let mut content_acc = None;
                    let mut tool_calls_acc = Vec::new();

                    if let Some(candidates) = resp.candidates {
                        if let Some(first) = candidates.first() {
                            if let Some(content) = &first.content {
                                if let Some(parts) = &content.parts {
                                    for part in parts {
                                        match part {
                                            ResponsePart::Text { text } => {
                                                content_acc = Some(text.clone());
                                            }
                                            ResponsePart::FunctionCallWrapper {
                                                function_call,
                                                thought_signature,
                                            } => {
                                                let gemini_id = thought_signature.clone();
                                                let synthetic_id =
                                                    format!("call_{}", Uuid::new_v4().simple());
                                                let name = function_call
                                                    .get("name")
                                                    .and_then(|s| s.as_str())
                                                    .map(|s| s.to_string());
                                                let args = function_call
                                                    .get("args")
                                                    .map(|v| v.to_string());

                                                tool_calls_acc.push(ToolCallChunk {
                                                    index: 0,
                                                    id: Some(synthetic_id),
                                                    name,
                                                    arguments: args,
                                                    signature: gemini_id,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Extract usage from usageMetadata (appears in final chunk)
                    let usage = resp.usage_metadata.map(|u| TokenUsage {
                        input_tokens: u.prompt_token_count.unwrap_or(0),
                        output_tokens: u.candidates_token_count.unwrap_or(0),
                        total_tokens: u.total_token_count.unwrap_or(0),
                    });

                    Ok(ChatStreamChunk {
                        content: content_acc,
                        tool_calls: tool_calls_acc,
                        usage,
                        finish_reason: None,
                    })
                }
                Err(e) => Err(anyhow::anyhow!("SSE Stream Error: {}", e)),
            }
        });

        Ok(Box::pin(mapped_stream))
    }
}
