// src/adapters/telegram.rs
#![cfg(not(target_arch = "wasm32"))]

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

use crate::services::config::JuglansConfig;
use super::{run_agent_for_message, PlatformMessage};

/// 启动 Telegram Bot（long polling 模式）
pub async fn start(config: JuglansConfig, project_root: PathBuf, agent_slug: String) -> Result<()> {
    let bot_config = config.bot.as_ref()
        .and_then(|b| b.telegram.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Missing [bot.telegram] config in juglans.toml"))?;

    let token = bot_config.token.clone();

    info!("🤖 Starting Telegram Bot...");
    info!("   Agent: {}", agent_slug);

    let client = reqwest::Client::new();
    let base_url = format!("https://api.telegram.org/bot{}", token);

    // 验证 token
    let me_resp: serde_json::Value = client
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
        .unwrap_or("unknown");
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

                // 提取消息
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

                // 异步处理消息
                let config = config.clone();
                let project_root = project_root.clone();
                let agent_slug = agent_slug.clone();
                let client = client.clone();
                let base_url = base_url.clone();

                tokio::spawn(async move {
                    let platform_msg = PlatformMessage {
                        platform_user_id: user_id,
                        platform_chat_id: chat_id.to_string(),
                        text,
                        username,
                    };

                    // 发送 "typing" 状态
                    let _ = client
                        .post(format!("{}/sendChatAction", base_url))
                        .json(&serde_json::json!({
                            "chat_id": chat_id,
                            "action": "typing"
                        }))
                        .send()
                        .await;

                    match run_agent_for_message(&config, &project_root, &agent_slug, &platform_msg)
                        .await
                    {
                        Ok(reply) => {
                            // 分段发送（Telegram 消息最大 4096 字符）
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
                                    // 降级：不带 parse_mode 重试
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

/// 将长消息分割为多段（Telegram 限制 4096 字符）
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

        // 尝试在换行处分割
        let split_pos = remaining[..max_len]
            .rfind('\n')
            .unwrap_or(max_len);

        chunks.push(remaining[..split_pos].to_string());
        remaining = &remaining[split_pos..].trim_start();
    }

    chunks
}
