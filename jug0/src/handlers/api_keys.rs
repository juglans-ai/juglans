// src/handlers/api_keys.rs
use axum::{
    extract::{Extension, Json, Path},
    Json as AxumJson,
};
use rand::{distributions::Alphanumeric, Rng};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{hash_key, AuthUser};
use crate::entities::api_keys;
use crate::errors::AppError;
use crate::request::CreateApiKeyRequest;
use crate::response::CreateApiKeyResponse;
use crate::AppState;

// GET /api/keys
pub async fn list_keys(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
) -> Result<AxumJson<Vec<api_keys::Model>>, AppError> {
    let keys = api_keys::Entity::find()
        // 【修复】user.sub -> user.id (UUID)
        .filter(api_keys::Column::UserId.eq(user.id))
        .order_by_desc(api_keys::Column::CreatedAt)
        .all(&state.db)
        .await?;
    Ok(AxumJson(keys))
}

// POST /api/keys
pub async fn create_key(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<AxumJson<CreateApiKeyResponse>, AppError> {
    let random_part: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    let raw_key = format!("jug0_sk_{}", random_part);

    let hashed = hash_key(&raw_key);
    let now = chrono::Utc::now().naive_utc();

    let expires_at = req.days_valid.map(|d| now + chrono::Duration::days(d));

    let new_key = api_keys::ActiveModel {
        id: Set(Uuid::new_v4()),
        // 【修复】user.sub -> user.id (UUID)
        user_id: Set(user.id),
        name: Set(req.name.clone()),
        prefix: Set(format!("jug0_sk_{}...", &random_part[0..4])),
        key_hash: Set(hashed),
        created_at: Set(Some(now)),
        expires_at: Set(expires_at),
        last_used_at: Set(None),
    };

    let saved = new_key.insert(&state.db).await?;

    Ok(AxumJson(CreateApiKeyResponse {
        id: saved.id,
        name: saved.name,
        key: raw_key,
        expires_at: saved.expires_at,
    }))
}

// DELETE /api/keys/:id
pub async fn delete_key(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let result = api_keys::Entity::delete_many()
        .filter(api_keys::Column::Id.eq(id))
        // 【修复】user.sub -> user.id (UUID)
        .filter(api_keys::Column::UserId.eq(user.id))
        .exec(&state.db)
        .await?;

    if result.rows_affected == 0 {
        return Err(AppError::NotFound("API Key not found".to_string()));
    }

    Ok(AxumJson(serde_json::json!({ "success": true, "id": id })))
}
