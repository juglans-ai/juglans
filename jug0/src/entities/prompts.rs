// src/entities/prompts.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "prompts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub org_id: Option<String>, // 新增
    pub user_id: Option<Uuid>,  // 变更 UUID

    pub slug: String,
    pub name: Option<String>,

    #[sea_orm(column_type = "Text")]
    pub content: String,

    #[sea_orm(column_type = "JsonBinary")]
    pub input_variables: Option<serde_json::Value>,

    pub r#type: Option<String>,

    #[sea_orm(column_type = "JsonBinary")]
    pub tags: Option<serde_json::Value>,

    #[sea_orm(column_type = "JsonBinary")]
    pub allowed_agent_slugs: Option<serde_json::Value>,

    pub is_public: bool,
    pub is_system: bool,
    pub usage_count: i32,

    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
