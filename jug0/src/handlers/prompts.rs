// src/handlers/prompts.rs
use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::auth::AuthUser;
use crate::entities::prompts;
use crate::errors::AppError;
use crate::request::{CreatePromptRequest, PromptFilter, RenderPromptRequest, UpdatePromptRequest};
use crate::response::{PromptWithOwner, RenderPromptResponse};
use crate::AppState;

/// GET /api/prompts
pub async fn list_prompts(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Query(params): Query<PromptFilter>,
) -> Result<Json<Vec<PromptWithOwner>>, AppError> {
    let prompts = state.prompt_repo.list_for_user(&user, &params).await?;
    Ok(Json(prompts))
}

/// GET /api/prompts/:key (ID or Slug)
pub async fn get_prompt(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(key): Path<String>,
) -> Result<Json<prompts::Model>, AppError> {
    let prompt = state.prompt_repo.get_by_id_or_slug(&user, &key).await?;
    Ok(Json(prompt))
}

/// POST /api/prompts
pub async fn create_prompt(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreatePromptRequest>,
) -> Result<Json<prompts::Model>, AppError> {
    let saved = state.prompt_repo.create(&user, req).await?;
    Ok(Json(saved))
}

/// PATCH /api/prompts/:id
pub async fn update_prompt(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_slug): Path<String>,
    Json(req): Json<UpdatePromptRequest>,
) -> Result<Json<prompts::Model>, AppError> {
    let updated = state.prompt_repo.update(&user, &id_or_slug, req).await?;
    Ok(Json(updated))
}

/// DELETE /api/prompts/:id
pub async fn delete_prompt(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let prompt_id = state.prompt_repo.delete(&user, &id_or_slug).await?;
    Ok(Json(json!({ "success": true, "id": prompt_id })))
}

/// POST /api/prompts/:key/render
pub async fn render_prompt(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(key): Path<String>,
    Json(req): Json<RenderPromptRequest>,
) -> Result<Json<RenderPromptResponse>, AppError> {
    let response = state.prompt_repo.render(&user, &key, req).await?;
    Ok(Json(response))
}
