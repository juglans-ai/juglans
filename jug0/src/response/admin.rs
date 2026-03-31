// src/response/admin.rs
//
// Admin usage statistics response types

use serde::Serialize;

use super::usage::ModelUsage;

/// Global usage statistics (all users, current month)
#[derive(Debug, Serialize)]
pub struct GlobalUsageStats {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_tokens: i64,
    pub by_model: Vec<ModelUsage>,
    pub by_user: Vec<UserUsage>,
    pub period_start: String,
    pub period_end: String,
}

/// Per-user usage breakdown
#[derive(Debug, Serialize)]
pub struct UserUsage {
    pub user_id: String,
    pub name: Option<String>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_tokens: i64,
    pub message_count: i64,
}

/// User quota info for admin
#[derive(Debug, Serialize)]
pub struct UserQuotaResponse {
    pub user_id: String,
    pub monthly_limit: Option<i64>,
    pub effective_limit: Option<i64>,
    pub current_usage: i64,
    pub remaining: Option<i64>,
    pub period: String,
}

/// Chat metadata for admin listing
#[derive(Debug, Serialize)]
pub struct AdminChat {
    pub id: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub message_count: i32,
    pub incognito: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}
