// src/services/interface.rs
use async_trait::async_trait;
use serde_json::Value;
use anyhow::Result;
use crate::services::jug0::ChatOutput;
use tokio::sync::mpsc::UnboundedSender;

/// 定义 JWL 运行时所需的外部能力接口
#[async_trait]
pub trait JuglansRuntime: Send + Sync {
    /// 核心对话能力
    /// 增加 token_sender 用于流式透传
    async fn chat(
        &self, 
        agent_config: Value, 
        messages: Vec<Value>, 
        tools: Option<Vec<Value>>,
        chat_id: Option<&str>,
        token_sender: Option<UnboundedSender<String>> // 【新增】
    ) -> Result<ChatOutput>;

    /// 资源加载能力：获取提示词内容
    async fn fetch_prompt(&self, slug: &str) -> Result<String>;

    /// 记忆能力：语义搜索
    async fn search_memories(&self, query: &str, limit: u64) -> Result<Vec<Value>>;
}