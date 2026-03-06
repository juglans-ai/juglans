// src/builtins/system.rs
use super::Tool;
use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::services::interface::JuglansRuntime;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// 将字符串解析为 context 值，保留超大整数为字符串以避免 f64 精度丢失。
fn parse_context_value(value_str: &str) -> Value {
    match serde_json::from_str::<Value>(value_str) {
        Ok(Value::Number(n))
            if n.as_f64()
                .map(|f| f.abs() > 9_007_199_254_740_992.0)
                .unwrap_or(false)
                && value_str.bytes().all(|b| b.is_ascii_digit() || b == b'-') =>
        {
            // 超过 f64 精度的大整数（如 Google/Apple user ID），保留为字符串
            json!(value_str)
        }
        Ok(v) => v,
        Err(_) => json!(value_str),
    }
}

pub struct Timer;
#[async_trait]
impl Tool for Timer {
    fn name(&self) -> &str {
        "timer"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Support both 'ms' (preferred) and 'seconds' (backward compatible)
        let duration_ms: u64 = if let Some(ms) = params.get("ms") {
            ms.parse().unwrap_or(1000)
        } else if let Some(secs) = params.get("seconds") {
            secs.parse::<u64>().unwrap_or(1) * 1000
        } else {
            1000 // default 1 second
        };

        if !context.has_event_sender() {
            println!("⏳ Sleeping for {} ms...", duration_ms);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
        Ok(Some(
            json!({ "status": "finished", "duration_ms": duration_ms }),
        ))
    }
}

pub struct SetContext;
#[async_trait]
impl Tool for SetContext {
    fn name(&self) -> &str {
        "set_context"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // 支持两种模式：
        // 1. 传统模式：set_context(path="key", value="val")
        // 2. 多字段模式：set_context(key1=$input.val1, key2=$input.val2)

        if let (Some(path), Some(value_str)) = (params.get("path"), params.get("value")) {
            // 传统模式
            let value = parse_context_value(value_str);
            let stripped_path = path.strip_prefix("$ctx.").unwrap_or(path).trim_matches('"');
            context.set(stripped_path.to_string(), value)?;
        } else {
            // 多字段模式：每个 key=value 对都设置到 ctx 中
            for (key, value_str) in params {
                // 跳过保留字段
                if key == "path" || key == "value" {
                    continue;
                }
                let value = parse_context_value(value_str);
                context.set(key.clone(), value)?;
            }
        }
        Ok(None)
    }
}

pub struct Notify;
#[async_trait]
impl Tool for Notify {
    fn name(&self) -> &str {
        "notify"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // 如果传入 status，则更新 ctx.reply.status，实现透明思维流
        if let Some(status) = params.get("status") {
            context.set("reply.status".to_string(), json!(status))?;
            if !context.has_event_sender() {
                println!("💡 [Status] {}", status);
            }
        }

        let msg = params.get("message").map(|s| s.as_str()).unwrap_or("");
        if !msg.is_empty() {
            if !context.has_event_sender() {
                println!("🔔 [Notification] {}", msg);
            }
        }

        Ok(Some(json!({ "status": "sent", "content": msg })))
    }
}

/// print(message="text") — 纯输出，无 emoji 前缀，不修改 context
/// 与 notify 的区别：print 只做 println，适合调试和 Hello World
pub struct Print;
#[async_trait]
impl Tool for Print {
    fn name(&self) -> &str {
        "print"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let msg = params
            .get("message")
            .or_else(|| params.get("value"))
            .map(|s| s.as_str())
            .unwrap_or("");
        if !context.has_event_sender() {
            println!("{}", msg);
        }
        Ok(Some(json!(msg)))
    }
}

/// reply(message="内容", state="context_visible") - 直接返回内容，不调用 AI
/// 用于系统事件处理等场景，需要返回固定文本但不走 LLM
/// 支持 state 参数控制 SSE/持久化，包括组合语法 input:output
pub struct Reply {
    runtime: Arc<dyn JuglansRuntime>,
}

impl Reply {
    pub fn new(runtime: Arc<dyn JuglansRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl Tool for Reply {
    fn name(&self) -> &str {
        "reply"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let message = params.get("message").map(|s| s.as_str()).unwrap_or("");

        // 支持组合语法 input:output（与 chat() 一致）
        let state_raw = params
            .get("state")
            .map(|s| s.as_str())
            .unwrap_or("context_visible");
        let (input_state, output_state) = match state_raw.split_once(':') {
            Some((i, o)) => (i, o),
            None => (state_raw, state_raw),
        };

        // should_stream 基于 output_state
        let should_stream = output_state == "context_visible" || output_state == "display_only";

        // SSE 输出
        if should_stream {
            context.emit(WorkflowEvent::Token(message.to_string()));
        }

        // 持久化 reply 消息到 jug0（用 output_state 控制 reply 自身的持久化）
        let should_persist_reply =
            output_state == "context_visible" || output_state == "context_hidden";
        if should_persist_reply {
            if let Ok(Some(chat_id_val)) = context.resolve_path("reply.chat_id") {
                if let Some(chat_id) = chat_id_val.as_str() {
                    let _ = self
                        .runtime
                        .create_message(chat_id, "assistant", message, output_state)
                        .await;
                }
            }
        }

        // 用 input_state 回溯更新原始用户消息状态
        if let (Ok(Some(chat_id_val)), Ok(Some(umid_val))) = (
            context.resolve_path("reply.chat_id"),
            context.resolve_path("reply.user_message_id"),
        ) {
            if let (Some(chat_id), Some(umid)) = (chat_id_val.as_str(), umid_val.as_i64()) {
                let _ = self
                    .runtime
                    .update_message_state(chat_id, umid as i32, input_state)
                    .await;
            }
        }

        // 更新 reply.output（与 chat() 一致）
        let current = context
            .resolve_path("reply.output")
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        context.set(
            "reply.output".to_string(),
            json!(format!("{}{}", current, message)),
        )?;

        Ok(Some(json!({
            "content": message,
            "status": "sent"
        })))
    }
}

/// feishu_webhook(message="内容") - 通过飞书 Webhook 推送消息到群
/// 从 juglans.toml [bot.feishu] webhook_url 读取地址
pub struct FeishuWebhook;

#[async_trait]
impl Tool for FeishuWebhook {
    fn name(&self) -> &str {
        "feishu_webhook"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let message = params
            .get("message")
            .ok_or_else(|| anyhow!("feishu_webhook() requires 'message' parameter"))?;

        // 优先从参数获取 webhook_url，否则从 context 获取（bot 启动时注入）
        let webhook_url = if let Some(url) = params.get("webhook_url") {
            url.clone()
        } else if let Ok(Some(url_val)) = context.resolve_path("bot.feishu_webhook_url") {
            url_val.as_str().unwrap_or("").to_string()
        } else {
            // 尝试从配置文件加载
            match crate::services::config::JuglansConfig::load() {
                Ok(config) => config
                    .bot
                    .as_ref()
                    .and_then(|b| b.feishu.as_ref())
                    .and_then(|f| f.webhook_url.clone())
                    .ok_or_else(|| anyhow!("No webhook_url in [bot.feishu] config"))?,
                Err(_) => return Err(anyhow!("Cannot load config for feishu webhook_url")),
            }
        };

        // 直接调用飞书 webhook API
        let client = reqwest::Client::new();
        let resp = client
            .post(&webhook_url)
            .json(&json!({
                "msg_type": "text",
                "content": {
                    "text": message
                }
            }))
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(json!({}));
        let ok = body["code"].as_i64() == Some(0) || status.is_success();

        if !ok {
            return Err(anyhow!("Feishu webhook error: {:?}", body));
        }

        Ok(Some(json!({
            "status": "sent",
            "content": message
        })))
    }
}

/// return(value=expr) — 将表达式求值结果作为 $output 返回
/// 用于函数定义中返回计算结果：`[add(a, b)]: return(value=$ctx.a + $ctx.b)`
pub struct Return;
#[async_trait]
impl Tool for Return {
    fn name(&self) -> &str {
        "return"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        if let Some(value_str) = params.get("value") {
            let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
            Ok(Some(value))
        } else if let Some((_key, value_str)) = params.iter().next() {
            let value = serde_json::from_str(value_str).unwrap_or(json!(value_str));
            Ok(Some(value))
        } else {
            Ok(Some(Value::Null))
        }
    }
}

// Shell 已被 devtools::Bash 替代（注册为 "bash" + "sh" 别名）
