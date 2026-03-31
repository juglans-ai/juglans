// src/request/vectors.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ─── Space Management ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSpaceRequest {
    pub space: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Embedding model: "openai" | "qwen" (default: from MEMORY_EMBEDDING_MODEL env)
    #[serde(default)]
    pub model: Option<String>,
    /// If true, all users in the same org can search this space
    #[serde(default)]
    pub public: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteSpaceQuery {
    /// Also used as Path parameter
    pub space: Option<String>,
}

// ─── Vector Operations ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VectorUpsertRequest {
    pub space: String,
    /// Embedding model override (must match space's model for correct search)
    #[serde(default)]
    pub model: Option<String>,
    pub points: Vec<VectorPoint>,
}

#[derive(Debug, Deserialize)]
pub struct VectorPoint {
    /// Point identifier (will be converted to deterministic UUID v5)
    pub id: String,
    /// Text to auto-embed (mutually exclusive with `embedding`)
    #[serde(default)]
    pub text: Option<String>,
    /// Pre-computed embedding vector (skips embedding step)
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    /// Additional metadata stored with the vector
    #[serde(default)]
    pub payload: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct VectorSearchRequest {
    pub space: String,
    /// Search query text (will be auto-embedded)
    pub query: String,
    /// Embedding model override
    #[serde(default)]
    pub model: Option<String>,
    /// Max results (default: 10)
    #[serde(default)]
    pub limit: Option<u64>,
    /// Min similarity score (default: 0.3)
    #[serde(default)]
    pub threshold: Option<f32>,
    /// Additional payload filters
    #[serde(default)]
    pub filters: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct VectorDeleteRequest {
    pub space: String,
    pub ids: Vec<String>,
}

// ─── Response Types ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct VectorSearchResult {
    pub id: String,
    pub score: f32,
    pub payload: HashMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct SpaceInfo {
    pub space: String,
    pub model: String,
    pub public: bool,
    pub description: String,
    pub created_at: String,
}
