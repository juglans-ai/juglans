// src/providers/storage/postgres.rs
use super::StorageProvider;
use anyhow::Result;
use async_trait::async_trait;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde_json::Value;

/// PostgresStorage wraps a SeaORM `DatabaseConnection` and implements the StorageProvider trait.
#[derive(Clone)]
pub struct PostgresStorage {
    db: DatabaseConnection,
}

impl PostgresStorage {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

#[async_trait]
impl StorageProvider for PostgresStorage {
    async fn query_raw(&self, sql: &str) -> Result<Vec<Value>> {
        let results = self
            .db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                sql.to_string(),
            ))
            .await?;

        // Convert QueryResult rows to JSON (best-effort: returns row count info)
        let values: Vec<Value> = results
            .iter()
            .enumerate()
            .map(|(i, _)| serde_json::json!({ "row": i }))
            .collect();

        Ok(values)
    }

    async fn ping(&self) -> Result<()> {
        self.db
            .execute(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT 1".to_string(),
            ))
            .await?;
        Ok(())
    }

    fn connection(&self) -> &DatabaseConnection {
        &self.db
    }
}
