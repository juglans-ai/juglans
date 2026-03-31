// src/response/organizations.rs
//
// Organization response types

use serde::Serialize;

/// Response after setting public key
#[derive(Debug, Serialize)]
pub struct SetPublicKeyResponse {
    pub success: bool,
    pub org_id: String,
    pub key_algorithm: String,
    pub message: String,
}

/// Organization info response
#[derive(Debug, Serialize)]
pub struct OrgInfoResponse {
    pub id: String,
    pub name: String,
    pub has_public_key: bool,
    pub key_algorithm: Option<String>,
}
