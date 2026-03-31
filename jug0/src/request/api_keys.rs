// src/request/api_keys.rs
//
// API key request types

use serde::Deserialize;

/// Create API key request
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    /// Key validity in days (optional, None = no expiry)
    pub days_valid: Option<i64>,
}
