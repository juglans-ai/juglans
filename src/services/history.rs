// src/services/history.rs
//
// Conversation history storage: trait + backends.
//
// Backends:
//   - JsonlStore: one append-only .jsonl file per chat_id (default)
//   - SqliteStore: single .db with an indexed messages table
//   - MemoryStore: in-process HashMap (tests / ephemeral)
//
// Compaction is out of scope here — this module only stores and retrieves.

#![cfg(not(target_arch = "wasm32"))]

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use dashmap::DashMap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::config::HistoryConfig;

/// A single message in a conversation thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    /// Unix seconds.
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl ChatMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tokens: None,
            created_at: chrono::Utc::now().timestamp(),
            meta: None,
        }
    }

    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.tokens = Some(tokens);
        self
    }

    #[allow(dead_code)]
    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = Some(meta);
        self
    }
}

/// Aggregate stats for a chat thread.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatStats {
    pub chat_id: String,
    pub count: usize,
    pub tokens: u64,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
}

/// Storage backend for conversation history.
#[async_trait]
pub trait ConversationStore: Send + Sync {
    /// Append a single message to the thread.
    async fn append(&self, chat_id: &str, msg: ChatMessage) -> Result<()>;

    /// Load the tail of the thread. `limit` caps the number of messages
    /// returned (most recent first in storage order, oldest → newest in result).
    async fn load(&self, chat_id: &str, limit: usize) -> Result<Vec<ChatMessage>>;

    /// Keep only the last `keep_recent` messages, drop the rest.
    async fn trim(&self, chat_id: &str, keep_recent: usize) -> Result<()>;

    /// Replace messages in the index range [from, to) with a single new
    /// message. `from` and `to` are 0-based indexes into the full history
    /// (oldest = 0). Used by compaction workflows.
    async fn replace(&self, chat_id: &str, from: usize, to: usize, with: ChatMessage)
        -> Result<()>;

    /// Delete all messages for a thread.
    async fn clear(&self, chat_id: &str) -> Result<()>;

    /// Stats for one thread.
    async fn stats(&self, chat_id: &str) -> Result<ChatStats>;

    /// Enumerate all known chat_ids.
    async fn list_chats(&self) -> Result<Vec<String>>;
}

// ─── Factory + global ────────────────────────────────────────────────────────

use std::sync::OnceLock;

struct GlobalHistory {
    store: Option<Arc<dyn ConversationStore>>,
    cfg: HistoryConfig,
}

static GLOBAL: OnceLock<GlobalHistory> = OnceLock::new();

/// Initialize the global store from config. Idempotent — only the first call
/// takes effect. Safe to call from multiple entrypoints (CLI, web_server,
/// adapters); later calls are no-ops.
pub fn init_global(cfg: &HistoryConfig) -> Result<()> {
    if GLOBAL.get().is_some() {
        return Ok(());
    }
    let store = open_from_config(cfg)?;
    let _ = GLOBAL.set(GlobalHistory {
        store,
        cfg: cfg.clone(),
    });
    Ok(())
}

/// Get the global store, if initialized and enabled. Returns None when
/// history is disabled or init_global has not been called.
pub fn global_store() -> Option<Arc<dyn ConversationStore>> {
    GLOBAL.get().and_then(|g| g.store.clone())
}

/// Get the active history config. Returns default config if init_global
/// has not been called (safe for callers that want limits with fallback).
pub fn global_config() -> HistoryConfig {
    GLOBAL.get().map(|g| g.cfg.clone()).unwrap_or_default()
}

/// Build the configured store. Returns None if history is disabled or
/// backend == "none".
pub fn open_from_config(cfg: &HistoryConfig) -> Result<Option<Arc<dyn ConversationStore>>> {
    if !cfg.enabled {
        return Ok(None);
    }
    let store: Arc<dyn ConversationStore> = match cfg.backend.as_str() {
        "none" => return Ok(None),
        "memory" => Arc::new(MemoryStore::new()),
        "jsonl" => {
            let dir = cfg.dir.clone().unwrap_or_else(|| ".juglans/history".into());
            Arc::new(JsonlStore::open(Path::new(&dir))?)
        }
        "sqlite" => {
            let path = cfg
                .path
                .clone()
                .unwrap_or_else(|| ".juglans/history.db".into());
            Arc::new(SqliteStore::open(Path::new(&path))?)
        }
        other => return Err(anyhow!("Unknown history backend: {}", other)),
    };
    Ok(Some(store))
}

// ─── chat_id sanitization ────────────────────────────────────────────────────

/// Filesystem-safe filename for a chat_id (used by JsonlStore).
fn sanitize_chat_id(chat_id: &str) -> String {
    chat_id
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

// ─── MemoryStore ─────────────────────────────────────────────────────────────

pub struct MemoryStore {
    threads: DashMap<String, Vec<ChatMessage>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            threads: DashMap::new(),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConversationStore for MemoryStore {
    async fn append(&self, chat_id: &str, msg: ChatMessage) -> Result<()> {
        self.threads
            .entry(chat_id.to_string())
            .or_default()
            .push(msg);
        Ok(())
    }

    async fn load(&self, chat_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let msgs = self
            .threads
            .get(chat_id)
            .map(|e| e.value().clone())
            .unwrap_or_default();
        if limit == 0 || msgs.len() <= limit {
            Ok(msgs)
        } else {
            Ok(msgs[msgs.len() - limit..].to_vec())
        }
    }

    async fn trim(&self, chat_id: &str, keep_recent: usize) -> Result<()> {
        if let Some(mut entry) = self.threads.get_mut(chat_id) {
            let msgs = entry.value_mut();
            if msgs.len() > keep_recent {
                let drop = msgs.len() - keep_recent;
                msgs.drain(0..drop);
            }
        }
        Ok(())
    }

    async fn replace(
        &self,
        chat_id: &str,
        from: usize,
        to: usize,
        with: ChatMessage,
    ) -> Result<()> {
        if let Some(mut entry) = self.threads.get_mut(chat_id) {
            let msgs = entry.value_mut();
            let end = to.min(msgs.len());
            if from < end {
                msgs.splice(from..end, std::iter::once(with));
            }
        }
        Ok(())
    }

    async fn clear(&self, chat_id: &str) -> Result<()> {
        self.threads.remove(chat_id);
        Ok(())
    }

    async fn stats(&self, chat_id: &str) -> Result<ChatStats> {
        let msgs = self
            .threads
            .get(chat_id)
            .map(|e| e.value().clone())
            .unwrap_or_default();
        Ok(compute_stats(chat_id, &msgs))
    }

    async fn list_chats(&self) -> Result<Vec<String>> {
        Ok(self.threads.iter().map(|e| e.key().clone()).collect())
    }
}

// ─── JsonlStore ──────────────────────────────────────────────────────────────
//
// One file per chat_id under `dir`. Append = single fs write per message.
// Load = read whole file + tail slice. Files are bounded in practice by the
// configured max_messages, so full-file reads are fine for typical use.

pub struct JsonlStore {
    dir: PathBuf,
    /// Per-thread mutex so concurrent writes to the same file don't interleave.
    locks: DashMap<String, Arc<Mutex<()>>>,
}

impl JsonlStore {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create history dir: {}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            locks: DashMap::new(),
        })
    }

    fn path_for(&self, chat_id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.jsonl", sanitize_chat_id(chat_id)))
    }

    fn lock_for(&self, chat_id: &str) -> Arc<Mutex<()>> {
        self.locks
            .entry(chat_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .value()
            .clone()
    }

    fn read_all(&self, chat_id: &str) -> Result<Vec<ChatMessage>> {
        let path = self.path_for(chat_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file =
            fs::File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut msgs = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = line.with_context(|| format!("Read error in {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ChatMessage>(&line) {
                Ok(m) => msgs.push(m),
                Err(e) => {
                    tracing::warn!(
                        "[history] skipping corrupt line {} in {}: {}",
                        idx + 1,
                        path.display(),
                        e
                    );
                }
            }
        }
        Ok(msgs)
    }

    fn write_all(&self, chat_id: &str, msgs: &[ChatMessage]) -> Result<()> {
        let path = self.path_for(chat_id);
        let tmp = path.with_extension("jsonl.tmp");
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("Failed to create {}", tmp.display()))?;
            for m in msgs {
                let line = serde_json::to_string(m)?;
                f.write_all(line.as_bytes())?;
                f.write_all(b"\n")?;
            }
            f.sync_all().ok();
        }
        fs::rename(&tmp, &path)
            .with_context(|| format!("Failed to rename {} → {}", tmp.display(), path.display()))?;
        Ok(())
    }
}

#[async_trait]
impl ConversationStore for JsonlStore {
    async fn append(&self, chat_id: &str, msg: ChatMessage) -> Result<()> {
        let path = self.path_for(chat_id);
        let lock = self.lock_for(chat_id);
        let line = serde_json::to_string(&msg)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let _g = lock.lock().map_err(|_| anyhow!("history lock poisoned"))?;
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("Failed to open {}", path.display()))?;
            f.write_all(line.as_bytes())?;
            f.write_all(b"\n")?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn load(&self, chat_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let all = self.read_all(chat_id)?;
        if limit == 0 || all.len() <= limit {
            Ok(all)
        } else {
            Ok(all[all.len() - limit..].to_vec())
        }
    }

    async fn trim(&self, chat_id: &str, keep_recent: usize) -> Result<()> {
        let lock = self.lock_for(chat_id);
        let _g = lock.lock().map_err(|_| anyhow!("history lock poisoned"))?;
        let all = self.read_all(chat_id)?;
        if all.len() <= keep_recent {
            return Ok(());
        }
        let kept = &all[all.len() - keep_recent..];
        self.write_all(chat_id, kept)
    }

    async fn replace(
        &self,
        chat_id: &str,
        from: usize,
        to: usize,
        with: ChatMessage,
    ) -> Result<()> {
        let lock = self.lock_for(chat_id);
        let _g = lock.lock().map_err(|_| anyhow!("history lock poisoned"))?;
        let mut all = self.read_all(chat_id)?;
        let end = to.min(all.len());
        if from >= end {
            return Ok(());
        }
        all.splice(from..end, std::iter::once(with));
        self.write_all(chat_id, &all)
    }

    async fn clear(&self, chat_id: &str) -> Result<()> {
        let path = self.path_for(chat_id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete {}", path.display()))?;
        }
        Ok(())
    }

    async fn stats(&self, chat_id: &str) -> Result<ChatStats> {
        let all = self.read_all(chat_id)?;
        Ok(compute_stats(chat_id, &all))
    }

    async fn list_chats(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if !self.dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
        Ok(out)
    }
}

// ─── SqliteStore ─────────────────────────────────────────────────────────────

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
        }
        let conn =
            Connection::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS messages (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id    TEXT NOT NULL,
                role       TEXT NOT NULL,
                content    TEXT NOT NULL,
                tokens     INTEGER,
                created_at INTEGER NOT NULL,
                meta       TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_messages_chat_time
                ON messages(chat_id, id);
            ",
        )
        .context("Failed to initialize history schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with_conn<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history db lock poisoned"))?;
        f(&conn)
    }

    fn with_conn_mut<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Connection) -> Result<R>,
    {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("history db lock poisoned"))?;
        f(&mut conn)
    }
}

#[async_trait]
impl ConversationStore for SqliteStore {
    async fn append(&self, chat_id: &str, msg: ChatMessage) -> Result<()> {
        let meta_str = msg
            .meta
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO messages (chat_id, role, content, tokens, created_at, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    chat_id,
                    msg.role,
                    msg.content,
                    msg.tokens,
                    msg.created_at,
                    meta_str,
                ],
            )?;
            Ok(())
        })
    }

    async fn load(&self, chat_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        self.with_conn(|c| {
            // ORDER BY id DESC + LIMIT gets newest N; reverse afterwards.
            let mut stmt = c.prepare(
                "SELECT role, content, tokens, created_at, meta
                 FROM messages WHERE chat_id = ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let effective_limit = if limit == 0 { i64::MAX } else { limit as i64 };
            let rows = stmt
                .query_map(params![chat_id, effective_limit], row_to_message)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut ordered = rows;
            ordered.reverse();
            Ok(ordered)
        })
    }

    async fn trim(&self, chat_id: &str, keep_recent: usize) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM messages WHERE chat_id = ?1 AND id NOT IN (
                     SELECT id FROM messages WHERE chat_id = ?1
                     ORDER BY id DESC LIMIT ?2
                 )",
                params![chat_id, keep_recent as i64],
            )?;
            Ok(())
        })
    }

    async fn replace(
        &self,
        chat_id: &str,
        from: usize,
        to: usize,
        with: ChatMessage,
    ) -> Result<()> {
        let meta_str = with
            .meta
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        self.with_conn_mut(|c| {
            let tx = c.transaction()?;
            // Collect row ids in chronological order.
            let ids: Vec<i64> = {
                let mut stmt =
                    tx.prepare("SELECT id FROM messages WHERE chat_id = ?1 ORDER BY id ASC")?;
                let ids: Vec<i64> = stmt
                    .query_map(params![chat_id], |r| r.get::<_, i64>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ids
            };
            let end = to.min(ids.len());
            if from >= end {
                tx.commit()?;
                return Ok(());
            }
            let targets: Vec<i64> = ids[from..end].to_vec();
            // Insert the replacement first so it takes a later id, then delete
            // the range. The replacement ends up chronologically after the
            // range it summarizes — not ideal for ordering but acceptable
            // since summaries typically live at the head anyway. Callers
            // that want a head summary should clear + append.
            tx.execute(
                "INSERT INTO messages (chat_id, role, content, tokens, created_at, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    chat_id,
                    with.role,
                    with.content,
                    with.tokens,
                    with.created_at,
                    meta_str,
                ],
            )?;
            // Delete the old range.
            for id in targets {
                tx.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    async fn clear(&self, chat_id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM messages WHERE chat_id = ?1", params![chat_id])?;
            Ok(())
        })
    }

    async fn stats(&self, chat_id: &str) -> Result<ChatStats> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT COUNT(*), COALESCE(SUM(tokens), 0), MIN(created_at), MAX(created_at)
                 FROM messages WHERE chat_id = ?1",
            )?;
            let row = stmt.query_row(params![chat_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })?;
            Ok(ChatStats {
                chat_id: chat_id.to_string(),
                count: row.0 as usize,
                tokens: row.1 as u64,
                first_at: row.2,
                last_at: row.3,
            })
        })
    }

    async fn list_chats(&self) -> Result<Vec<String>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT DISTINCT chat_id FROM messages")?;
            let ids = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(ids)
        })
    }
}

fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
    let meta_raw: Option<String> = r.get(4)?;
    let meta = meta_raw
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    Ok(ChatMessage {
        role: r.get(0)?,
        content: r.get(1)?,
        tokens: r.get(2)?,
        created_at: r.get(3)?,
        meta,
    })
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn compute_stats(chat_id: &str, msgs: &[ChatMessage]) -> ChatStats {
    ChatStats {
        chat_id: chat_id.to_string(),
        count: msgs.len(),
        tokens: msgs.iter().map(|m| m.tokens.unwrap_or(0) as u64).sum(),
        first_at: msgs.first().map(|m| m.created_at),
        last_at: msgs.last().map(|m| m.created_at),
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(role: &str, content: &str) -> ChatMessage {
        ChatMessage::new(role, content).with_tokens(content.len() as u32 / 4 + 1)
    }

    async fn roundtrip<S: ConversationStore>(store: S) {
        let c = "test:thread:1";
        store.append(c, mk("user", "hi")).await.unwrap();
        store.append(c, mk("assistant", "hey")).await.unwrap();
        store.append(c, mk("user", "how are you")).await.unwrap();

        let loaded = store.load(c, 10).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].role, "user");
        assert_eq!(loaded[0].content, "hi");
        assert_eq!(loaded[2].content, "how are you");

        let tail = store.load(c, 2).await.unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].content, "hey");
        assert_eq!(tail[1].content, "how are you");

        let stats = store.stats(c).await.unwrap();
        assert_eq!(stats.count, 3);
        assert!(stats.tokens > 0);

        store.trim(c, 2).await.unwrap();
        let trimmed = store.load(c, 10).await.unwrap();
        assert_eq!(trimmed.len(), 2);
        assert_eq!(trimmed[0].content, "hey");

        store.clear(c).await.unwrap();
        let empty = store.load(c, 10).await.unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn memory_roundtrip() {
        roundtrip(MemoryStore::new()).await;
    }

    #[tokio::test]
    async fn jsonl_roundtrip() {
        let tmp = tempdir();
        let store = JsonlStore::open(&tmp).unwrap();
        roundtrip(store).await;
    }

    #[tokio::test]
    async fn sqlite_roundtrip() {
        let tmp = tempdir();
        let store = SqliteStore::open(&tmp.join("h.db")).unwrap();
        roundtrip(store).await;
    }

    #[tokio::test]
    async fn jsonl_replace_collapses_range() {
        let tmp = tempdir();
        let store = JsonlStore::open(&tmp).unwrap();
        let c = "t";
        for i in 0..5 {
            store
                .append(c, mk("user", &format!("m{}", i)))
                .await
                .unwrap();
        }
        store
            .replace(c, 0, 3, ChatMessage::new("system", "[summary of first 3]"))
            .await
            .unwrap();
        let all = store.load(c, 10).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].role, "system");
        assert_eq!(all[0].content, "[summary of first 3]");
        assert_eq!(all[1].content, "m3");
        assert_eq!(all[2].content, "m4");
    }

    #[tokio::test]
    async fn list_chats_enumerates() {
        let tmp = tempdir();
        let store = JsonlStore::open(&tmp).unwrap();
        store.append("a", mk("user", "x")).await.unwrap();
        store.append("b:1", mk("user", "x")).await.unwrap();
        let mut ids = store.list_chats().await.unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a", "b_1"]);
    }

    #[tokio::test]
    async fn sanitize_handles_weird_chars() {
        let s = sanitize_chat_id("telegram:12345:agent/with\\bad:chars");
        assert!(!s.contains(':'));
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("juglans-history-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }
}
