// src/response/embeddings.rs
//
// Embedding response types

use serde::Serialize;

/// Embedding response
#[derive(Debug, Serialize)]
pub struct EmbeddingResponse {
    pub embedding: Vec<f32>,
    pub model: String,
    pub dimension: usize,
}
