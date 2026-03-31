// src/handlers/usage.rs
//
// Usage statistics handler - aggregates token usage from message metadata

use axum::{extract::Extension, Json};
use chrono::{Datelike, NaiveDateTime, NaiveTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::AuthUser;
use crate::entities::{chats, messages};
use crate::errors::AppError;
use crate::response::{ModelUsage, UsageStats};
use crate::services::quota;
use crate::AppState;

/// GET /api/usage/stats
///
/// Returns token usage statistics for the authenticated user for the current month.
/// Aggregates usage data stored in message metadata.
pub async fn get_usage_stats(
    Extension(state): Extension<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<Json<UsageStats>, AppError> {
    let user_id = auth_user.id;

    // Get start of current month
    let now = Utc::now().naive_utc();
    let start_of_month = NaiveDateTime::new(
        now.date().with_day(1).unwrap_or(now.date()),
        NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    );

    // First, get all chat IDs belonging to this user
    let user_chats = chats::Entity::find()
        .filter(chats::Column::UserId.eq(user_id))
        .all(&state.db)
        .await?;

    let chat_ids: Vec<_> = user_chats.iter().map(|c| c.id).collect();

    if chat_ids.is_empty() {
        let limit = quota::get_effective_limit(&state.db, &state.cache, user_id).await?;
        let is_unlimited = limit == i64::MAX;
        return Ok(Json(UsageStats {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            by_model: vec![],
            period_start: start_of_month.format("%Y-%m-%d").to_string(),
            period_end: now.format("%Y-%m-%d").to_string(),
            quota_limit: if is_unlimited { None } else { Some(limit) },
            quota_remaining: if is_unlimited { None } else { Some(limit) },
        }));
    }

    // Query assistant messages from user's chats this month
    let user_messages = messages::Entity::find()
        .filter(messages::Column::ChatId.is_in(chat_ids))
        .filter(messages::Column::Role.eq("assistant"))
        .filter(messages::Column::CreatedAt.gte(start_of_month))
        .all(&state.db)
        .await?;

    // Aggregate usage statistics
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut model_map: HashMap<String, (i64, i64, i64)> = HashMap::new();

    for msg in user_messages {
        if let Some(metadata) = msg.metadata {
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
                entry.2 += 1; // message count
            }
        }
    }

    // Convert to response format
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

    // Sort by total tokens descending
    by_model.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    let total_tokens = total_input + total_output;
    let limit = quota::get_effective_limit(&state.db, &state.cache, user_id).await?;
    let is_unlimited = limit == i64::MAX;

    Ok(Json(UsageStats {
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_tokens,
        by_model,
        period_start: start_of_month.format("%Y-%m-%d").to_string(),
        period_end: now.format("%Y-%m-%d").to_string(),
        quota_limit: if is_unlimited { None } else { Some(limit) },
        quota_remaining: if is_unlimited {
            None
        } else {
            Some((limit - total_tokens).max(0))
        },
    }))
}
