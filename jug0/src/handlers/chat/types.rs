// src/handlers/chat/types.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ============================================
// Chat ID Input (UUID or @handle)
// ============================================

/// Chat ID input - can be UUID, @handle, or arbitrary external ID
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ChatIdInput {
    Uuid(Uuid),
    Handle(String),     // "@jarvis" — starts with @
    ExternalId(String), // "oc_xxx" — arbitrary platform ID
}

impl<'de> Deserialize<'de> for ChatIdInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Try to parse as UUID first
        if let Ok(uuid) = Uuid::parse_str(&s) {
            Ok(ChatIdInput::Uuid(uuid))
        } else if s.starts_with('@') {
            // @handle syntax
            Ok(ChatIdInput::Handle(s))
        } else {
            // Arbitrary external ID (e.g. Feishu group "oc_xxx")
            Ok(ChatIdInput::ExternalId(s))
        }
    }
}

/// Resolved handle target
#[derive(Debug, Clone)]
pub struct ResolvedHandle {
    pub target_type: String,
    pub target_id: Uuid,
}

// ============================================
// 消息部分（MessagePart）
// ============================================

// Re-export from providers (canonical location)
pub use crate::providers::llm::MessagePart;

// ============================================
// Agent 配置
// ============================================

#[derive(Deserialize, Serialize, Debug, Clone)]
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

// ============================================
// Chat 请求/响应
// ============================================

#[derive(Deserialize, Serialize)]
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

#[derive(Serialize)]
pub struct ChatSyncResponse {
    pub chat_id: Uuid,
    pub message_id: i32,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<Value>>,
}

#[derive(Deserialize)]
pub struct StopRequest {
    /// Chat ID: UUID or @handle
    pub chat_id: ChatIdInput,
}

#[derive(Deserialize)]
pub struct ListChatsQuery {
    pub limit: Option<u64>,
}

// ============================================
// Tool Result Bridge（Client Tool 桥接）
// ============================================

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub content: String,
}

/// Channel payload: tool results + optional updated tool definitions
#[derive(Debug, Clone)]
pub struct ToolResultWithTools {
    pub results: Vec<ToolResultPayload>,
    pub tools: Option<Vec<Value>>,
    /// If true, run_chat_stream should NOT restart the LLM stream after this result.
    /// Used by Claude Code provider: MCP already returned the result to the running process.
    pub skip_restart: bool,
}

#[derive(Deserialize, Serialize)]
pub struct ToolResultRequest {
    pub call_id: String,
    pub results: Vec<ToolResultPayload>,
    /// Agent slug（用于查找 workflow endpoint）
    pub agent_slug: Option<String>,
    /// Chat ID (used to lookup cached workflow forward info)
    #[serde(default)]
    pub chat_id: Option<String>,
    /// Model for standard chat continuation
    #[serde(default)]
    pub model: Option<String>,
    /// Client tool definitions for continuation
    #[serde(default)]
    pub tools: Option<Vec<Value>>,
    /// Agent config for continuation
    #[serde(default)]
    pub agent: Option<AgentConfig>,
}

/// Cached workflow forward info (stored in AppState.workflow_forwards)
#[derive(Debug, Clone)]
pub struct WorkflowForwardInfo {
    pub endpoint_url: String,
    pub agent_id: Uuid,
    pub author_user_id: Uuid,
}

// ============================================
// 消息创建请求
// ============================================

fn default_message_type() -> String {
    "chat".to_string()
}

fn default_state() -> String {
    "context_visible".to_string()
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CreateMessageRequest {
    /// 角色：user, assistant, tool, system
    pub role: String,

    /// 消息类型：chat, command, command_result, tool_call, tool_result, system
    #[serde(default = "default_message_type")]
    pub message_type: String,

    /// 消息状态：context_visible | context_hidden | display_only | silent
    #[serde(default = "default_state")]
    pub state: String,

    /// 消息内容片段
    pub parts: Vec<MessagePart>,

    /// 工具调用列表（仅 assistant 消息）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,

    /// 工具调用 ID（仅 tool 消息）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// 引用的消息 message_id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_message_id: Option<i32>,

    /// 扩展元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

// ============================================
// 消息响应
// ============================================

#[derive(Serialize, Debug, Clone)]
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

// ============================================
// 上下文查询
// ============================================

#[derive(Deserialize, Debug)]
pub struct ContextQuery {
    /// 包含所有 state 的消息（默认 false，仅返回 context_visible/context_hidden）
    #[serde(default)]
    pub include_all: bool,

    /// 从哪个 message_id 开始
    pub from_message_id: Option<i32>,

    /// 最多返回多少条
    pub limit: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct ContextResponse {
    pub chat_id: Uuid,
    pub messages: Vec<MessageResponse>,
}

// ============================================
// 流事件
// ============================================

#[derive(Default, Debug, Clone, Serialize)]
pub struct ToolCallAccumulator {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 内部流事件（用于流式处理）
#[derive(Debug, Clone)]
pub enum InternalStreamEvent {
    /// 元数据（包含 chat_id、message_id 和 UUID）
    Meta {
        chat_id: Uuid,
        user_message_id: i32, // 用户消息的 message_id
        user_message_uuid: Option<Uuid>,
    },
    /// 内容块
    Content(String),
    /// 工具调用
    ToolCall { message_id: i32, tools: Vec<Value> },
    /// 完成（包含 assistant 消息的 UUID）
    Done {
        message_id: i32,
        assistant_message_uuid: Option<Uuid>,
    },
    /// 错误
    Error(String),
}

/// SSE 事件（序列化后发送给客户端）
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

// ============================================
// 消息操作请求
// ============================================

#[derive(Deserialize, Debug)]
pub struct UpdateMessageRequest {
    pub parts: Option<Vec<MessagePart>>,
    pub metadata: Option<Value>,
    pub state: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct RegenerateRequest {
    /// 可选：切换模型
    pub model: Option<String>,
    /// 是否保留原消息（默认 false，删除后重新生成）
    #[serde(default)]
    pub keep_message: bool,
}

#[derive(Deserialize, Debug)]
pub struct BranchRequest {
    /// 从哪个 message_id 开始分支
    pub from_message_id: i32,
    /// 新会话标题
    pub title: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct BranchResponse {
    pub chat_id: Uuid,
    pub branched_from: BranchSource,
    pub message_count: i32,
}

#[derive(Serialize, Debug)]
pub struct BranchSource {
    pub chat_id: Uuid,
    pub message_id: i32,
}
