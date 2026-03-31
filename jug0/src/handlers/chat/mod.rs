// src/handlers/chat/mod.rs
pub mod helpers;
pub mod logic;
pub mod types;

use axum::{
    body::Body,
    extract::{Extension, Json, Query},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
};
use futures::StreamExt;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    Set,
};
use serde_json::json;
use std::convert::Infallible;
use std::env;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{AuthUser, ExecutionClaims, OptionalAuthUser};
use crate::entities::{agents, chats, handles, messages, prompts};
use crate::errors::AppError;
use crate::services::mcp::McpServerConfig;
use crate::services::quota;
use crate::AppState;

use logic::run_chat_stream;
pub use types::*;

/// Resolve @handle to agent (returns agent_id if found)
pub async fn resolve_handle_to_agent(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    handle: &str,
) -> Result<Option<Uuid>, AppError> {
    // Strip @ prefix if present
    let handle_name = handle.strip_prefix('@').unwrap_or(handle);

    let handle_record = handles::Entity::find()
        .filter(handles::Column::OrgId.eq(org_id))
        .filter(handles::Column::Handle.eq(handle_name))
        .one(db)
        .await
        .map_err(AppError::Database)?;

    match handle_record {
        Some(h) if h.target_type == "agent" => Ok(Some(h.target_id)),
        Some(_) => Ok(None), // Handle exists but not an agent (e.g., user)
        None => Ok(None),
    }
}

/// Find existing chat for user + agent (most recently updated)
/// Used for persistent @handle conversations (Telegram-style DM)
pub async fn find_existing_agent_chat(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    user_id: Uuid,
    agent_id: Uuid,
) -> Result<Option<chats::Model>, AppError> {
    chats::Entity::find()
        .filter(chats::Column::OrgId.eq(org_id))
        .filter(chats::Column::UserId.eq(user_id))
        .filter(chats::Column::AgentId.eq(agent_id))
        .order_by_desc(chats::Column::UpdatedAt)
        .one(db)
        .await
        .map_err(AppError::Database)
}

/// Find chat by external_id (arbitrary platform identifier like Feishu "oc_xxx")
pub async fn find_chat_by_external_id(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    external_id: &str,
) -> Result<Option<chats::Model>, AppError> {
    chats::Entity::find()
        .filter(chats::Column::OrgId.eq(org_id))
        .filter(chats::Column::ExternalId.eq(external_id))
        .one(db)
        .await
        .map_err(AppError::Database)
}

/// Resolve chat_id string (UUID, @handle, or external_id) to actual chat UUID
/// If @handle and no existing chat, returns None (caller should create new chat)
pub async fn resolve_chat_id_or_handle(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    user_id: Uuid,
    id_or_handle: &str,
) -> Result<(Option<Uuid>, Option<Uuid>), AppError> {
    // Try parsing as UUID first
    if let Ok(uuid) = Uuid::parse_str(id_or_handle) {
        return Ok((Some(uuid), None));
    }

    // Try resolving as @handle
    if id_or_handle.starts_with('@') {
        let agent_id = resolve_handle_to_agent(db, org_id, id_or_handle)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Handle '{}' not found", id_or_handle)))?;
        let existing_chat = find_existing_agent_chat(db, org_id, user_id, agent_id).await?;
        return Ok((existing_chat.map(|c| c.id), Some(agent_id)));
    }

    // Try external_id
    let existing_chat = find_chat_by_external_id(db, org_id, id_or_handle).await?;
    Ok((existing_chat.map(|c| c.id), None))
}

/// Resolve chat_id string to existing chat UUID (for GET/DELETE operations)
/// Returns error if no existing chat found
pub async fn resolve_chat_id_strict(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    user_id: Uuid,
    id_or_handle: &str,
) -> Result<Uuid, AppError> {
    // Try parsing as UUID first
    if let Ok(uuid) = Uuid::parse_str(id_or_handle) {
        return Ok(uuid);
    }

    // Try resolving as @handle
    if id_or_handle.starts_with('@') {
        let agent_id = resolve_handle_to_agent(db, org_id, id_or_handle)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Handle '{}' not found", id_or_handle)))?;
        let existing_chat = find_existing_agent_chat(db, org_id, user_id, agent_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("No chat with agent '{}'", id_or_handle)))?;
        return Ok(existing_chat.id);
    }

    // Try external_id
    let existing_chat = find_chat_by_external_id(db, org_id, id_or_handle)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Chat with external_id '{}' not found",
                id_or_handle
            ))
        })?;
    Ok(existing_chat.id)
}

/// 转发请求到 workflow endpoint 并流式返回结果
/// execution_token: 签名的 token，用于 juglans-agent 回调时验证原始调用者
/// override_chat_id: 覆盖请求中的 chat_id（用于确保 workflow 使用 jug0 创建的 chat）
async fn forward_to_workflow(
    client: &reqwest::Client,
    endpoint_url: &str,
    req: &ChatRequest,
    stream_mode: bool,
    execution_token: Option<String>,
    override_chat_id: Option<Uuid>,
    user_message_id: Option<i32>,
    user_message_uuid: Option<Uuid>,
) -> Result<Response, AppError> {
    // 构建转发请求体，覆盖 stream 参数
    let mut forward_body = serde_json::to_value(req)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to serialize request: {}", e)))?;

    if let Some(obj) = forward_body.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::json!(stream_mode));

        // 覆盖 chat_id 为 jug0 创建的真实 UUID
        if let Some(cid) = override_chat_id {
            obj.insert("chat_id".to_string(), serde_json::json!(cid.to_string()));
        }

        // 传递用户消息 ID（workflow 用于回溯更新用户消息状态）
        if let Some(umid) = user_message_id {
            obj.insert("user_message_id".to_string(), serde_json::json!(umid));
        }
    }

    let mut request_builder = client
        .post(endpoint_url)
        .header("Content-Type", "application/json");

    // 携带 Execution Token（如果有）
    if let Some(token) = execution_token {
        tracing::debug!("🔐 [Forward] Attaching X-Execution-Token to workflow request");
        request_builder = request_builder.header("X-Execution-Token", token);
    }

    let response = request_builder
        .json(&forward_body)
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Workflow request failed: {}", e)))?;

    let resp_status = response.status();
    let resp_ct = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    tracing::info!(
        "📡 [Forward] Agent response: status={}, content-type={}",
        resp_status,
        resp_ct
    );

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AppError::Internal(anyhow::anyhow!(
            "Workflow returned error {}: {}",
            status,
            error_body
        )));
    }

    if stream_mode {
        // SSE 流式转发 — 解析上游 SSE 字节，重新用 Axum Sse::new() 发射
        // 这与非 workflow 路径使用完全相同的 SSE 机制，确保 Caddy/CF 正确流式传输
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(256);

        // spawn 独立任务：从上游 agent 读取 SSE 字节流，解析为 Event 对象
        let byte_stream = response.bytes_stream();
        tokio::spawn(async move {
            let mut stream = std::pin::pin!(byte_stream);
            let mut buffer = String::new();
            let mut chunk_count = 0usize;
            let mut total_bytes = 0usize;

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        chunk_count += 1;
                        total_bytes += bytes.len();

                        let text = match std::str::from_utf8(&bytes) {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        buffer.push_str(text);

                        // 按 SSE 事件边界（\n\n）分割
                        while let Some(pos) = buffer.find("\n\n") {
                            let event_block = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            // 解析 SSE 事件块
                            let mut event_type = None;
                            let mut data_lines = Vec::new();

                            for line in event_block.lines() {
                                if let Some(et) = line.strip_prefix("event: ") {
                                    event_type = Some(et.to_string());
                                } else if let Some(d) = line.strip_prefix("data: ") {
                                    data_lines.push(d.to_string());
                                }
                                // SSE comments (: ...) are silently skipped
                            }

                            if !data_lines.is_empty() {
                                let data = data_lines.join("\n");
                                let mut event = Event::default().data(data);
                                if let Some(ref et) = event_type {
                                    event = event.event(et.clone());
                                }
                                if tx.send(event).await.is_err() {
                                    tracing::warn!(
                                        "📡 [Forward] receiver dropped after {} chunks",
                                        chunk_count
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Workflow stream read error: {}", e);
                        break;
                    }
                }
            }
            tracing::info!(
                "📡 [Forward] stream ended: {} chunks, {} bytes total",
                chunk_count,
                total_bytes
            );
        });

        // 构建 meta 事件
        let meta_event = override_chat_id.map(|cid| {
            Event::default().event("meta").data(
                serde_json::json!({
                    "type": "meta",
                    "chat_id": cid,
                    "user_message_id": user_message_id.unwrap_or(0),
                    "user_message_uuid": user_message_uuid,
                })
                .to_string(),
            )
        });

        // 用 async_stream 构建 Event 流，通过 Sse::new() 发射（与非 workflow 路径一致）
        let sse_stream = async_stream::stream! {
            // 先发 meta 事件
            if let Some(meta) = meta_event {
                yield Ok::<Event, Infallible>(meta);
            }
            // 转发上游解析出的事件
            while let Some(event) = rx.recv().await {
                yield Ok(event);
            }
        };

        Ok(Sse::new(sse_stream)
            .keep_alive(axum::response::sse::KeepAlive::default())
            .into_response())
    } else {
        // 非流式，直接返回 JSON
        let json_body = response
            .text()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to read response: {}", e)))?;

        Ok(Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .body(Body::from(json_body))
            .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to build response: {}", e)))?)
    }
}

/// Handle tool_result: standard chat continuation or workflow forwarding
pub async fn tool_result_handler(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ToolResultRequest>,
) -> Result<Response, AppError> {
    // Check MCP sessions — Claude Code tools/call waiting for result.
    // Send to BOTH: MCP endpoint (so Claude Code continues) AND tool_result_channels
    // (so run_chat_stream unblocks). The skip_restart flag tells run_chat_stream
    // to NOT restart the LLM stream — Claude Code is still running.
    for session in state.tool_sessions.iter() {
        for result in &req.results {
            if let Some((_, sender)) = session.result_senders.remove(&result.tool_call_id) {
                tracing::info!("[ToolResult] MCP route: call_id={} → session {}", result.tool_call_id, session.key());
                let _ = sender.send(result.content.clone());

                // Also unblock run_chat_stream (with skip_restart=true)
                if let Ok(chat_id) = Uuid::parse_str(&req.call_id) {
                    if let Some(stream_sender) = state.tool_result_channels.get(&chat_id) {
                        let _ = stream_sender.send(types::ToolResultWithTools {
                            results: req.results.clone(),
                            tools: req.tools.clone(),
                            skip_restart: true,
                        }).await;
                    }
                }
                return Ok(Json(json!({"status": "ok", "mcp_routed": true})).into_response());
            }
        }
    }

    // Try parsing call_id as UUID → check if there's an active SSE stream waiting
    if let Ok(chat_id) = Uuid::parse_str(&req.call_id) {
        // Only use new path if there's an active channel (avoids false match with workflow call_ids)
        if let Some(sender) = state.tool_result_channels.get(&chat_id) {
            // Active channel in tool_result_channels proves this is a valid session
            // created by chat_handler — skip ownership DB query
            sender
                .send(types::ToolResultWithTools {
                    results: req.results.clone(),
                    tools: req.tools.clone(),
                    skip_restart: false,
                })
                .await
                .map_err(|_| {
                    AppError::Internal(anyhow::anyhow!("Stream no longer waiting for tool results"))
                })?;

            tracing::info!(
                "[ToolResult] Pushed {} results to stream for chat {}",
                req.results.len(),
                chat_id
            );

            return Ok(Json(json!({
                "status": "ok",
                "chat_id": chat_id,
            }))
            .into_response());
        }
        // No active channel — check workflow_forwards using call_id as chat_id
        // (workflow SSE may omit chat_id in meta, so req.chat_id can be None)
        if let Some(info) = state.workflow_forwards.get(&chat_id) {
            let base_url = info
                .endpoint_url
                .trim_end_matches('/')
                .trim_end_matches("/api/chat");
            let forward_url = format!("{}/api/chat/tool-result", base_url);

            let execution_token = {
                let claims = ExecutionClaims::new(
                    user.id,
                    user.org_id.clone(),
                    info.author_user_id,
                    info.agent_id,
                    Some(chat_id),
                    300,
                );
                claims.encode(&state.signing_key)
            };

            tracing::info!(
                "🌉 Forwarding tool_result (call_id as chat_id: {}) via workflow_forwards to: {}",
                chat_id,
                forward_url
            );

            let response = state
                .http_client
                .post(&forward_url)
                .header("X-Execution-Token", &execution_token)
                .json(&json!({
                    "call_id": req.call_id,
                    "results": req.results,
                    "tools": req.tools,
                }))
                .send()
                .await
                .map_err(|e| {
                    AppError::Internal(anyhow::anyhow!("Tool result forward failed: {}", e))
                })?;

            let body: serde_json::Value = response.json().await.map_err(|e| {
                AppError::Internal(anyhow::anyhow!(
                    "Failed to parse tool result response: {}",
                    e
                ))
            })?;

            return Ok(Json(body).into_response());
        }
    }

    // ─── Workflow forwarding: prefer cached mapping via req.chat_id (zero DB queries) ───
    if let Some(chat_id) = req.chat_id.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
        if let Some(info) = state.workflow_forwards.get(&chat_id) {
            let base_url = info
                .endpoint_url
                .trim_end_matches('/')
                .trim_end_matches("/api/chat");
            let forward_url = format!("{}/api/chat/tool-result", base_url);

            let execution_token = {
                let claims = ExecutionClaims::new(
                    user.id,
                    user.org_id.clone(),
                    info.author_user_id,
                    info.agent_id,
                    Some(chat_id),
                    300,
                );
                claims.encode(&state.signing_key)
            };

            tracing::info!(
                "🌉 Forwarding tool_result (call_id: {}, chat_id: {}) via cached mapping to: {}",
                req.call_id,
                chat_id,
                forward_url
            );

            let response = state
                .http_client
                .post(&forward_url)
                .header("X-Execution-Token", &execution_token)
                .json(&json!({
                    "call_id": req.call_id,
                    "results": req.results,
                    "tools": req.tools,
                }))
                .send()
                .await
                .map_err(|e| {
                    AppError::Internal(anyhow::anyhow!("Tool result forward failed: {}", e))
                })?;

            let body: serde_json::Value = response.json().await.map_err(|e| {
                AppError::Internal(anyhow::anyhow!(
                    "Failed to parse tool result response: {}",
                    e
                ))
            })?;

            return Ok(Json(body).into_response());
        }
    }

    // ─── Fallback: agent_slug → DB lookup ───
    let agent_slug = req.agent_slug.as_deref().unwrap_or("default");

    let agent = agents::Entity::find()
        .filter(agents::Column::Slug.eq(agent_slug))
        .filter(
            Condition::any()
                .add(
                    Condition::all()
                        .add(agents::Column::OrgId.eq(&user.org_id))
                        .add(agents::Column::UserId.eq(user.id)),
                )
                .add(agents::Column::OrgId.eq(crate::official_org_slug()))
                .add(agents::Column::IsPublic.eq(true)),
        )
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Agent '{}' not found", agent_slug)))?;

    let endpoint_url = agent.endpoint_url.as_ref().ok_or_else(|| {
        AppError::BadRequest(format!("Agent '{}' has no endpoint configured", agent_slug))
    })?;

    let base_url = endpoint_url
        .trim_end_matches('/')
        .trim_end_matches("/api/chat");
    let forward_url = format!("{}/api/chat/tool-result", base_url);

    let execution_token = {
        let author_user_id = agent.user_id.unwrap_or(user.id);
        let claims = ExecutionClaims::new(
            user.id,
            user.org_id.clone(),
            author_user_id,
            agent.id,
            None,
            300,
        );
        claims.encode(&state.signing_key)
    };

    tracing::info!(
        "🌉 Forwarding tool_result (call_id: {}) to workflow endpoint: {}",
        req.call_id,
        forward_url
    );

    let response = state
        .http_client
        .post(&forward_url)
        .header("X-Execution-Token", &execution_token)
        .json(&json!({
            "call_id": req.call_id,
            "results": req.results,
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Tool result forward failed: {}", e)))?;

    let body: serde_json::Value = response.json().await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "Failed to parse tool result response: {}",
            e
        ))
    })?;

    Ok(Json(body).into_response())
}

pub async fn list_chats_handler(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Query(params): Query<ListChatsQuery>,
) -> Result<Json<Vec<chats::Model>>, AppError> {
    let limit = params.limit.unwrap_or(50);

    let chats = chats::Entity::find()
        .filter(chats::Column::OrgId.eq(&user.org_id))
        .filter(chats::Column::UserId.eq(user.id))
        .filter(
            Condition::any()
                .add(chats::Column::Incognito.eq(false))
                .add(chats::Column::Incognito.is_null()),
        )
        .order_by_desc(chats::Column::UpdatedAt)
        .limit(limit)
        .all(&state.db)
        .await?;

    Ok(Json(chats))
}

pub async fn stop_chat_handler(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<StopRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Resolve chat_id (UUID, @handle, or external_id)
    let chat_id = match &req.chat_id {
        ChatIdInput::Uuid(id) => *id,
        ChatIdInput::Handle(h) => {
            resolve_chat_id_strict(&state.db, &user.org_id, user.id, h).await?
        }
        ChatIdInput::ExternalId(ext_id) => {
            find_chat_by_external_id(&state.db, &user.org_id, ext_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!("Chat with external_id '{}' not found", ext_id))
                })?
                .id
        }
    };

    let exists = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::OrgId.eq(&user.org_id))
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?;

    if exists.is_none() {
        return Err(AppError::NotFound(
            "Chat not found or access denied".to_string(),
        ));
    }

    if let Some(token) = state.active_requests.get(&chat_id) {
        token.cancel();
    }

    Ok(Json(json!({ "status": "stopped", "chat_id": chat_id })))
}

pub async fn chat_handler(
    Extension(state): Extension<Arc<AppState>>,
    OptionalAuthUser(user_opt): OptionalAuthUser,
    Json(req): Json<ChatRequest>,
) -> Result<Response, AppError> {
    let start_time = std::time::Instant::now();

    // === NO AUTH: 返回友好引导 SSE 流 ===
    if user_opt.is_none() {
        let stream = async_stream::stream! {
            yield Ok::<_, Infallible>(Event::default().event("meta").data(
                json!({"type":"meta","chat_id":"00000000-0000-0000-0000-000000000000","user_message_id":0}).to_string()
            ));

            let chunks = [
                "Hello, I'm JUG0 by Juglans. Nice to meet you! ",
                "However, I need to remind you that you haven't bound an API key. ",
                "You can register at jug0.com and receive $5 in credits for testing. ",
                "May your AI journey be smooth!\n\n",
                "你好，我是 Juglans 的 JUG0，非常高兴遇到你！",
                "但是我需要提醒你，你没有绑定 API Key。",
                "你可以到 jug0.com 注册，会有 $5 的余额提供给你测试。",
                "愿你的 AI 之路顺遂！",
            ];

            for chunk in chunks {
                yield Ok(Event::default().data(
                    json!({"type":"content","text":chunk}).to_string()
                ));
            }

            yield Ok(Event::default().event("done").data(
                json!({"type":"done","message_id":1}).to_string()
            ));
        };
        return Ok(Sse::new(stream)
            .keep_alive(axum::response::sse::KeepAlive::default())
            .into_response());
    }
    let user = user_opt.unwrap();

    // === QUOTA CHECK ===
    quota::check_quota(&state.db, &state.cache, user.id).await?;

    let global_enable_memory =
        env::var("ENABLE_MEMORY").unwrap_or_else(|_| "false".to_string()) == "true";
    let default_model = env::var("DEFAULT_LLM_MODEL").unwrap_or_else(|_| "qwen-plus".to_string());
    let fallback_model = env::var("FALLBACK_LLM_MODEL").unwrap_or_else(|_| default_model.clone());

    // Resolve chat_id input: UUID / @handle / external_id
    let (existing_chat_id, handle_agent_id, external_id_to_save) = match &req.chat_id {
        Some(ChatIdInput::Uuid(id)) => (Some(*id), None, None),
        Some(ChatIdInput::Handle(h)) => {
            // Resolve @handle to agent
            let agent_id = resolve_handle_to_agent(&state.db, &user.org_id, h)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("Handle '{}' not found", h)))?;

            // Find existing chat with this agent (Telegram-style persistent DM)
            let existing_chat =
                find_existing_agent_chat(&state.db, &user.org_id, user.id, agent_id).await?;

            (existing_chat.map(|c| c.id), Some(agent_id), None)
        }
        Some(ChatIdInput::ExternalId(ext_id)) => {
            // Arbitrary platform ID (e.g. Feishu group "oc_xxx")
            let existing_chat = find_chat_by_external_id(&state.db, &user.org_id, ext_id).await?;

            (existing_chat.map(|c| c.id), None, Some(ext_id.clone()))
        }
        None => (None, None, None),
    };

    // Extract agent config: agent.* > req.* (backward compat)
    let (target_slug, req_model, req_tools, req_sys_prompt, req_stream, req_memory) =
        if let Some(agent_conf) = &req.agent {
            let slug = agent_conf
                .slug
                .clone()
                .or(agent_conf.id.map(|u| u.to_string()))
                .unwrap_or_else(|| "default".to_string());
            // 优先使用 agent.* 的值，回退到顶层 req.* (deprecated)
            let model = agent_conf.model.clone().or_else(|| req.model.clone());
            let tools = agent_conf.tools.clone().or_else(|| req.tools.clone());
            let stream = agent_conf.stream.or(req.stream).unwrap_or(true);
            let memory = agent_conf.memory.or(req.memory).unwrap_or(false);
            (
                slug,
                model,
                tools,
                agent_conf.system_prompt.clone(),
                stream,
                memory,
            )
        } else if let Some(agent_id) = handle_agent_id {
            // If @handle resolved to agent, use that agent's ID as slug
            let stream = req.stream.unwrap_or(true);
            let memory = req.memory.unwrap_or(false);
            (
                agent_id.to_string(),
                req.model.clone(),
                req.tools.clone(),
                None,
                stream,
                memory,
            )
        } else {
            let stream = req.stream.unwrap_or(true);
            let memory = req.memory.unwrap_or(false);
            (
                "default".to_string(),
                req.model.clone(),
                req.tools.clone(),
                None,
                stream,
                memory,
            )
        };

    let should_use_memory = global_enable_memory && req_memory;

    // Find agent by ID (from @handle) or by slug
    let agent_opt = if let Some(agent_id) = handle_agent_id {
        // @handle resolved to agent ID - lookup by ID
        agents::Entity::find_by_id(agent_id)
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(agents::Column::OrgId.eq(&user.org_id))
                            .add(agents::Column::UserId.eq(user.id)),
                    )
                    .add(agents::Column::OrgId.eq(crate::official_org_slug()))
                    .add(agents::Column::IsPublic.eq(true)),
            )
            .one(&state.db)
            .await?
    } else {
        // Normal slug lookup
        agents::Entity::find()
            .filter(agents::Column::Slug.eq(&target_slug))
            .filter(
                Condition::any()
                    .add(
                        Condition::all()
                            .add(agents::Column::OrgId.eq(&user.org_id))
                            .add(agents::Column::UserId.eq(user.id)),
                    )
                    .add(agents::Column::OrgId.eq(crate::official_org_slug()))
                    .add(agents::Column::IsPublic.eq(true)), // Allow public agents
            )
            .one(&state.db)
            .await?
    };
    tracing::info!(
        "⏱ [Chat] Agent lookup: {}ms",
        start_time.elapsed().as_millis()
    );

    // 检查 agent 是否配置了 endpoint_url，如果是则转发到该 endpoint
    if let Some(ref agent) = agent_opt {
        if let Some(ref endpoint_url) = agent.endpoint_url {
            // ======= 新增：先创建 chat 并保存用户消息 =======
            let is_incognito = matches!(&req.history, Some(serde_json::Value::Bool(false)));
            let (chat_id, last_message_id) = match existing_chat_id {
                Some(id) => {
                    let chat = chats::Entity::find_by_id(id)
                        .filter(chats::Column::OrgId.eq(&user.org_id))
                        .filter(chats::Column::UserId.eq(user.id))
                        .one(&state.db)
                        .await?;
                    match chat {
                        Some(c) => (id, c.last_message_id.unwrap_or(0)),
                        None => {
                            // Chat not found — workflow sub-agent may inherit stale parent chat_id
                            tracing::warn!(
                                "Chat {} not found for user {}, creating new chat",
                                id,
                                user.id
                            );
                            let new_chat = chats::ActiveModel {
                                id: Set(Uuid::new_v4()),
                                org_id: Set(Some(user.org_id.clone())),
                                user_id: Set(Some(user.id)),
                                title: Set(Some("Workflow Chat".to_string())),
                                agent_id: Set(Some(agent.id)),
                                last_message_id: Set(Some(0)),
                                incognito: Set(Some(is_incognito)),
                                ..Default::default()
                            };
                            (new_chat.insert(&state.db).await?.id, 0)
                        }
                    }
                }
                None => {
                    let title_text = req
                        .messages
                        .iter()
                        .find(|p| p.part_type == "text")
                        .and_then(|p| p.content.clone())
                        .unwrap_or_else(|| "New Chat".to_string());

                    let new_chat = chats::ActiveModel {
                        id: Set(Uuid::new_v4()),
                        org_id: Set(Some(user.org_id.clone())),
                        user_id: Set(Some(user.id)),
                        title: Set(Some(title_text.chars().take(30).collect())),
                        agent_id: Set(Some(agent.id)),
                        model: Set(agent.default_model.clone()),
                        last_message_id: Set(Some(0)),
                        external_id: Set(external_id_to_save.clone()),
                        incognito: Set(Some(is_incognito)),
                        ..Default::default()
                    };
                    (new_chat.insert(&state.db).await?.id, 0)
                }
            };

            // 保存用户原始消息 — 解析组合语法，取 input_state
            let wf_state_raw = req
                .state
                .clone()
                .unwrap_or_else(|| messages::states::CONTEXT_VISIBLE.to_string());
            let wf_input_state = match wf_state_raw.split_once(':') {
                Some((i, _)) => i.to_string(),
                None => wf_state_raw,
            };
            let user_message_id = last_message_id + 1;
            let user_message_uuid = Uuid::new_v4();
            let user_msg = messages::ActiveModel {
                id: Set(user_message_uuid),
                chat_id: Set(chat_id),
                message_id: Set(user_message_id),
                role: Set("user".to_string()),
                message_type: Set("chat".to_string()),
                state: Set(wf_input_state),
                parts: Set(serde_json::to_value(&req.messages)?),
                ..Default::default()
            };
            user_msg.insert(&state.db).await?;

            // 更新 chat.last_message_id
            chats::Entity::update_many()
                .col_expr(
                    chats::Column::LastMessageId,
                    sea_orm::sea_query::Expr::value(user_message_id),
                )
                .filter(chats::Column::Id.eq(chat_id))
                .exec(&state.db)
                .await?;
            // ======= 新增结束 =======

            // 转发到 workflow endpoint
            // endpoint_url 可能是 base URL 或包含 /api/chat，统一处理
            let base_url = endpoint_url
                .trim_end_matches('/')
                .trim_end_matches("/api/chat");
            let chat_url = format!("{}/api/chat", base_url);

            // 生成 Execution Token，用于 juglans-agent 回调时验证原始调用者
            let execution_token = {
                let author_user_id = agent.user_id.unwrap_or(user.id);
                let claims = ExecutionClaims::new(
                    user.id,             // caller_user_id
                    user.org_id.clone(), // caller_org_id
                    author_user_id,      // author_user_id
                    agent.id,            // agent_id
                    Some(chat_id),       // chat_id (现在有真实的)
                    300,                 // TTL: 5 分钟
                );
                claims.encode(&state.signing_key)
            };

            tracing::info!(
                "🔐 [Forward] Forwarding chat to agent '{}' at {} with execution token (caller: {}, author: {}, chat_id: {})",
                agent.slug, chat_url, user.id, agent.user_id.unwrap_or(user.id), chat_id
            );

            // Cache workflow forward info for tool_result routing (zero-DB-query path)
            state.workflow_forwards.insert(
                chat_id,
                WorkflowForwardInfo {
                    endpoint_url: endpoint_url.clone(),
                    agent_id: agent.id,
                    author_user_id: agent.user_id.unwrap_or(user.id),
                },
            );

            return forward_to_workflow(
                &state.http_client,
                &chat_url,
                &req,
                req_stream,
                Some(execution_token),
                Some(chat_id), // 传递 jug0 创建的 chat_id，确保 workflow 使用同一个会话
                Some(user_message_id), // 传递用户消息 ID，供 workflow 回溯更新状态
                Some(user_message_uuid), // 传递用户消息 UUID，注入到 meta SSE 事件
            )
            .await;
        }
    }

    let (agent_id, final_model, mcp_config_json, final_sys_prompt) = match agent_opt {
        Some(a) => {
            let model = req_model
                .or(a.default_model)
                .unwrap_or(default_model.clone());
            let sys_prompt = if let Some(sp) = req_sys_prompt {
                Some(sp)
            } else if let Some(sp_id) = a.system_prompt_id {
                prompts::Entity::find_by_id(sp_id)
                    .one(&state.db)
                    .await?
                    .map(|m| m.content)
            } else {
                None
            };
            (Some(a.id), model, a.mcp_config, sys_prompt)
        }
        None => {
            if req_model.is_none() && req_sys_prompt.is_none() && target_slug != "default" {
                return Err(AppError::NotFound(format!(
                    "Agent '{}' not found",
                    target_slug
                )));
            }
            let model = req_model.unwrap_or(fallback_model.clone());
            (None, model, None, req_sys_prompt)
        }
    };

    let mut server_tools = Vec::new();
    if let Some(config_json) = mcp_config_json {
        if let Ok(configs) = serde_json::from_value::<Vec<McpServerConfig>>(config_json) {
            for config in configs {
                match state.mcp_client.fetch_tools(&config).await {
                    Ok(tools) => server_tools.extend(tools),
                    Err(e) => tracing::error!("Failed to fetch tools from {}: {}", config.name, e),
                }
            }
        }
    }
    tracing::info!(
        "⏱ [Chat] MCP tools fetch: {}ms (server_tools: {})",
        start_time.elapsed().as_millis(),
        server_tools.len()
    );

    // 获取或创建 chat，同时获取 last_message_id
    let is_incognito = matches!(&req.history, Some(serde_json::Value::Bool(false)));
    let (chat_id, mut last_message_id) = match existing_chat_id {
        Some(id) => {
            let chat = chats::Entity::find_by_id(id)
                .filter(chats::Column::OrgId.eq(&user.org_id))
                .filter(chats::Column::UserId.eq(user.id))
                .one(&state.db)
                .await?;
            match chat {
                Some(c) => (id, c.last_message_id.unwrap_or(0)),
                None => {
                    // Chat not found — workflow sub-agent may inherit stale parent chat_id
                    tracing::warn!(
                        "Chat {} not found for user {}, creating new chat",
                        id,
                        user.id
                    );
                    let title_text = req
                        .messages
                        .iter()
                        .find(|p| p.part_type == "text")
                        .and_then(|p| p.content.clone())
                        .unwrap_or_else(|| "New Chat".to_string());
                    let new_chat = chats::ActiveModel {
                        id: Set(Uuid::new_v4()),
                        org_id: Set(Some(user.org_id.clone())),
                        user_id: Set(Some(user.id)),
                        title: Set(Some(title_text.chars().take(30).collect())),
                        agent_id: Set(agent_id),
                        model: Set(Some(final_model.clone())),
                        last_message_id: Set(Some(0)),
                        external_id: Set(external_id_to_save.clone()),
                        incognito: Set(Some(is_incognito)),
                        ..Default::default()
                    };
                    (new_chat.insert(&state.db).await?.id, 0)
                }
            }
        }
        None => {
            let title_text = req
                .messages
                .iter()
                .find(|p| p.part_type == "text")
                .and_then(|p| p.content.clone())
                .unwrap_or_else(|| "New Chat".to_string());

            let new_chat = chats::ActiveModel {
                id: Set(Uuid::new_v4()),
                org_id: Set(Some(user.org_id.clone())),
                user_id: Set(Some(user.id)),
                title: Set(Some(title_text.chars().take(30).collect())),
                agent_id: Set(agent_id),
                model: Set(Some(final_model.clone())),
                last_message_id: Set(Some(0)),
                external_id: Set(external_id_to_save.clone()),
                incognito: Set(Some(is_incognito)),
                ..Default::default()
            };
            (new_chat.insert(&state.db).await?.id, 0)
        }
    };
    tracing::info!(
        "⏱ [Chat] Chat init: {}ms (chat_id: {})",
        start_time.elapsed().as_millis(),
        chat_id
    );

    if let Some(old_token) = state.active_requests.get(&chat_id) {
        old_token.cancel();
    }

    // Persist client tools to chat.metadata for tool-result continuation
    if let Some(tools) = &req_tools {
        if !tools.is_empty() {
            let metadata = json!({ "client_tools": tools });
            chats::Entity::update_many()
                .col_expr(
                    chats::Column::Metadata,
                    sea_orm::sea_query::Expr::value(metadata),
                )
                .filter(chats::Column::Id.eq(chat_id))
                .exec(&state.db)
                .await?;
        }
    }

    // 解析 state 组合语法（input:output），默认 context_visible
    let state_raw = req
        .state
        .clone()
        .unwrap_or_else(|| messages::states::CONTEXT_VISIBLE.to_string());
    let (input_state, output_state) = match state_raw.split_once(':') {
        Some((i, o)) => (i.to_string(), o.to_string()),
        None => (state_raw.clone(), state_raw.clone()),
    };

    // 保存用户消息（分配 message_id）— 用 input_state
    let is_tool_result = req
        .messages
        .first()
        .map(|p| p.part_type == "tool_result")
        .unwrap_or(false);
    let role = if is_tool_result { "tool" } else { "user" };
    let message_type = if is_tool_result {
        "tool_result"
    } else {
        "chat"
    };

    last_message_id += 1;
    let user_message_id = last_message_id;
    let user_message_uuid = Uuid::new_v4();

    let user_msg = messages::ActiveModel {
        id: Set(user_message_uuid),
        chat_id: Set(chat_id),
        message_id: Set(user_message_id),
        role: Set(role.to_string()),
        message_type: Set(message_type.to_string()),
        state: Set(input_state),
        parts: Set(serde_json::to_value(&req.messages)?),
        tool_call_id: Set(if is_tool_result {
            req.messages.first().and_then(|p| p.tool_call_id.clone())
        } else {
            None
        }),
        ..Default::default()
    };
    user_msg.insert(&state.db).await?;

    // Auto-generate chat title in background (only for user messages, not tool results)
    if !is_tool_result {
        let user_text = req.messages.iter()
            .find(|m| m.part_type == "text")
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        if !user_text.is_empty() {
            let title_db = state.db.clone();
            let title_providers = state.providers.clone();
            let title_model = env::var("DEFAULT_LLM_MODEL").unwrap_or_else(|_| "qwen-plus".into());
            tokio::spawn(async move {
                generate_chat_title(title_db, title_providers, chat_id, &title_model, &user_text).await;
            });
        }
    }

    // 更新 chat.last_message_id
    chats::Entity::update_many()
        .col_expr(
            chats::Column::LastMessageId,
            sea_orm::sea_query::Expr::value(user_message_id),
        )
        .filter(chats::Column::Id.eq(chat_id))
        .exec(&state.db)
        .await?;

    tracing::info!(
        "⏱ [Chat] User msg saved: {}ms | model: {}, sys_prompt: {}, req_tools: {}, server_tools: {}",
        start_time.elapsed().as_millis(),
        final_model,
        final_sys_prompt.is_some(),
        req_tools.as_ref().map(|t| t.len()).unwrap_or(0),
        server_tools.len()
    );

    // 创建 tool-result channel（仅流式模式）
    let tool_result_rx = if req_stream {
        let (tx, rx) = tokio::sync::mpsc::channel::<types::ToolResultWithTools>(1);
        state.tool_result_channels.insert(chat_id, tx);
        Some(rx)
    } else {
        None
    };

    let internal_stream = run_chat_stream(
        state.db.clone(),
        state.active_requests.clone(),
        state.mcp_client.clone(),
        state.providers.clone(),
        state.memory_service.clone(),
        state.cache.clone(),
        user.clone(),
        chat_id,
        last_message_id,   // 传递当前 last_message_id
        user_message_uuid, // 传递用户消息 UUID
        final_model,
        final_sys_prompt,
        req_tools,
        server_tools,
        should_use_memory,
        req.history.clone(), // 上下文控制
        output_state,        // AI 回复的消息状态
        tool_result_rx,
        state.tool_result_channels.clone(),
    );
    tracing::info!(
        "⏱ [Chat] Stream setup: {}ms — handing off to SSE",
        start_time.elapsed().as_millis()
    );

    if req_stream {
        let sse_stream = internal_stream.map(move |res| match res {
            Ok(event) => match event {
                InternalStreamEvent::Meta {
                    chat_id,
                    user_message_id,
                    user_message_uuid,
                } => Ok::<Event, AppError>(
                    Event::default().event("meta").data(
                        json!({
                            "type": "meta",
                            "chat_id": chat_id,
                            "user_message_id": user_message_id,
                            "user_message_uuid": user_message_uuid
                        })
                        .to_string(),
                    ),
                ),
                InternalStreamEvent::Content(text) => Ok::<Event, AppError>(
                    Event::default().data(json!({ "type": "content", "text": text }).to_string()),
                ),
                InternalStreamEvent::ToolCall { message_id, tools } => Ok::<Event, AppError>(
                    Event::default().event("tool_call").data(
                        json!({
                            "type": "tool_call",
                            "call_id": chat_id.to_string(),
                            "message_id": message_id,
                            "tools": tools
                        })
                        .to_string(),
                    ),
                ),
                InternalStreamEvent::Done {
                    message_id,
                    assistant_message_uuid,
                } => {
                    let duration_ms = start_time.elapsed().as_millis();
                    tracing::info!(
                        "✅ Chat completed in {}ms (message_id: {})",
                        duration_ms,
                        message_id
                    );
                    Ok::<Event, AppError>(
                        Event::default().event("done").data(
                            json!({
                                "type": "done",
                                "message_id": message_id,
                                "assistant_message_uuid": assistant_message_uuid,
                                "duration_ms": duration_ms
                            })
                            .to_string(),
                        ),
                    )
                }
                InternalStreamEvent::Error(err) => {
                    Ok::<Event, AppError>(Event::default().event("error").data(err))
                }
            },
            Err(e) => Ok::<Event, AppError>(Event::default().event("error").data(e.to_string())),
        });
        Ok(Sse::new(sse_stream)
            .keep_alive(axum::response::sse::KeepAlive::default())
            .into_response())
    } else {
        let mut final_content = String::new();
        let mut tool_calls = None;
        let mut resp_role = "assistant".to_string();
        let mut final_message_id = last_message_id;

        let mut pinned_stream = Box::pin(internal_stream);
        while let Some(event_res) = pinned_stream.next().await {
            match event_res {
                Ok(event) => match event {
                    InternalStreamEvent::Meta {
                        user_message_id, ..
                    } => {
                        final_message_id = user_message_id;
                    }
                    InternalStreamEvent::Content(text) => final_content.push_str(&text),
                    InternalStreamEvent::ToolCall { message_id, tools } => {
                        resp_role = "tool_call".to_string();
                        tool_calls = Some(tools);
                        final_message_id = message_id;
                    }
                    InternalStreamEvent::Done { message_id, .. } => {
                        final_message_id = message_id;
                    }
                    InternalStreamEvent::Error(err) => {
                        return Err(AppError::Internal(anyhow::anyhow!("Chat Error: {}", err)))
                    }
                },
                Err(e) => return Err(e),
            }
        }

        let duration_ms = start_time.elapsed().as_millis();
        tracing::info!(
            "✅ Chat completed in {}ms (chat_id: {}, message_id: {})",
            duration_ms,
            chat_id,
            final_message_id
        );

        Ok(Json(ChatSyncResponse {
            chat_id,
            message_id: final_message_id,
            role: resp_role,
            content: final_content,
            tool_calls,
        })
        .into_response())
    }
}

/// Generate a concise chat title using the default LLM provider (background task)
async fn generate_chat_title(
    db: sea_orm::DatabaseConnection,
    providers: crate::providers::ProviderFactory,
    chat_id: Uuid,
    model: &str,
    user_text: &str,
) {
    use futures::StreamExt;

    let truncated = if user_text.len() > 200 { &user_text[..200] } else { user_text };
    let prompt = format!(
        "Generate a concise title (max 8 words) for this conversation based on the user's message. Reply with ONLY the title, no quotes, no explanation.\n\nUser message: \"{}\"",
        truncated
    );

    // Build a minimal messages::Model for the prompt
    let fake_msg = crate::entities::messages::Model {
        id: Uuid::new_v4(),
        chat_id,
        message_id: 1,
        role: "user".to_string(),
        message_type: "chat".to_string(),
        state: "context_visible".to_string(),
        parts: serde_json::json!([{ "type": "text", "content": prompt }]),
        tool_calls: None,
        tool_call_id: None,
        ref_message_id: None,
        metadata: None,
        created_at: None,
        updated_at: None,
    };

    let (provider, actual_model) = providers.get_provider(model);
    let msg: crate::providers::llm::Message = (&fake_msg).into();
    match provider.stream_chat(&actual_model, None, vec![msg], None).await {
        Ok(mut stream) => {
            let mut title = String::new();
            while let Some(Ok(chunk)) = stream.next().await {
                if let Some(text) = chunk.content {
                    title.push_str(&text);
                }
            }
            let title = title.trim().trim_matches('"').trim_matches('\'').to_string();
            if !title.is_empty() && title.len() < 200 {
                let mut chat: chats::ActiveModel = Default::default();
                chat.id = Set(chat_id);
                chat.title = Set(Some(title));
                if let Err(e) = chats::Entity::update(chat).exec(&db).await {
                    tracing::warn!("Auto-title update failed: {e}");
                }
            }
        }
        Err(e) => tracing::warn!("Auto-title generation failed: {e}"),
    }
}
