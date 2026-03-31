// src/request/users.rs
//
// User sync request types (internal API)

use serde::Deserialize;

/// Single user sync request
#[derive(Debug, Deserialize)]
pub struct SyncUserRequest {
    /// User ID from juglans-api (e.g., "user_xxx" or cuid)
    pub id: String,
    /// Username (globally unique, GitHub-style)
    pub username: String,
    /// Display name
    pub name: Option<String>,
    /// Organization ID (defaults to OFFICIAL_ORG_SLUG env var)
    pub org_id: Option<String>,
    /// Monthly token limit. None/absent = no change, Some(-1) = unlimited, Some(N) = N tokens.
    #[serde(default)]
    pub monthly_limit: Option<i64>,
}

/// Batch user sync request
#[derive(Debug, Deserialize)]
pub struct BatchSyncRequest {
    pub users: Vec<SyncUserRequest>,
}
