// src/response/common.rs
//
// Common response types shared across handlers

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Owner information for resources (agents, prompts, workflows)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerInfo {
    pub id: Uuid,
    pub username: Option<String>,
    pub name: Option<String>,
}

/// Public user profile (minimal info for public display)
#[derive(Debug, Clone, Serialize)]
pub struct PublicUserProfile {
    pub id: Uuid,
    pub username: String,
    pub name: Option<String>,
}

/// Simple success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuccessResponse {
    pub fn new() -> Self {
        Self {
            success: true,
            id: None,
            message: None,
        }
    }

    pub fn with_id(id: Uuid) -> Self {
        Self {
            success: true,
            id: Some(id),
            message: None,
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            success: true,
            id: None,
            message: Some(message.into()),
        }
    }
}

impl Default for SuccessResponse {
    fn default() -> Self {
        Self::new()
    }
}
