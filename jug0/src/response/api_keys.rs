// src/response/api_keys.rs
//
// API key response types

use serde::Serialize;
use uuid::Uuid;

/// Response when creating a new API key (includes raw key, only shown once)
#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: Uuid,
    pub name: String,
    /// The raw API key - only shown once at creation time
    pub key: String,
    pub expires_at: Option<chrono::NaiveDateTime>,
}
