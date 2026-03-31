// src/providers/cache/redis.rs
use super::CacheProvider;
use crate::services::cache::CacheService;
use async_trait::async_trait;

/// RedisCache wraps the existing CacheService and implements the CacheProvider trait.
#[derive(Clone)]
pub struct RedisCache {
    inner: CacheService,
}

impl RedisCache {
    pub fn new(cache_service: CacheService) -> Self {
        Self {
            inner: cache_service,
        }
    }

    /// Access the underlying CacheService for backward compatibility.
    pub fn inner(&self) -> &CacheService {
        &self.inner
    }
}

#[async_trait]
impl CacheProvider for RedisCache {
    async fn get_raw(&self, key: &str) -> Option<String> {
        self.inner.get::<String>(key).await
    }

    async fn set_raw(&self, key: &str, value: &str, ttl_secs: u64) -> anyhow::Result<()> {
        // CacheService::set expects a Serialize type; &str implements Serialize.
        self.inner
            .set(key, &value.to_string(), ttl_secs)
            .await
            .map_err(|e| anyhow::anyhow!("Redis set error: {}", e))
    }

    async fn mget_raw(&self, keys: &[String]) -> Vec<Option<String>> {
        if keys.is_empty() {
            return Vec::new();
        }
        // CacheService::mget returns Vec<T> (only successful deserializations).
        // For the raw trait we need Option per key. We use get_raw per key as fallback.
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get_raw(key).await);
        }
        results
    }

    async fn del(&self, key: &str) -> anyhow::Result<()> {
        self.inner
            .del(key)
            .await
            .map_err(|e| anyhow::anyhow!("Redis del error: {}", e))
    }

    async fn del_pattern(&self, pattern: &str) -> anyhow::Result<()> {
        self.inner
            .del_pattern(pattern)
            .await
            .map_err(|e| anyhow::anyhow!("Redis del_pattern error: {}", e))
    }

    async fn scan_keys(&self, pattern: &str) -> Vec<String> {
        self.inner.scan_keys(pattern).await
    }
}
