// src/services/cache.rs
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};

#[derive(Clone)]
pub struct CacheService {
    conn: ConnectionManager,
}

impl CacheService {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    /// GET key → JSON deserialize. Returns None on miss or error.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn.get(key).await.ok()?;
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }

    /// SET key json_value (with optional TTL, 0 = no expiry)
    pub async fn set<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> Result<(), redis::RedisError> {
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(value).map_err(|e| {
            redis::RedisError::from((
                redis::ErrorKind::IoError,
                "JSON serialization failed",
                e.to_string(),
            ))
        })?;
        if ttl_secs == 0 {
            conn.set(key, json).await
        } else {
            conn.set_ex(key, json, ttl_secs).await
        }
    }

    /// MGET: get multiple values by keys
    pub async fn mget<T: DeserializeOwned>(&self, keys: &[String]) -> Vec<T> {
        if keys.is_empty() {
            return Vec::new();
        }
        let mut conn = self.conn.clone();
        let raw: Vec<Option<String>> = match conn.mget(keys).await {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        raw.into_iter()
            .filter_map(|opt| opt.and_then(|s| serde_json::from_str(&s).ok()))
            .collect()
    }

    /// DEL key
    pub async fn del(&self, key: &str) -> Result<(), redis::RedisError> {
        let mut conn = self.conn.clone();
        conn.del(key).await
    }

    /// SCAN + DEL all keys matching pattern
    pub async fn del_pattern(&self, pattern: &str) -> Result<(), redis::RedisError> {
        let keys = self.scan_keys(pattern).await;
        if !keys.is_empty() {
            let mut conn = self.conn.clone();
            let _: () = conn.del(&keys).await?;
        }
        Ok(())
    }

    /// SCAN keys matching pattern (for mutation rebuild)
    pub async fn scan_keys(&self, pattern: &str) -> Vec<String> {
        let mut conn = self.conn.clone();
        let mut keys = Vec::new();
        let mut cursor: u64 = 0;
        loop {
            let result: Result<(u64, Vec<String>), _> = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut conn)
                .await;
            match result {
                Ok((next_cursor, batch)) => {
                    keys.extend(batch);
                    if next_cursor == 0 {
                        break;
                    }
                    cursor = next_cursor;
                }
                Err(e) => {
                    tracing::warn!("Redis SCAN error: {}", e);
                    break;
                }
            }
        }
        keys
    }
}
