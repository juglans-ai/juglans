// src/providers/memory/mod.rs
#[cfg(feature = "server")]
pub mod qdrant;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A single point in the vector memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPoint {
    pub id: Uuid,
    pub vector: Vec<f32>,
    pub payload: HashMap<String, serde_json::Value>,
}

/// Filter criteria for memory queries.
#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    /// Key-value string matches (AND logic).
    pub fields: HashMap<String, String>,
}

impl MemoryFilter {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// Convert to a JSON object for backward compatibility with VectorDbService.
    pub fn to_json(&self) -> Option<serde_json::Value> {
        if self.fields.is_empty() {
            None
        } else {
            let map: serde_json::Map<String, serde_json::Value> = self
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            Some(serde_json::Value::Object(map))
        }
    }
}

/// A scored result from a vector similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResult {
    pub id: Uuid,
    pub score: f32,
    pub payload: HashMap<String, serde_json::Value>,
}

/// Trait for vector memory storage backends (e.g., Qdrant, Pinecone, Weaviate).
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    /// Ensure the backing collection/index exists with the given vector dimension.
    async fn ensure_collection(&self, name: &str, dimension: u64) -> Result<()>;

    /// Return the number of points in a collection.
    async fn collection_point_count(&self, name: &str) -> Result<u64>;

    /// Upsert (insert or update) a batch of memory points.
    async fn upsert(&self, collection: &str, points: Vec<MemoryPoint>) -> Result<()>;

    /// Delete points by their IDs.
    async fn delete(&self, collection: &str, ids: Vec<Uuid>) -> Result<()>;

    /// Perform a vector similarity search.
    async fn search(
        &self,
        collection: &str,
        vector: Vec<f32>,
        limit: u64,
        score_threshold: Option<f32>,
        filter: Option<MemoryFilter>,
    ) -> Result<Vec<MemoryResult>>;

    /// Scroll (list) points with optional filtering.
    async fn scroll(
        &self,
        collection: &str,
        limit: Option<u32>,
        filter: Option<MemoryFilter>,
    ) -> Result<Vec<MemoryResult>>;
}
