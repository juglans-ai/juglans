// src/providers/embedding/openai.rs
use super::EmbeddingProvider;
use anyhow::Result;
use async_openai::{
    config::OpenAIConfig,
    types::{CreateEmbeddingRequestArgs, EncodingFormat},
    Client,
};
use async_trait::async_trait;

pub struct OpenAIEmbedding {
    client: Client<OpenAIConfig>,
    model: String,
}

impl OpenAIEmbedding {
    pub fn new() -> Self {
        let raw_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let config = OpenAIConfig::new().with_api_key(raw_key);
        Self {
            client: Client::with_config(config),
            model: "text-embedding-3-small".to_string(), // 1536 dims, 成本低
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbedding {
    fn dimension(&self) -> u64 {
        1536
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let request = CreateEmbeddingRequestArgs::default()
            .model(&self.model)
            .input(text)
            .encoding_format(EncodingFormat::Float)
            .build()?;

        let response = self.client.embeddings().create(request).await?;

        // 取第一个结果
        let embedding = response
            .data
            .first()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))?
            .embedding
            .clone();

        Ok(embedding)
    }

    async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let request = CreateEmbeddingRequestArgs::default()
            .model(&self.model)
            .input(texts)
            .encoding_format(EncodingFormat::Float)
            .build()?;

        let response = self.client.embeddings().create(request).await?;

        let embeddings = response.data.into_iter().map(|d| d.embedding).collect();

        Ok(embeddings)
    }
}
