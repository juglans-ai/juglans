use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "workflows")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub slug: String,
    pub name: Option<String>,
    pub description: Option<String>,

    /// juglans web server 地址 (可选)
    pub endpoint_url: Option<String>,

    /// .jgflow 源码存档 (可选，存储为 JSONB)
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub definition: Option<serde_json::Value>,

    /// 所属组织，默认 "juglans"
    #[serde(default = "default_org_id")]
    pub org_id: Option<String>,
    pub user_id: Option<Uuid>,

    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub trigger_config: Option<serde_json::Value>,

    pub is_active: Option<bool>,

    /// 是否公开 (类似 GitHub public repo)
    pub is_public: Option<bool>,

    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

fn default_org_id() -> Option<String> {
    Some("juglans".to_string())
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
