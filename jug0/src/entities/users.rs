// src/entities/users.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid, // Internal UUID

    pub org_id: Option<String>,
    pub external_id: Option<String>, // Node.js User ID

    #[sea_orm(unique)]
    pub email: Option<String>,

    #[serde(skip_serializing)]
    pub password_hash: Option<String>,

    pub name: Option<String>,

    #[sea_orm(unique)]
    pub username: Option<String>, // GitHub-style username, globally unique

    pub role: String,

    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::organizations::Entity",
        from = "Column::OrgId",
        to = "super::organizations::Column::Id"
    )]
    Organization,
}

impl Related<super::organizations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Organization.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
