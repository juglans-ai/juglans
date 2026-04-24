// src/builtins/platforms/wechat.rs

#![cfg(not(target_arch = "wasm32"))]

use super::resolve_target;
use crate::adapters::wechat as wx;
use crate::builtins::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;

fn param_str<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(|s| s.trim_matches('"'))
}

pub struct SendMessage;
#[async_trait]
impl Tool for SendMessage {
    fn name(&self) -> &str {
        "wechat.send_message"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        ctx: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let text = param_str(params, "text")
            .ok_or_else(|| anyhow!("wechat.send_message: missing `text`"))?
            .to_string();
        let user_id = resolve_target(params, ctx, &["user_id", "chat_id", "to"], "wechat")?;

        // WeChat credentials live in the QR-login session file, not the toml.
        let project_root = env::current_dir()?;
        let session = wx::load_session(&project_root).ok_or_else(|| {
            anyhow!(
                "wechat.send_message: no WeChat session found in .juglans/wechat/. \
                 Run `juglans bot wechat` once to complete QR login."
            )
        })?;

        let http = reqwest::Client::new();
        wx::send_text_message(
            &http,
            &session.base_url,
            &session.token,
            &user_id,
            &text,
            None,
        )
        .await?;

        Ok(Some(json!({
            "status": "sent",
            "target": user_id,
        })))
    }
}
