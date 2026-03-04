// src/builtins/database.rs
//
// ORM builtin tools for juglans — multi-database support via sqlx Any driver.
// Supported backends: PostgreSQL, MySQL/MariaDB, SQLite.
// Tools: db.connect, db.disconnect, db.find, db.find_one, db.create, db.create_many,
//        db.upsert, db.update, db.delete, db.count, db.aggregate, db.query, db.exec,
//        db.begin, db.commit, db.rollback, db.create_table, db.drop_table,
//        db.alter_table, db.tables, db.columns

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::{json, Value};
use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::pool::PoolConnection;
use sqlx::{Any, AnyPool, Column, Row, TypeInfo};
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Backend type detection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum BackendType {
    Postgres,
    MySQL,
    SQLite,
}

fn detect_backend(url: &str) -> Result<BackendType> {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(BackendType::Postgres)
    } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
        Ok(BackendType::MySQL)
    } else if url.starts_with("sqlite://") || url.starts_with("sqlite:") {
        Ok(BackendType::SQLite)
    } else {
        Err(anyhow!(
            "Unsupported database URL scheme. Use postgres://, mysql://, or sqlite://"
        ))
    }
}

fn get_backend(alias: &str) -> BackendType {
    BACKENDS
        .get(alias)
        .map(|r| *r.value())
        .unwrap_or(BackendType::Postgres)
}

// ---------------------------------------------------------------------------
// Global connection pool manager
// ---------------------------------------------------------------------------

static POOLS: LazyLock<DashMap<String, AnyPool>> = LazyLock::new(DashMap::new);

/// Backend type per pool alias.
static BACKENDS: LazyLock<DashMap<String, BackendType>> = LazyLock::new(DashMap::new);

/// Active transaction connections keyed by pool alias.
static TRANSACTIONS: LazyLock<DashMap<String, tokio::sync::Mutex<PoolConnection<Any>>>> =
    LazyLock::new(DashMap::new);

/// Whether sqlx any drivers have been installed.
static DRIVERS_INSTALLED: std::sync::Once = std::sync::Once::new();

fn get_pool(alias: &str) -> Result<AnyPool> {
    POOLS.get(alias).map(|r| r.value().clone()).ok_or_else(|| {
        if alias == "default" {
            anyhow!("Database not connected. Call db.connect(url=...) first.")
        } else {
            anyhow!("Database pool '{}' not found.", alias)
        }
    })
}

fn pool_alias(params: &HashMap<String, String>) -> String {
    params
        .get("pool")
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

fn has_transaction(alias: &str) -> bool {
    TRANSACTIONS.contains_key(alias)
}

/// Get a mutable AnyConnection reference from a PoolConnection via DerefMut.
fn any_conn(conn: &mut PoolConnection<Any>) -> &mut sqlx::AnyConnection {
    conn.deref_mut()
}

/// Execute a query on either the active transaction connection or the pool.
async fn exec_query(alias: &str, sql: &str, bind_vals: &[Value]) -> Result<Vec<AnyRow>> {
    if let Some(tx_conn) = TRANSACTIONS.get(alias) {
        let mut guard = tx_conn.lock().await;
        let conn = any_conn(&mut guard);
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.fetch_all(conn).await?)
    } else {
        let pool = get_pool(alias)?;
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.fetch_all(&pool).await?)
    }
}

/// Execute a statement (INSERT/UPDATE/DELETE/DDL) on either transaction or pool.
async fn exec_statement(
    alias: &str,
    sql: &str,
    bind_vals: &[Value],
) -> Result<sqlx::any::AnyQueryResult> {
    if let Some(tx_conn) = TRANSACTIONS.get(alias) {
        let mut guard = tx_conn.lock().await;
        let conn = any_conn(&mut guard);
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.execute(conn).await?)
    } else {
        let pool = get_pool(alias)?;
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.execute(&pool).await?)
    }
}

/// Fetch one optional row on either transaction or pool.
async fn exec_fetch_optional(
    alias: &str,
    sql: &str,
    bind_vals: &[Value],
) -> Result<Option<AnyRow>> {
    if let Some(tx_conn) = TRANSACTIONS.get(alias) {
        let mut guard = tx_conn.lock().await;
        let conn = any_conn(&mut guard);
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.fetch_optional(conn).await?)
    } else {
        let pool = get_pool(alias)?;
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.fetch_optional(&pool).await?)
    }
}

/// Fetch one row on either transaction or pool.
async fn exec_fetch_one(alias: &str, sql: &str, bind_vals: &[Value]) -> Result<AnyRow> {
    if let Some(tx_conn) = TRANSACTIONS.get(alias) {
        let mut guard = tx_conn.lock().await;
        let conn = any_conn(&mut guard);
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.fetch_one(conn).await?)
    } else {
        let pool = get_pool(alias)?;
        let query = sqlx::query(sql);
        let query = bind_values(query, bind_vals);
        Ok(query.fetch_one(&pool).await?)
    }
}

// ---------------------------------------------------------------------------
// Where clause builder: JSON → SQL
// ---------------------------------------------------------------------------

/// Parse a JSON where clause into a parameterized SQL WHERE string.
///
/// Supports:
///   {"age": 18}              → age = $1
///   {"age >": 18}            → age > $1
///   {"name like": "%ali%"}   → name LIKE $1
///   {"status in": ["a","b"]} → status IN ($1,$2)
///   {"id": null}             → id IS NULL
///   {"id !=": null}          → id IS NOT NULL
///   {"$or": [{...}, {...}]}  → (... OR ...)
fn build_where(where_json: &Value, param_offset: usize) -> (String, Vec<Value>) {
    let (clauses, bind_values, _) = build_conditions(where_json, param_offset);

    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (sql, bind_values)
}

/// Build SQL condition clauses from a JSON object, returning (clauses, bind_values, next_idx).
fn build_conditions(where_json: &Value, param_offset: usize) -> (Vec<String>, Vec<Value>, usize) {
    let mut clauses = Vec::new();
    let mut bind_values = Vec::new();
    let mut idx = param_offset;

    if let Some(obj) = where_json.as_object() {
        for (key, value) in obj {
            // Handle $or
            if key == "$or" {
                if let Some(arr) = value.as_array() {
                    let mut or_parts = Vec::new();
                    for item in arr {
                        let (sub_clauses, sub_binds, new_idx) = build_conditions(item, idx);
                        idx = new_idx;
                        bind_values.extend(sub_binds);
                        if !sub_clauses.is_empty() {
                            or_parts.push(sub_clauses.join(" AND "));
                        }
                    }
                    if !or_parts.is_empty() {
                        clauses.push(format!("({})", or_parts.join(" OR ")));
                    }
                }
                continue;
            }

            let (column, op) = parse_column_op(key);

            if value.is_null() {
                if op == "!=" || op == "<>" {
                    clauses.push(format!("\"{}\" IS NOT NULL", column));
                } else {
                    clauses.push(format!("\"{}\" IS NULL", column));
                }
                continue;
            }

            if op.eq_ignore_ascii_case("in") {
                if let Some(arr) = value.as_array() {
                    let placeholders: Vec<String> = arr
                        .iter()
                        .map(|v| {
                            idx += 1;
                            bind_values.push(v.clone());
                            format!("${}", idx)
                        })
                        .collect();
                    clauses.push(format!("\"{}\" IN ({})", column, placeholders.join(", ")));
                }
                continue;
            }

            idx += 1;
            bind_values.push(value.clone());
            let sql_op = match op.as_str() {
                "=" | "" => "=",
                ">" => ">",
                ">=" => ">=",
                "<" => "<",
                "<=" => "<=",
                "!=" | "<>" => "!=",
                "like" | "LIKE" => "LIKE",
                "ilike" | "ILIKE" => "LIKE", // SQLite/MySQL don't have ILIKE; PG handles LIKE case-sensitively
                other => other,
            };
            clauses.push(format!("\"{}\" {} ${}", column, sql_op, idx));
        }
    }

    (clauses, bind_values, idx)
}

/// Parse "column_name op" into (column, op).
fn parse_column_op(key: &str) -> (String, String) {
    let ops = [
        ">=", "<=", "!=", "<>", ">", "<", "ilike", "ILIKE", "like", "LIKE", "in", "IN",
    ];
    for op in ops {
        if let Some(col) = key.strip_suffix(&format!(" {}", op)) {
            return (col.trim().to_string(), op.to_string());
        }
    }
    (key.trim().to_string(), "=".to_string())
}

/// Bind JSON values to a sqlx query (Any driver).
fn bind_values<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    values: &'q [Value],
) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
    for v in values {
        query = match v {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    query.bind(i)
                } else if let Some(f) = n.as_f64() {
                    query.bind(f)
                } else {
                    query.bind(n.to_string())
                }
            }
            Value::String(s) => query.bind(s.as_str()),
            Value::Bool(b) => query.bind(*b),
            Value::Null => query.bind(None::<String>),
            other => query.bind(other.to_string()),
        };
    }
    query
}

/// Convert an AnyRow to a JSON object.
/// Handles type names from PostgreSQL, MySQL, and SQLite.
fn row_to_json(row: &AnyRow) -> Value {
    let mut obj = serde_json::Map::new();
    for col in row.columns() {
        let name = col.name();
        let type_name = col.type_info().name().to_uppercase();
        let val: Value = match type_name.as_str() {
            // Integer types
            "INT2" | "SMALLINT" | "SMALLSERIAL" | "TINYINT" => row
                .try_get::<i32, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
            "INT4" | "INT" | "INTEGER" | "SERIAL" | "MEDIUMINT" => row
                .try_get::<i32, _>(name)
                .or_else(|_| row.try_get::<i64, _>(name).map(|v| v as i32))
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
            "INT8" | "BIGINT" | "BIGSERIAL" => row
                .try_get::<i64, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // Float types
            "FLOAT4" | "REAL" | "FLOAT" => row
                .try_get::<f64, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
            "FLOAT8" | "DOUBLE PRECISION" | "DOUBLE" => row
                .try_get::<f64, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // Boolean
            "BOOL" | "BOOLEAN" => row
                .try_get::<bool, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // JSON (PostgreSQL JSONB, MySQL JSON)
            "JSON" | "JSONB" => row
                .try_get::<String, _>(name)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or(Value::Null),

            // Text / VARCHAR / CHAR
            "TEXT" | "VARCHAR" | "CHAR" | "CHARACTER VARYING" | "NVARCHAR" | "LONGTEXT"
            | "MEDIUMTEXT" | "TINYTEXT" | "ENUM" | "SET" => row
                .try_get::<String, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // Date/Time — try as string (works across all backends via Any)
            "TIMESTAMP"
            | "TIMESTAMPTZ"
            | "TIMESTAMP WITHOUT TIME ZONE"
            | "TIMESTAMP WITH TIME ZONE"
            | "DATETIME" => row
                .try_get::<String, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
            "DATE" => row
                .try_get::<String, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
            "TIME" | "TIMETZ" => row
                .try_get::<String, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // Numeric / Decimal
            "NUMERIC" | "DECIMAL" => row
                .try_get::<String, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // UUID (PostgreSQL native, others store as text)
            "UUID" => row
                .try_get::<String, _>(name)
                .map(|v| json!(v))
                .unwrap_or(Value::Null),

            // Binary
            "BYTEA" | "BLOB" | "BINARY" | "VARBINARY" | "LONGBLOB" | "MEDIUMBLOB" | "TINYBLOB" => {
                row.try_get::<Vec<u8>, _>(name)
                    .map(|v| {
                        json!(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &v
                        ))
                    })
                    .unwrap_or(Value::Null)
            }

            // NULL type (SQLite)
            "NULL" => Value::Null,

            // Default: try integer first (SQLite often returns INTEGER for everything),
            // then float, then string
            _ => row
                .try_get::<i64, _>(name)
                .map(|v| json!(v))
                .or_else(|_| row.try_get::<f64, _>(name).map(|v| json!(v)))
                .or_else(|_| row.try_get::<String, _>(name).map(|v| json!(v)))
                .unwrap_or(Value::Null),
        };
        obj.insert(name.to_string(), val);
    }
    Value::Object(obj)
}

// ---------------------------------------------------------------------------
// db.connect
// ---------------------------------------------------------------------------

pub struct DbConnect;

#[async_trait]
impl Tool for DbConnect {
    fn name(&self) -> &str {
        "db.connect"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let url = params
            .get("url")
            .ok_or_else(|| anyhow!("db.connect: missing 'url' parameter"))?;
        let alias = pool_alias(params);
        let max_conn: u32 = params
            .get("max_connections")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        // Install Any drivers once
        DRIVERS_INSTALLED.call_once(|| {
            sqlx::any::install_default_drivers();
        });

        let backend = detect_backend(url)?;

        // For SQLite: ensure the database file exists before connecting
        // (sqlx AnyPool does not set create_if_missing by default)
        if backend == BackendType::SQLite {
            let base = url.split('?').next().unwrap_or(url);
            let file_path = base
                .strip_prefix("sqlite:///")
                .map(|p| format!("/{p}"))
                .or_else(|| base.strip_prefix("sqlite://").map(str::to_string))
                .or_else(|| {
                    base.strip_prefix("sqlite:").and_then(|p| {
                        if p.starts_with(':') {
                            None // :memory:
                        } else {
                            Some(p.to_string())
                        }
                    })
                });
            if let Some(path) = file_path {
                let p = std::path::Path::new(&path);
                if !p.exists() {
                    if let Some(parent) = p.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(false)
                        .open(p);
                }
            }
        }

        let pool = AnyPoolOptions::new()
            .max_connections(max_conn)
            .connect(url)
            .await
            .map_err(|e| anyhow!("db.connect failed: {}", e))?;

        POOLS.insert(alias.clone(), pool);
        BACKENDS.insert(alias.clone(), backend);

        Ok(Some(json!({
            "connected": true,
            "pool": alias,
            "backend": format!("{:?}", backend),
            "max_connections": max_conn,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.query — raw SQL SELECT
// ---------------------------------------------------------------------------

pub struct DbQuery;

#[async_trait]
impl Tool for DbQuery {
    fn name(&self) -> &str {
        "db.query"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let sql = params
            .get("sql")
            .ok_or_else(|| anyhow!("db.query: missing 'sql' parameter"))?;

        let bind_params = params
            .get("params")
            .and_then(|p| serde_json::from_str::<Vec<Value>>(p).ok())
            .unwrap_or_default();

        let rows = exec_query(&alias, sql, &bind_params).await?;
        let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();
        let count = json_rows.len();

        Ok(Some(json!({
            "rows": json_rows,
            "count": count,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.exec — raw SQL DDL/DML
// ---------------------------------------------------------------------------

pub struct DbExec;

#[async_trait]
impl Tool for DbExec {
    fn name(&self) -> &str {
        "db.exec"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let sql = params
            .get("sql")
            .ok_or_else(|| anyhow!("db.exec: missing 'sql' parameter"))?;

        let bind_params = params
            .get("params")
            .and_then(|p| serde_json::from_str::<Vec<Value>>(p).ok())
            .unwrap_or_default();

        let result = exec_statement(&alias, sql, &bind_params).await?;

        Ok(Some(json!({
            "affected": result.rows_affected(),
        })))
    }
}

// ---------------------------------------------------------------------------
// db.find — query builder SELECT (multiple rows)
// ---------------------------------------------------------------------------

pub struct DbFind;

#[async_trait]
impl Tool for DbFind {
    fn name(&self) -> &str {
        "db.find"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.find: missing 'table' parameter"))?;

        let columns = params
            .get("columns")
            .map(|c| c.to_string())
            .unwrap_or_else(|| "*".to_string());

        let where_json = params
            .get("where")
            .and_then(|w| serde_json::from_str::<Value>(w).ok())
            .unwrap_or(json!({}));

        let (where_sql, bind_vals) = build_where(&where_json, 0);

        let mut sql = format!("SELECT {} FROM \"{}\"{}", columns, table, where_sql);

        if let Some(order) = params.get("order") {
            sql.push_str(&format!(" ORDER BY {}", order));
        }
        if let Some(limit) = params.get("limit") {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = params.get("offset") {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let rows = exec_query(&alias, &sql, &bind_vals).await?;
        let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();
        let count = json_rows.len();

        Ok(Some(json!({
            "rows": json_rows,
            "count": count,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.find_one — query builder SELECT (single row)
// ---------------------------------------------------------------------------

pub struct DbFindOne;

#[async_trait]
impl Tool for DbFindOne {
    fn name(&self) -> &str {
        "db.find_one"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.find_one: missing 'table' parameter"))?;

        let columns = params
            .get("columns")
            .map(|c| c.to_string())
            .unwrap_or_else(|| "*".to_string());

        let where_json = params
            .get("where")
            .and_then(|w| serde_json::from_str::<Value>(w).ok())
            .unwrap_or(json!({}));

        let (where_sql, bind_vals) = build_where(&where_json, 0);
        let sql = format!(
            "SELECT {} FROM \"{}\" {} LIMIT 1",
            columns, table, where_sql
        );

        let row = exec_fetch_optional(&alias, &sql, &bind_vals).await?;

        match row {
            Some(r) => Ok(Some(row_to_json(&r))),
            None => Ok(Some(Value::Null)),
        }
    }
}

// ---------------------------------------------------------------------------
// db.create — INSERT RETURNING *
// ---------------------------------------------------------------------------

pub struct DbCreate;

#[async_trait]
impl Tool for DbCreate {
    fn name(&self) -> &str {
        "db.create"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.create: missing 'table' parameter"))?;

        let data_json: Value = params
            .get("data")
            .and_then(|d| serde_json::from_str(d).ok())
            .ok_or_else(|| anyhow!("db.create: missing or invalid 'data' parameter"))?;

        let obj = data_json
            .as_object()
            .ok_or_else(|| anyhow!("db.create: 'data' must be a JSON object"))?;

        let columns: Vec<String> = obj.keys().map(|k| format!("\"{}\"", k)).collect();
        let values: Vec<&Value> = obj.values().collect();
        let placeholders: Vec<String> = (1..=values.len()).map(|i| format!("${}", i)).collect();
        let owned_values: Vec<Value> = values.into_iter().cloned().collect();

        match backend {
            BackendType::Postgres | BackendType::SQLite => {
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING *",
                    table,
                    columns.join(", "),
                    placeholders.join(", "),
                );
                let row = exec_fetch_one(&alias, &sql, &owned_values).await?;
                Ok(Some(row_to_json(&row)))
            }
            BackendType::MySQL => {
                // MySQL: no RETURNING → INSERT then SELECT via LAST_INSERT_ID
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({})",
                    table,
                    columns.join(", "),
                    placeholders.join(", "),
                );
                exec_statement(&alias, &sql, &owned_values).await?;

                // Fetch inserted row
                let select_sql = format!("SELECT * FROM \"{}\" WHERE ROWID = LAST_INSERT_ROWID() OR \"id\" = LAST_INSERT_ID() LIMIT 1", table);
                let row = exec_fetch_optional(&alias, &select_sql, &[]).await?;
                match row {
                    Some(r) => Ok(Some(row_to_json(&r))),
                    None => {
                        // Fallback: try a simpler query
                        let fallback =
                            format!("SELECT * FROM \"{}\" ORDER BY \"id\" DESC LIMIT 1", table);
                        let row = exec_fetch_optional(&alias, &fallback, &[]).await?;
                        Ok(Some(
                            row.map(|r| row_to_json(&r))
                                .unwrap_or(json!({"inserted": true})),
                        ))
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// db.create_many — batch INSERT RETURNING *
// ---------------------------------------------------------------------------

pub struct DbCreateMany;

#[async_trait]
impl Tool for DbCreateMany {
    fn name(&self) -> &str {
        "db.create_many"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.create_many: missing 'table' parameter"))?;

        let data_json: Value = params
            .get("data")
            .and_then(|d| serde_json::from_str(d).ok())
            .ok_or_else(|| anyhow!("db.create_many: missing or invalid 'data' parameter"))?;

        let arr = data_json
            .as_array()
            .ok_or_else(|| anyhow!("db.create_many: 'data' must be a JSON array"))?;

        if arr.is_empty() {
            return Ok(Some(json!({ "rows": [], "count": 0 })));
        }

        let first_obj = arr[0]
            .as_object()
            .ok_or_else(|| anyhow!("db.create_many: each item must be a JSON object"))?;

        let col_names: Vec<&String> = first_obj.keys().collect();
        let columns: Vec<String> = col_names.iter().map(|k| format!("\"{}\"", k)).collect();

        let mut all_values: Vec<Value> = Vec::new();
        let mut value_groups: Vec<String> = Vec::new();
        let mut idx = 0usize;

        for item in arr {
            let obj = item
                .as_object()
                .ok_or_else(|| anyhow!("db.create_many: each item must be a JSON object"))?;

            let mut placeholders = Vec::new();
            for col_name in &col_names {
                idx += 1;
                placeholders.push(format!("${}", idx));
                all_values.push(obj.get(*col_name).cloned().unwrap_or(Value::Null));
            }
            value_groups.push(format!("({})", placeholders.join(", ")));
        }

        match backend {
            BackendType::Postgres | BackendType::SQLite => {
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES {} RETURNING *",
                    table,
                    columns.join(", "),
                    value_groups.join(", "),
                );
                let rows = exec_query(&alias, &sql, &all_values).await?;
                let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();
                let count = json_rows.len();
                Ok(Some(json!({ "rows": json_rows, "count": count })))
            }
            BackendType::MySQL => {
                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES {}",
                    table,
                    columns.join(", "),
                    value_groups.join(", "),
                );
                let result = exec_statement(&alias, &sql, &all_values).await?;
                Ok(Some(json!({
                    "inserted": true,
                    "count": result.rows_affected(),
                })))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// db.upsert — INSERT ON CONFLICT / ON DUPLICATE KEY
// ---------------------------------------------------------------------------

pub struct DbUpsert;

#[async_trait]
impl Tool for DbUpsert {
    fn name(&self) -> &str {
        "db.upsert"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.upsert: missing 'table' parameter"))?;

        let data_json: Value = params
            .get("data")
            .and_then(|d| serde_json::from_str(d).ok())
            .ok_or_else(|| anyhow!("db.upsert: missing or invalid 'data' parameter"))?;

        let obj = data_json
            .as_object()
            .ok_or_else(|| anyhow!("db.upsert: 'data' must be a JSON object"))?;

        let conflict_str = params
            .get("conflict")
            .ok_or_else(|| anyhow!("db.upsert: missing 'conflict' parameter"))?;

        let conflict_cols: Vec<String> =
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(conflict_str) {
                arr
            } else {
                vec![conflict_str.clone()]
            };

        let columns: Vec<String> = obj.keys().map(|k| format!("\"{}\"", k)).collect();
        let values: Vec<&Value> = obj.values().collect();
        let placeholders: Vec<String> = (1..=values.len()).map(|i| format!("${}", i)).collect();

        let mut all_values: Vec<Value> = values.iter().map(|v| (*v).clone()).collect();
        let mut idx = values.len();

        match backend {
            BackendType::Postgres | BackendType::SQLite => {
                let conflict_sql = conflict_cols
                    .iter()
                    .map(|c| format!("\"{}\"", c))
                    .collect::<Vec<_>>()
                    .join(", ");

                let conflict_action = if let Some(update_str) = params.get("update") {
                    if let Ok(update_obj) =
                        serde_json::from_str::<serde_json::Map<String, Value>>(update_str)
                    {
                        let set_parts: Vec<String> = update_obj
                            .iter()
                            .map(|(col, val)| {
                                idx += 1;
                                all_values.push(val.clone());
                                format!("\"{}\" = ${}", col, idx)
                            })
                            .collect();
                        format!("DO UPDATE SET {}", set_parts.join(", "))
                    } else {
                        "DO NOTHING".to_string()
                    }
                } else {
                    "DO NOTHING".to_string()
                };

                let sql = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT ({}) {} RETURNING *",
                    table,
                    columns.join(", "),
                    placeholders.join(", "),
                    conflict_sql,
                    conflict_action,
                );
                let row = exec_fetch_one(&alias, &sql, &all_values).await?;
                Ok(Some(row_to_json(&row)))
            }
            BackendType::MySQL => {
                // MySQL: ON DUPLICATE KEY UPDATE
                let update_action = if let Some(update_str) = params.get("update") {
                    if let Ok(update_obj) =
                        serde_json::from_str::<serde_json::Map<String, Value>>(update_str)
                    {
                        let set_parts: Vec<String> = update_obj
                            .iter()
                            .map(|(col, val)| {
                                idx += 1;
                                all_values.push(val.clone());
                                format!("\"{}\" = ${}", col, idx)
                            })
                            .collect();
                        format!("ON DUPLICATE KEY UPDATE {}", set_parts.join(", "))
                    } else {
                        // MySQL: INSERT IGNORE for DO NOTHING equivalent
                        String::new()
                    }
                } else {
                    String::new()
                };

                let insert_keyword = if update_action.is_empty() {
                    "INSERT IGNORE"
                } else {
                    "INSERT"
                };

                let sql = format!(
                    "{} INTO \"{}\" ({}) VALUES ({}) {}",
                    insert_keyword,
                    table,
                    columns.join(", "),
                    placeholders.join(", "),
                    update_action,
                );
                exec_statement(&alias, &sql, &all_values).await?;

                // Fetch the row back
                let select_sql =
                    format!("SELECT * FROM \"{}\" ORDER BY \"id\" DESC LIMIT 1", table);
                let row = exec_fetch_optional(&alias, &select_sql, &[]).await?;
                Ok(Some(
                    row.map(|r| row_to_json(&r))
                        .unwrap_or(json!({"upserted": true})),
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// db.update — UPDATE ... SET ... WHERE ...
// ---------------------------------------------------------------------------

pub struct DbUpdate;

#[async_trait]
impl Tool for DbUpdate {
    fn name(&self) -> &str {
        "db.update"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.update: missing 'table' parameter"))?;

        let set_json: Value = params
            .get("set")
            .and_then(|s| serde_json::from_str(s).ok())
            .ok_or_else(|| anyhow!("db.update: missing or invalid 'set' parameter"))?;

        let set_obj = set_json
            .as_object()
            .ok_or_else(|| anyhow!("db.update: 'set' must be a JSON object"))?;

        let mut set_parts = Vec::new();
        let mut bind_vals: Vec<Value> = Vec::new();
        let mut idx = 0usize;

        for (col, val) in set_obj {
            idx += 1;
            set_parts.push(format!("\"{}\" = ${}", col, idx));
            bind_vals.push(val.clone());
        }

        let where_json = params
            .get("where")
            .and_then(|w| serde_json::from_str::<Value>(w).ok())
            .unwrap_or(json!({}));

        let (where_sql, where_vals) = build_where(&where_json, idx);
        bind_vals.extend(where_vals);

        let returning = params
            .get("returning")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        if returning && (backend == BackendType::Postgres || backend == BackendType::SQLite) {
            let sql = format!(
                "UPDATE \"{}\" SET {}{} RETURNING *",
                table,
                set_parts.join(", "),
                where_sql,
            );
            let rows = exec_query(&alias, &sql, &bind_vals).await?;
            let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();
            let count = json_rows.len();
            Ok(Some(json!({ "rows": json_rows, "count": count })))
        } else {
            let sql = format!(
                "UPDATE \"{}\" SET {}{}",
                table,
                set_parts.join(", "),
                where_sql,
            );
            let result = exec_statement(&alias, &sql, &bind_vals).await?;
            Ok(Some(json!({ "affected": result.rows_affected() })))
        }
    }
}

// ---------------------------------------------------------------------------
// db.delete — DELETE ... WHERE ...
// ---------------------------------------------------------------------------

pub struct DbDelete;

#[async_trait]
impl Tool for DbDelete {
    fn name(&self) -> &str {
        "db.delete"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.delete: missing 'table' parameter"))?;

        let where_json = params
            .get("where")
            .and_then(|w| serde_json::from_str::<Value>(w).ok())
            .unwrap_or(json!({}));

        let (where_sql, bind_vals) = build_where(&where_json, 0);

        let returning = params
            .get("returning")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        if returning && (backend == BackendType::Postgres || backend == BackendType::SQLite) {
            let sql = format!("DELETE FROM \"{}\"{} RETURNING *", table, where_sql);
            let rows = exec_query(&alias, &sql, &bind_vals).await?;
            let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();
            let count = json_rows.len();
            Ok(Some(json!({ "rows": json_rows, "count": count })))
        } else {
            let sql = format!("DELETE FROM \"{}\"{}", table, where_sql);
            let result = exec_statement(&alias, &sql, &bind_vals).await?;
            Ok(Some(json!({ "affected": result.rows_affected() })))
        }
    }
}

// ---------------------------------------------------------------------------
// db.count — SELECT COUNT(*)
// ---------------------------------------------------------------------------

pub struct DbCount;

#[async_trait]
impl Tool for DbCount {
    fn name(&self) -> &str {
        "db.count"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.count: missing 'table' parameter"))?;

        let where_json = params
            .get("where")
            .and_then(|w| serde_json::from_str::<Value>(w).ok())
            .unwrap_or(json!({}));

        let (where_sql, bind_vals) = build_where(&where_json, 0);
        let sql = format!(
            "SELECT COUNT(*) as \"count\" FROM \"{}\"{}",
            table, where_sql
        );

        let rows = exec_query(&alias, &sql, &bind_vals).await?;
        let count: i64 = rows
            .first()
            .and_then(|r| r.try_get::<i64, _>("count").ok())
            .or_else(|| {
                rows.first()
                    .and_then(|r| r.try_get::<i32, _>("count").ok().map(|v| v as i64))
            })
            .unwrap_or(0);

        Ok(Some(json!({ "count": count })))
    }
}

// ---------------------------------------------------------------------------
// db.aggregate — SUM/AVG/MIN/MAX + GROUP BY
// ---------------------------------------------------------------------------

pub struct DbAggregate;

#[async_trait]
impl Tool for DbAggregate {
    fn name(&self) -> &str {
        "db.aggregate"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.aggregate: missing 'table' parameter"))?;

        let agg_fn = params.get("fn").ok_or_else(|| {
            anyhow!("db.aggregate: missing 'fn' parameter (sum/avg/min/max/count)")
        })?;

        let column = params.get("column").map(|c| c.as_str()).unwrap_or("*");

        let fn_upper = agg_fn.to_uppercase();
        if !["SUM", "AVG", "MIN", "MAX", "COUNT"].contains(&fn_upper.as_str()) {
            return Err(anyhow!(
                "db.aggregate: unsupported function '{}'. Use sum/avg/min/max/count.",
                agg_fn
            ));
        }

        let where_json = params
            .get("where")
            .and_then(|w| serde_json::from_str::<Value>(w).ok())
            .unwrap_or(json!({}));

        let (where_sql, bind_vals) = build_where(&where_json, 0);

        let group_by = params.get("group_by");
        let having = params.get("having");

        let select_part = if let Some(gb) = group_by {
            format!("{}, {}(\"{}\") as \"result\"", gb, fn_upper, column)
        } else {
            format!("{}(\"{}\") as \"result\"", fn_upper, column)
        };

        let mut sql = format!("SELECT {} FROM \"{}\"{}", select_part, table, where_sql);

        if let Some(gb) = group_by {
            sql.push_str(&format!(" GROUP BY {}", gb));
        }
        if let Some(h) = having {
            sql.push_str(&format!(" HAVING {}", h));
        }

        let rows = exec_query(&alias, &sql, &bind_vals).await?;

        if group_by.is_some() {
            let json_rows: Vec<Value> = rows.iter().map(row_to_json).collect();
            Ok(Some(json!({
                "rows": json_rows,
                "count": json_rows.len(),
            })))
        } else {
            let result = rows
                .first()
                .map(row_to_json)
                .and_then(|obj| obj.get("result").cloned())
                .unwrap_or(Value::Null);
            Ok(Some(json!({ "result": result })))
        }
    }
}

// ---------------------------------------------------------------------------
// db.begin — start transaction
// ---------------------------------------------------------------------------

pub struct DbBegin;

#[async_trait]
impl Tool for DbBegin {
    fn name(&self) -> &str {
        "db.begin"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);

        if has_transaction(&alias) {
            return Err(anyhow!(
                "db.begin: transaction already active for pool '{}'",
                alias
            ));
        }

        let pool = get_pool(&alias)?;
        let mut conn = pool.acquire().await?;

        sqlx::query("BEGIN").execute(any_conn(&mut conn)).await?;

        TRANSACTIONS.insert(alias.clone(), tokio::sync::Mutex::new(conn));

        Ok(Some(json!({
            "transaction": true,
            "pool": alias,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.commit — commit transaction
// ---------------------------------------------------------------------------

pub struct DbCommit;

#[async_trait]
impl Tool for DbCommit {
    fn name(&self) -> &str {
        "db.commit"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);

        let (_, tx_mutex) = TRANSACTIONS
            .remove(&alias)
            .ok_or_else(|| anyhow!("db.commit: no active transaction for pool '{}'", alias))?;

        let mut conn = tx_mutex.into_inner();
        sqlx::query("COMMIT").execute(any_conn(&mut conn)).await?;

        Ok(Some(json!({
            "committed": true,
            "pool": alias,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.rollback — rollback transaction
// ---------------------------------------------------------------------------

pub struct DbRollback;

#[async_trait]
impl Tool for DbRollback {
    fn name(&self) -> &str {
        "db.rollback"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);

        let (_, tx_mutex) = TRANSACTIONS
            .remove(&alias)
            .ok_or_else(|| anyhow!("db.rollback: no active transaction for pool '{}'", alias))?;

        let mut conn = tx_mutex.into_inner();
        sqlx::query("ROLLBACK").execute(any_conn(&mut conn)).await?;

        Ok(Some(json!({
            "rolled_back": true,
            "pool": alias,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.create_table — DDL CREATE TABLE
// ---------------------------------------------------------------------------

pub struct DbCreateTable;

#[async_trait]
impl Tool for DbCreateTable {
    fn name(&self) -> &str {
        "db.create_table"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let name = params
            .get("name")
            .ok_or_else(|| anyhow!("db.create_table: missing 'name' parameter"))?;

        let columns_json: Value = params
            .get("columns")
            .and_then(|c| serde_json::from_str(c).ok())
            .ok_or_else(|| anyhow!("db.create_table: missing or invalid 'columns' parameter"))?;

        let columns_obj = columns_json
            .as_object()
            .ok_or_else(|| anyhow!("db.create_table: 'columns' must be a JSON object"))?;

        let col_defs: Vec<String> = columns_obj
            .iter()
            .map(|(col, def)| {
                let def_str = def.as_str().unwrap_or("text");
                format!("\"{}\" {}", col, def_str)
            })
            .collect();

        let if_not_exists = params
            .get("if_not_exists")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);

        let exists_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };

        let sql = format!(
            "CREATE TABLE {}\"{}\" ({})",
            exists_clause,
            name,
            col_defs.join(", "),
        );

        exec_statement(&alias, &sql, &[]).await?;

        Ok(Some(json!({
            "created": true,
            "table": name,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.drop_table — DDL DROP TABLE
// ---------------------------------------------------------------------------

pub struct DbDropTable;

#[async_trait]
impl Tool for DbDropTable {
    fn name(&self) -> &str {
        "db.drop_table"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let name = params
            .get("name")
            .ok_or_else(|| anyhow!("db.drop_table: missing 'name' parameter"))?;

        let if_exists = params
            .get("if_exists")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);

        let exists_clause = if if_exists { "IF EXISTS " } else { "" };

        let sql = format!("DROP TABLE {}\"{}\"", exists_clause, name);
        exec_statement(&alias, &sql, &[]).await?;

        Ok(Some(json!({
            "dropped": true,
            "table": name,
        })))
    }
}

// ---------------------------------------------------------------------------
// db.alter_table — DDL ALTER TABLE
// ---------------------------------------------------------------------------

pub struct DbAlterTable;

#[async_trait]
impl Tool for DbAlterTable {
    fn name(&self) -> &str {
        "db.alter_table"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let name = params
            .get("name")
            .ok_or_else(|| anyhow!("db.alter_table: missing 'name' parameter"))?;

        let mut statements = Vec::new();

        if let Some(add_str) = params.get("add") {
            if let Ok(add_obj) = serde_json::from_str::<serde_json::Map<String, Value>>(add_str) {
                for (col, def) in &add_obj {
                    let def_str = def.as_str().unwrap_or("text");
                    statements.push(format!(
                        "ALTER TABLE \"{}\" ADD COLUMN \"{}\" {}",
                        name, col, def_str
                    ));
                }
            }
        }

        if let Some(drop_str) = params.get("drop") {
            if let Ok(drop_arr) = serde_json::from_str::<Vec<String>>(drop_str) {
                for col in &drop_arr {
                    statements.push(format!("ALTER TABLE \"{}\" DROP COLUMN \"{}\"", name, col));
                }
            }
        }

        if let Some(rename_str) = params.get("rename") {
            if let Ok(rename_obj) =
                serde_json::from_str::<serde_json::Map<String, Value>>(rename_str)
            {
                for (old_name, new_name) in &rename_obj {
                    let new_str = new_name.as_str().unwrap_or("");
                    if !new_str.is_empty() {
                        statements.push(format!(
                            "ALTER TABLE \"{}\" RENAME COLUMN \"{}\" TO \"{}\"",
                            name, old_name, new_str
                        ));
                    }
                }
            }
        }

        if statements.is_empty() {
            return Err(anyhow!(
                "db.alter_table: at least one of 'add', 'drop', or 'rename' is required"
            ));
        }

        for sql in &statements {
            exec_statement(&alias, sql, &[]).await?;
        }

        Ok(Some(json!({
            "altered": true,
            "table": name,
            "statements": statements.len(),
        })))
    }
}

// ---------------------------------------------------------------------------
// db.tables — list all user tables (backend-conditional)
// ---------------------------------------------------------------------------

pub struct DbTables;

#[async_trait]
impl Tool for DbTables {
    fn name(&self) -> &str {
        "db.tables"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);

        let sql = match backend {
            BackendType::Postgres => {
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
                 ORDER BY table_name"
            }
            BackendType::MySQL => {
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE' \
                 ORDER BY table_name"
            }
            BackendType::SQLite => {
                "SELECT name as table_name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx_%' \
                 ORDER BY name"
            }
        };

        let rows = exec_query(&alias, sql, &[]).await?;

        let tables: Vec<String> = rows
            .iter()
            .filter_map(|r| r.try_get::<String, _>("table_name").ok())
            .collect();

        Ok(Some(json!({ "tables": tables })))
    }
}

// ---------------------------------------------------------------------------
// db.columns — describe table structure (backend-conditional)
// ---------------------------------------------------------------------------

pub struct DbColumns;

#[async_trait]
impl Tool for DbColumns {
    fn name(&self) -> &str {
        "db.columns"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);
        let backend = get_backend(&alias);
        let table = params
            .get("table")
            .ok_or_else(|| anyhow!("db.columns: missing 'table' parameter"))?;

        let columns: Vec<Value> = match backend {
            BackendType::Postgres => {
                let rows = exec_query(
                    &alias,
                    "SELECT column_name, data_type, is_nullable, column_default \
                     FROM information_schema.columns \
                     WHERE table_schema = 'public' AND table_name = $1 \
                     ORDER BY ordinal_position",
                    &[Value::String(table.clone())],
                )
                .await?;

                rows.iter()
                    .map(|r| {
                        json!({
                            "name": r.try_get::<String, _>("column_name").unwrap_or_default(),
                            "type": r.try_get::<String, _>("data_type").unwrap_or_default(),
                            "nullable": r.try_get::<String, _>("is_nullable").unwrap_or_default() == "YES",
                            "default": r.try_get::<String, _>("column_default").ok(),
                        })
                    })
                    .collect()
            }
            BackendType::MySQL => {
                let rows = exec_query(
                    &alias,
                    "SELECT column_name, data_type, is_nullable, column_default \
                     FROM information_schema.columns \
                     WHERE table_schema = DATABASE() AND table_name = $1 \
                     ORDER BY ordinal_position",
                    &[Value::String(table.clone())],
                )
                .await?;

                rows.iter()
                    .map(|r| {
                        json!({
                            "name": r.try_get::<String, _>("column_name")
                                .or_else(|_| r.try_get::<String, _>("COLUMN_NAME"))
                                .unwrap_or_default(),
                            "type": r.try_get::<String, _>("data_type")
                                .or_else(|_| r.try_get::<String, _>("DATA_TYPE"))
                                .unwrap_or_default(),
                            "nullable": r.try_get::<String, _>("is_nullable")
                                .or_else(|_| r.try_get::<String, _>("IS_NULLABLE"))
                                .unwrap_or_default() == "YES",
                            "default": r.try_get::<String, _>("column_default")
                                .or_else(|_| r.try_get::<String, _>("COLUMN_DEFAULT"))
                                .ok(),
                        })
                    })
                    .collect()
            }
            BackendType::SQLite => {
                // PRAGMA table_info does not support parameter binding, use string interpolation
                // (table name comes from workflow params, not user input in the SQL injection sense)
                let sql = format!("PRAGMA table_info(\"{}\")", table);
                let rows = exec_query(&alias, &sql, &[]).await?;

                rows.iter()
                    .map(|r| {
                        let notnull = r.try_get::<i32, _>("notnull").unwrap_or(0);
                        json!({
                            "name": r.try_get::<String, _>("name").unwrap_or_default(),
                            "type": r.try_get::<String, _>("type").unwrap_or_default(),
                            "nullable": notnull == 0,
                            "default": r.try_get::<String, _>("dflt_value").ok(),
                        })
                    })
                    .collect()
            }
        };

        Ok(Some(json!({ "columns": columns })))
    }
}

// ---------------------------------------------------------------------------
// db.disconnect — close connection pool
// ---------------------------------------------------------------------------

pub struct DbDisconnect;

#[async_trait]
impl Tool for DbDisconnect {
    fn name(&self) -> &str {
        "db.disconnect"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let alias = pool_alias(params);

        // Rollback any active transaction first
        if let Some((_, tx_mutex)) = TRANSACTIONS.remove(&alias) {
            let mut conn = tx_mutex.into_inner();
            let _ = sqlx::query("ROLLBACK").execute(any_conn(&mut conn)).await;
        }

        BACKENDS.remove(&alias);

        let removed = POOLS.remove(&alias);
        if let Some((_, pool)) = removed {
            pool.close().await;
            Ok(Some(json!({
                "disconnected": true,
                "pool": alias,
            })))
        } else {
            Ok(Some(json!({
                "disconnected": false,
                "pool": alias,
                "reason": "pool not found",
            })))
        }
    }
}
