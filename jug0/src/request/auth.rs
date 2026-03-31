// src/request/auth.rs
//
// Authentication request types

use serde::Deserialize;

/// Login request
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Register request
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: Option<String>,
    /// Organization ID (required at registration)
    pub org_id: String,
}
