// src/providers/llm/mod.rs
pub mod anthropic;
pub mod byteplus;
pub mod chatgpt;
pub mod claude_code;
pub mod deepseek;
pub mod factory;
pub mod gemini;
pub mod juglans;
pub mod mcp_types;
pub mod openai;
pub mod qwen;
pub mod xai;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

// ---------------------------------------------------------------------------
// Lightweight message types (no SeaORM dependency)
// ---------------------------------------------------------------------------

/// Generic chat message for LLM providers.
/// DB-agnostic equivalent of the SeaORM `messages::Model`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub parts: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// Message content part (text, image, data, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub content: Option<String>,
    pub data: Option<Value>,
    pub role: Option<String>,
    pub tool_call_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Stream types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolCallChunk {
    pub index: i32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
    pub signature: Option<String>,
}

/// Token usage statistics from LLM providers
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone)]
pub struct ChatStreamChunk {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallChunk>,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>,
        history: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>>;
}
