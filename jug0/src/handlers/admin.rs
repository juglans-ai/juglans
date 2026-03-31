// src/handlers/admin.rs
//
// Admin usage statistics handlers - aggregates token usage across all users

use axum::{
    extract::{Extension, Path, Query},
    Json,
};
use chrono::{Datelike, NaiveDateTime, NaiveTime, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{agents, chats, messages, user_quotas, users};
use crate::errors::AppError;
use crate::response::admin::{AdminChat, GlobalUsageStats, UserQuotaResponse, UserUsage};
use crate::response::usage::ModelUsage;
use crate::services::quota;
use crate::AppState;

/// GET /api/admin/usage
///
/// Returns global token usage statistics for the current month across all users.
pub async fn global_usage(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<GlobalUsageStats>, AppError> {
    let now = Utc::now().naive_utc();
    let start_of_month = NaiveDateTime::new(
        now.date().with_day(1).unwrap_or(now.date()),
        NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    );

    // Get all chats (no user filter)
    let all_chats = chats::Entity::find().all(&state.db).await?;
    let chat_ids: Vec<_> = all_chats.iter().map(|c| c.id).collect();

    if chat_ids.is_empty() {
        return Ok(Json(GlobalUsageStats {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            by_model: vec![],
            by_user: vec![],
            period_start: start_of_month.format("%Y-%m-%d").to_string(),
            period_end: now.format("%Y-%m-%d").to_string(),
        }));
    }

    // Build chat_id -> user_id mapping
    let chat_user_map: HashMap<uuid::Uuid, Option<uuid::Uuid>> =
        all_chats.iter().map(|c| (c.id, c.user_id)).collect();

    // Load all users for name lookup
    let all_users = users::Entity::find().all(&state.db).await?;
    let user_name_map: HashMap<uuid::Uuid, Option<String>> =
        all_users.iter().map(|u| (u.id, u.name.clone())).collect();

    // Query assistant messages from this month
    let all_messages = messages::Entity::find()
        .filter(messages::Column::ChatId.is_in(chat_ids))
        .filter(messages::Column::Role.eq("assistant"))
        .filter(messages::Column::CreatedAt.gte(start_of_month))
        .all(&state.db)
        .await?;

    // Aggregate
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut model_map: HashMap<String, (i64, i64, i64)> = HashMap::new();
    // user_id -> (input, output, count)
    let mut user_map: HashMap<uuid::Uuid, (i64, i64, i64)> = HashMap::new();

    for msg in &all_messages {
        if let Some(metadata) = &msg.metadata {
            let model = metadata
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            if let Some(usage) = metadata.get("usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                total_input += input;
                total_output += output;

                let entry = model_map.entry(model).or_insert((0, 0, 0));
                entry.0 += input;
                entry.1 += output;
                entry.2 += 1;

                // Attribute to user via chat
                if let Some(Some(uid)) = chat_user_map.get(&msg.chat_id) {
                    let ue = user_map.entry(*uid).or_insert((0, 0, 0));
                    ue.0 += input;
                    ue.1 += output;
                    ue.2 += 1;
                }
            }
        }
    }

    let mut by_model: Vec<ModelUsage> = model_map
        .into_iter()
        .map(|(model, (input, output, count))| ModelUsage {
            model,
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            message_count: count,
        })
        .collect();
    by_model.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    let mut by_user: Vec<UserUsage> = user_map
        .into_iter()
        .map(|(uid, (input, output, count))| UserUsage {
            user_id: uid.to_string(),
            name: user_name_map.get(&uid).cloned().flatten(),
            total_input_tokens: input,
            total_output_tokens: output,
            total_tokens: input + output,
            message_count: count,
        })
        .collect();
    by_user.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    Ok(Json(GlobalUsageStats {
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_tokens: total_input + total_output,
        by_model,
        by_user,
        period_start: start_of_month.format("%Y-%m-%d").to_string(),
        period_end: now.format("%Y-%m-%d").to_string(),
    }))
}

/// GET /api/admin/users
///
/// Returns all users with their current month token usage, sorted by total_tokens desc.
pub async fn user_usage_list(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<UserUsage>>, AppError> {
    let now = Utc::now().naive_utc();
    let start_of_month = NaiveDateTime::new(
        now.date().with_day(1).unwrap_or(now.date()),
        NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    );

    let all_users = users::Entity::find().all(&state.db).await?;
    let all_chats = chats::Entity::find().all(&state.db).await?;
    let chat_ids: Vec<_> = all_chats.iter().map(|c| c.id).collect();

    // Build chat_id -> user_id mapping
    let chat_user_map: HashMap<uuid::Uuid, Option<uuid::Uuid>> =
        all_chats.iter().map(|c| (c.id, c.user_id)).collect();

    // Query assistant messages this month
    let all_messages = if chat_ids.is_empty() {
        vec![]
    } else {
        messages::Entity::find()
            .filter(messages::Column::ChatId.is_in(chat_ids))
            .filter(messages::Column::Role.eq("assistant"))
            .filter(messages::Column::CreatedAt.gte(start_of_month))
            .all(&state.db)
            .await?
    };

    // Aggregate per user
    let mut user_map: HashMap<uuid::Uuid, (i64, i64, i64)> = HashMap::new();
    for msg in &all_messages {
        if let Some(metadata) = &msg.metadata {
            if let Some(usage) = metadata.get("usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                if let Some(Some(uid)) = chat_user_map.get(&msg.chat_id) {
                    let entry = user_map.entry(*uid).or_insert((0, 0, 0));
                    entry.0 += input;
                    entry.1 += output;
                    entry.2 += 1;
                }
            }
        }
    }

    let mut result: Vec<UserUsage> = all_users
        .iter()
        .map(|u| {
            let (input, output, count) = user_map.get(&u.id).copied().unwrap_or((0, 0, 0));
            UserUsage {
                user_id: u.id.to_string(),
                name: u.name.clone(),
                total_input_tokens: input,
                total_output_tokens: output,
                total_tokens: input + output,
                message_count: count,
            }
        })
        .collect();

    result.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct AdminChatsQuery {
    pub limit: Option<u64>,
}

/// GET /api/admin/chats
///
/// Returns all chats with user and agent info, sorted by updated_at desc.
pub async fn admin_chats(
    Extension(state): Extension<Arc<AppState>>,
    Query(query): Query<AdminChatsQuery>,
) -> Result<Json<Vec<AdminChat>>, AppError> {
    let limit = query.limit.unwrap_or(100);

    let all_chats = chats::Entity::find()
        .order_by_desc(chats::Column::UpdatedAt)
        .all(&state.db)
        .await?;

    // Batch fetch users and agents
    let user_ids: Vec<uuid::Uuid> = all_chats.iter().filter_map(|c| c.user_id).collect();
    let agent_ids: Vec<uuid::Uuid> = all_chats.iter().filter_map(|c| c.agent_id).collect();

    let user_map: HashMap<uuid::Uuid, String> = if !user_ids.is_empty() {
        users::Entity::find()
            .filter(users::Column::Id.is_in(user_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|u| {
                let name = u.name.or(u.username).unwrap_or_else(|| u.id.to_string());
                (u.id, name)
            })
            .collect()
    } else {
        HashMap::new()
    };

    let agent_map: HashMap<uuid::Uuid, String> = if !agent_ids.is_empty() {
        agents::Entity::find()
            .filter(agents::Column::Id.is_in(agent_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|a| {
                let name = a.name.unwrap_or_else(|| a.slug.clone());
                (a.id, name)
            })
            .collect()
    } else {
        HashMap::new()
    };

    let result: Vec<AdminChat> = all_chats
        .into_iter()
        .take(limit as usize)
        .map(|c| AdminChat {
            id: c.id.to_string(),
            user_name: c.user_id.and_then(|uid| user_map.get(&uid).cloned()),
            user_id: c.user_id.map(|u| u.to_string()),
            agent_name: c.agent_id.and_then(|aid| agent_map.get(&aid).cloned()),
            agent_id: c.agent_id.map(|a| a.to_string()),
            model: c.model,
            title: c.title,
            message_count: c.last_message_id.unwrap_or(0),
            incognito: c.incognito.unwrap_or(false),
            created_at: c
                .created_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            updated_at: c
                .updated_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        })
        .collect();

    Ok(Json(result))
}

// ── Quota Management ──────────────────────────────────────────

/// GET /api/admin/users/:user_id/quota
///
/// Returns quota info for a specific user.
pub async fn get_user_quota(
    Extension(state): Extension<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserQuotaResponse>, AppError> {
    let now = Utc::now().naive_utc();
    let period = now.format("%Y-%m").to_string();

    let effective_limit =
        quota::get_effective_limit(&state.db, &state.cache, user_id).await?;
    let current_usage = quota::get_month_usage(&state.db, &state.cache, user_id).await?;

    let quota_record = user_quotas::Entity::find()
        .filter(user_quotas::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?;

    let monthly_limit = quota_record.and_then(|q| q.monthly_limit);

    let is_unlimited = effective_limit == i64::MAX;

    Ok(Json(UserQuotaResponse {
        user_id: user_id.to_string(),
        monthly_limit,
        effective_limit: if is_unlimited {
            None
        } else {
            Some(effective_limit)
        },
        current_usage,
        remaining: if is_unlimited {
            None
        } else {
            Some((effective_limit - current_usage).max(0))
        },
        period,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SetUserQuotaRequest {
    pub monthly_limit: Option<i64>,
}

/// PATCH /api/admin/users/:user_id/quota
///
/// Sets a custom monthly token limit for a user.
/// Pass `monthly_limit: null` to reset to default.
pub async fn set_user_quota(
    Extension(state): Extension<Arc<AppState>>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<SetUserQuotaRequest>,
) -> Result<Json<UserQuotaResponse>, AppError> {
    // Ensure user exists
    let _ = users::Entity::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User {} not found", user_id)))?;

    // Upsert quota record
    let existing = user_quotas::Entity::find()
        .filter(user_quotas::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?;

    if let Some(existing) = existing {
        let mut active: user_quotas::ActiveModel = existing.into();
        active.monthly_limit = Set(req.monthly_limit);
        active.updated_at = Set(Some(Utc::now().naive_utc()));
        active.update(&state.db).await?;
    } else {
        let new_quota = user_quotas::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            monthly_limit: Set(req.monthly_limit),
            created_at: Set(Some(Utc::now().naive_utc())),
            updated_at: Set(Some(Utc::now().naive_utc())),
        };
        new_quota.insert(&state.db).await?;
    }

    // Invalidate caches
    let config_key = format!("quota:config:{}", user_id);
    let _ = state.cache.del(&config_key).await;

    // Return updated quota info
    get_user_quota(Extension(state), Path(user_id)).await
}
