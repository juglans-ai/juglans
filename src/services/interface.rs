// src/services/interface.rs
use crate::services::jug0::ChatOutput;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// Tool execution callback -- provided by the caller, invoked inline when runtime.chat() receives a tool_call event
#[async_trait]
pub trait ChatToolHandler: Send + Sync {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String>;
}

/// Chat request parameters, replacing 9 individual arguments
pub struct ChatRequest {
    pub agent_config: Value,
    pub messages: Vec<Value>,
    pub tools: Option<Vec<Value>>,
    pub chat_id: Option<String>,
    pub token_sender: Option<UnboundedSender<String>>,
    pub meta_sender: Option<UnboundedSender<Value>>,
    pub state: Option<String>,
    pub history: Option<String>,
    pub tool_handler: Option<Arc<dyn ChatToolHandler>>,
}

/// External capability interface required by the Juglans runtime
#[async_trait]
pub trait JuglansRuntime: Send + Sync {
    /// Core chat capability (SSE unified stream).
    ///
    /// When tool_handler is Some, tool_call events are handled within the SSE stream (execute tools + POST /tool-result),
    /// always returning ChatOutput::Final. When None, breaks on tool_call and returns ChatOutput::ToolCalls.
    async fn chat(&self, req: ChatRequest) -> Result<ChatOutput>;

    /// Resource loading: fetch prompt content
    async fn fetch_prompt(&self, slug: &str) -> Result<String>;

    /// Memory: semantic search
    async fn search_memories(&self, query: &str, limit: u64) -> Result<Vec<Value>>;

    /// Fetch chat history
    async fn fetch_chat_history(&self, chat_id: &str, include_all: bool) -> Result<Vec<Value>>;

    /// Create message (persist non-AI tool messages like reply to jug0)
    async fn create_message(
        &self,
        chat_id: &str,
        role: &str,
        content: &str,
        state: &str,
    ) -> Result<()>;

    /// Update message state (workflow node retroactively controls user message visibility)
    async fn update_message_state(&self, chat_id: &str, message_id: i32, state: &str)
        -> Result<()>;

    // ─── Vector Storage & Search ─────────────────────────────

    /// Create a vector space
    async fn vector_create_space(
        &self,
        space: &str,
        model: Option<&str>,
        public: bool,
    ) -> Result<Value>;

    /// Upsert vectors into a space
    async fn vector_upsert(
        &self,
        space: &str,
        points: Vec<Value>,
        model: Option<&str>,
    ) -> Result<Value>;

    /// Search vectors in a space
    async fn vector_search(
        &self,
        space: &str,
        query: &str,
        limit: u64,
        model: Option<&str>,
    ) -> Result<Vec<Value>>;

    /// List all vector spaces
    async fn vector_list_spaces(&self) -> Result<Vec<Value>>;

    /// Delete a vector space
    async fn vector_delete_space(&self, space: &str) -> Result<Value>;

    /// Delete specific vectors by ID from a space
    async fn vector_delete(&self, space: &str, ids: Vec<String>) -> Result<Value>;
}
