// src/builtins/platforms/mod.rs
//
// Per-platform outbound messaging builtins, registered under dotted names
// (`telegram.send_message`, `discord.send_message`, ...). Matches the
// `db.*` / `history.*` convention. Each platform module defines its own
// Tool impls that delegate to `pub(crate)` helpers in the corresponding
// `src/adapters/<platform>.rs`.
//
// Common behavior:
//   - Target (chat_id / channel_id / user_id) is optional in each tool;
//     when absent it falls back to `input.platform_chat_id` (set by the
//     adapter on inbound messages). This lets a reply branch in a bot
//     workflow push without re-threading the target.
//   - Credentials come from `JuglansConfig::load()` → `config.bot.<platform>`.
//   - Return value: `{ "status": "sent", "target": "<resolved>", ... }`.

#![cfg(not(target_arch = "wasm32"))]

use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};

pub mod discord;
pub mod feishu;
pub mod telegram;
pub mod wechat;

/// Resolve the target id for a send-like builtin. Looks up the first
/// present param key; if none, falls back to `input.platform_chat_id`.
/// Returns an error if neither source yields a non-empty value.
pub(crate) fn resolve_target(
    params: &std::collections::HashMap<String, String>,
    ctx: &WorkflowContext,
    keys: &[&str],
    platform_hint: &str,
) -> Result<String> {
    for k in keys {
        if let Some(v) = params.get(*k) {
            let trimmed = v.trim_matches('"').trim();
            if !trimmed.is_empty() && !trimmed.starts_with("[Missing:") {
                return Ok(trimmed.to_string());
            }
        }
    }
    if let Ok(Some(v)) = ctx.resolve_path("input.platform_chat_id") {
        if let Some(s) = v.as_str() {
            if !s.is_empty() {
                return Ok(s.to_string());
            }
        }
    }
    Err(anyhow!(
        "{}.send_message: no target — pass `{}` explicitly, or run from a bot workflow where `input.platform_chat_id` is set",
        platform_hint,
        keys.first().copied().unwrap_or("chat_id")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mk(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn explicit_param_wins_over_context() {
        let ctx = WorkflowContext::new();
        ctx.set("input.platform_chat_id".into(), json!("from_ctx"))
            .ok();
        let params = mk(&[("chat_id", "explicit")]);
        let out = resolve_target(&params, &ctx, &["chat_id"], "telegram").unwrap();
        assert_eq!(out, "explicit");
    }

    #[test]
    fn falls_back_to_context_platform_chat_id() {
        let ctx = WorkflowContext::new();
        ctx.set("input.platform_chat_id".into(), json!("from_ctx"))
            .ok();
        let params = mk(&[]);
        let out = resolve_target(&params, &ctx, &["chat_id"], "telegram").unwrap();
        assert_eq!(out, "from_ctx");
    }

    #[test]
    fn first_matching_key_wins() {
        let ctx = WorkflowContext::new();
        let params = mk(&[("channel_id", "chan"), ("chat_id", "chat")]);
        let out = resolve_target(&params, &ctx, &["channel_id", "chat_id"], "discord").unwrap();
        assert_eq!(out, "chan");
    }

    #[test]
    fn skips_empty_and_missing_markers() {
        let ctx = WorkflowContext::new();
        ctx.set("input.platform_chat_id".into(), json!("from_ctx"))
            .ok();
        let params = mk(&[("chat_id", "[Missing: foo.bar]")]);
        let out = resolve_target(&params, &ctx, &["chat_id"], "telegram").unwrap();
        assert_eq!(out, "from_ctx");
    }

    #[test]
    fn empty_everywhere_is_an_error() {
        let ctx = WorkflowContext::new();
        let params = mk(&[]);
        let err = resolve_target(&params, &ctx, &["chat_id"], "telegram").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("telegram.send_message"));
        assert!(msg.contains("chat_id"));
    }

    #[test]
    fn strips_surrounding_quotes() {
        let ctx = WorkflowContext::new();
        let params = mk(&[("chat_id", "\"12345\"")]);
        let out = resolve_target(&params, &ctx, &["chat_id"], "telegram").unwrap();
        assert_eq!(out, "12345");
    }
}
