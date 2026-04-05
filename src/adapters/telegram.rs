// src/adapters/telegram.rs
#![cfg(not(target_arch = "wasm32"))]

use anyhow::Result;
use dashmap::DashSet;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, info};

use super::{chat_via_jug0, run_agent_for_message, PlatformMessage};
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
    use_jug0: bool,
    processed_updates: DashSet<i64>,
}

impl TelegramWebhookHandler {
    /// Create from JuglansConfig; returns Some if Telegram config is complete
    pub fn from_config(config: &JuglansConfig, project_root: &Path) -> Option<Self> {
        let bot_config = config.bot.as_ref()?.telegram.as_ref()?;
        let token = bot_config.token.clone();
        let agent_slug = bot_config.agent.clone();

        let use_jug0 = match bot_config.mode.as_deref() {
            Some("local") => false,
            Some("jug0") => true,
            _ => !config.jug0.base_url.is_empty(),
        };

        Some(Self {
            config: config.clone(),
            project_root: project_root.to_path_buf(),
            agent_slug,
            token,
            use_jug0,
            processed_updates: DashSet::new(),
        })
    }

    /// Handle Telegram webhook Update JSON, return response
    pub async fn handle_update(&self, body: Value) -> Value {
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
        let use_jug0 = self.use_jug0;

        tokio::spawn(async move {
            let base_url = format!("https://api.telegram.org/bot{}", token);
            let client = reqwest::Client::new();

            // Send typing status
            let _ = client
                .post(format!("{}/sendChatAction", base_url))
                .json(&json!({"chat_id": chat_id, "action": "typing"}))
                .send()
                .await;

            let result = if use_jug0 {
                chat_via_jug0(
                    &config,
                    &agent_slug,
                    &platform_msg,
                    &super::NoopToolExecutor,
                )
                .await
            } else {
                run_agent_for_message(&config, &project_root, &agent_slug, &platform_msg, None)
                    .await
            };

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

/// Start Telegram Bot (long polling mode)
pub async fn start(config: JuglansConfig, project_root: PathBuf, agent_slug: String) -> Result<()> {
    let bot_config = config
        .bot
        .as_ref()
        .and_then(|b| b.telegram.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Missing [bot.telegram] config in juglans.toml"))?;

    let token = bot_config.token.clone();

    info!("🤖 Starting Telegram Bot...");
    info!("   Agent: {}", agent_slug);

    let client = reqwest::Client::new();
    let base_url = format!("https://api.telegram.org/bot{}", token);

    // Verify token
    let me_resp: serde_json::Value = client
        .get(format!("{}/getMe", base_url))
        .send()
        .await?
        .json()
        .await?;

    if me_resp["ok"].as_bool() != Some(true) {
        return Err(anyhow::anyhow!("Invalid Telegram bot token: {:?}", me_resp));
    }

    let bot_name = me_resp["result"]["username"].as_str().unwrap_or("unknown");
    info!("   Bot: @{}", bot_name);
    info!("   Ready! Waiting for messages...");

    let config = Arc::new(config);
    let project_root = Arc::new(project_root);
    let agent_slug = Arc::new(agent_slug);

    // Long polling loop
    let mut offset: i64 = 0;

    loop {
        let updates: serde_json::Value = match client
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
                    error!("Failed to parse updates: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            },
            Err(e) => {
                error!("Failed to get updates: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        if let Some(results) = updates["result"].as_array() {
            for update in results {
                let update_id = update["update_id"].as_i64().unwrap_or(0);
                offset = update_id + 1;

                // Extract message
                let msg = if let Some(m) = update.get("message") {
                    m
                } else {
                    continue;
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
                    "📩 [Telegram] {} (@{}): {}",
                    first_name,
                    username.as_deref().unwrap_or("?"),
                    if text.len() > 50 { &text[..50] } else { &text }
                );

                // Process message asynchronously
                let config = config.clone();
                let project_root = project_root.clone();
                let agent_slug = agent_slug.clone();
                let client = client.clone();
                let base_url = base_url.clone();

                tokio::spawn(async move {
                    let platform_msg = PlatformMessage {
                        event_type: "message".into(),
                        event_data: serde_json::json!({ "text": &text }),
                        platform_user_id: user_id,
                        platform_chat_id: chat_id.to_string(),
                        text,
                        username,
                        platform: "telegram".into(),
                    };

                    // Send "typing" status
                    let _ = client
                        .post(format!("{}/sendChatAction", base_url))
                        .json(&serde_json::json!({
                            "chat_id": chat_id,
                            "action": "typing"
                        }))
                        .send()
                        .await;

                    match run_agent_for_message(
                        &config,
                        &project_root,
                        &agent_slug,
                        &platform_msg,
                        None,
                    )
                    .await
                    {
                        Ok(reply) => {
                            // Send in chunks (Telegram message limit: 4096 characters)
                            let chunks = split_message(&reply.text, 4096);
                            for chunk in chunks {
                                let send_result = client
                                    .post(format!("{}/sendMessage", base_url))
                                    .json(&serde_json::json!({
                                        "chat_id": chat_id,
                                        "text": chunk,
                                        "parse_mode": "Markdown"
                                    }))
                                    .send()
                                    .await;

                                if let Err(e) = send_result {
                                    error!("Failed to send message: {}", e);
                                    // Fallback: retry without parse_mode
                                    let _ = client
                                        .post(format!("{}/sendMessage", base_url))
                                        .json(&serde_json::json!({
                                            "chat_id": chat_id,
                                            "text": chunk
                                        }))
                                        .send()
                                        .await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Agent execution failed: {}", e);
                            let _ = client
                                .post(format!("{}/sendMessage", base_url))
                                .json(&serde_json::json!({
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

/// Split long message into chunks (Telegram limit: 4096 characters)
fn split_message(text: &str, max_len: usize) -> Vec<String> {
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
