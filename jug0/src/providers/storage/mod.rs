// src/providers/storage/mod.rs
#[cfg(feature = "server")]
pub mod postgres;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// Trait for persistent storage backends (e.g., PostgreSQL, SQLite).
///
/// This provides a high-level abstraction over the database layer.
/// Implementations delegate to SeaORM or other ORMs internally.
#[async_trait]
pub trait StorageProvider: Send + Sync {
    /// Execute a raw SQL query and return results as JSON values.
    async fn query_raw(&self, sql: &str) -> Result<Vec<Value>>;

    /// Check if the database connection is healthy.
    async fn ping(&self) -> Result<()>;

    /// Get the underlying database connection (for SeaORM compatibility).
    /// Returns a reference to the DatabaseConnection.
    fn connection(&self) -> &sea_orm::DatabaseConnection;
}
