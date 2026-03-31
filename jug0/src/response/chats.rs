// src/response/chats.rs
//
// Chat-related response types

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// Sync (non-streaming) chat response
#[derive(Debug, Serialize)]
pub struct ChatSyncResponse {
    pub chat_id: Uuid,
    pub message_id: i32,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<Value>>,
}

/// Message response
#[derive(Debug, Clone, Serialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub chat_id: Uuid,
    pub message_id: i32,
    pub role: String,
    pub message_type: String,
    pub state: String,
    pub parts: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_message_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    pub created_at: Option<DateTime<Utc>>,
}

impl From<crate::entities::messages::Model> for MessageResponse {
    fn from(m: crate::entities::messages::Model) -> Self {
        Self {
            id: m.id,
            chat_id: m.chat_id,
            message_id: m.message_id,
            role: m.role,
            message_type: m.message_type,
            state: m.state,
            parts: m.parts,
            tool_calls: m.tool_calls,
            tool_call_id: m.tool_call_id,
            ref_message_id: m.ref_message_id,
            metadata: m.metadata,
            created_at: m
                .created_at
                .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)),
        }
    }
}

/// Context response (messages list)
#[derive(Debug, Serialize)]
pub struct ContextResponse {
    pub chat_id: Uuid,
    pub messages: Vec<MessageResponse>,
}

/// Branch response
#[derive(Debug, Serialize)]
pub struct BranchResponse {
    pub chat_id: Uuid,
    pub branched_from: BranchSource,
    pub message_count: i32,
}

#[derive(Debug, Serialize)]
pub struct BranchSource {
    pub chat_id: Uuid,
    pub message_id: i32,
}

/// SSE stream events
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "meta")]
    Meta {
        chat_id: Uuid,
        message_id: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_message_uuid: Option<Uuid>,
    },

    #[serde(rename = "content")]
    Content { text: String },

    #[serde(rename = "tool_call")]
    ToolCall { message_id: i32, tools: Vec<Value> },

    #[serde(rename = "done")]
    Done {
        message_id: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        assistant_message_uuid: Option<Uuid>,
    },

    #[serde(rename = "error")]
    Error { message: String },
}
