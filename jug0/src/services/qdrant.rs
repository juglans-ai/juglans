// src/services/qdrant.rs
use anyhow::Result;
use qdrant_client::prelude::*;
use qdrant_client::qdrant::{
    point_id::PointIdOptions,
    vectors::VectorsOptions,
    Condition,
    CreateCollection,
    Distance,
    Filter,
    GetPoints,
    PointId,
    PointStruct,
    ScrollPoints,
    SearchPoints,
    Vector,
    VectorParams,
    Vectors,
    VectorsConfig,
    WithPayloadSelector,
    WithVectorsSelector,
    WriteOrdering,
    WriteOrderingType, // 直接使用这个枚举
};
use std::env;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct VectorDbService {
    pub client: Arc<QdrantClient>,
}

impl VectorDbService {
    pub fn new() -> Result<Self> {
        let url = env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
        let client = QdrantClient::from_url(&url).build()?;
        Ok(Self {
            client: Arc::new(client),
        })
    }

    pub async fn ensure_collection(&self, name: &str, dim: u64) -> Result<()> {
        if !self.client.collection_exists(name).await? {
            tracing::info!("Creating Qdrant collection: {} (dim: {})", name, dim);

            self.client
                .create_collection(&CreateCollection {
                    collection_name: name.to_string(),
                    vectors_config: Some(VectorsConfig {
                        config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                            VectorParams {
                                size: dim,
                                distance: Distance::Cosine.into(),
                                ..Default::default()
                            },
                        )),
                    }),
                    ..Default::default()
                })
                .await?;

            let index_fields = vec!["user_id", "agent_id", "run_id"];
            for field in index_fields {
                let _ = self
                    .client
                    .create_field_index(
                        name,
                        field,
                        qdrant_client::qdrant::FieldType::Keyword,
                        None,
                        None,
                    )
                    .await;
            }
        }
        Ok(())
    }

    pub async fn upsert_points(
        &self,
        collection_name: &str,
        points_data: Vec<(
            Uuid,
            Vec<f32>,
            std::collections::HashMap<String, qdrant_client::qdrant::Value>,
        )>,
    ) -> Result<()> {
        let mut points = Vec::new();
        let mut first_id = None;

        for (id, vector, payload) in points_data {
            if first_id.is_none() {
                first_id = Some(id);
            }

            let point_id = PointId {
                point_id_options: Some(PointIdOptions::Uuid(id.to_string())),
            };

            let vectors = Vectors {
                vectors_options: Some(VectorsOptions::Vector(Vector {
                    data: vector,
                    ..Default::default()
                })),
            };

            points.push(PointStruct {
                id: Some(point_id),
                vectors: Some(vectors),
                payload,
            });
        }

        let count = points.len();
        let ordering = Some(WriteOrdering {
            r#type: WriteOrderingType::Strong.into(),
        });

        self.client
            .upsert_points(collection_name, None, points, ordering)
            .await?;
        tracing::info!(
            "Qdrant: Upserted {} points into {} with Strong ordering.",
            count,
            collection_name
        );

        if let Some(fid) = first_id {
            let pid = PointId {
                point_id_options: Some(PointIdOptions::Uuid(fid.to_string())),
            };
            let res = self
                .client
                .get_points(
                    collection_name,
                    None,
                    &[pid],
                    Some(WithVectorsSelector::from(true)),
                    Some(WithPayloadSelector::from(true)),
                    None,
                )
                .await?;

            if !res.result.is_empty() {
                tracing::info!("Qdrant: [SUCCESS] Write verified for point {}.", fid);
            } else {
                tracing::error!("Qdrant: [CRITICAL] Write verification FAILED for {}.", fid);
            }
        }

        Ok(())
    }

    pub async fn get_collection_info(&self, collection_name: &str) -> Result<u64> {
        let info = self.client.collection_info(collection_name).await?;
        Ok(info.result.and_then(|r| r.points_count).unwrap_or(0))
    }

    pub async fn delete_points(&self, collection_name: &str, ids: Vec<Uuid>) -> Result<()> {
        let pids: Vec<PointId> = ids
            .into_iter()
            .map(|id| PointId {
                point_id_options: Some(PointIdOptions::Uuid(id.to_string())),
            })
            .collect();

        let selector = qdrant_client::qdrant::PointsSelector {
            points_selector_one_of: Some(
                qdrant_client::qdrant::points_selector::PointsSelectorOneOf::Points(
                    qdrant_client::qdrant::PointsIdsList { ids: pids },
                ),
            ),
        };
        self.client
            .delete_points(collection_name, None, &selector, None)
            .await?;
        Ok(())
    }

    pub async fn search(
        &self,
        collection_name: &str,
        vector: Vec<f32>,
        limit: u64,
        score_threshold: Option<f32>,
        filters: Option<serde_json::Value>,
    ) -> Result<Vec<qdrant_client::qdrant::ScoredPoint>> {
        let filter = self.build_filter(filters);
        let search_result = self
            .client
            .search_points(&SearchPoints {
                collection_name: collection_name.to_string(),
                vector,
                filter,
                limit,
                score_threshold,
                with_payload: Some(true.into()),
                ..Default::default()
            })
            .await?;
        Ok(search_result.result)
    }

    fn build_filter(&self, filters: Option<serde_json::Value>) -> Option<Filter> {
        // 修复：先解包并绑定到变量，延长其生命周期
        let filters_owned = filters?;
        let obj = filters_owned.as_object()?;

        let mut conditions = Vec::new();
        for (key, val) in obj {
            if let Some(s) = val.as_str() {
                conditions.push(Condition::matches(key, s.to_string()));
            }
        }
        if conditions.is_empty() {
            None
        } else {
            Some(Filter::must(conditions))
        }
    }

    pub async fn scroll(
        &self,
        collection_name: &str,
        limit: Option<u32>,
        filters: Option<serde_json::Value>,
    ) -> Result<Vec<qdrant_client::qdrant::RetrievedPoint>> {
        let filter = self.build_filter(filters);
        let res = self
            .client
            .scroll(&ScrollPoints {
                collection_name: collection_name.to_string(),
                filter,
                limit,
                with_payload: Some(true.into()),
                ..Default::default()
            })
            .await?;
        Ok(res.result)
    }
}
