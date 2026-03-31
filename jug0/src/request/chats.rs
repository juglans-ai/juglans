// src/request/chats.rs
//
// Chat-related request types

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Chat ID input - can be UUID (existing chat) or @handle (start chat with agent)
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ChatIdInput {
    Uuid(Uuid),
    Handle(String),
}

impl<'de> Deserialize<'de> for ChatIdInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Ok(uuid) = Uuid::parse_str(&s) {
            Ok(ChatIdInput::Uuid(uuid))
        } else {
            Ok(ChatIdInput::Handle(s))
        }
    }
}

/// Message part (text, tool_result, etc.)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessagePart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub content: Option<String>,
    pub data: Option<Value>,
    pub role: Option<String>,
    pub tool_call_id: Option<String>,
}

/// Agent configuration in chat request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub slug: Option<String>,
    pub id: Option<Uuid>,
    pub model: Option<String>,
    pub tools: Option<Vec<Value>>,
    pub system_prompt: Option<String>,
    /// 是否流式返回（默认 true）
    pub stream: Option<bool>,
    /// 是否启用记忆（默认 true）
    pub memory: Option<bool>,
}

/// Main chat request
#[derive(Debug, Deserialize, Serialize)]
pub struct ChatRequest {
    /// Chat ID: UUID for existing chat, or @handle to start with agent
    pub chat_id: Option<ChatIdInput>,
    pub messages: Vec<MessagePart>,
    pub agent: Option<AgentConfig>,

    /// 消息状态，控制持久化和流式行为
    /// context_visible(默认) | context_hidden | display_only | silent
    #[serde(default)]
    pub state: Option<String>,

    /// 上下文控制：true(默认)/false/自定义消息数组
    #[serde(default)]
    pub history: Option<Value>,

    // --- Deprecated: 以下字段将移除，请使用 agent.* ---
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<Value>>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub memory: Option<bool>,
}

/// Stop chat generation request
#[derive(Debug, Deserialize)]
pub struct StopRequest {
    /// Chat ID: UUID or @handle
    pub chat_id: ChatIdInput,
}

/// List chats query params
#[derive(Debug, Deserialize)]
pub struct ListChatsQuery {
    pub limit: Option<u64>,
}

/// Tool result payload
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub content: String,
}

/// Tool result bridge request
#[derive(Debug, Deserialize, Serialize)]
pub struct ToolResultRequest {
    pub call_id: String,
    pub results: Vec<ToolResultPayload>,
    /// Agent slug (for finding workflow endpoint)
    pub agent_slug: Option<String>,
}

fn default_message_type() -> String {
    "chat".to_string()
}

fn default_state() -> String {
    "context_visible".to_string()
}

/// Create message request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateMessageRequest {
    /// Role: user, assistant, tool, system
    pub role: String,

    /// Message type: chat, command, command_result, tool_call, tool_result, system
    #[serde(default = "default_message_type")]
    pub message_type: String,

    /// 消息状态：context_visible | context_hidden | display_only | silent
    #[serde(default = "default_state")]
    pub state: String,

    /// Message content parts
    pub parts: Vec<MessagePart>,

    /// Tool calls (assistant messages only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,

    /// Tool call ID (tool messages only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Reference message_id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_message_id: Option<i32>,

    /// Extended metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Context query params
#[derive(Debug, Deserialize)]
pub struct ContextQuery {
    /// 包含所有 state 的消息（默认 false，仅返回 context_visible/context_hidden）
    #[serde(default)]
    pub include_all: bool,

    /// Start from message_id
    pub from_message_id: Option<i32>,

    /// Max results
    pub limit: Option<i64>,
}

/// Update message request
#[derive(Debug, Deserialize)]
pub struct UpdateMessageRequest {
    pub parts: Option<Vec<MessagePart>>,
    pub metadata: Option<Value>,
    pub state: Option<String>,
}

/// Regenerate request
#[derive(Debug, Deserialize)]
pub struct RegenerateRequest {
    /// Optional: switch model
    pub model: Option<String>,
    /// Keep original message (default false, delete and regenerate)
    #[serde(default)]
    pub keep_message: bool,
}

/// Branch request
#[derive(Debug, Deserialize)]
pub struct BranchRequest {
    /// Branch from this message_id
    pub from_message_id: i32,
    /// New chat title
    pub title: Option<String>,
}

/// Update chat request
#[derive(Debug, Deserialize)]
pub struct UpdateChatRequest {
    pub title: Option<String>,
    pub model: Option<String>,
    pub agent_id: Option<Uuid>,
    pub incognito: Option<bool>,
    pub metadata: Option<Value>,
}

/// Batch delete messages request
#[derive(Debug, Deserialize)]
pub struct BatchDeleteMessagesRequest {
    /// Chat-local message_ids to delete
    pub message_ids: Vec<i32>,
}

/// Truncate messages request
#[derive(Debug, Deserialize)]
pub struct TruncateRequest {
    /// Delete all messages with message_id > this value
    pub from_message_id: i32,
}
