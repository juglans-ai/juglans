// src/adapters/feishu.rs
#![cfg(not(target_arch = "wasm32"))]

use anyhow::Result;
use axum::{
    extract::Extension,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use dashmap::DashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

use crate::services::config::JuglansConfig;
use super::{run_agent_for_message, chat_via_jug0, ToolExecutor, PlatformMessage};

/// 飞书 Bot 共享状态
struct FeishuState {
    config: JuglansConfig,
    project_root: PathBuf,
    agent_slug: String,
    app_id: String,
    app_secret: String,
    /// API base URL (https://open.feishu.cn 或 https://open.larksuite.com)
    base_url: String,
    /// 缓存的 tenant_access_token
    access_token: Mutex<Option<(String, std::time::Instant)>>,
    /// 是否通过 jug0 SSE 模式执行（薄客户端模式）
    use_jug0: bool,
    /// 已处理的 event_id 集合（飞书 at-least-once 去重）
    processed_events: DashSet<String>,
}

/// 飞书平台工具执行器 — 通过 Python subprocess 调用 bill_utils.py
struct FeishuToolExecutor {
    project_root: PathBuf,
    app_id: String,
    app_secret: String,
    base_url: String,
    approvers: Vec<String>,
    jug0_base_url: String,
    jug0_api_key: String,
    platform_chat_id: String,
    platform_user_id: String,
}

impl FeishuToolExecutor {
    fn from_state(state: &FeishuState) -> Self {
        let bot_config = state.config.bot.as_ref().and_then(|b| b.feishu.as_ref());
        let approvers = bot_config
            .map(|c| c.approvers.clone())
            .unwrap_or_default();

        Self {
            project_root: state.project_root.clone(),
            app_id: state.app_id.clone(),
            app_secret: state.app_secret.clone(),
            base_url: state.base_url.clone(),
            approvers,
            jug0_base_url: state.config.jug0.base_url.clone(),
            jug0_api_key: state.config.account.api_key.clone().unwrap_or_default(),
            platform_chat_id: String::new(),
            platform_user_id: String::new(),
        }
    }

    fn with_message(state: &FeishuState, msg: &PlatformMessage) -> Self {
        let mut executor = Self::from_state(state);
        executor.platform_chat_id = msg.platform_chat_id.clone();
        executor.platform_user_id = msg.platform_user_id.clone();
        executor
    }
}

#[async_trait::async_trait]
impl ToolExecutor for FeishuToolExecutor {
    async fn execute(&self, tool_name: &str, args: Value) -> anyhow::Result<String> {
        // 注入系统参数到 args
        let mut full_args = args.clone();
        if let Some(obj) = full_args.as_object_mut() {
            obj.insert("_app_id".into(), json!(self.app_id));
            obj.insert("_app_secret".into(), json!(self.app_secret));
            obj.insert("_base_url".into(), json!(self.base_url));
            obj.insert("_approvers".into(), json!(self.approvers));
            obj.insert("_jug0_base_url".into(), json!(self.jug0_base_url));
            obj.insert("_jug0_api_key".into(), json!(self.jug0_api_key));
            obj.insert("_chat_id".into(), json!(self.platform_chat_id));
            obj.insert("_user_id".into(), json!(self.platform_user_id));
        }

        let python_code = format!(
            r#"
import json, sys
sys.path.insert(0, sys.argv[1])
import bill_utils

args = json.loads(sys.argv[2])

# 提取系统参数
_sys = {{k: args.pop(k) for k in list(args) if k.startswith('_')}}

# 注入上下文（供 create_bill / clear_chat_history 使用）
bill_utils._context = {{
    "user_id": _sys.get("_user_id"),
    "chat_id": _sys.get("_chat_id"),
    "app_id": _sys.get("_app_id"),
    "app_secret": _sys.get("_app_secret"),
    "base_url": _sys.get("_base_url", "https://open.larksuite.com"),
    "approvers": _sys.get("_approvers", []),
    "jug0_base_url": _sys.get("_jug0_base_url", "http://localhost:3000"),
    "jug0_api_key": _sys.get("_jug0_api_key", ""),
}}

func = getattr(bill_utils, "{}")
result = func(**args)

if isinstance(result, str):
    print(result)
else:
    print(json.dumps(result, ensure_ascii=False))
"#,
            tool_name
        );

        let output = tokio::process::Command::new("python3")
            .arg("-c")
            .arg(&python_code)
            .arg(self.project_root.to_str().unwrap_or("."))
            .arg(serde_json::to_string(&full_args)?)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("[FeishuToolExecutor] {} failed: {}", tool_name, stderr);
            return Err(anyhow::anyhow!("Python tool {} failed: {}", tool_name, stderr));
        }

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        info!("[FeishuToolExecutor] {} → {}", tool_name, if result.len() > 100 { &result[..100] } else { &result });
        Ok(result)
    }
}

/// 飞书事件推送结构
#[derive(Deserialize)]
struct FeishuEventPayload {
    /// URL 验证时的 challenge
    challenge: Option<String>,
    /// 事件头部
    header: Option<FeishuHeader>,
    /// 事件内容
    event: Option<Value>,
}

#[derive(Deserialize)]
struct FeishuHeader {
    event_type: Option<String>,
    event_id: Option<String>,
}

/// 启动飞书 Bot（自动选择模式）
pub async fn start(
    config: JuglansConfig,
    project_root: PathBuf,
    agent_slug: String,
    port: u16,
) -> Result<()> {
    let bot_config = config.bot.as_ref()
        .and_then(|b| b.feishu.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Missing [bot.feishu] config in juglans.toml"))?;

    // 提前提取，避免借用冲突
    let webhook_url = bot_config.webhook_url.clone();
    let has_app_credentials = bot_config.app_id.is_some() && bot_config.app_secret.is_some();
    drop(bot_config);

    if let Some(url) = webhook_url {
        start_webhook_mode(config, project_root, agent_slug, url).await
    } else if has_app_credentials {
        start_event_mode(config, project_root, agent_slug, port).await
    } else {
        Err(anyhow::anyhow!("[bot.feishu] requires webhook_url or (app_id + app_secret)"))
    }
}

/// Webhook 模式：交互式 REPL + 飞书群推送
async fn start_webhook_mode(
    config: JuglansConfig,
    project_root: PathBuf,
    agent_slug: String,
    webhook_url: String,
) -> Result<()> {
    info!("🤖 Starting Feishu Bot (webhook mode)...");
    info!("   Agent: {}", agent_slug);
    info!("   Webhook: {}...{}", &webhook_url[..40.min(webhook_url.len())], if webhook_url.len() > 40 { "" } else { "" });
    info!("   Type messages below. Replies will be sent to Feishu group.");
    println!();

    let stdin = std::io::stdin();
    let mut input = String::new();

    loop {
        print!("📤 > ");
        std::io::Write::flush(&mut std::io::stdout())?;
        input.clear();
        if stdin.read_line(&mut input)? == 0 {
            break;
        }
        let text = input.trim();
        if text.is_empty() { continue; }
        if text == "exit" || text == "quit" { break; }

        let msg = PlatformMessage {
            event_type: "message".into(),
            event_data: json!({ "text": text }),
            platform_user_id: "cli".to_string(),
            platform_chat_id: "cli".to_string(),
            text: text.to_string(),
            username: None,
        };

        match run_agent_for_message(&config, &project_root, &agent_slug, &msg, None).await {
            Ok(reply) => {
                println!("💬 {}", reply.text);
                // 推送到飞书群
                if let Err(e) = send_webhook(&webhook_url, &reply.text).await {
                    warn!("⚠️  Webhook send failed: {}", e);
                } else {
                    info!("✅ Sent to Feishu group");
                }
            }
            Err(e) => {
                error!("❌ Agent error: {}", e);
            }
        }
        println!();
    }

    Ok(())
}

/// 事件订阅模式：启动 HTTP 服务接收飞书事件
async fn start_event_mode(
    config: JuglansConfig,
    project_root: PathBuf,
    agent_slug: String,
    port: u16,
) -> Result<()> {
    let bot_config = config.bot.as_ref()
        .and_then(|b| b.feishu.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Missing [bot.feishu] config"))?;

    let app_id = bot_config.app_id.clone()
        .ok_or_else(|| anyhow::anyhow!("[bot.feishu] event mode requires app_id"))?;
    let app_secret = bot_config.app_secret.clone()
        .ok_or_else(|| anyhow::anyhow!("[bot.feishu] event mode requires app_secret"))?;
    let base_url = bot_config.base_url.clone();

    info!("🤖 Starting Feishu Bot (event subscription mode)...");
    info!("   Agent: {}", agent_slug);
    info!("   App ID: {}", app_id);
    info!("   API Base: {}", base_url);

    // 执行模式：优先读 [bot.feishu] mode，否则按 jug0 base_url 自动判断
    let use_jug0 = match bot_config.mode.as_deref() {
        Some("local") => false,
        Some("jug0") => true,
        _ => !config.jug0.base_url.is_empty(),
    };
    if use_jug0 {
        info!("   Mode: SSE client (via jug0 at {})", config.jug0.base_url);
    } else {
        info!("   Mode: local execution");
    }

    let state = Arc::new(FeishuState {
        config,
        project_root,
        agent_slug,
        app_id,
        app_secret,
        base_url,
        access_token: Mutex::new(None),
        use_jug0,
        processed_events: DashSet::new(),
    });

    let app = Router::new()
        .route("/feishu/event", post(handle_feishu_event))
        .layer(Extension(state));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!("   Listening on: http://0.0.0.0:{}", port);
    info!("   Webhook URL: http://<your-domain>:{}/feishu/event", port);
    info!("   Ready! Configure this URL in Feishu Open Platform.");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// 通过 Webhook URL 发送消息到飞书群（自定义机器人）
pub async fn send_webhook(webhook_url: &str, text: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let resp = client
        .post(webhook_url)
        .json(&json!({
            "msg_type": "text",
            "content": {
                "text": text
            }
        }))
        .send()
        .await?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));

    if body["code"].as_i64() != Some(0) && !status.is_success() {
        warn!("[Feishu Webhook] Send failed: {} {:?}", status, body);
        return Err(anyhow::anyhow!("Feishu webhook error: {:?}", body));
    }

    info!("[Feishu Webhook] Message sent successfully");
    Ok(())
}

/// 通过 Webhook 发送富文本消息（Markdown 风格的 post 消息）
pub async fn send_webhook_rich(webhook_url: &str, title: &str, content_lines: Vec<Vec<Value>>) -> Result<()> {
    let client = reqwest::Client::new();

    let resp = client
        .post(webhook_url)
        .json(&json!({
            "msg_type": "post",
            "content": {
                "post": {
                    "zh_cn": {
                        "title": title,
                        "content": content_lines
                    }
                }
            }
        }))
        .send()
        .await?;

    let body: Value = resp.json().await.unwrap_or(json!({}));
    if body["code"].as_i64() != Some(0) {
        warn!("[Feishu Webhook] Rich message send failed: {:?}", body);
    }

    Ok(())
}

/// 处理飞书事件推送
async fn handle_feishu_event(
    Extension(state): Extension<Arc<FeishuState>>,
    Json(payload): Json<FeishuEventPayload>,
) -> impl IntoResponse {
    // 1. URL 验证（飞书开放平台配置回调 URL 时的 challenge 验证）
    if let Some(challenge) = payload.challenge {
        info!("[Feishu] URL verification challenge received");
        return Json(json!({ "challenge": challenge }));
    }

    // 2. 处理事件
    let event_type = payload
        .header
        .as_ref()
        .and_then(|h| h.event_type.as_deref())
        .unwrap_or("");

    let event_id = payload
        .header
        .as_ref()
        .and_then(|h| h.event_id.clone())
        .unwrap_or_default();

    // 飞书事件去重（at-least-once delivery）
    if !event_id.is_empty() && !state.processed_events.insert(event_id.clone()) {
        info!("[Feishu] Duplicate event {}, skipping", event_id);
        return Json(json!({"code": 0, "msg": "duplicate"}));
    }

    match event_type {
        "im.message.receive_v1" => {
            // 消息事件
            if let Some(event) = payload.event {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_message_event(&state, &event).await {
                        error!("[Feishu] Failed to handle message: {}", e);
                    }
                });
            }
        }
        "card.action.trigger" => {
            // 卡片按钮回调事件：同步执行 workflow，在回调响应中直接返回更新后的卡片
            // （PATCH API 在 card.action.trigger 回调期间无效，必须通过响应返回卡片）
            if let Some(event) = payload.event {
                let result = handle_card_action_event(&state, &event).await;
                return match result {
                    Ok(reply_text) => {
                        // 尝试将 reply 解析为卡片 JSON（handle_card_action 返回 card_json）
                        match serde_json::from_str::<Value>(&reply_text) {
                            Ok(card) if card.get("header").is_some() => {
                                // 有效的卡片 JSON：在回调响应中直接返回更新后的卡片
                                Json(json!({
                                    "toast": { "type": "success", "content": "已处理" },
                                    "card": { "type": "raw", "data": card }
                                }))
                            }
                            _ => {
                                // 非卡片内容（如错误消息），只返回 toast
                                Json(json!({
                                    "toast": {
                                        "type": "info",
                                        "content": if reply_text.is_empty() || reply_text == "(No response)" {
                                            "已处理".to_string()
                                        } else {
                                            reply_text
                                        }
                                    }
                                }))
                            }
                        }
                    }
                    Err(e) => {
                        error!("[Feishu Card] Error: {}", e);
                        Json(json!({
                            "toast": { "type": "error", "content": format!("处理失败: {}", e) }
                        }))
                    }
                };
            }
            return Json(json!({ "toast": { "type": "error", "content": "无效事件" } }));
        }
        _ => {
            warn!("[Feishu] Unhandled event type: {} (id: {})", event_type, event_id);
        }
    }

    Json(json!({ "code": 0, "msg": "ok" }))
}

/// 处理飞书消息事件
async fn handle_message_event(state: &FeishuState, event: &Value) -> Result<()> {
    let message = event.get("message").ok_or_else(|| anyhow::anyhow!("No message in event"))?;

    // 提取消息内容
    let msg_type = message["message_type"].as_str().unwrap_or("");
    if msg_type != "text" {
        info!("[Feishu] Skipping non-text message (type: {})", msg_type);
        return Ok(());
    }

    let content_str = message["content"].as_str().unwrap_or("{}");
    let content: Value = serde_json::from_str(content_str).unwrap_or(json!({}));
    let raw_text = content["text"].as_str().unwrap_or("");

    // 清理 @mention 占位符（如 @_user_1），保留实际用户消息
    let text = raw_text
        .split_whitespace()
        .filter(|s| !s.starts_with("@_user_"))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if text.is_empty() {
        return Ok(());
    }

    let chat_id = message["chat_id"].as_str().unwrap_or("").to_string();
    let chat_type = message["chat_type"].as_str().unwrap_or("unknown");
    let empty = json!({});
    let sender = event.get("sender").unwrap_or(&empty);
    let sender_id = sender["sender_id"]["open_id"]
        .as_str()
        .unwrap_or("")
        .to_string();

    info!(
        "📩 [Feishu] User {} (chat_type: {}, chat_id: {}): {}",
        sender_id,
        chat_type,
        chat_id,
        if text.chars().count() > 50 { &text[..text.char_indices().nth(50).map(|(i,_)|i).unwrap_or(text.len())] } else { &text }
    );

    let platform_msg = PlatformMessage {
        event_type: "message".into(),
        event_data: json!({ "text": &text }),
        platform_user_id: sender_id,
        platform_chat_id: chat_id.clone(),
        text,
        username: None,
    };

    // 执行 agent — 根据模式选择本地执行或 SSE 客户端
    let result = if state.use_jug0 {
        let tool_executor = FeishuToolExecutor::with_message(state, &platform_msg);
        chat_via_jug0(&state.config, &state.agent_slug, &platform_msg, &tool_executor).await
    } else {
        let tool_executor = FeishuToolExecutor::with_message(state, &platform_msg);
        run_agent_for_message(&state.config, &state.project_root, &state.agent_slug, &platform_msg, Some(&tool_executor)).await
    };

    match result {
        Ok(reply) => {
            if !reply.text.is_empty() && reply.text != "(No response)" {
                let token = get_access_token(&state.app_id, &state.app_secret, &state.base_url, &state.access_token).await?;
                send_feishu_message(&token, &chat_id, &reply.text, &state.base_url).await?;
            }
        }
        Err(e) => {
            error!("[Feishu] Agent error: {}", e);
            let token = get_access_token(&state.app_id, &state.app_secret, &state.base_url, &state.access_token).await?;
            send_feishu_message(&token, &chat_id, &format!("Error: {}", e), &state.base_url).await?;
        }
    }

    Ok(())
}

/// 获取飞书 tenant_access_token（带缓存）
async fn get_access_token(
    app_id: &str,
    app_secret: &str,
    base_url: &str,
    cache: &Mutex<Option<(String, std::time::Instant)>>,
) -> Result<String> {
    // 检查缓存（token 有效期 2 小时，提前 5 分钟刷新）
    if let Ok(guard) = cache.lock() {
        if let Some((ref token, ref created)) = *guard {
            if created.elapsed() < std::time::Duration::from_secs(7000) {
                return Ok(token.clone());
            }
        }
    }

    let client = reqwest::Client::new();
    let resp: Value = client
        .post(format!("{}/open-apis/auth/v3/tenant_access_token/internal", base_url))
        .json(&json!({
            "app_id": app_id,
            "app_secret": app_secret
        }))
        .send()
        .await?
        .json()
        .await?;

    // 检查 API 响应码
    let code = resp["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        return Err(anyhow::anyhow!(
            "Feishu token API error: code={}, msg={}",
            code,
            resp["msg"]
        ));
    }

    let token = resp["tenant_access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Feishu returned empty access token: {:?}", resp))?
        .to_string();

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((token.clone(), std::time::Instant::now()));
    }

    info!("[Feishu] Access token refreshed (len={})", token.len());
    Ok(token)
}

/// 发送飞书消息（事件订阅模式，需要 access_token）
async fn send_feishu_message(token: &str, chat_id: &str, text: &str, base_url: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/open-apis/im/v1/messages", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .query(&[("receive_id_type", "chat_id")])
        .json(&json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": serde_json::to_string(&json!({"text": text}))?
        }))
        .send()
        .await?;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(json!({}));
    let api_code = body["code"].as_i64().unwrap_or(-1);

    if !status.is_success() || api_code != 0 {
        warn!("[Feishu] Send message failed: HTTP {} | code: {} | body: {:?}", status, api_code, body);
        return Err(anyhow::anyhow!("Feishu send failed: code={}, msg={}", api_code, body["msg"]));
    }

    info!("[Feishu] Message sent to chat_id: {}", chat_id);
    Ok(())
}

/// 处理飞书卡片按钮回调事件 (card.action.trigger)
///
/// 构建标准化事件信封，统一走 workflow 路由。
/// workflow 通过 switch $input.event_type 决定走 Python 直调。
async fn handle_card_action_event(state: &FeishuState, event: &Value) -> Result<String> {
    let action_value = event
        .pointer("/action/value")
        .ok_or_else(|| anyhow::anyhow!("No action.value in card event"))?;

    let operator_id = event
        .pointer("/operator/open_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let chat_id = event
        .pointer("/context/open_chat_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let action_str = serde_json::to_string(action_value).unwrap_or_default();

    info!(
        "🔘 [Feishu Card] Action from user {}: {}",
        operator_id, action_str
    );

    // 标准化事件信封
    let platform_msg = PlatformMessage {
        event_type: "card_action".into(),
        event_data: action_value.clone(),
        platform_user_id: operator_id.to_string(),
        platform_chat_id: chat_id.to_string(),
        text: String::new(),
        username: None,
    };

    // 统一走 workflow（由 workflow switch 路由到 Python 直调）
    let tool_executor = FeishuToolExecutor::with_message(state, &platform_msg);
    let reply = run_agent_for_message(
        &state.config, &state.project_root, &state.agent_slug,
        &platform_msg, Some(&tool_executor),
    ).await?;

    // 尝试解析为卡片 JSON
    if let Ok(card) = serde_json::from_str::<Value>(&reply.text) {
        if card.get("header").is_some() {
            return Ok(reply.text);
        }
    }
    Ok(reply.text)
}
