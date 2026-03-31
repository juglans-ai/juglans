// src/entities/models.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String, // 模型 ID，如 "gpt-4o"

    pub provider: String, // openai, deepseek, gemini, qwen

    pub name: Option<String>,
    pub owned_by: Option<String>,
    pub context_length: Option<i32>,

    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub capabilities: Option<serde_json::Value>,

    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub pricing: Option<serde_json::Value>,

    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub raw_data: Option<serde_json::Value>,

    pub is_available: bool,

    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
