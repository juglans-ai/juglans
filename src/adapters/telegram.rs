// src/adapters/telegram.rs
#![cfg(not(target_arch = "wasm32"))]

use anyhow::Result;
use dashmap::DashSet;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info};

use axum::Json;

use super::{run_agent_for_message, Channel, MessageDispatcher, PlatformMessage};
use crate::services::config::JuglansConfig;

// ======================================================================
// Public webhook handler (for web_server.rs serverless integration)
// ======================================================================

/// Telegram webhook handler, embeddable in web_server.
///
/// Receives Telegram Bot API webhook pushes, processes messages and replies.
/// Suitable for serverless deployment (FC containers are not suited for long-polling).
pub struct TelegramWebhookHandler {
    config: JuglansConfig,
    project_root: PathBuf,
    agent_slug: String,
    token: String,
    processed_updates: DashSet<i64>,
}

impl TelegramWebhookHandler {
    /// Variant that threads a channel egress reference so workflow `reply()`
    /// calls route back through Telegram. The plain `handle_update` stays as
    /// the no-origin fallback.
    pub async fn handle_update_with_channel(
        &self,
        body: Value,
        channel: Arc<dyn crate::core::context::ChannelEgress>,
    ) -> Value {
        self.handle_update_inner(body, Some(channel)).await
    }

    /// Handle Telegram webhook Update JSON, return response.
    /// No-origin fallback used by juglans-wallet–style external orchestrators;
    /// the unified channel path goes through [`handle_update_with_channel`].
    #[allow(dead_code)]
    pub async fn handle_update(&self, body: Value) -> Value {
        self.handle_update_inner(body, None).await
    }

    async fn handle_update_inner(
        &self,
        body: Value,
        channel: Option<Arc<dyn crate::core::context::ChannelEgress>>,
    ) -> Value {
        let update_id = body["update_id"].as_i64().unwrap_or(0);

        // Deduplication
        if update_id != 0 && !self.processed_updates.insert(update_id) {
            return json!({"ok": true, "description": "duplicate"});
        }

        // Extract message
        let msg = match body.get("message") {
            Some(m) => m,
            None => return json!({"ok": true}),
        };

        let text = msg["text"].as_str().unwrap_or("").to_string();
        if text.is_empty() {
            return json!({"ok": true});
        }

        let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0);
        let user_id = msg["from"]["id"].as_i64().unwrap_or(0).to_string();
        let username = msg["from"]["username"].as_str().map(|s| s.to_string());
        let first_name = msg["from"]["first_name"].as_str().unwrap_or("User");

        info!(
            "[Telegram Webhook] {} (@{}): {}",
            first_name,
            username.as_deref().unwrap_or("?"),
            if text.chars().count() > 50 {
                &text[..text
                    .char_indices()
                    .nth(50)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len())]
            } else {
                &text
            }
        );

        let platform_msg = PlatformMessage {
            event_type: "message".into(),
            event_data: json!({ "text": &text }),
            platform_user_id: user_id,
            platform_chat_id: chat_id.to_string(),
            text,
            username,
            platform: "telegram".into(),
        };

        // Process asynchronously (don't block webhook response)
        let config = self.config.clone();
        let project_root = self.project_root.clone();
        let agent_slug = self.agent_slug.clone();
        let token = self.token.clone();
        let origin = channel.map(|ch| crate::core::context::ChannelOrigin {
            channel: ch,
            conversation: chat_id.to_string(),
        });

        tokio::spawn(async move {
            let base_url = format!("https://api.telegram.org/bot{}", token);
            let client = reqwest::Client::new();

            // Send typing status
            let _ = client
                .post(format!("{}/sendChatAction", base_url))
                .json(&json!({"chat_id": chat_id, "action": "typing"}))
                .send()
                .await;

            let result =
                run_agent_for_message(
                    &config,
                    &project_root,
                    &agent_slug,
                    &platform_msg,
                    None,
                    origin,
                )
                .await;

            match result {
                Ok(reply) => {
                    if reply.text.is_empty() || reply.text == "(No response)" {
                        return;
                    }
                    let chunks = split_message(&reply.text, 4096);
                    for chunk in chunks {
                        let send_result = client
                            .post(format!("{}/sendMessage", base_url))
                            .json(&json!({
                                "chat_id": chat_id,
                                "text": chunk,
                                "parse_mode": "Markdown"
                            }))
                            .send()
                            .await;

                        if let Err(e) = send_result {
                            error!("[Telegram Webhook] Send failed: {}", e);
                            let _ = client
                                .post(format!("{}/sendMessage", base_url))
                                .json(&json!({"chat_id": chat_id, "text": chunk}))
                                .send()
                                .await;
                        }
                    }
                }
                Err(e) => {
                    error!("[Telegram Webhook] Agent error: {}", e);
                    let _ = client
                        .post(format!("{}/sendMessage", base_url))
                        .json(&json!({"chat_id": chat_id, "text": format!("Error: {}", e)}))
                        .send()
                        .await;
                }
            }
        });

        json!({"ok": true})
    }
}

/// One Telegram bot identity = one [`TelegramChannel`].
///
/// `id()` is the stable channel identifier `"telegram:<bot_id_prefix>"` derived
/// from the public bot id portion of the token (the digits before the colon).
/// The verified `@username` is captured during `run()` and used only in log
/// formatting, never as the channel id — keeping the id stable from construction
/// matters more than user-friendliness for orchestrator maps and metrics.
pub struct TelegramChannel {
    id: String,
    token: String,
    client: reqwest::Client,
}

impl TelegramChannel {
    pub fn new(token: String) -> Self {
        // Stable id derived from the public bot id prefix. Avoids API call at
        // construction; the secret half of the token is never logged.
        let bot_id = token.split(':').next().unwrap_or("unknown");
        Self {
            id: format!("telegram:{}", bot_id),
            token,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl crate::core::context::ChannelEgress for TelegramChannel {
    async fn send(&self, conversation: &str, text: &str) -> Result<()> {
        send_message_api(
            &self.client,
            &self.token,
            conversation,
            text,
            Some("Markdown"),
        )
        .await
        .map(|_| ())
    }

    async fn start_stream(
        &self,
        conversation: &str,
    ) -> Result<Box<dyn crate::core::context::StreamHandle>> {
        Ok(Box::new(TelegramStreamHandle::new(
            self.client.clone(),
            self.token.clone(),
            conversation.to_string(),
        )))
    }
}

#[async_trait::async_trait]
impl Channel for TelegramChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "telegram"
    }

    async fn run(self: Arc<Self>, dispatcher: Arc<dyn MessageDispatcher>) -> Result<()> {
        // Auto-inject ChannelOrigin into every dispatched message so workflows
        // calling `reply()` route their output back through this channel.
        let dispatcher = Arc::new(super::OriginAwareDispatcher::new(
            self.clone(),
            dispatcher,
        )) as Arc<dyn MessageDispatcher>;

        let base_url = format!("https://api.telegram.org/bot{}", self.token);

        // Verify token + capture bot username for nicer logs.
        let me_resp: serde_json::Value = self
            .client
            .get(format!("{}/getMe", base_url))
            .send()
            .await?
            .json()
            .await?;
        if me_resp["ok"].as_bool() != Some(true) {
            return Err(anyhow::anyhow!("Invalid Telegram bot token: {:?}", me_resp));
        }
        let bot_name = me_resp["result"]["username"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        info!("🤖 Telegram Bot @{} ready, waiting for updates", bot_name);

        let mut offset: i64 = 0;
        loop {
            let updates: serde_json::Value = match self
                .client
                .get(format!("{}/getUpdates", base_url))
                .query(&[
                    ("offset", offset.to_string()),
                    ("timeout", "30".to_string()),
                ])
                .send()
                .await
            {
                Ok(resp) => match resp.json().await {
                    Ok(v) => v,
                    Err(e) => {
                        error!("[telegram:{}] failed to parse updates: {}", bot_name, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        continue;
                    }
                },
                Err(e) => {
                    error!("[telegram:{}] failed to get updates: {}", bot_name, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if let Some(results) = updates["result"].as_array() {
                for update in results {
                    let update_id = update["update_id"].as_i64().unwrap_or(0);
                    offset = update_id + 1;

                    let msg = match update.get("message") {
                        Some(m) => m,
                        None => continue,
                    };
                    let text = msg["text"].as_str().unwrap_or("").to_string();
                    if text.is_empty() {
                        continue;
                    }
                    let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0);
                    let user_id = msg["from"]["id"].as_i64().unwrap_or(0).to_string();
                    let username = msg["from"]["username"].as_str().map(|s| s.to_string());
                    let first_name = msg["from"]["first_name"].as_str().unwrap_or("User");

                    info!(
                        "📩 [telegram:{}] {} (@{}): {}",
                        bot_name,
                        first_name,
                        username.as_deref().unwrap_or("?"),
                        if text.len() > 50 { &text[..50] } else { &text }
                    );

                    let dispatcher = dispatcher.clone();
                    let client = self.client.clone();
                    let base_url = base_url.clone();
                    let bot_name = bot_name.clone();

                    tokio::spawn(async move {
                        let platform_msg = PlatformMessage {
                            event_type: "message".into(),
                            event_data: json!({ "text": &text }),
                            platform_user_id: user_id,
                            platform_chat_id: chat_id.to_string(),
                            text,
                            username,
                            platform: "telegram".into(),
                        };

                        let _ = client
                            .post(format!("{}/sendChatAction", base_url))
                            .json(&json!({ "chat_id": chat_id, "action": "typing" }))
                            .send()
                            .await;

                        match dispatcher.dispatch(&platform_msg).await {
                            Ok(reply) => {
                                for chunk in split_message(&reply.text, 4096) {
                                    let send_result = client
                                        .post(format!("{}/sendMessage", base_url))
                                        .json(&json!({
                                            "chat_id": chat_id,
                                            "text": chunk,
                                            "parse_mode": "Markdown"
                                        }))
                                        .send()
                                        .await;
                                    if let Err(e) = send_result {
                                        error!(
                                            "[telegram:{}] sendMessage failed: {} — retrying without parse_mode",
                                            bot_name, e
                                        );
                                        let _ = client
                                            .post(format!("{}/sendMessage", base_url))
                                            .json(&json!({
                                                "chat_id": chat_id,
                                                "text": chunk
                                            }))
                                            .send()
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("[telegram:{}] agent error: {}", bot_name, e);
                                let _ = client
                                    .post(format!("{}/sendMessage", base_url))
                                    .json(&json!({
                                        "chat_id": chat_id,
                                        "text": format!("❌ Error: {}", e)
                                    }))
                                    .send()
                                    .await;
                            }
                        }
                    });
                }
            }
        }
    }
}

/// Build Telegram channel instances from `juglans.toml`. Per-instance mode
/// selection:
///
/// - `mode = "polling"` → [`TelegramChannel`] (active long-poll)
/// - `mode = "webhook"` → [`TelegramWebhookChannel`] (passive HTTP route)
/// - `mode` unset → automatic: webhook when `server.endpoint_url` is set
///   (production deployment), polling otherwise (local dev).
pub fn discover_channels(
    config: &JuglansConfig,
    project_root: &Path,
) -> Vec<(Arc<dyn Channel>, String)> {
    let mut tokens_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<(Arc<dyn Channel>, String)> = Vec::new();
    let auto_webhook = config.server.endpoint_url.is_some();

    for (instance_id, cfg) in &config.channels.telegram {
        if cfg.token.is_empty() || !tokens_seen.insert(cfg.token.clone()) {
            continue;
        }
        let mode = cfg.mode.as_deref().unwrap_or("");
        let use_webhook = match mode {
            "webhook" => true,
            "polling" => false,
            _ => auto_webhook,
        };

        let channel: Arc<dyn Channel> = if use_webhook {
            let handler = TelegramWebhookHandler {
                config: config.clone(),
                project_root: project_root.to_path_buf(),
                agent_slug: cfg.agent.clone(),
                token: cfg.token.clone(),
                processed_updates: DashSet::new(),
            };
            Arc::new(TelegramWebhookChannel::new(instance_id.clone(), handler))
        } else {
            Arc::new(TelegramChannel::new(cfg.token.clone()))
        };
        out.push((channel, cfg.agent.clone()));
    }

    out
}


/// Telegram Bot API base URL.
pub(crate) const TELEGRAM_API: &str = "https://api.telegram.org";

/// Telegram message character limit.
pub(crate) const TELEGRAM_MAX_LEN: usize = 4096;

/// Send a text message to a Telegram chat. Falls back to plain-text if
/// `parse_mode`-formatted send fails (typical cause: malformed Markdown).
/// Chunks long messages using `split_message`.
pub(crate) async fn send_message_api(
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    text: &str,
    parse_mode: Option<&str>,
) -> anyhow::Result<usize> {
    let base_url = format!("{}/bot{}", TELEGRAM_API, token);
    let chunks = split_message(text, TELEGRAM_MAX_LEN);
    let chunk_count = chunks.len();
    for chunk in chunks {
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": chunk,
        });
        if let Some(pm) = parse_mode {
            body["parse_mode"] = serde_json::json!(pm);
        }
        let resp = http
            .post(format!("{}/sendMessage", base_url))
            .json(&body)
            .send()
            .await?;
        if resp.status().is_success() {
            continue;
        }
        // Fallback without parse_mode
        let resp2 = http
            .post(format!("{}/sendMessage", base_url))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
            }))
            .send()
            .await?;
        if !resp2.status().is_success() {
            let status = resp2.status();
            let err = resp2.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Telegram sendMessage failed: {} {}",
                status,
                err
            ));
        }
    }
    Ok(chunk_count)
}

/// Send typing action (auto-expires after a few seconds, best-effort).
pub(crate) async fn send_typing(http: &reqwest::Client, token: &str, chat_id: &str) {
    let base_url = format!("{}/bot{}", TELEGRAM_API, token);
    let _ = http
        .post(format!("{}/sendChatAction", base_url))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "action": "typing",
        }))
        .send()
        .await;
}

/// Edit a previously-sent message.
pub(crate) async fn edit_message_api(
    http: &reqwest::Client,
    token: &str,
    chat_id: &str,
    message_id: i64,
    text: &str,
    parse_mode: Option<&str>,
) -> anyhow::Result<()> {
    let base_url = format!("{}/bot{}", TELEGRAM_API, token);
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": text,
    });
    if let Some(pm) = parse_mode {
        body["parse_mode"] = serde_json::json!(pm);
    }
    let resp = http
        .post(format!("{}/editMessageText", base_url))
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Telegram editMessageText failed: {} {}",
            status,
            err
        ));
    }
    Ok(())
}

/// Split long message into chunks (Telegram limit: 4096 characters).
/// Prefers splitting at a newline, falls back to `max_len`.
pub(crate) fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        // Try to split at a newline
        let split_pos = remaining[..max_len].rfind('\n').unwrap_or(max_len);

        chunks.push(remaining[..split_pos].to_string());
        remaining = remaining[split_pos..].trim_start();
    }

    chunks
}

// ======================================================================
// Channel impls — uniform Channel API
// ======================================================================

/// Passive Telegram channel: mounts a webhook route at
/// `/webhook/telegram/<instance_id>` for the platform to POST updates to.
/// Egress uses the Telegram Bot API.
pub struct TelegramWebhookChannel {
    id: String,
    instance_id: String,
    handler: Arc<TelegramWebhookHandler>,
    #[allow(dead_code)] // read in `send` (currently uncallable until point 9)
    token: String,
}

impl TelegramWebhookChannel {
    pub fn new(instance_id: String, handler: TelegramWebhookHandler) -> Self {
        let token = handler.token.clone();
        Self {
            id: format!("telegram:{}", instance_id),
            instance_id,
            handler: Arc::new(handler),
            token,
        }
    }
}

#[async_trait::async_trait]
impl Channel for TelegramWebhookChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "telegram"
    }

    fn install_routes(
        self: Arc<Self>,
        router: axum::Router,
        _dispatcher: Arc<dyn MessageDispatcher>,
    ) -> axum::Router {
        let path = format!("/webhook/telegram/{}", self.instance_id);
        let handler = self.handler.clone();
        let channel: Arc<dyn crate::core::context::ChannelEgress> = self.clone();
        router.route(
            &path,
            axum::routing::post(move |Json(body): Json<Value>| {
                let handler = handler.clone();
                let channel = channel.clone();
                async move { Json(handler.handle_update_with_channel(body, channel).await) }
            }),
        )
    }
}

#[async_trait::async_trait]
impl crate::core::context::ChannelEgress for TelegramWebhookChannel {
    async fn send(&self, conversation: &str, text: &str) -> Result<()> {
        send_message_api(
            &reqwest::Client::new(),
            &self.token,
            conversation,
            text,
            Some("Markdown"),
        )
        .await
        .map(|_| ())
    }

    async fn start_stream(
        &self,
        conversation: &str,
    ) -> Result<Box<dyn crate::core::context::StreamHandle>> {
        Ok(Box::new(TelegramStreamHandle::new(
            reqwest::Client::new(),
            self.token.clone(),
            conversation.to_string(),
        )))
    }
}

// ─── Streaming reply (Phase 4) ──────────────────────────────────────────────
//
// Streams a chat() / reply() output token-by-token to a Telegram chat. The
// underlying transport is `sendMessage` for the first chunk + `editMessageText`
// for subsequent updates, debounced to ~1Hz to stay under TG's edit rate
// limit. Works on both DMs and groups uniformly.
//
// (sendMessageDraft, introduced in Bot API 9.3 (Dec 2025) and fully opened in
// 9.5 (Mar 2026), provides a smoother native draft-bubble experience in DMs
// and is a planned future optimization. The current edit-based implementation
// is universally compatible — no API version dependency.)

const TELEGRAM_STREAM_EDIT_INTERVAL_MS: u128 = 1000;

struct TelegramStreamHandle {
    http: reqwest::Client,
    token: String,
    chat_id: String,
    /// Accumulated text. Each token is appended; sends/edits use the full buffer.
    buffer: String,
    /// `Some(id)` once the initial sendMessage has succeeded.
    message_id: Option<i64>,
    /// Wall-clock instant of the most recent successful send/edit. Used to
    /// debounce subsequent edits.
    last_edit: std::time::Instant,
    /// True when the buffer changed since the last send/edit. `finalize`
    /// uses this to decide whether one more edit is needed.
    pending: bool,
}

impl TelegramStreamHandle {
    fn new(http: reqwest::Client, token: String, chat_id: String) -> Self {
        Self {
            http,
            token,
            chat_id,
            buffer: String::new(),
            message_id: None,
            last_edit: std::time::Instant::now(),
            pending: false,
        }
    }

    async fn send_initial(&mut self) -> Result<()> {
        let base_url = format!("{}/bot{}", TELEGRAM_API, self.token);
        let resp = self
            .http
            .post(format!("{}/sendMessage", base_url))
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "text": self.buffer,
            }))
            .send()
            .await?;
        let body: Value = resp.json().await.unwrap_or(serde_json::json!({}));
        if body["ok"].as_bool() != Some(true) {
            return Err(anyhow::anyhow!(
                "Telegram sendMessage failed: {}",
                body
            ));
        }
        let id = body
            .pointer("/result/message_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                anyhow::anyhow!("Telegram sendMessage: no message_id in response")
            })?;
        self.message_id = Some(id);
        self.last_edit = std::time::Instant::now();
        self.pending = false;
        Ok(())
    }

    async fn flush_edit(&mut self) -> Result<()> {
        if let Some(id) = self.message_id {
            edit_message_api(&self.http, &self.token, &self.chat_id, id, &self.buffer, None)
                .await?;
            self.last_edit = std::time::Instant::now();
            self.pending = false;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::core::context::StreamHandle for TelegramStreamHandle {
    async fn push_token(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.buffer.push_str(text);
        self.pending = true;

        if self.message_id.is_none() {
            // First non-empty token: kick off the message.
            return self.send_initial().await;
        }

        // Debounce: skip edits that arrive within the rate-limit window.
        // The pending flag ensures the buffer state is flushed on finalize
        // even if the last few tokens land inside the debounce window.
        if self.last_edit.elapsed().as_millis() >= TELEGRAM_STREAM_EDIT_INTERVAL_MS {
            self.flush_edit().await?;
        }
        Ok(())
    }

    async fn finalize(mut self: Box<Self>) -> Result<()> {
        // Empty stream: nothing was ever sent. Skip — treating empty as "no
        // message" matches what `send` would do and keeps the chat clean.
        if self.message_id.is_none() {
            return Ok(());
        }
        if self.pending {
            self.flush_edit().await?;
        }
        Ok(())
    }
}
