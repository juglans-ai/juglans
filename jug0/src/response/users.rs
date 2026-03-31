// src/response/users.rs
//
// User sync response types (internal API)

use serde::Serialize;

/// Single user sync response
#[derive(Debug, Serialize)]
pub struct SyncUserResponse {
    pub success: bool,
    pub jug0_user_id: String,
    pub message: String,
}

/// Batch user sync response
#[derive(Debug, Serialize)]
pub struct BatchSyncResponse {
    pub success: bool,
    pub synced: usize,
    pub errors: Vec<String>,
}
