// src/handlers/models.rs
use axum::{
    extract::{Extension, Query},
    Json,
};
use std::sync::Arc;

use crate::errors::AppError;
use crate::request::ModelsQuery;
use crate::response::ModelsResponse;
use crate::services::models::SyncReport;
use crate::AppState;

/// GET /api/models
/// List all available models with optional provider filter
pub async fn list_models(
    Extension(state): Extension<Arc<AppState>>,
    Query(params): Query<ModelsQuery>,
) -> Result<Json<ModelsResponse>, AppError> {
    // Force refresh if requested
    if params.refresh.unwrap_or(false) {
        let _ = state.models_service.refresh().await;
    }

    let models = state
        .models_service
        .get_models(params.provider.as_deref())
        .await
        .map_err(AppError::Internal)?;

    let providers = state
        .models_service
        .get_provider_status()
        .await
        .map_err(AppError::Internal)?;

    Ok(Json(ModelsResponse { models, providers }))
}

/// POST /api/models/sync
/// Force sync models from all providers
pub async fn sync_models(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<SyncReport>, AppError> {
    let report = state
        .models_service
        .refresh()
        .await
        .map_err(AppError::Internal)?;

    Ok(Json(report))
}
