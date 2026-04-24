// src/builtins/platforms/discord.rs

#![cfg(not(target_arch = "wasm32"))]

use super::resolve_target;
use crate::adapters::discord as dc;
use crate::builtins::Tool;
use crate::core::context::WorkflowContext;
use crate::services::config::JuglansConfig;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

fn load_token() -> Result<String> {
    let config = JuglansConfig::load().map_err(|e| anyhow!("load config: {}", e))?;
    config
        .bot
        .as_ref()
        .and_then(|b| b.discord.as_ref())
        .map(|d| d.token.clone())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow!("Missing [bot.discord].token in juglans.toml"))
}

fn param_str<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(|s| s.trim_matches('"'))
}

pub struct SendMessage;
#[async_trait]
impl Tool for SendMessage {
    fn name(&self) -> &str {
        "discord.send_message"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let text = param_str(params, "text")
            .ok_or_else(|| anyhow!("discord.send_message: missing `text`"))?
            .to_string();
        let channel_id = resolve_target(params, ctx, &["channel_id", "chat_id"], "discord")?;
        let token = load_token()?;
        let http = reqwest::Client::new();
        dc::send_channel_message(&http, &token, &channel_id, &text).await?;
        let chunks = dc::split_message(&text, dc::MAX_MESSAGE_LEN).len();
        Ok(Some(json!({
            "status": "sent",
            "target": channel_id,
            "chunks": chunks,
        })))
    }
}

pub struct Typing;
#[async_trait]
impl Tool for Typing {
    fn name(&self) -> &str {
        "discord.typing"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let channel_id = resolve_target(params, ctx, &["channel_id", "chat_id"], "discord")?;
        let token = load_token()?;
        let http = reqwest::Client::new();
        dc::send_typing(&http, &token, &channel_id).await;
        Ok(Some(json!({ "status": "sent", "target": channel_id })))
    }
}

pub struct EditMessage;
#[async_trait]
impl Tool for EditMessage {
    fn name(&self) -> &str {
        "discord.edit_message"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let channel_id = resolve_target(params, ctx, &["channel_id", "chat_id"], "discord")?;
        let message_id = param_str(params, "message_id")
            .ok_or_else(|| anyhow!("discord.edit_message: missing `message_id`"))?
            .to_string();
        let text = param_str(params, "text")
            .ok_or_else(|| anyhow!("discord.edit_message: missing `text`"))?
            .to_string();
        let token = load_token()?;
        let http = reqwest::Client::new();
        let url = format!(
            "{}/channels/{}/messages/{}",
            dc::DISCORD_API,
            channel_id,
            message_id
        );
        let resp = http
            .patch(&url)
            .header("Authorization", format!("Bot {}", token))
            .json(&json!({ "content": text }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "discord.edit_message: PATCH failed: {} {}",
                status,
                body
            ));
        }
        Ok(Some(json!({
            "status": "edited",
            "target": channel_id,
            "message_id": message_id,
        })))
    }
}

pub struct React;
#[async_trait]
impl Tool for React {
    fn name(&self) -> &str {
        "discord.react"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let channel_id = resolve_target(params, ctx, &["channel_id", "chat_id"], "discord")?;
        let message_id = param_str(params, "message_id")
            .ok_or_else(|| anyhow!("discord.react: missing `message_id`"))?
            .to_string();
        let emoji =
            param_str(params, "emoji").ok_or_else(|| anyhow!("discord.react: missing `emoji`"))?;
        let token = load_token()?;
        let http = reqwest::Client::new();
        // For unicode emoji: URL-encode the literal character.
        // For custom guild emoji: the caller passes "name:id" — still URL-encode.
        let encoded = urlencoding::encode(emoji);
        let url = format!(
            "{}/channels/{}/messages/{}/reactions/{}/@me",
            dc::DISCORD_API,
            channel_id,
            message_id,
            encoded
        );
        let resp = http
            .put(&url)
            .header("Authorization", format!("Bot {}", token))
            .header("Content-Length", "0")
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "discord.react: PUT reactions failed: {} {}",
                status,
                body
            ));
        }
        Ok(Some(json!({
            "status": "reacted",
            "target": channel_id,
            "message_id": message_id,
            "emoji": emoji,
        })))
    }
}
