// src/handlers/embeddings.rs
use axum::{
    extract::{Extension, Json},
    Json as AxumJson,
};
use std::sync::Arc;

use crate::auth::AuthUser;
use crate::errors::AppError;
use crate::request::EmbeddingRequest;
use crate::response::EmbeddingResponse;
use crate::AppState;

// POST /api/embeddings
pub async fn create_embedding(
    Extension(state): Extension<Arc<AppState>>,
    _user: AuthUser,
    Json(req): Json<EmbeddingRequest>,
) -> Result<AxumJson<EmbeddingResponse>, AppError> {
    let model_name = req.model.unwrap_or_else(|| "openai".to_string());
    let provider = state.embedding_factory.get_provider(&model_name);

    let vector = provider.embed(&req.input).await.map_err(|e| {
        AppError::Provider(async_openai::error::OpenAIError::ApiError(
            async_openai::error::ApiError {
                message: e.to_string(),
                // 【修改】Wrap in Some
                r#type: Some("embedding_error".to_string()),
                param: None,
                code: None,
            },
        ))
    })?;

    Ok(AxumJson(EmbeddingResponse {
        dimension: vector.len(),
        embedding: vector,
        model: model_name,
    }))
}
