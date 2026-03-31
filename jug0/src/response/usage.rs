// src/response/usage.rs
//
// Usage statistics response types

use serde::Serialize;

/// Usage statistics for current month
#[derive(Debug, Serialize)]
pub struct UsageStats {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_tokens: i64,
    pub by_model: Vec<ModelUsage>,
    pub period_start: String,
    pub period_end: String,
    /// Monthly token quota limit (null = unlimited)
    pub quota_limit: Option<i64>,
    /// Remaining tokens in quota (null = unlimited)
    pub quota_remaining: Option<i64>,
}

/// Per-model usage breakdown
#[derive(Debug, Serialize)]
pub struct ModelUsage {
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub message_count: i64,
}
