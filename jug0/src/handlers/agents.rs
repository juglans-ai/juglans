// src/handlers/agents.rs
use axum::{
    extract::{Extension, Json, Path},
    Json as AxumJson,
};
use std::sync::Arc;

use crate::auth::AuthUser;
use crate::entities::agents;
use crate::errors::AppError;
use crate::request::{CreateAgentRequest, UpdateAgentRequest};
use crate::response::{AgentDetailResponse, AgentWithOwner};
use crate::AppState;

/// GET /api/agents
pub async fn list_agents(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
) -> Result<AxumJson<Vec<AgentWithOwner>>, AppError> {
    let agents = state.agent_repo.list_for_user(&user).await?;
    Ok(AxumJson(agents))
}

/// GET /api/agents/:id_or_slug
pub async fn get_agent(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_slug): Path<String>,
) -> Result<AxumJson<AgentDetailResponse>, AppError> {
    let agent = state
        .agent_repo
        .get_by_id_or_slug(&user, &id_or_slug)
        .await?;
    Ok(AxumJson(agent))
}

/// POST /api/agents
pub async fn create_agent(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreateAgentRequest>,
) -> Result<AxumJson<agents::Model>, AppError> {
    let saved = state.agent_repo.create(&user, req).await?;
    Ok(AxumJson(saved))
}

/// PATCH /api/agents/:id_or_slug
pub async fn update_agent(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_slug): Path<String>,
    Json(req): Json<UpdateAgentRequest>,
) -> Result<AxumJson<agents::Model>, AppError> {
    let updated = state.agent_repo.update(&user, &id_or_slug, req).await?;
    Ok(AxumJson(updated))
}

/// DELETE /api/agents/:id_or_slug
pub async fn delete_agent(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_slug): Path<String>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let agent_id = state.agent_repo.delete(&user, &id_or_slug).await?;
    Ok(AxumJson(
        serde_json::json!({ "success": true, "id": agent_id }),
    ))
}
