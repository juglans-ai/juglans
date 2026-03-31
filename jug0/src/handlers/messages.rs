// src/handlers/messages.rs
use axum::{
    extract::{Extension, Json, Path, Query},
    Json as AxumJson,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, IntoActiveModel,
    QueryFilter, QueryOrder, QuerySelect, Set, Statement,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::entities::{chats, messages};
use crate::errors::AppError;
use crate::handlers::chat::resolve_chat_id_strict;
use crate::handlers::chat::types::{
    ContextQuery, ContextResponse, CreateMessageRequest, MessageResponse, UpdateMessageRequest,
};
use crate::request::chats::{BatchDeleteMessagesRequest, TruncateRequest};
use crate::AppState;

// ============================================
// 辅助函数
// ============================================

/// 验证 chat 所有权
async fn ensure_chat_owner(
    state: &AppState,
    user: &AuthUser,
    chat_id: Uuid,
) -> Result<chats::Model, AppError> {
    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::OrgId.eq(&user.org_id))
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Chat {} not found", chat_id)))?;

    Ok(chat)
}

/// 验证消息所有权（通过 UUID）
async fn ensure_message_owner(
    state: &AppState,
    user_id: Uuid,
    message_id: Uuid,
) -> Result<messages::Model, AppError> {
    let message = messages::Entity::find_by_id(message_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Message {} not found", message_id)))?;

    let chat = chats::Entity::find_by_id(message.chat_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Associated chat not found".to_string()))?;

    if let Some(owner) = chat.user_id {
        if owner != user_id {
            return Err(AppError::Unauthorized("Access denied".to_string()));
        }
    } else {
        return Err(AppError::Unauthorized("Access denied".to_string()));
    }

    Ok(message)
}

/// 获取消息（通过 chat_id + message_id）
async fn get_message_by_chat_and_id(
    state: &AppState,
    chat_id: Uuid,
    message_id: i32,
) -> Result<Option<messages::Model>, AppError> {
    let message = messages::Entity::find()
        .filter(messages::Column::ChatId.eq(chat_id))
        .filter(messages::Column::MessageId.eq(message_id))
        .one(&state.db)
        .await?;

    Ok(message)
}

// ============================================
// 消息创建
// ============================================

/// POST /api/chats/:chat_id/messages (supports UUID or @handle)
/// 创建消息（自动分配 message_id）
pub async fn create_message(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(chat_id_or_handle): Path<String>,
    Json(req): Json<CreateMessageRequest>,
) -> Result<AxumJson<MessageResponse>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 1. 验证 chat 所有权
    let chat = ensure_chat_owner(&state, &user, chat_id).await?;

    // 2. 分配 message_id
    let next_message_id = chat.last_message_id.unwrap_or(0) + 1;

    // 3. 创建消息
    let new_message = messages::ActiveModel {
        id: Set(Uuid::new_v4()),
        chat_id: Set(chat_id),
        message_id: Set(next_message_id),
        role: Set(req.role),
        message_type: Set(req.message_type),
        state: Set(req.state.clone()),
        parts: Set(serde_json::to_value(&req.parts)?),
        tool_calls: Set(req.tool_calls.and_then(|tc| serde_json::to_value(tc).ok())),
        tool_call_id: Set(req.tool_call_id),
        ref_message_id: Set(req.ref_message_id),
        metadata: Set(req.metadata),
        ..Default::default()
    };
    let inserted = new_message.insert(&state.db).await?;

    // 4. 更新 chat.last_message_id
    chats::Entity::update_many()
        .col_expr(
            chats::Column::LastMessageId,
            sea_orm::sea_query::Expr::value(next_message_id),
        )
        .col_expr(
            chats::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(chrono::Utc::now().naive_utc()),
        )
        .filter(chats::Column::Id.eq(chat_id))
        .exec(&state.db)
        .await?;

    Ok(AxumJson(MessageResponse::from(inserted)))
}

// ============================================
// 消息列表
// ============================================

/// GET /api/chats/:chat_id/messages (supports UUID or @handle)
/// 列出消息
pub async fn list_messages(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(chat_id_or_handle): Path<String>,
    Query(query): Query<ContextQuery>,
) -> Result<AxumJson<Vec<MessageResponse>>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 验证 chat 所有权
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    // 构建查询
    let mut db_query = messages::Entity::find().filter(messages::Column::ChatId.eq(chat_id));

    // state 过滤（默认仅返回用户可见的消息：context_visible + display_only）
    if !query.include_all {
        db_query = db_query.filter(messages::Column::State.is_in([
            messages::states::CONTEXT_VISIBLE,
            messages::states::DISPLAY_ONLY,
        ]));
    }

    // 分页
    if let Some(from_id) = query.from_message_id {
        db_query = db_query.filter(messages::Column::MessageId.gte(from_id));
    }

    if let Some(limit) = query.limit {
        db_query = db_query.limit(limit as u64);
    }

    let messages = db_query
        .order_by_asc(messages::Column::MessageId)
        .all(&state.db)
        .await?;

    Ok(AxumJson(
        messages.into_iter().map(MessageResponse::from).collect(),
    ))
}

// ============================================
// 上下文查询
// ============================================

/// GET /api/chats/:chat_id/context (supports UUID or @handle)
/// 获取 AI 上下文（默认仅包含 context_visible / context_hidden 消息）
pub async fn get_context(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(chat_id_or_handle): Path<String>,
    Query(query): Query<ContextQuery>,
) -> Result<AxumJson<ContextResponse>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 验证 chat 所有权
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    // 构建查询（默认仅包含 context_visible / context_hidden）
    let mut db_query = messages::Entity::find().filter(messages::Column::ChatId.eq(chat_id));

    if !query.include_all {
        db_query = db_query.filter(messages::Column::State.is_in([
            messages::states::CONTEXT_VISIBLE,
            messages::states::CONTEXT_HIDDEN,
        ]));
    }

    if let Some(from_id) = query.from_message_id {
        db_query = db_query.filter(messages::Column::MessageId.gte(from_id));
    }

    if let Some(limit) = query.limit {
        db_query = db_query.limit(limit as u64);
    }

    let messages = db_query
        .order_by_asc(messages::Column::MessageId)
        .all(&state.db)
        .await?;

    Ok(AxumJson(ContextResponse {
        chat_id,
        messages: messages.into_iter().map(MessageResponse::from).collect(),
    }))
}

/// GET /api/chats/:chat_id/history (supports UUID or @handle)
/// 获取完整历史（包含所有 state，用于审计/回放）
pub async fn get_history(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(chat_id_or_handle): Path<String>,
    Query(query): Query<ContextQuery>,
) -> Result<AxumJson<ContextResponse>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 验证 chat 所有权
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    // 获取所有消息（包含所有 state）
    let mut db_query = messages::Entity::find().filter(messages::Column::ChatId.eq(chat_id));

    if let Some(from_id) = query.from_message_id {
        db_query = db_query.filter(messages::Column::MessageId.gte(from_id));
    }

    if let Some(limit) = query.limit {
        db_query = db_query.limit(limit as u64);
    }

    let messages = db_query
        .order_by_asc(messages::Column::MessageId)
        .all(&state.db)
        .await?;

    Ok(AxumJson(ContextResponse {
        chat_id,
        messages: messages.into_iter().map(MessageResponse::from).collect(),
    }))
}

// ============================================
// 消息获取/更新/删除（按 UUID）
// ============================================

/// GET /api/messages/:id
/// 获取消息（通过 UUID）
pub async fn get_message(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<AxumJson<MessageResponse>, AppError> {
    let message = ensure_message_owner(&state, user.id, id).await?;
    Ok(AxumJson(MessageResponse::from(message)))
}

/// PATCH /api/messages/:id
/// 更新消息（通过 UUID）
pub async fn update_message(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateMessageRequest>,
) -> Result<AxumJson<MessageResponse>, AppError> {
    let message = ensure_message_owner(&state, user.id, id).await?;

    let mut active = message.into_active_model();

    if let Some(parts) = req.parts {
        active.parts = Set(serde_json::to_value(&parts)?);
    }

    if let Some(metadata) = req.metadata {
        active.metadata = Set(Some(metadata));
    }

    if let Some(state_val) = req.state {
        active.state = Set(state_val);
    }

    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));

    let updated = active.update(&state.db).await?;
    Ok(AxumJson(MessageResponse::from(updated)))
}

/// DELETE /api/messages/:id
/// 删除消息（通过 UUID）
pub async fn delete_message(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let _ = ensure_message_owner(&state, user.id, id).await?;

    messages::Entity::delete_by_id(id).exec(&state.db).await?;

    Ok(AxumJson(json!({
        "success": true,
        "id": id
    })))
}

// ============================================
// 消息获取/更新/删除（按 chat_id + message_id）
// ============================================

/// GET /api/chats/:chat_id/messages/:message_id (supports UUID or @handle)
/// 获取消息（通过 chat_id + message_id）
pub async fn get_message_by_id(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path((chat_id_or_handle, message_id)): Path<(String, i32)>,
) -> Result<AxumJson<MessageResponse>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 验证 chat 所有权
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    let message = get_message_by_chat_and_id(&state, chat_id, message_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Message {} not found", message_id)))?;

    Ok(AxumJson(MessageResponse::from(message)))
}

/// PATCH /api/chats/:chat_id/messages/:message_id (supports UUID or @handle)
/// 更新消息（通过 chat_id + message_id）
pub async fn update_message_by_id(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path((chat_id_or_handle, message_id)): Path<(String, i32)>,
    Json(req): Json<UpdateMessageRequest>,
) -> Result<AxumJson<MessageResponse>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 验证 chat 所有权
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    let message = get_message_by_chat_and_id(&state, chat_id, message_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Message {} not found", message_id)))?;

    let mut active = message.into_active_model();

    if let Some(parts) = req.parts {
        active.parts = Set(serde_json::to_value(&parts)?);
    }

    if let Some(metadata) = req.metadata {
        active.metadata = Set(Some(metadata));
    }

    if let Some(state_val) = req.state {
        active.state = Set(state_val);
    }

    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));

    let updated = active.update(&state.db).await?;
    Ok(AxumJson(MessageResponse::from(updated)))
}

/// DELETE /api/chats/:chat_id/messages/:message_id (supports UUID or @handle)
/// 删除消息（通过 chat_id + message_id）
pub async fn delete_message_by_id(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path((chat_id_or_handle, message_id)): Path<(String, i32)>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    // 验证 chat 所有权
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    let message = get_message_by_chat_and_id(&state, chat_id, message_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Message {} not found", message_id)))?;

    messages::Entity::delete_by_id(message.id)
        .exec(&state.db)
        .await?;

    Ok(AxumJson(json!({
        "success": true,
        "chat_id": chat_id,
        "message_id": message_id
    })))
}

// ============================================
// 辅助函数（供其他 handler 使用）
// ============================================

/// 加载上下文（供 chat handler 使用）
pub async fn load_context_for_chat(
    db: &sea_orm::DatabaseConnection,
    chat_id: Uuid,
    include_all: bool,
) -> Result<Vec<messages::Model>, AppError> {
    let mut query = messages::Entity::find().filter(messages::Column::ChatId.eq(chat_id));

    if !include_all {
        query = query.filter(messages::Column::State.is_in([
            messages::states::CONTEXT_VISIBLE,
            messages::states::CONTEXT_HIDDEN,
        ]));
    }

    query
        .order_by_asc(messages::Column::MessageId)
        .all(db)
        .await
        .map_err(Into::into)
}

/// 创建消息并更新 last_message_id（供 chat handler 使用）
pub async fn create_message_internal(
    db: &sea_orm::DatabaseConnection,
    chat_id: Uuid,
    last_message_id: i32,
    role: &str,
    message_type: &str,
    state: &str,
    parts: serde_json::Value,
    tool_calls: Option<serde_json::Value>,
    tool_call_id: Option<String>,
    ref_message_id: Option<i32>,
    metadata: Option<serde_json::Value>,
) -> Result<(messages::Model, i32), AppError> {
    let next_message_id = last_message_id + 1;

    let new_message = messages::ActiveModel {
        id: Set(Uuid::new_v4()),
        chat_id: Set(chat_id),
        message_id: Set(next_message_id),
        role: Set(role.to_string()),
        message_type: Set(message_type.to_string()),
        state: Set(state.to_string()),
        parts: Set(parts),
        tool_calls: Set(tool_calls),
        tool_call_id: Set(tool_call_id),
        ref_message_id: Set(ref_message_id),
        metadata: Set(metadata),
        ..Default::default()
    };
    let inserted = new_message.insert(db).await?;

    // 更新 chat.last_message_id
    let update_result = chats::Entity::update_many()
        .col_expr(
            chats::Column::LastMessageId,
            sea_orm::sea_query::Expr::value(next_message_id),
        )
        .col_expr(
            chats::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(chrono::Utc::now().naive_utc()),
        )
        .filter(chats::Column::Id.eq(chat_id))
        .exec(db)
        .await?;

    if update_result.rows_affected == 0 {
        tracing::warn!(
            "Failed to update chat.last_message_id for chat_id={}",
            chat_id
        );
    }

    Ok((inserted, next_message_id))
}

// ============================================
// 批量操作
// ============================================

/// POST /api/chat/:id/messages/batch-delete
/// 批量删除指定 message_id 的消息
pub async fn batch_delete_messages(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(chat_id_or_handle): Path<String>,
    Json(req): Json<BatchDeleteMessagesRequest>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    let _ = ensure_chat_owner(&state, &user, chat_id).await?;

    if req.message_ids.is_empty() {
        return Ok(AxumJson(json!({ "deleted_count": 0 })));
    }

    // Build parameterized IN clause
    let placeholders: Vec<String> = (0..req.message_ids.len())
        .map(|i| format!("${}", i + 2))
        .collect();
    let sql = format!(
        "DELETE FROM messages WHERE chat_id = $1 AND message_id IN ({})",
        placeholders.join(", ")
    );

    let mut values: Vec<sea_orm::Value> = vec![chat_id.into()];
    for id in &req.message_ids {
        values.push((*id).into());
    }

    let result = state
        .db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            values,
        ))
        .await?;

    Ok(AxumJson(json!({
        "deleted_count": result.rows_affected()
    })))
}

/// POST /api/chat/:id/messages/truncate
/// 删除 message_id > from_message_id 的所有消息
pub async fn truncate_messages(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(chat_id_or_handle): Path<String>,
    Json(req): Json<TruncateRequest>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let chat_id =
        resolve_chat_id_strict(&state.db, &user.org_id, user.id, &chat_id_or_handle).await?;
    let chat = ensure_chat_owner(&state, &user, chat_id).await?;

    let result = state
        .db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM messages WHERE chat_id = $1 AND message_id > $2",
            [chat_id.into(), req.from_message_id.into()],
        ))
        .await?;

    // Update last_message_id
    let mut active: chats::ActiveModel = chat.into();
    active.last_message_id = Set(Some(req.from_message_id));
    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
    active.update(&state.db).await?;

    Ok(AxumJson(json!({
        "truncated_to": req.from_message_id,
        "deleted_count": result.rows_affected()
    })))
}
