// src/services/quota.rs
//
// Token quota enforcement service

use chrono::{Datelike, NaiveDateTime, NaiveTime, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::entities::{chats, messages, user_quotas};
use crate::errors::AppError;
use crate::services::cache::CacheService;

/// Default monthly token limit for users without an explicit limit set.
const DEFAULT_MONTHLY_LIMIT: i64 = 100_000;

/// Get current month's total token usage for a user.
/// Cached in Redis with 60s TTL for performance.
pub async fn get_month_usage(
    db: &DatabaseConnection,
    cache: &CacheService,
    user_id: Uuid,
) -> Result<i64, AppError> {
    let now = Utc::now().naive_utc();
    let cache_key = format!("quota:usage:{}:{}", user_id, now.format("%Y-%m"));

    // Check cache first
    if let Some(cached) = cache.get::<i64>(&cache_key).await {
        return Ok(cached);
    }

    // Query from DB
    let start_of_month = NaiveDateTime::new(
        now.date().with_day(1).unwrap_or(now.date()),
        NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    );

    let user_chats = chats::Entity::find()
        .filter(chats::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    let chat_ids: Vec<_> = user_chats.iter().map(|c| c.id).collect();

    if chat_ids.is_empty() {
        let _ = cache.set(&cache_key, &0i64, 60).await;
        return Ok(0);
    }

    let user_messages = messages::Entity::find()
        .filter(messages::Column::ChatId.is_in(chat_ids))
        .filter(messages::Column::Role.eq("assistant"))
        .filter(messages::Column::CreatedAt.gte(start_of_month))
        .all(db)
        .await?;

    let mut total: i64 = 0;
    for msg in user_messages {
        if let Some(metadata) = msg.metadata {
            if let Some(usage) = metadata.get("usage") {
                let input = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                total += input + output;
            }
        }
    }

    let _ = cache.set(&cache_key, &total, 60).await;
    Ok(total)
}

/// Get effective monthly token limit for a user.
/// Cached in Redis with 300s TTL.
///
/// Logic:
/// - No user_quotas record → DEFAULT_MONTHLY_LIMIT (100k)
/// - Record with monthly_limit = NULL → unlimited (i64::MAX)
/// - Record with monthly_limit = N → N
pub async fn get_effective_limit(
    db: &DatabaseConnection,
    cache: &CacheService,
    user_id: Uuid,
) -> Result<i64, AppError> {
    let config_key = format!("quota:config:{}", user_id);

    if let Some(cached) = cache.get::<i64>(&config_key).await {
        return Ok(cached);
    }

    let record = user_quotas::Entity::find()
        .filter(user_quotas::Column::UserId.eq(user_id))
        .one(db)
        .await?;

    let limit = match record {
        None => DEFAULT_MONTHLY_LIMIT,
        Some(r) => match r.monthly_limit {
            None => i64::MAX,
            Some(l) if l < 0 => i64::MAX, // negative = unlimited
            Some(l) => l,
        },
    };

    let _ = cache.set(&config_key, &limit, 300).await;
    Ok(limit)
}

/// Check if user has remaining quota.
/// Returns Ok(remaining) or Err(QuotaExceeded).
pub async fn check_quota(
    db: &DatabaseConnection,
    cache: &CacheService,
    user_id: Uuid,
) -> Result<i64, AppError> {
    let limit = get_effective_limit(db, cache, user_id).await?;

    // Unlimited
    if limit == i64::MAX {
        return Ok(i64::MAX);
    }

    let usage = get_month_usage(db, cache, user_id).await?;
    let remaining = limit - usage;

    if remaining <= 0 {
        return Err(AppError::QuotaExceeded(format!(
            "Monthly token quota exceeded (used: {} / limit: {}). Upgrade your plan for more tokens.",
            usage, limit
        )));
    }

    Ok(remaining)
}

/// Invalidate cached usage after tokens are consumed.
pub async fn invalidate_usage_cache(cache: &CacheService, user_id: Uuid) {
    let now = Utc::now().naive_utc();
    let cache_key = format!("quota:usage:{}:{}", user_id, now.format("%Y-%m"));
    let _ = cache.del(&cache_key).await;
}
