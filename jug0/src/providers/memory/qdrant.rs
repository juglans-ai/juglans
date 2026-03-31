// src/providers/memory/qdrant.rs
use super::{MemoryFilter, MemoryPoint, MemoryProvider, MemoryResult};
use crate::services::qdrant::VectorDbService;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

/// QdrantMemory wraps the existing VectorDbService and implements the MemoryProvider trait.
#[derive(Clone)]
pub struct QdrantMemory {
    inner: VectorDbService,
}

impl QdrantMemory {
    pub fn new(vector_db: VectorDbService) -> Self {
        Self { inner: vector_db }
    }

    /// Access the underlying VectorDbService for backward compatibility.
    pub fn inner(&self) -> &VectorDbService {
        &self.inner
    }
}

/// Helper: convert a JSON payload map into Qdrant-compatible payload.
fn json_payload_to_qdrant(
    payload: HashMap<String, serde_json::Value>,
) -> HashMap<String, qdrant_client::qdrant::Value> {
    use crate::services::memory::utils::json_to_qdrant_value;
    payload
        .into_iter()
        .map(|(k, v)| (k, json_to_qdrant_value(v)))
        .collect()
}

#[async_trait]
impl MemoryProvider for QdrantMemory {
    async fn ensure_collection(&self, name: &str, dimension: u64) -> Result<()> {
        self.inner.ensure_collection(name, dimension).await
    }

    async fn collection_point_count(&self, name: &str) -> Result<u64> {
        self.inner.get_collection_info(name).await
    }

    async fn upsert(&self, collection: &str, points: Vec<MemoryPoint>) -> Result<()> {
        let points_data: Vec<(
            Uuid,
            Vec<f32>,
            HashMap<String, qdrant_client::qdrant::Value>,
        )> = points
            .into_iter()
            .map(|p| (p.id, p.vector, json_payload_to_qdrant(p.payload)))
            .collect();
        self.inner.upsert_points(collection, points_data).await
    }

    async fn delete(&self, collection: &str, ids: Vec<Uuid>) -> Result<()> {
        self.inner.delete_points(collection, ids).await
    }

    async fn search(
        &self,
        collection: &str,
        vector: Vec<f32>,
        limit: u64,
        score_threshold: Option<f32>,
        filter: Option<MemoryFilter>,
    ) -> Result<Vec<MemoryResult>> {
        let filter_json = filter.and_then(|f| f.to_json());
        let scored_points = self
            .inner
            .search(collection, vector, limit, score_threshold, filter_json)
            .await?;

        use crate::services::memory::utils::qdrant_payload_to_map;

        let results = scored_points
            .into_iter()
            .filter_map(|point| {
                let id_str = match point.id {
                    Some(id) => match id.point_id_options {
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => u,
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => {
                            n.to_string()
                        }
                        None => return None,
                    },
                    None => return None,
                };
                let uid = Uuid::parse_str(&id_str).ok()?;
                let payload = qdrant_payload_to_map(point.payload);
                Some(MemoryResult {
                    id: uid,
                    score: point.score,
                    payload,
                })
            })
            .collect();

        Ok(results)
    }

    async fn scroll(
        &self,
        collection: &str,
        limit: Option<u32>,
        filter: Option<MemoryFilter>,
    ) -> Result<Vec<MemoryResult>> {
        let filter_json = filter.and_then(|f| f.to_json());
        let retrieved = self.inner.scroll(collection, limit, filter_json).await?;

        use crate::services::memory::utils::qdrant_payload_to_map;

        let results = retrieved
            .into_iter()
            .filter_map(|point| {
                let id_str = match point.id {
                    Some(id) => match id.point_id_options {
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => u,
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => {
                            n.to_string()
                        }
                        None => return None,
                    },
                    None => return None,
                };
                let uid = Uuid::parse_str(&id_str).ok()?;
                let payload = qdrant_payload_to_map(point.payload);
                Some(MemoryResult {
                    id: uid,
                    score: 1.0, // scroll doesn't produce scores
                    payload,
                })
            })
            .collect();

        Ok(results)
    }
}
