// src/builtins/history.rs
//
// DSL-callable conversation history primitives.
//
// These tools operate on the global ConversationStore initialized from
// juglans.toml [history]. When history is disabled or the backend isn't
// initialized, every tool returns a null value without error so workflows
// stay resilient.
//
// Exposed names:
//   history.load(chat_id, limit=20)              → Array of messages
//   history.append(chat_id, role, content)       → { ok: true }
//   history.replace(chat_id, from, to, content, role?) → { ok: true }
//   history.trim(chat_id, keep_recent=20)        → { ok: true }
//   history.clear(chat_id)                       → { ok: true }
//   history.stats(chat_id)                       → { count, tokens, ... }
//   history.list_chats()                         → Array of chat_ids

#![cfg(not(target_arch = "wasm32"))]

use super::Tool;
use crate::core::context::WorkflowContext;
use crate::services::history::{global_store, ChatMessage};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

fn missing(name: &str) -> anyhow::Error {
    anyhow!("history.{}: missing required parameter 'chat_id'", name)
}

fn parse_usize(s: Option<&String>, default: usize) -> usize {
    s.and_then(|v| v.parse::<usize>().ok()).unwrap_or(default)
}

fn message_to_json(m: &ChatMessage) -> Value {
    let mut obj = json!({
        "role": m.role,
        "content": m.content,
        "created_at": m.created_at,
    });
    if let Some(t) = m.tokens {
        obj["tokens"] = json!(t);
    }
    if let Some(ref meta) = m.meta {
        obj["meta"] = meta.clone();
    }
    obj
}

pub struct HistoryLoad;
#[async_trait]
impl Tool for HistoryLoad {
    fn name(&self) -> &str {
        "history.load"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params.get("chat_id").ok_or_else(|| missing("load"))?;
        let limit = parse_usize(params.get("limit"), 20);

        let store = match global_store() {
            Some(s) => s,
            None => return Ok(Some(json!([]))),
        };
        let msgs = store.load(chat_id, limit).await?;
        let arr: Vec<Value> = msgs.iter().map(message_to_json).collect();
        Ok(Some(Value::Array(arr)))
    }
}

pub struct HistoryAppend;
#[async_trait]
impl Tool for HistoryAppend {
    fn name(&self) -> &str {
        "history.append"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params.get("chat_id").ok_or_else(|| missing("append"))?;
        let role = params
            .get("role")
            .cloned()
            .ok_or_else(|| anyhow!("history.append: missing 'role'"))?;
        let content = params
            .get("content")
            .cloned()
            .ok_or_else(|| anyhow!("history.append: missing 'content'"))?;

        let store = match global_store() {
            Some(s) => s,
            None => return Ok(Some(json!({ "ok": false, "reason": "history disabled" }))),
        };
        let mut msg = ChatMessage::new(role, content);
        if let Some(Ok(t)) = params.get("tokens").map(|s| s.parse::<u32>()) {
            msg.tokens = Some(t);
        }
        store.append(chat_id, msg).await?;
        Ok(Some(json!({ "ok": true })))
    }
}

pub struct HistoryReplace;
#[async_trait]
impl Tool for HistoryReplace {
    fn name(&self) -> &str {
        "history.replace"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params.get("chat_id").ok_or_else(|| missing("replace"))?;
        let from = parse_usize(params.get("from"), 0);
        let to = parse_usize(params.get("to"), 0);
        let content = params
            .get("content")
            .cloned()
            .ok_or_else(|| anyhow!("history.replace: missing 'content'"))?;
        let role = params
            .get("role")
            .cloned()
            .unwrap_or_else(|| "system".to_string());

        let store = match global_store() {
            Some(s) => s,
            None => return Ok(Some(json!({ "ok": false, "reason": "history disabled" }))),
        };
        let msg = ChatMessage::new(role, content);
        store.replace(chat_id, from, to, msg).await?;
        Ok(Some(json!({ "ok": true })))
    }
}

pub struct HistoryTrim;
#[async_trait]
impl Tool for HistoryTrim {
    fn name(&self) -> &str {
        "history.trim"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params.get("chat_id").ok_or_else(|| missing("trim"))?;
        let keep_recent = parse_usize(params.get("keep_recent"), 20);

        let store = match global_store() {
            Some(s) => s,
            None => return Ok(Some(json!({ "ok": false, "reason": "history disabled" }))),
        };
        store.trim(chat_id, keep_recent).await?;
        Ok(Some(json!({ "ok": true })))
    }
}

pub struct HistoryClear;
#[async_trait]
impl Tool for HistoryClear {
    fn name(&self) -> &str {
        "history.clear"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params.get("chat_id").ok_or_else(|| missing("clear"))?;

        let store = match global_store() {
            Some(s) => s,
            None => return Ok(Some(json!({ "ok": false, "reason": "history disabled" }))),
        };
        store.clear(chat_id).await?;
        Ok(Some(json!({ "ok": true })))
    }
}

pub struct HistoryStats;
#[async_trait]
impl Tool for HistoryStats {
    fn name(&self) -> &str {
        "history.stats"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = params.get("chat_id").ok_or_else(|| missing("stats"))?;

        let store = match global_store() {
            Some(s) => s,
            None => {
                return Ok(Some(json!({
                    "chat_id": chat_id,
                    "count": 0,
                    "tokens": 0,
                    "enabled": false,
                })));
            }
        };
        let s = store.stats(chat_id).await?;
        Ok(Some(json!({
            "chat_id": s.chat_id,
            "count": s.count,
            "tokens": s.tokens,
            "first_at": s.first_at,
            "last_at": s.last_at,
        })))
    }
}

pub struct HistoryListChats;
#[async_trait]
impl Tool for HistoryListChats {
    fn name(&self) -> &str {
        "history.list_chats"
    }
    async fn execute(
        &self,
        _params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let store = match global_store() {
            Some(s) => s,
            None => return Ok(Some(json!([]))),
        };
        let ids = store.list_chats().await?;
        Ok(Some(json!(ids)))
    }
}
