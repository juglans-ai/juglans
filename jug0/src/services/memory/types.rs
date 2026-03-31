// src/services/memory/types.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 存入向量数据库的 Payload 结构
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MemoryPayload {
    pub id: Uuid,
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub run_id: Option<String>,
    pub content: String, // 实际的记忆文本
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: Option<chrono::NaiveDateTime>,
    #[serde(flatten)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// 步骤 1: 事实提取的 LLM 响应结构
#[derive(Debug, Deserialize)]
pub struct FactExtractionResult {
    #[serde(default)]
    pub facts: Vec<String>,
}

/// 记忆操作类型
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")] // 兼容 Python 版的 "ADD", "UPDATE"
pub enum MemoryActionType {
    Add,
    Update,
    Delete,
    None,
}

/// 步骤 4: 记忆更新决策的 LLM 响应单项
#[derive(Debug, Deserialize, Clone)]
pub struct MemoryOperation {
    /// 对于 ADD，是新 ID；对于 UPDATE/DELETE，是旧 ID (可能是 integer string 或 uuid string)
    pub id: Option<String>,
    pub text: Option<String>,
    pub event: MemoryActionType,
    pub old_memory: Option<String>,
}

/// 步骤 4: 记忆更新决策的完整 LLM 响应
#[derive(Debug, Deserialize)]
pub struct MemoryUpdateResult {
    #[serde(default)]
    pub memory: Vec<MemoryOperation>,
}

/// 搜索结果
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: Uuid,
    pub content: String,
    pub score: f32,
    pub created_at: String,
    pub metadata: HashMap<String, serde_json::Value>,
}
