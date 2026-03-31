// src/services/vector_db.rs
use anyhow::Result;
use sea_orm::{DatabaseConnection, ConnectionTrait, Statement, DbBackend, FromQueryResult, Value as SeaValue};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone)]
pub struct VectorDbService {
    db: DatabaseConnection,
}

#[derive(Debug, FromQueryResult)]
pub struct VectorRow {
    pub id: Uuid,
    pub content: String,
    pub metadata: Value,
    pub similarity: f32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, FromQueryResult)]
struct VerificationRow {
    pub id: Uuid,
}

impl VectorDbService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// 初始化数据库架构
    pub async fn setup_schema(&self) -> Result<()> {
        tracing::info!("[VectorDB] Step 1: Ensuring pgvector extension exists...");
        self.db.execute(Statement::from_string(
            DbBackend::Postgres,
            "CREATE EXTENSION IF NOT EXISTS vector;".to_string(),
        )).await?;

        tracing::info!("[VectorDB] Step 2: Ensuring 'memories_vectors' table exists...");
        let create_table_sql = r#"
            CREATE TABLE IF NOT EXISTS memories_vectors (
                id UUID PRIMARY KEY,
                user_id VARCHAR(255),
                agent_id VARCHAR(255),
                run_id VARCHAR(255),
                content TEXT,
                embedding VECTOR(1536),
                metadata JSONB,
                created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
            );
        "#;
        self.db.execute(Statement::from_string(DbBackend::Postgres, create_table_sql.to_string())).await?;

        tracing::info!("[VectorDB] Step 3: Ensuring indexes exist...");
        let indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_memories_vectors_embedding ON memories_vectors USING hnsw (embedding vector_cosine_ops);",
            "CREATE INDEX IF NOT EXISTS idx_memories_vectors_user_id ON memories_vectors (user_id);",
            "CREATE INDEX IF NOT EXISTS idx_memories_vectors_agent_id ON memories_vectors (agent_id);",
        ];

        for sql in indexes {
            self.db.execute(Statement::from_string(DbBackend::Postgres, sql.to_string())).await?;
        }
        
        tracing::info!("[VectorDB] Schema setup completed.");
        Ok(())
    }

    /// 插入或更新向量点
    pub async fn upsert_point(
        &self,
        id: Uuid,
        vector: Vec<f32>,
        content: String,
        user_id: Option<String>,
        agent_id: Option<String>,
        run_id: Option<String>,
        metadata: Value,
    ) -> Result<()> {
        tracing::info!("[VectorDB] >>> Starting Upsert for ID: {}, Content Length: {}", id, content.len());

        // Step 1: 向量转换
        // pgvector 要求格式为 [0.1,0.2,0.3]
        let vector_str = format!("[{}]", vector.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
        tracing::debug!("[VectorDB] Generated vector string (start): {}", &vector_str[..50]);

        // Step 2: 构造 SQL
        // 注意：$6 必须显式转型为 ::vector
        let sql = r#"
            INSERT INTO memories_vectors (id, user_id, agent_id, run_id, content, embedding, metadata, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6::vector, $7, NOW())
            ON CONFLICT (id) DO UPDATE SET
                content = EXCLUDED.content,
                embedding = EXCLUDED.embedding,
                metadata = EXCLUDED.metadata,
                updated_at = NOW();
        "#;

        // Step 3: 参数绑定与执行
        tracing::info!("[VectorDB] Executing INSERT SQL...");
        let stmt = Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            vec![
                id.into(), 
                user_id.into(), 
                agent_id.into(), 
                run_id.into(), 
                content.clone().into(), 
                vector_str.into(), 
                metadata.into()
            ],
        );

        let exec_res = self.db.execute(stmt).await;
        
        match exec_res {
            Ok(res) => {
                tracing::info!("[VectorDB] SQL Execution OK. Rows affected: {}", res.rows_affected());
            },
            Err(e) => {
                tracing::error!("[VectorDB] SQL Execution FAILED: {:?}", e);
                return Err(e.into());
            }
        }

        // Step 4: 立即验证 (Verify)
        // 这一步非常重要，用来确认数据是否真的进入了当前数据库
        tracing::info!("[VectorDB] Verifying insertion for ID: {}...", id);
        let verify_stmt = Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT id FROM memories_vectors WHERE id = $1",
            vec![id.into()],
        );
        
        let found = VerificationRow::find_by_statement(verify_stmt).one(&self.db).await?;
        if found.is_some() {
            tracing::info!("[VectorDB] [SUCCESS] Verification passed: Point found in DB.");
        } else {
            tracing::error!("[VectorDB] [CRITICAL] Verification failed: Point NOT FOUND after successful INSERT.");
        }

        Ok(())
    }

    /// 向量搜索
    pub async fn search(
        &self,
        vector: Vec<f32>,
        limit: u64,
        threshold: f32,
        user_id: Option<String>,
        agent_id: Option<String>,
    ) -> Result<Vec<VectorRow>> {
        let vector_str = format!("[{}]", vector.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
        
        tracing::info!("[VectorDB] Searching memory (threshold: {}, user_id: {:?})", threshold, user_id);

        let mut sql = String::from(r#"
            SELECT id, content, metadata, created_at, 
                   (1 - (embedding <=> $1::vector)) as similarity
            FROM memories_vectors
            WHERE (1 - (embedding <=> $1::vector)) >= $2
        "#);

        let mut values = vec![vector_str.into(), threshold.into()];

        if let Some(uid) = user_id {
            values.push(uid.into());
            sql.push_str(&format!(" AND user_id = ${}", values.len()));
        }
        if let Some(aid) = agent_id {
            values.push(aid.into());
            sql.push_str(&format!(" AND agent_id = ${}", values.len()));
        }
        
        sql.push_str(&format!(" ORDER BY embedding <=> $1::vector ASC LIMIT ${}", values.len() + 1));
        values.push((limit as i64).into());

        let res = VectorRow::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            values,
        )).all(&self.db).await?;

        tracing::info!("[VectorDB] Search returned {} rows.", res.len());
        Ok(res)
    }

    /// 获取列表
    pub async fn list(
        &self,
        user_id: String,
        agent_id: Option<String>,
        limit: u32,
    ) -> Result<Vec<VectorRow>> {
        tracing::info!("[VectorDB] Listing memories for user: {}, agent: {:?}", user_id, agent_id);

        let mut sql = String::from(r#"
            SELECT id, content, metadata, created_at, 1.0 as similarity
            FROM memories_vectors
            WHERE user_id = $1
        "#);
        
        let mut values = vec![user_id.into()];

        if let Some(aid) = agent_id {
            values.push(aid.into());
            sql.push_str(" AND agent_id = $2");
        }
        
        sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ${}", values.len() + 1));
        values.push((limit as i64).into());

        let res = VectorRow::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            &sql,
            values,
        )).all(&self.db).await?;

        tracing::info!("[VectorDB] List returned {} rows.", res.len());
        Ok(res)
    }

    pub async fn delete_point(&self, id: Uuid) -> Result<()> {
        tracing::info!("[VectorDB] Deleting point: {}", id);
        self.db.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM memories_vectors WHERE id = $1",
            vec![id.into()],
        )).await?;
        Ok(())
    }
}