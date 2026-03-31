// src/response/auth.rs
//
// Authentication response types

use serde::Serialize;
use uuid::Uuid;

/// User DTO for auth responses
#[derive(Debug, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub email: Option<String>,
    pub name: Option<String>,
    pub role: String,
}

/// Login/Register response
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserDto,
}

/// Current user info response (GET /api/auth/me)
#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub role: Option<String>,
    pub org_id: Option<String>,
    pub org_name: Option<String>,
}
