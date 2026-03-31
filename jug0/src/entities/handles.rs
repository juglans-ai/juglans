// src/entities/handles.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "handles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub org_id: String,
    pub handle: String,
    pub target_type: String, // "agent" | "user"
    pub target_id: Uuid,
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub created_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub updated_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

// Helper methods
impl Model {
    pub fn is_agent(&self) -> bool {
        self.target_type == "agent"
    }

    pub fn is_user(&self) -> bool {
        self.target_type == "user"
    }
}
