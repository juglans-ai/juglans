use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "deploys")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub slug: String,
    pub org_id: Option<String>,
    pub user_id: Option<Uuid>,
    /// GitHub repo (owner/repo format)
    pub repo: String,
    pub branch: Option<String>,
    /// pending | building | deploying | deployed | failed | deleted
    pub status: String,
    /// https://{slug}.juglans.app
    pub url: Option<String>,
    /// ACR image URI
    pub image_uri: Option<String>,
    /// Environment variables (encrypted secrets)
    #[sea_orm(column_type = "JsonBinary")]
    pub env: serde_json::Value,
    pub error_message: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
