// src/builtins/platforms/feishu.rs
//
// Feishu / Lark outbound messaging builtins. Replaces the legacy
// `feishu_send` and `feishu_webhook` tools that lived in `system.rs`.
// Credential source: `JuglansConfig::load()` → `config.bot.feishu.{app_id,
// app_secret, base_url, webhook_url}`.

#![cfg(not(target_arch = "wasm32"))]

use super::resolve_target;
use crate::builtins::Tool;
use crate::core::context::WorkflowContext;
use crate::services::config::JuglansConfig;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

fn param_str<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(|s| s.trim_matches('"'))
}

/// Load Feishu OpenAPI credentials from the first event-mode channel.
fn load_feishu() -> Result<(String, String, String)> {
    let config = JuglansConfig::load().map_err(|e| anyhow!("load config: {}", e))?;
    let feishu = config
        .channels
        .feishu
        .values()
        .find(|c| c.app_id.is_some() && c.app_secret.is_some())
        .ok_or_else(|| {
            anyhow!("No Feishu event channel configured (need [channels.feishu.<id>] with app_id + app_secret)")
        })?;
    let app_id = feishu
        .app_id
        .clone()
        .ok_or_else(|| anyhow!("Missing app_id"))?;
    let app_secret = feishu
        .app_secret
        .clone()
        .ok_or_else(|| anyhow!("Missing app_secret"))?;
    let base_url = feishu.base_url.clone();
    Ok((app_id, app_secret, base_url))
}

async fn fetch_tenant_token(
    http: &reqwest::Client,
    base_url: &str,
    app_id: &str,
    app_secret: &str,
) -> Result<String> {
    let resp = http
        .post(format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            base_url
        ))
        .json(&json!({
            "app_id": app_id,
            "app_secret": app_secret,
        }))
        .send()
        .await?;
    let body: Value = resp.json().await?;
    body["tenant_access_token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("Feishu tenant_access_token missing: {:?}", body))
}

// ─── feishu.send_message ────────────────────────────────────────────────────

pub struct SendMessage;
#[async_trait]
impl Tool for SendMessage {
    fn name(&self) -> &str {
        "feishu.send_message"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let text = param_str(params, "text")
            .or_else(|| param_str(params, "message"))
            .ok_or_else(|| anyhow!("feishu.send_message: missing `text`"))?
            .to_string();
        let chat_id = resolve_target(params, ctx, &["chat_id"], "feishu")?;

        let (app_id, app_secret, base_url) = load_feishu()?;
        let http = reqwest::Client::new();
        let token = fetch_tenant_token(&http, &base_url, &app_id, &app_secret).await?;

        let content = json!({ "text": text }).to_string();
        let resp = http
            .post(format!(
                "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
                base_url
            ))
            .bearer_auth(&token)
            .json(&json!({
                "receive_id": chat_id,
                "msg_type": "text",
                "content": content,
            }))
            .send()
            .await?;
        let body: Value = resp.json().await?;
        if body["code"].as_i64() != Some(0) {
            return Err(anyhow!("Feishu send error: {:?}", body));
        }

        Ok(Some(json!({
            "status": "sent",
            "target": chat_id,
            "type": "text",
        })))
    }
}

// ─── feishu.send_image ──────────────────────────────────────────────────────

pub struct SendImage;
#[async_trait]
impl Tool for SendImage {
    fn name(&self) -> &str {
        "feishu.send_image"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let image_src = param_str(params, "image")
            .ok_or_else(|| anyhow!("feishu.send_image: missing `image` (URL or local path)"))?
            .to_string();
        let chat_id = resolve_target(params, ctx, &["chat_id"], "feishu")?;

        let (app_id, app_secret, base_url) = load_feishu()?;
        let http = reqwest::Client::new();
        let token = fetch_tenant_token(&http, &base_url, &app_id, &app_secret).await?;

        // 1. Fetch image bytes (URL or local path)
        let image_bytes = if image_src.starts_with("http://") || image_src.starts_with("https://") {
            http.get(&image_src).send().await?.bytes().await?.to_vec()
        } else {
            tokio::fs::read(&image_src)
                .await
                .map_err(|e| anyhow!("Cannot read image file '{}': {}", image_src, e))?
        };

        // 2. Upload to Feishu → image_key
        let form = reqwest::multipart::Form::new()
            .text("image_type", "message")
            .part(
                "image",
                reqwest::multipart::Part::bytes(image_bytes)
                    .file_name("image.png")
                    .mime_str("image/png")?,
            );
        let upload_resp = http
            .post(format!("{}/open-apis/im/v1/images", base_url))
            .bearer_auth(&token)
            .multipart(form)
            .send()
            .await?;
        let upload_body: Value = upload_resp.json().await?;
        let image_key = upload_body["data"]["image_key"]
            .as_str()
            .ok_or_else(|| anyhow!("Feishu image upload failed: {:?}", upload_body))?
            .to_string();

        // 3. Send image message
        let content = json!({ "image_key": image_key }).to_string();
        let send_resp = http
            .post(format!(
                "{}/open-apis/im/v1/messages?receive_id_type=chat_id",
                base_url
            ))
            .bearer_auth(&token)
            .json(&json!({
                "receive_id": chat_id,
                "msg_type": "image",
                "content": content,
            }))
            .send()
            .await?;
        let body: Value = send_resp.json().await?;
        if body["code"].as_i64() != Some(0) {
            return Err(anyhow!("Feishu send image error: {:?}", body));
        }

        Ok(Some(json!({
            "status": "sent",
            "target": chat_id,
            "type": "image",
            "image_key": image_key,
        })))
    }
}

// ─── feishu.send_webhook ────────────────────────────────────────────────────

pub struct SendWebhook;
#[async_trait]
impl Tool for SendWebhook {
    fn name(&self) -> &str {
        "feishu.send_webhook"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let message = param_str(params, "message")
            .or_else(|| param_str(params, "text"))
            .ok_or_else(|| anyhow!("feishu.send_webhook: missing `message`"))?
            .to_string();

        // webhook_url: param → first [channels.feishu.<id>] with incoming_webhook_url
        let webhook_url = if let Some(u) = param_str(params, "webhook_url") {
            u.to_string()
        } else {
            let config = JuglansConfig::load().map_err(|e| anyhow!("load config: {}", e))?;
            config
                .channels
                .feishu
                .values()
                .find_map(|f| f.incoming_webhook_url.clone())
                .ok_or_else(|| {
                    anyhow!(
                        "feishu.send_webhook: no webhook URL — pass `webhook_url=` or set `[channels.feishu.<id>].incoming_webhook_url`"
                    )
                })?
        };

        let http = reqwest::Client::new();
        let resp = http
            .post(&webhook_url)
            .json(&json!({
                "msg_type": "text",
                "content": { "text": message },
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Feishu webhook failed: {} {}", status, body));
        }

        Ok(Some(json!({
            "status": "sent",
            "type": "webhook",
        })))
    }
}
