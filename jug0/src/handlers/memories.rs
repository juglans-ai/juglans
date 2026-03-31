// src/handlers/memories.rs
use crate::auth::AuthUser;
use crate::errors::AppError;
use crate::request::{ListMemoryQuery, SearchMemoryRequest};
use crate::AppState;
use axum::{
    extract::{Extension, Json, Path, Query},
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// GET /api/memories
pub async fn list_memories(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Query(params): Query<ListMemoryQuery>,
) -> Result<impl IntoResponse, AppError> {
    let results = state
        .memory_service
        .list_memories(
            // 【修复】user.sub -> user.id.to_string()
            user.id.to_string(),
            params.agent_id,
            params.limit,
        )
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(Json(results))
}

/// DELETE /api/memories/:id
pub async fn delete_memory(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    state
        .memory_service
        .delete_memory(id, user.id.to_string())
        .await
        .map_err(|e| AppError::Internal(e))?;
    Ok(Json(json!({ "status": "ok" })))
}

/// POST /api/memories/search
pub async fn search_memories(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<SearchMemoryRequest>,
) -> Result<impl IntoResponse, AppError> {
    let results = state
        .memory_service
        .search(
            req.query,
            // 【修复】user.sub -> user.id.to_string()
            Some(user.id.to_string()),
            req.agent_id,
            None, // run_id
            req.limit.unwrap_or(10),
        )
        .await
        .map_err(|e| AppError::Internal(e))?;

    Ok(Json(results))
}
