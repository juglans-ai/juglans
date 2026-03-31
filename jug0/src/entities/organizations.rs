// src/entities/organizations.rs
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "organizations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String, // e.g. the value of OFFICIAL_ORG_SLUG
    pub name: String,
    pub api_key_hash: String,
    /// PEM 格式的公钥，用于验证该 ORG 签发的 JWT
    pub public_key: Option<String>,
    /// 密钥算法: RS256, ES256, EdDSA
    pub key_algorithm: Option<String>,
    pub created_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::users::Entity")]
    Users,
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
