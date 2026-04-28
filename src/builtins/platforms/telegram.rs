// src/builtins/platforms/telegram.rs

#![cfg(not(target_arch = "wasm32"))]

use super::resolve_target;
use crate::adapters::telegram as tg;
use crate::builtins::Tool;
use crate::core::context::WorkflowContext;
use crate::services::config::JuglansConfig;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

fn load_token() -> Result<String> {
    let config = JuglansConfig::load().map_err(|e| anyhow!("load config: {}", e))?;
    // Pick the first telegram channel's token. When point 9 (per-channel reply
    // routing in the workflow `reply()` builtin) lands, this should accept an
    // explicit `channel="telegram:beta"` parameter and resolve to that token.
    config
        .channels
        .telegram
        .values()
        .map(|t| t.token.clone())
        .find(|t| !t.is_empty())
        .ok_or_else(|| anyhow!("No telegram token configured ([channels.telegram.<id>].token)"))
}

fn param_str<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(|s| s.trim_matches('"'))
}

pub struct SendMessage;
#[async_trait]
impl Tool for SendMessage {
    fn name(&self) -> &str {
        "telegram.send_message"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let text = param_str(params, "text")
            .ok_or_else(|| anyhow!("telegram.send_message: missing `text`"))?
            .to_string();
        let chat_id = resolve_target(params, ctx, &["chat_id"], "telegram")?;
        let parse_mode = param_str(params, "parse_mode");
        let token = load_token()?;
        let http = reqwest::Client::new();
        let chunks = tg::send_message_api(&http, &token, &chat_id, &text, parse_mode).await?;
        Ok(Some(json!({
            "status": "sent",
            "target": chat_id,
            "chunks": chunks,
        })))
    }
}

pub struct Typing;
#[async_trait]
impl Tool for Typing {
    fn name(&self) -> &str {
        "telegram.typing"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = resolve_target(params, ctx, &["chat_id"], "telegram")?;
        let token = load_token()?;
        let http = reqwest::Client::new();
        tg::send_typing(&http, &token, &chat_id).await;
        Ok(Some(json!({ "status": "sent", "target": chat_id })))
    }
}

pub struct EditMessage;
#[async_trait]
impl Tool for EditMessage {
    fn name(&self) -> &str {
        "telegram.edit_message"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let chat_id = resolve_target(params, ctx, &["chat_id"], "telegram")?;
        let message_id = param_str(params, "message_id")
            .ok_or_else(|| anyhow!("telegram.edit_message: missing `message_id`"))?
            .parse::<i64>()
            .map_err(|e| anyhow!("telegram.edit_message: message_id not an integer: {}", e))?;
        let text = param_str(params, "text")
            .ok_or_else(|| anyhow!("telegram.edit_message: missing `text`"))?
            .to_string();
        let parse_mode = param_str(params, "parse_mode");
        let token = load_token()?;
        let http = reqwest::Client::new();
        tg::edit_message_api(&http, &token, &chat_id, message_id, &text, parse_mode).await?;
        Ok(Some(json!({
            "status": "edited",
            "target": chat_id,
            "message_id": message_id,
        })))
    }
}
