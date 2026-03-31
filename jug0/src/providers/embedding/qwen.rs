// src/providers/embedding/qwen.rs
use super::EmbeddingProvider;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

pub struct QwenEmbedding {
    client: Client,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct QwenEmbeddingRequest {
    model: String,
    input: QwenEmbeddingInput,
}

#[derive(Serialize)]
struct QwenEmbeddingInput {
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct QwenEmbeddingResponse {
    output: QwenEmbeddingOutput,
    // usage: Value,
    // request_id: String,
}

#[derive(Deserialize)]
struct QwenEmbeddingOutput {
    embeddings: Vec<QwenEmbeddingItem>,
}

#[derive(Deserialize)]
struct QwenEmbeddingItem {
    embedding: Vec<f32>,
    text_index: usize,
}

impl QwenEmbedding {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_key: env::var("DASHSCOPE_API_KEY").unwrap_or_default(),
            model: env::var("MEMORY_EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-v2".to_string()),
        }
    }
}

#[async_trait]
impl EmbeddingProvider for QwenEmbedding {
    fn dimension(&self) -> u64 {
        // v3 可以指定维度，但 v1/v2 默认是 1536，v3 默认也是 1024 或 1536 取决于参数
        // 这里暂时硬编码，如果用 v3 且需要其他维度，需修改 MemoryService 的配置
        1536
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let res = self.embed_batch(vec![text.to_string()]).await?;
        res.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let url = "https://dashscope.aliyuncs.com/api/v1/services/embeddings/text-embedding/text-embedding";

        tracing::debug!(
            "Qwen Embedding: Requesting batch of size {} using model {}",
            texts.len(),
            self.model
        );

        // DashScope API 限制 batch size ≤ 10，需要分批请求
        const MAX_BATCH_SIZE: usize = 10;
        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(MAX_BATCH_SIZE) {
            let request = QwenEmbeddingRequest {
                model: self.model.clone(),
                input: QwenEmbeddingInput {
                    texts: chunk.to_vec(),
                },
            };

            let response = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await?;

            if !response.status().is_success() {
                let err = response.text().await?;
                tracing::error!("Qwen Embedding API Error: {}", err);
                return Err(anyhow::anyhow!("DashScope Embedding Error: {}", err));
            }

            let body: QwenEmbeddingResponse = response.json().await?;

            // 确保结果按输入顺序排序（DashScope 可能会乱序返回，通过 text_index 排序）
            let mut sorted_res = body.output.embeddings;
            sorted_res.sort_by_key(|e| e.text_index);

            all_embeddings.extend(sorted_res.into_iter().map(|e| e.embedding));
        }

        tracing::debug!(
            "Qwen Embedding: Success ({} embeddings)",
            all_embeddings.len()
        );

        Ok(all_embeddings)
    }
}
