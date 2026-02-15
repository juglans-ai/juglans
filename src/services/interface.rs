// src/services/interface.rs
use crate::services::jug0::ChatOutput;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

/// 工具执行回调 — 由调用方提供，runtime.chat() 收到 tool_call 事件时内联调用
#[async_trait]
pub trait ChatToolHandler: Send + Sync {
    async fn handle_tool_call(&self, tool_name: &str, arguments_json: &str) -> Result<String>;
}

/// 定义 JWL 运行时所需的外部能力接口
#[async_trait]
pub trait JuglansRuntime: Send + Sync {
    /// 核心对话能力（SSE 统一流）
    ///
    /// 当 tool_handler 为 Some 时，tool_call 事件在 SSE 流内处理（执行工具 + POST /tool-result），
    /// 始终返回 ChatOutput::Final。当为 None 时，遇到 tool_call 即 break 返回 ChatOutput::ToolCalls。
    async fn chat(
        &self,
        agent_config: Value,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
        chat_id: Option<&str>,
        token_sender: Option<UnboundedSender<String>>,
        meta_sender: Option<UnboundedSender<Value>>,
        state: Option<&str>,
        history: Option<&str>,
        tool_handler: Option<Arc<dyn ChatToolHandler>>,
    ) -> Result<ChatOutput>;

    /// 资源加载能力：获取提示词内容
    async fn fetch_prompt(&self, slug: &str) -> Result<String>;

    /// 记忆能力：语义搜索
    async fn search_memories(&self, query: &str, limit: u64) -> Result<Vec<Value>>;

    /// 获取聊天历史
    async fn fetch_chat_history(&self, chat_id: &str, include_all: bool) -> Result<Vec<Value>>;

    /// 创建消息（reply 等非 AI 工具持久化消息到 jug0）
    async fn create_message(
        &self,
        chat_id: &str,
        role: &str,
        content: &str,
        state: &str,
    ) -> Result<()>;

    /// 更新消息状态（workflow 节点回溯控制用户消息可见性）
    async fn update_message_state(
        &self,
        chat_id: &str,
        message_id: i32,
        state: &str,
    ) -> Result<()>;
}
