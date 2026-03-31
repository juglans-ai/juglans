// src/request/embeddings.rs
//
// Embedding request types

use serde::Deserialize;

/// Create embedding request
#[derive(Debug, Deserialize)]
pub struct EmbeddingRequest {
    pub input: String,
    pub model: Option<String>,
}
