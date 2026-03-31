// src/handlers/context.rs
use axum::{
    extract::{Extension, Path},
    Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, ModelTrait,
    QueryFilter, QueryOrder, Set, Statement,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::handlers::chat::resolve_chat_id_strict;
use crate::request::chats::{BranchRequest, RegenerateRequest, UpdateChatRequest};
use crate::{
    entities::{chats, messages},
    errors::AppError,
    AppState,
};

// GET /api/chat/:id (supports UUID or @handle)
pub async fn get_history(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    // 1. 检查 Chat 是否存在且属于当前用户
    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    // 2. 获取消息列表
    let history = messages::Entity::find()
        .filter(messages::Column::ChatId.eq(chat_id))
        .order_by_asc(messages::Column::CreatedAt)
        .all(&state.db)
        .await?;

    Ok(Json(serde_json::json!({
        "chat": chat,
        "messages": history
    })))
}

// DELETE /api/chat/:id (supports UUID or @handle)
pub async fn delete_chat(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Resolve chat_id (UUID or @handle)
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    // 1. 检查是否存在且属于当前用户
    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    // 2. 删除 (先删消息，再删会话)
    messages::Entity::delete_many()
        .filter(messages::Column::ChatId.eq(chat_id))
        .exec(&state.db)
        .await?;

    chat.delete(&state.db).await?;

    Ok(Json(
        serde_json::json!({ "status": "deleted", "id": chat_id }),
    ))
}

// POST /api/chat/:id/clear — clear old messages, keep current turn
pub async fn clear_chat_history(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    let _chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    // 找到最后一条 user message 的 message_id，删除之前的所有消息
    let last_user_msg = messages::Entity::find()
        .filter(messages::Column::ChatId.eq(chat_id))
        .filter(messages::Column::Role.eq("user"))
        .order_by_desc(messages::Column::MessageId)
        .one(&state.db)
        .await?;

    let deleted_count = if let Some(last_user) = last_user_msg {
        let result = state
            .db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM messages WHERE chat_id = $1 AND message_id < $2",
                [chat_id.into(), last_user.message_id.into()],
            ))
            .await?;
        result.rows_affected()
    } else {
        0
    };

    Ok(Json(serde_json::json!({
        "status": "cleared",
        "id": chat_id,
        "deleted_count": deleted_count
    })))
}

// DELETE /api/chat/:id/messages — clear all messages, keep chat
pub async fn clear_chat_messages(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    // 只删消息，保留 chat 记录
    let result = messages::Entity::delete_many()
        .filter(messages::Column::ChatId.eq(chat_id))
        .exec(&state.db)
        .await?;

    // 重置 last_message_id
    let mut active: chats::ActiveModel = chat.into();
    active.last_message_id = Set(Some(0));
    active.update(&state.db).await?;

    Ok(Json(serde_json::json!({
        "status": "cleared",
        "id": chat_id,
        "deleted_count": result.rows_affected
    })))
}

// PATCH /api/chat/:id — update chat properties
pub async fn update_chat(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
    Json(req): Json<UpdateChatRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    let mut active: chats::ActiveModel = chat.into();

    if let Some(title) = req.title {
        active.title = Set(Some(title));
    }
    if let Some(model) = req.model {
        active.model = Set(Some(model));
    }
    if let Some(agent_id) = req.agent_id {
        active.agent_id = Set(Some(agent_id));
    }
    if let Some(incognito) = req.incognito {
        active.incognito = Set(Some(incognito));
    }
    if let Some(metadata) = req.metadata {
        active.metadata = Set(Some(metadata));
    }

    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
    let updated = active.update(&state.db).await?;

    Ok(Json(serde_json::json!(updated)))
}

// POST /api/chat/:id/branch — fork chat from a message
pub async fn branch_chat(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
    Json(req): Json<BranchRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    // Create new chat inheriting properties
    let new_chat_id = Uuid::new_v4();
    let title = req.title.unwrap_or_else(|| {
        format!(
            "Branch from {}",
            chat.title.as_deref().unwrap_or("Untitled")
        )
    });

    let new_chat = chats::ActiveModel {
        id: Set(new_chat_id),
        org_id: Set(chat.org_id.clone()),
        user_id: Set(chat.user_id),
        agent_id: Set(chat.agent_id),
        external_id: Set(None),
        model: Set(chat.model.clone()),
        title: Set(Some(title)),
        last_message_id: Set(Some(req.from_message_id)),
        metadata: Set(chat.metadata.clone()),
        incognito: Set(Some(false)),
        ..Default::default()
    };
    new_chat.insert(&state.db).await?;

    // Copy messages up to from_message_id
    let source_messages = messages::Entity::find()
        .filter(messages::Column::ChatId.eq(chat_id))
        .filter(messages::Column::MessageId.lte(req.from_message_id))
        .order_by_asc(messages::Column::MessageId)
        .all(&state.db)
        .await?;

    for msg in &source_messages {
        let new_msg = messages::ActiveModel {
            id: Set(Uuid::new_v4()),
            chat_id: Set(new_chat_id),
            message_id: Set(msg.message_id),
            role: Set(msg.role.clone()),
            message_type: Set(msg.message_type.clone()),
            state: Set(msg.state.clone()),
            parts: Set(msg.parts.clone()),
            tool_calls: Set(msg.tool_calls.clone()),
            tool_call_id: Set(msg.tool_call_id.clone()),
            ref_message_id: Set(msg.ref_message_id),
            metadata: Set(msg.metadata.clone()),
            ..Default::default()
        };
        new_msg.insert(&state.db).await?;
    }

    Ok(Json(serde_json::json!({
        "id": new_chat_id,
        "branched_from": chat_id,
        "from_message_id": req.from_message_id,
        "messages_copied": source_messages.len()
    })))
}

// POST /api/chat/:id/regenerate — prepare to regenerate last assistant response
pub async fn regenerate_chat(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_or_handle): Path<String>,
    Json(req): Json<RegenerateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let chat_id = resolve_chat_id_strict(&state.db, &user.org_id, user.id, &id_or_handle).await?;

    let chat = chats::Entity::find_by_id(chat_id)
        .filter(chats::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Chat {} not found or access denied", chat_id))
        })?;

    // Find last assistant message
    let last_assistant = messages::Entity::find()
        .filter(messages::Column::ChatId.eq(chat_id))
        .filter(messages::Column::Role.eq("assistant"))
        .order_by_desc(messages::Column::MessageId)
        .one(&state.db)
        .await?;

    let mut deleted_message_id: Option<i32> = None;

    if let Some(ref assistant_msg) = last_assistant {
        if !req.keep_message {
            // Delete the assistant message
            deleted_message_id = Some(assistant_msg.message_id);
            messages::Entity::delete_by_id(assistant_msg.id)
                .exec(&state.db)
                .await?;

            // Update last_message_id
            let new_last = assistant_msg.message_id - 1;
            let mut active: chats::ActiveModel = chat.clone().into();
            active.last_message_id = Set(Some(new_last));
            active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
            active.update(&state.db).await?;
        }
    }

    // Update model if requested
    if let Some(ref model) = req.model {
        let fresh = chats::Entity::find_by_id(chat_id)
            .one(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Chat not found".to_string()))?;
        let mut active: chats::ActiveModel = fresh.into();
        active.model = Set(Some(model.clone()));
        active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
        active.update(&state.db).await?;
    }

    // Find last user message (the prompt to regenerate from)
    let last_user = messages::Entity::find()
        .filter(messages::Column::ChatId.eq(chat_id))
        .filter(messages::Column::Role.eq("user"))
        .order_by_desc(messages::Column::MessageId)
        .one(&state.db)
        .await?;

    Ok(Json(serde_json::json!({
        "ready": true,
        "chat_id": chat_id,
        "deleted_message_id": deleted_message_id,
        "last_user_message": last_user.map(|m| serde_json::json!({
            "message_id": m.message_id,
            "parts": m.parts,
        })),
        "model": req.model,
    })))
}
