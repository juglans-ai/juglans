// src/services/memory/service.rs
use anyhow::Result;
use chrono::Utc;
use futures::future::join_all;
use qdrant_client::qdrant::PointId;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entities::messages;
use crate::providers::EmbeddingFactory;
use crate::providers::ProviderFactory;
use crate::services::memory::prompts::{
    agent_memory_extraction_prompt, get_update_memory_messages, user_memory_extraction_prompt,
};
use crate::services::memory::types::*;
use crate::services::memory::utils::*;
use crate::services::qdrant::VectorDbService;

#[derive(Clone)]
pub struct MemoryService {
    embedding_factory: EmbeddingFactory,
    vector_db: VectorDbService,
    providers: ProviderFactory,
    collection_name: String,
    embedding_dim: u64,
}

impl MemoryService {
    pub fn new(
        embedding_factory: EmbeddingFactory,
        vector_db: VectorDbService,
        providers: ProviderFactory,
    ) -> Self {
        // 修正默认维度为 1024，匹配 Qwen v4
        let embedding_dim = std::env::var("MEMORY_EMBEDDING_DIM")
            .unwrap_or_else(|_| "1024".to_string())
            .parse::<u64>()
            .unwrap_or(1024);

        Self {
            embedding_factory,
            vector_db,
            providers,
            collection_name: "mem0".to_string(),
            embedding_dim,
        }
    }

    pub async fn init(&self) -> Result<()> {
        tracing::info!(
            "Initializing Memory Collection: {} (dim: {})",
            self.collection_name,
            self.embedding_dim
        );
        self.vector_db
            .ensure_collection(&self.collection_name, self.embedding_dim)
            .await?;

        match self
            .vector_db
            .get_collection_info(&self.collection_name)
            .await
        {
            Ok(count) => tracing::info!(
                "[Memory] Current points in collection '{}': {}",
                self.collection_name,
                count
            ),
            Err(e) => tracing::warn!("[Memory] Failed to get collection info: {}", e),
        }
        Ok(())
    }

    // --- 核心管道: ADD ---

    pub async fn add_memory(
        &self,
        messages: Vec<crate::providers::llm::MessagePart>,
        user_id: Option<String>,
        agent_id: Option<String>,
        run_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<Vec<MemoryOperation>> {
        let metadata_map =
            self.prepare_base_metadata(user_id.clone(), agent_id.clone(), run_id, metadata);
        let user_prompt = self.format_messages_for_llm(&messages);

        tracing::info!("[Memory] Starting extraction for user: {:?}", user_id);

        let facts = self
            .extract_facts(&messages, &agent_id, user_prompt)
            .await?;
        if facts.is_empty() {
            return Ok(vec![]);
        }

        let (new_embeddings_map, retrieved_old_memories) = self
            .retrieve_related_memories(&facts, &user_id, &agent_id)
            .await?;

        let actions = self
            .resolve_memory_actions(&facts, retrieved_old_memories)
            .await?;

        if !actions.is_empty() {
            self.execute_operations(actions.clone(), new_embeddings_map, metadata_map)
                .await?;
        }

        Ok(actions)
    }

    // --- 核心方法: SEARCH ---

    pub async fn search(
        &self,
        query: String,
        user_id: Option<String>,
        agent_id: Option<String>,
        run_id: Option<String>,
        limit: u64,
    ) -> Result<Vec<SearchResult>> {
        let embedding = self
            .embedding_factory
            .get_provider("default")
            .embed(&query)
            .await?;

        let mut filter_map = serde_json::Map::new();
        if let Some(uid) = user_id {
            filter_map.insert("user_id".to_string(), json!(uid));
        }
        if let Some(aid) = agent_id {
            filter_map.insert("agent_id".to_string(), json!(aid));
        }
        if let Some(rid) = run_id {
            filter_map.insert("run_id".to_string(), json!(rid));
        }
        let search_filter = if filter_map.is_empty() {
            None
        } else {
            Some(Value::Object(filter_map))
        };

        let points = self
            .vector_db
            .search(
                &self.collection_name,
                embedding,
                limit,
                Some(0.35),
                search_filter,
            )
            .await?;

        let mut results = Vec::new();
        for point in points {
            let id_str = match point.id {
                Some(id) => match id.point_id_options {
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => u,
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => n.to_string(),
                    None => String::new(),
                },
                None => String::new(),
            };

            let payload_map = qdrant_payload_to_map(point.payload);
            let content = payload_map
                .get("data")
                .or(payload_map.get("memory"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Ok(uid) = Uuid::parse_str(&id_str) {
                results.push(SearchResult {
                    id: uid,
                    content,
                    score: point.score,
                    created_at: payload_map
                        .get("created_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    metadata: payload_map
                        .into_iter()
                        .filter(|(k, _)| k != "data" && k != "memory")
                        .collect(),
                });
            }
        }
        Ok(results)
    }

    /// Delete a specific memory by ID (must belong to the user)
    pub async fn delete_memory(&self, memory_id: Uuid, user_id: String) -> Result<()> {
        // Verify ownership: scroll with user_id filter and check if the point exists
        let mut filter_map = serde_json::Map::new();
        filter_map.insert("user_id".to_string(), json!(user_id));
        let filter = Some(Value::Object(filter_map));

        let points = self
            .vector_db
            .scroll(&self.collection_name, Some(100), filter)
            .await?;
        let owned = points.iter().any(|p| match &p.id {
            Some(id) => match &id.point_id_options {
                Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => {
                    Uuid::parse_str(u)
                        .map(|uid| uid == memory_id)
                        .unwrap_or(false)
                }
                _ => false,
            },
            None => false,
        });

        if !owned {
            return Err(anyhow::anyhow!("Memory not found or not owned by user"));
        }

        self.vector_db
            .delete_points(&self.collection_name, vec![memory_id])
            .await?;
        Ok(())
    }

    pub async fn list_memories(
        &self,
        user_id: String,
        agent_id: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<SearchResult>> {
        let mut filter_map = serde_json::Map::new();
        filter_map.insert("user_id".to_string(), json!(user_id));
        if let Some(aid) = agent_id {
            filter_map.insert("agent_id".to_string(), json!(aid));
        }
        let filter = Some(Value::Object(filter_map));

        let points = self
            .vector_db
            .scroll(&self.collection_name, limit.or(Some(100)), filter)
            .await?;

        let mut results = Vec::new();
        for point in points {
            let id_str = match point.id {
                Some(id) => match id.point_id_options {
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => u,
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => n.to_string(),
                    None => String::new(),
                },
                None => String::new(),
            };
            let payload_map = qdrant_payload_to_map(point.payload);
            let content = payload_map
                .get("data")
                .or(payload_map.get("memory"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Ok(uid) = Uuid::parse_str(&id_str) {
                results.push(SearchResult {
                    id: uid,
                    content,
                    score: 1.0,
                    created_at: payload_map
                        .get("created_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    metadata: payload_map
                        .into_iter()
                        .filter(|(k, _)| k != "data" && k != "memory")
                        .collect(),
                });
            }
        }
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }

    async fn extract_facts(
        &self,
        messages: &[crate::providers::llm::MessagePart],
        agent_id: &Option<String>,
        input_text: String,
    ) -> Result<Vec<String>> {
        let enable_agent_memory =
            std::env::var("ENABLE_AGENT_MEMORY").unwrap_or_else(|_| "false".to_string()) == "true";
        let has_assistant = messages
            .iter()
            .any(|m| m.role.as_deref() == Some("assistant"));
        let run_agent_extraction = enable_agent_memory && agent_id.is_some() && has_assistant;

        let user_task =
            self._extract_single_type(user_memory_extraction_prompt(), input_text.clone());
        let agent_task = async move {
            if run_agent_extraction {
                self._extract_single_type(agent_memory_extraction_prompt(), input_text)
                    .await
            } else {
                vec![]
            }
        };

        let (mut user_facts, agent_facts) = futures::future::join(user_task, agent_task).await;
        user_facts.extend(agent_facts);
        Ok(user_facts)
    }

    async fn _extract_single_type(&self, sys: String, inp: String) -> Vec<String> {
        match self
            .call_llm(sys, format!("Input:\n{}", inp), "qwen-plus")
            .await
        {
            Ok(resp) => {
                let cleaned = clean_json_response(&resp);
                serde_json::from_str::<FactExtractionResult>(&cleaned)
                    .map(|r| r.facts)
                    .unwrap_or_default()
            }
            Err(_) => vec![],
        }
    }

    async fn retrieve_related_memories(
        &self,
        facts: &[String],
        user_id: &Option<String>,
        agent_id: &Option<String>,
    ) -> Result<(HashMap<String, Vec<f32>>, Vec<Value>)> {
        let mut filter_map = serde_json::Map::new();
        if let Some(uid) = user_id {
            filter_map.insert("user_id".to_string(), json!(uid));
        }
        if let Some(aid) = agent_id {
            filter_map.insert("agent_id".to_string(), json!(aid));
        }
        let search_filter = if filter_map.is_empty() {
            None
        } else {
            Some(Value::Object(filter_map))
        };

        let mut search_futures = Vec::new();
        for fact in facts {
            let ef = self.embedding_factory.clone();
            let vdb = self.vector_db.clone();
            let coll = self.collection_name.clone();
            let filt = search_filter.clone();
            let text = fact.clone();
            search_futures.push(async move {
                let vec = ef.get_provider("default").embed(&text).await?;
                let pts = vdb.search(&coll, vec.clone(), 5, Some(0.35), filt).await?;
                Ok::<(String, Vec<f32>, Vec<_>), anyhow::Error>((text, vec, pts))
            });
        }

        let mut embeddings_map = HashMap::new();
        let mut old_memories = Vec::new();
        for res in join_all(search_futures).await {
            if let Ok((text, vec, pts)) = res {
                embeddings_map.insert(text, vec);
                for pt in pts {
                    let p_map = qdrant_payload_to_map(pt.payload);
                    if let Some(txt) = p_map
                        .get("data")
                        .or(p_map.get("memory"))
                        .and_then(|v| v.as_str())
                    {
                        let id = match pt.id {
                            Some(id) => match id.point_id_options {
                                Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => u,
                                Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => {
                                    n.to_string()
                                }
                                None => continue,
                            },
                            None => continue,
                        };
                        old_memories.push(json!({ "id": id, "text": txt }));
                    }
                }
            }
        }
        Ok((embeddings_map, old_memories))
    }

    async fn resolve_memory_actions(
        &self,
        facts: &[String],
        old_memories: Vec<Value>,
    ) -> Result<Vec<MemoryOperation>> {
        let old_str = serde_json::to_string_pretty(&old_memories)?;
        let new_str = serde_json::to_string_pretty(facts)?;
        let prompt = get_update_memory_messages(&old_str, &new_str, None);
        let response = self.call_llm("".to_string(), prompt, "qwen-plus").await?;
        let cleaned = clean_json_response(&response);

        match serde_json::from_str::<MemoryUpdateResult>(&cleaned) {
            Ok(res) => Ok(res.memory),
            Err(_) => match serde_json::from_str::<Vec<MemoryOperation>>(&cleaned) {
                Ok(ops) => Ok(ops),
                Err(_) => Ok(vec![]),
            },
        }
    }

    async fn execute_operations(
        &self,
        actions: Vec<MemoryOperation>,
        embeddings: HashMap<String, Vec<f32>>,
        base_meta: HashMap<String, Value>,
    ) -> Result<()> {
        let mut upserts_data = Vec::new();
        let mut deletes = Vec::new();
        let now = Utc::now().naive_utc().to_string();
        let embedding_model = std::env::var("MEMORY_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-v2".to_string());

        for op in actions {
            match op.event {
                MemoryActionType::Add => {
                    if let Some(txt) = op.text {
                        let vec = if let Some(v) = embeddings.get(&txt) {
                            v.clone()
                        } else {
                            self.embedding_factory
                                .get_provider(&embedding_model)
                                .embed(&txt)
                                .await?
                        };
                        let mut p = base_meta.clone();
                        p.insert("data".to_string(), json!(txt));
                        p.insert("created_at".to_string(), json!(now));
                        let q_payload = p
                            .into_iter()
                            .map(|(k, v)| (k, json_to_qdrant_value(v)))
                            .collect();
                        upserts_data.push((Uuid::new_v4(), vec, q_payload));
                    }
                }
                MemoryActionType::Update => {
                    if let (Some(id), Some(txt)) = (op.id, op.text) {
                        if let Ok(uid) = Uuid::parse_str(&id) {
                            let vec = self
                                .embedding_factory
                                .get_provider(&embedding_model)
                                .embed(&txt)
                                .await?;
                            let mut p = base_meta.clone();
                            p.insert("data".to_string(), json!(txt));
                            p.insert("updated_at".to_string(), json!(now));
                            let q_payload = p
                                .into_iter()
                                .map(|(k, v)| (k, json_to_qdrant_value(v)))
                                .collect();
                            upserts_data.push((uid, vec, q_payload));
                        }
                    }
                }
                MemoryActionType::Delete => {
                    if let Some(id) = op.id {
                        if let Ok(uid) = Uuid::parse_str(&id) {
                            deletes.push(uid);
                        }
                    }
                }
                _ => {}
            }
        }
        if !upserts_data.is_empty() {
            self.vector_db
                .upsert_points(&self.collection_name, upserts_data)
                .await?;
        }
        if !deletes.is_empty() {
            self.vector_db
                .delete_points(&self.collection_name, deletes)
                .await?;
        }
        Ok(())
    }

    fn prepare_base_metadata(
        &self,
        user_id: Option<String>,
        agent_id: Option<String>,
        run_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> HashMap<String, Value> {
        let mut meta = metadata.unwrap_or_default();
        if let Some(id) = user_id {
            meta.insert("user_id".to_string(), json!(id));
        }
        if let Some(id) = agent_id {
            meta.insert("agent_id".to_string(), json!(id));
        }
        if let Some(id) = run_id {
            meta.insert("run_id".to_string(), json!(id));
        }
        meta
    }

    fn format_messages_for_llm(&self, messages: &[crate::providers::llm::MessagePart]) -> String {
        messages
            .iter()
            .map(|m| {
                format!(
                    "{}: {}",
                    m.role.as_deref().unwrap_or("unknown"),
                    m.content.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn call_llm(&self, system: String, user: String, model: &str) -> Result<String> {
        let (provider, actual_model) = self.providers.get_provider(model);
        let history = vec![crate::providers::llm::Message {
            role: "user".to_string(),
            parts: json!([{ "type": "text", "content": user }]),
            tool_calls: None,
            tool_call_id: None,
        }];
        let mut stream = provider
            .stream_chat(&actual_model, Some(system), history, None)
            .await?;
        let mut full = String::new();
        while let Some(Ok(chunk)) = futures::StreamExt::next(&mut stream).await {
            if let Some(c) = chunk.content {
                full.push_str(&c);
            }
        }
        Ok(full)
    }
}
