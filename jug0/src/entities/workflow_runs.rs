// src/entities/workflow_runs.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "workflow_runs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,

    pub workflow_id: Uuid,

    /// "cron" | "manual" | "api"
    pub trigger: String,

    /// "pending" | "running" | "success" | "failed"
    pub status: String,

    pub started_at: Option<DateTime>,
    pub completed_at: Option<DateTime>,

    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub result: Option<serde_json::Value>,

    pub error: Option<String>,

    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::workflows::Entity",
        from = "Column::WorkflowId",
        to = "super::workflows::Column::Id"
    )]
    Workflow,
}

impl Related<super::workflows::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Workflow.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
