// src/services/models.rs
use anyhow::Result;
use chrono::Utc;
use reqwest::Client;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::entities::{model_sync_log, models};

const MEMORY_CACHE_TTL_SECS: u64 = 300; // 5 minutes

/// Model info returned to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub name: Option<String>,
    pub owned_by: Option<String>,
    pub context_length: Option<i32>,
    pub capabilities: Option<serde_json::Value>,
    pub is_available: bool,
}

impl From<models::Model> for ModelInfo {
    fn from(m: models::Model) -> Self {
        Self {
            id: m.id,
            provider: m.provider,
            name: m.name,
            owned_by: m.owned_by,
            context_length: m.context_length,
            capabilities: m.capabilities,
            is_available: m.is_available,
        }
    }
}

/// Provider sync status
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub name: String,
    pub is_available: bool,
    pub model_count: i32,
    pub last_synced: Option<chrono::DateTime<Utc>>,
}

/// Sync report after syncing all providers
#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub providers: Vec<ProviderSyncResult>,
    pub total_models: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSyncResult {
    pub provider: String,
    pub status: String,
    pub model_count: i32,
    pub error: Option<String>,
}

/// Memory cache entry
struct MemoryCache {
    data: Vec<ModelInfo>,
    expires_at: Instant,
}

#[derive(Clone)]
pub struct ModelsService {
    db: DatabaseConnection,
    http_client: Client,
    openai_key: Option<String>,
    deepseek_key: Option<String>,
    gemini_key: Option<String>,
    qwen_key: Option<String>,
    byteplus_key: Option<String>,
    xai_key: Option<String>,
    memory_cache: Arc<RwLock<Option<MemoryCache>>>,
}

impl ModelsService {
    pub fn new(
        db: DatabaseConnection,
        openai_key: Option<String>,
        deepseek_key: Option<String>,
        gemini_key: Option<String>,
        qwen_key: Option<String>,
        byteplus_key: Option<String>,
        xai_key: Option<String>,
    ) -> Self {
        Self {
            db,
            http_client: Client::new(),
            openai_key,
            deepseek_key,
            gemini_key,
            qwen_key,
            byteplus_key,
            xai_key,
            memory_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Get models with optional provider filter (uses cache)
    pub async fn get_models(&self, provider: Option<&str>) -> Result<Vec<ModelInfo>> {
        // Check memory cache first
        {
            let cache = self.memory_cache.read().await;
            if let Some(ref c) = *cache {
                if c.expires_at > Instant::now() {
                    let models = if let Some(p) = provider {
                        c.data.iter().filter(|m| m.provider == p).cloned().collect()
                    } else {
                        c.data.clone()
                    };
                    return Ok(models);
                }
            }
        }

        // Load from database
        let mut db_models = self.load_from_db(provider).await?;

        // Append Claude Code static models (not in DB — uses Max subscription via CLI)
        if provider.is_none() || provider == Some("claude-code") {
            db_models.extend(claude_code_models());
        }

        // Update memory cache (only if loading all)
        if provider.is_none() {
            let mut cache = self.memory_cache.write().await;
            *cache = Some(MemoryCache {
                data: db_models.clone(),
                expires_at: Instant::now() + Duration::from_secs(MEMORY_CACHE_TTL_SECS),
            });
        }

        Ok(db_models)
    }

    /// Force refresh from all providers
    pub async fn refresh(&self) -> Result<SyncReport> {
        let report = self.sync_all_providers().await?;

        // Clear memory cache
        let mut cache = self.memory_cache.write().await;
        *cache = None;

        Ok(report)
    }

    /// Load models from database
    async fn load_from_db(&self, provider: Option<&str>) -> Result<Vec<ModelInfo>> {
        let query = models::Entity::find()
            .filter(models::Column::IsAvailable.eq(true))
            .order_by_asc(models::Column::Provider)
            .order_by_asc(models::Column::Id);

        let db_models = if let Some(p) = provider {
            query
                .filter(models::Column::Provider.eq(p))
                .all(&self.db)
                .await?
        } else {
            query.all(&self.db).await?
        };

        Ok(db_models.into_iter().map(ModelInfo::from).collect())
    }

    /// Sync all providers to database
    pub async fn sync_all_providers(&self) -> Result<SyncReport> {
        let results = tokio::join!(
            self.sync_provider("openai"),
            self.sync_provider("deepseek"),
            self.sync_provider("gemini"),
            self.sync_provider("qwen"),
            self.sync_provider("byteplus"),
            self.sync_provider("xai"),
        );

        let provider_results = vec![
            self.build_sync_result("openai", results.0),
            self.build_sync_result("deepseek", results.1),
            self.build_sync_result("gemini", results.2),
            self.build_sync_result("qwen", results.3),
            self.build_sync_result("byteplus", results.4),
            self.build_sync_result("xai", results.5),
        ];

        let total = provider_results.iter().map(|r| r.model_count).sum();

        Ok(SyncReport {
            providers: provider_results,
            total_models: total,
        })
    }

    fn build_sync_result(&self, provider: &str, result: Result<usize>) -> ProviderSyncResult {
        match result {
            Ok(count) => ProviderSyncResult {
                provider: provider.to_string(),
                status: "success".to_string(),
                model_count: count as i32,
                error: None,
            },
            Err(e) => ProviderSyncResult {
                provider: provider.to_string(),
                status: "failed".to_string(),
                model_count: 0,
                error: Some(e.to_string()),
            },
        }
    }

    /// Sync a specific provider
    async fn sync_provider(&self, provider: &str) -> Result<usize> {
        let models = match provider {
            "openai" => self.fetch_openai_models().await,
            "deepseek" => self.fetch_deepseek_models().await,
            "gemini" => self.fetch_gemini_models().await,
            "qwen" => self.fetch_qwen_models().await,
            "byteplus" => self.fetch_byteplus_models().await,
            "xai" => self.fetch_xai_models().await,
            _ => return Err(anyhow::anyhow!("Unknown provider: {}", provider)),
        };

        match models {
            Ok(models_list) => {
                let count = models_list.len();

                // Upsert models
                for model in models_list {
                    self.upsert_model(model).await?;
                }

                // Log success
                self.log_sync(provider, "success", count as i32, None)
                    .await?;

                Ok(count)
            }
            Err(e) => {
                // Log failure
                self.log_sync(provider, "failed", 0, Some(e.to_string()))
                    .await?;
                Err(e)
            }
        }
    }

    async fn upsert_model(&self, model: models::ActiveModel) -> Result<()> {
        // Try to find existing
        let id = match &model.id {
            sea_orm::ActiveValue::Set(id) => id.clone(),
            _ => return Err(anyhow::anyhow!("Model ID not set")),
        };

        let existing = models::Entity::find_by_id(&id).one(&self.db).await?;

        if existing.is_some() {
            // Update
            model.update(&self.db).await?;
        } else {
            // Insert
            model.insert(&self.db).await?;
        }

        Ok(())
    }

    async fn log_sync(
        &self,
        provider: &str,
        status: &str,
        model_count: i32,
        error: Option<String>,
    ) -> Result<()> {
        let log = model_sync_log::ActiveModel {
            id: Set(Uuid::new_v4()),
            provider: Set(provider.to_string()),
            status: Set(status.to_string()),
            model_count: Set(Some(model_count)),
            error_message: Set(error),
            synced_at: Set(Some(Utc::now().naive_utc())),
        };
        log.insert(&self.db).await?;
        Ok(())
    }

    // ========== Provider-specific fetchers ==========

    async fn fetch_openai_models(&self) -> Result<Vec<models::ActiveModel>> {
        let api_key = self
            .openai_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OPENAI_API_KEY not configured"))?;

        let api_base = std::env::var("OPENAI_API_BASE")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let resp = self
            .http_client
            .get(format!("{}/models", api_base))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            return Err(anyhow::anyhow!("OpenAI API error: {}", err));
        }

        #[derive(Deserialize)]
        struct OpenAIModelsResponse {
            data: Vec<OpenAIModel>,
        }

        #[derive(Deserialize)]
        struct OpenAIModel {
            id: String,
            owned_by: Option<String>,
        }

        let data: OpenAIModelsResponse = resp.json().await?;
        let now = Utc::now().naive_utc();

        // Filter to chat models only
        let chat_models: Vec<_> = data
            .data
            .into_iter()
            .filter(|m| {
                m.id.starts_with("gpt-") || m.id.starts_with("o1") || m.id.starts_with("chatgpt")
            })
            .collect();

        Ok(chat_models
            .into_iter()
            .map(|m| {
                let prefixed_id = format!("openai/{}", m.id);
                models::ActiveModel {
                    id: Set(prefixed_id),
                    provider: Set("openai".to_string()),
                    name: Set(Some(m.id.clone())),
                    owned_by: Set(m.owned_by),
                    context_length: Set(None),
                    capabilities: Set(Some(json!({"chat": true}))),
                    pricing: Set(None),
                    raw_data: Set(None),
                    is_available: Set(true),
                    created_at: Set(Some(now)),
                    updated_at: Set(Some(now)),
                }
            })
            .collect())
    }

    async fn fetch_deepseek_models(&self) -> Result<Vec<models::ActiveModel>> {
        let api_key = self
            .deepseek_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("DEEPSEEK_API_KEY not configured"))?;

        let resp = self
            .http_client
            .get("https://api.deepseek.com/models")
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            return Err(anyhow::anyhow!("DeepSeek API error: {}", err));
        }

        #[derive(Deserialize)]
        struct DeepSeekModelsResponse {
            data: Vec<DeepSeekModel>,
        }

        #[derive(Deserialize)]
        struct DeepSeekModel {
            id: String,
            owned_by: Option<String>,
        }

        let data: DeepSeekModelsResponse = resp.json().await?;
        let now = Utc::now().naive_utc();

        Ok(data
            .data
            .into_iter()
            .map(|m| {
                let prefixed_id = format!("deepseek/{}", m.id);
                models::ActiveModel {
                    id: Set(prefixed_id),
                    provider: Set("deepseek".to_string()),
                    name: Set(Some(m.id.clone())),
                    owned_by: Set(m.owned_by),
                    context_length: Set(None),
                    capabilities: Set(Some(json!({"chat": true}))),
                    pricing: Set(None),
                    raw_data: Set(None),
                    is_available: Set(true),
                    created_at: Set(Some(now)),
                    updated_at: Set(Some(now)),
                }
            })
            .collect())
    }

    async fn fetch_gemini_models(&self) -> Result<Vec<models::ActiveModel>> {
        let api_key = self
            .gemini_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GEMINI_API_KEY not configured"))?;

        let resp = self
            .http_client
            .get(format!(
                "https://generativelanguage.googleapis.com/v1beta/models?key={}",
                api_key
            ))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            return Err(anyhow::anyhow!("Gemini API error: {}", err));
        }

        #[derive(Deserialize)]
        struct GeminiModelsResponse {
            models: Vec<GeminiModel>,
        }

        #[derive(Deserialize)]
        struct GeminiModel {
            name: String, // "models/gemini-pro"
            #[serde(rename = "displayName")]
            display_name: Option<String>,
            #[serde(rename = "inputTokenLimit")]
            input_token_limit: Option<i32>,
            #[serde(rename = "supportedGenerationMethods")]
            supported_methods: Option<Vec<String>>,
        }

        let data: GeminiModelsResponse = resp.json().await?;
        let now = Utc::now().naive_utc();

        // Filter to chat models
        let chat_models: Vec<_> = data
            .models
            .into_iter()
            .filter(|m| {
                m.supported_methods
                    .as_ref()
                    .map(|methods| {
                        methods
                            .iter()
                            .any(|method| method.contains("generateContent"))
                    })
                    .unwrap_or(false)
            })
            .collect();

        Ok(chat_models
            .into_iter()
            .map(|m| {
                // Extract model ID from "models/gemini-pro" -> "gemini-pro"
                let id = m
                    .name
                    .strip_prefix("models/")
                    .unwrap_or(&m.name)
                    .to_string();

                let prefixed_id = format!("gemini/{}", id);
                models::ActiveModel {
                    id: Set(prefixed_id),
                    provider: Set("gemini".to_string()),
                    name: Set(m.display_name.or(Some(id))),
                    owned_by: Set(Some("google".to_string())),
                    context_length: Set(m.input_token_limit),
                    capabilities: Set(Some(json!({"chat": true}))),
                    pricing: Set(None),
                    raw_data: Set(None),
                    is_available: Set(true),
                    created_at: Set(Some(now)),
                    updated_at: Set(Some(now)),
                }
            })
            .collect())
    }

    async fn fetch_qwen_models(&self) -> Result<Vec<models::ActiveModel>> {
        let api_key = self
            .qwen_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("QWEN_API_KEY not configured"))?;

        // Qwen uses OpenAI-compatible mode for model listing
        let resp = self
            .http_client
            .get("https://dashscope.aliyuncs.com/compatible-mode/v1/models")
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            return Err(anyhow::anyhow!("Qwen API error: {}", err));
        }

        #[derive(Deserialize)]
        struct QwenModelsResponse {
            data: Vec<QwenModel>,
        }

        #[derive(Deserialize)]
        struct QwenModel {
            id: String,
            owned_by: Option<String>,
        }

        let data: QwenModelsResponse = resp.json().await?;
        let now = Utc::now().naive_utc();

        Ok(data
            .data
            .into_iter()
            .map(|m| {
                let prefixed_id = format!("qwen/{}", m.id);
                models::ActiveModel {
                    id: Set(prefixed_id),
                    provider: Set("qwen".to_string()),
                    name: Set(Some(m.id.clone())),
                    owned_by: Set(m.owned_by.or(Some("alibaba".to_string()))),
                    context_length: Set(None),
                    capabilities: Set(Some(json!({"chat": true}))),
                    pricing: Set(None),
                    raw_data: Set(None),
                    is_available: Set(true),
                    created_at: Set(Some(now)),
                    updated_at: Set(Some(now)),
                }
            })
            .collect())
    }

    async fn fetch_byteplus_models(&self) -> Result<Vec<models::ActiveModel>> {
        let api_key = self
            .byteplus_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ARK_API_KEY not configured"))?;

        let api_base = std::env::var("ARK_API_BASE")
            .unwrap_or_else(|_| "https://ark.ap-southeast.bytepluses.com/api/v3".to_string());

        let resp = self
            .http_client
            .get(format!("{}/models", api_base))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            return Err(anyhow::anyhow!("BytePlus API error: {}", err));
        }

        #[derive(Deserialize)]
        struct BytePlusModelsResponse {
            data: Vec<BytePlusModel>,
        }

        #[derive(Deserialize)]
        struct BytePlusModel {
            id: String,
            owned_by: Option<String>,
        }

        let data: BytePlusModelsResponse = resp.json().await?;
        let now = Utc::now().naive_utc();

        Ok(data
            .data
            .into_iter()
            .map(|m| {
                let prefixed_id = format!("byteplus/{}", m.id);
                models::ActiveModel {
                    id: Set(prefixed_id),
                    provider: Set("byteplus".to_string()),
                    name: Set(Some(m.id.clone())),
                    owned_by: Set(m.owned_by.or(Some("bytedance".to_string()))),
                    context_length: Set(None),
                    capabilities: Set(Some(json!({"chat": true}))),
                    pricing: Set(None),
                    raw_data: Set(None),
                    is_available: Set(true),
                    created_at: Set(Some(now)),
                    updated_at: Set(Some(now)),
                }
            })
            .collect())
    }

    async fn fetch_xai_models(&self) -> Result<Vec<models::ActiveModel>> {
        let api_key = self
            .xai_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("XAI_API_KEY not configured"))?;

        let resp = self
            .http_client
            .get("https://api.x.ai/v1/models")
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            return Err(anyhow::anyhow!("xAI API error: {}", err));
        }

        #[derive(Deserialize)]
        struct XaiModelsResponse {
            data: Vec<XaiModel>,
        }

        #[derive(Deserialize)]
        struct XaiModel {
            id: String,
            owned_by: Option<String>,
        }

        let data: XaiModelsResponse = resp.json().await?;
        let now = Utc::now().naive_utc();

        Ok(data
            .data
            .into_iter()
            .map(|m| {
                let prefixed_id = format!("xai/{}", m.id);
                models::ActiveModel {
                    id: Set(prefixed_id),
                    provider: Set("xai".to_string()),
                    name: Set(Some(m.id.clone())),
                    owned_by: Set(m.owned_by.or(Some("xai".to_string()))),
                    context_length: Set(None),
                    capabilities: Set(Some(json!({"chat": true}))),
                    pricing: Set(None),
                    raw_data: Set(None),
                    is_available: Set(true),
                    created_at: Set(Some(now)),
                    updated_at: Set(Some(now)),
                }
            })
            .collect())
    }

    /// Get provider status summary
    pub async fn get_provider_status(&self) -> Result<Vec<ProviderStatus>> {
        let mut statuses = Vec::new();

        for provider in &["openai", "deepseek", "gemini", "qwen", "byteplus", "xai"] {
            // Count models
            let count = models::Entity::find()
                .filter(models::Column::Provider.eq(*provider))
                .filter(models::Column::IsAvailable.eq(true))
                .count(&self.db)
                .await? as i32;

            // Get last sync
            let last_sync = model_sync_log::Entity::find()
                .filter(model_sync_log::Column::Provider.eq(*provider))
                .filter(model_sync_log::Column::Status.eq("success"))
                .order_by_desc(model_sync_log::Column::SyncedAt)
                .one(&self.db)
                .await?;

            let last_synced = last_sync
                .and_then(|s| s.synced_at)
                .map(|dt| chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));

            statuses.push(ProviderStatus {
                name: provider.to_string(),
                is_available: count > 0,
                model_count: count,
                last_synced,
            });
        }

        Ok(statuses)
    }
}

/// Static Claude Code models (uses Max subscription via CLI, not API)
fn claude_code_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-code/sonnet".to_string(),
            provider: "claude-code".to_string(),
            name: Some("Claude Sonnet 4.6 (Code)".to_string()),
            owned_by: Some("anthropic".to_string()),
            context_length: Some(200000),
            capabilities: Some(json!({"tools": true, "streaming": true, "max_subscription": true})),
            is_available: true,
        },
        ModelInfo {
            id: "claude-code/opus".to_string(),
            provider: "claude-code".to_string(),
            name: Some("Claude Opus 4.6 (Code)".to_string()),
            owned_by: Some("anthropic".to_string()),
            context_length: Some(200000),
            capabilities: Some(json!({"tools": true, "streaming": true, "max_subscription": true})),
            is_available: true,
        },
        ModelInfo {
            id: "claude-code/haiku".to_string(),
            provider: "claude-code".to_string(),
            name: Some("Claude Haiku 4.5 (Code)".to_string()),
            owned_by: Some("anthropic".to_string()),
            context_length: Some(200000),
            capabilities: Some(json!({"tools": true, "streaming": true, "max_subscription": true})),
            is_available: true,
        },
    ]
}
