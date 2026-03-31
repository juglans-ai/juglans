// src/providers/embedding/mod.rs
pub mod openai;
pub mod qwen;

use anyhow::Result;
use async_trait::async_trait;
use std::env;
use std::sync::Arc; // 确保引入 env

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> u64;
}

#[derive(Clone)]
pub struct EmbeddingFactory {
    openai: Arc<openai::OpenAIEmbedding>,
    qwen: Arc<qwen::QwenEmbedding>,
}

impl EmbeddingFactory {
    pub fn new() -> Self {
        Self {
            openai: Arc::new(openai::OpenAIEmbedding::new()),
            qwen: Arc::new(qwen::QwenEmbedding::new()),
        }
    }

    pub fn get_provider(&self, model_name: &str) -> Arc<dyn EmbeddingProvider> {
        let m = model_name.to_lowercase();

        // 1. 显式匹配
        if m.contains("qwen") || m.contains("text-embedding") {
            return self.qwen.clone();
        }

        // 2. 如果是 "default" 或空，检查环境变量
        if m == "default" || m.is_empty() {
            let env_model = env::var("MEMORY_EMBEDDING_MODEL")
                .unwrap_or_default()
                .to_lowercase();
            if env_model.contains("qwen") || env_model.contains("text-embedding") {
                return self.qwen.clone();
            }
        }

        // 3. 默认回退到 OpenAI
        self.openai.clone()
    }
}
