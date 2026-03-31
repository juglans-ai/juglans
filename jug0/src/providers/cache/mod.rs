// src/providers/cache/mod.rs
#[cfg(feature = "server")]
pub mod redis;

use async_trait::async_trait;

/// Trait for cache backends (e.g., Redis, Memcached, in-memory).
///
/// Note: Generic serialization/deserialization is handled at the caller level.
/// The trait operates on raw strings to remain object-safe (compatible with `dyn CacheProvider`).
#[async_trait]
pub trait CacheProvider: Send + Sync {
    /// Get a raw JSON string by key. Returns `None` on cache miss or error.
    async fn get_raw(&self, key: &str) -> Option<String>;

    /// Set a key to a raw JSON string value with optional TTL (0 = no expiry).
    async fn set_raw(&self, key: &str, value: &str, ttl_secs: u64) -> anyhow::Result<()>;

    /// Get multiple raw values by keys.
    async fn mget_raw(&self, keys: &[String]) -> Vec<Option<String>>;

    /// Delete a single key.
    async fn del(&self, key: &str) -> anyhow::Result<()>;

    /// Delete all keys matching a glob pattern (e.g., "prefix:*").
    async fn del_pattern(&self, pattern: &str) -> anyhow::Result<()>;

    /// Scan for keys matching a glob pattern.
    async fn scan_keys(&self, pattern: &str) -> Vec<String>;
}
