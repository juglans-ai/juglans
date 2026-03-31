// src/response/prompts.rs
//
// Prompt-related response types

use super::common::OwnerInfo;
use crate::entities::prompts;
use serde::{Deserialize, Serialize};

/// Prompt with owner information for list responses
#[derive(Debug, Serialize, Deserialize)]
pub struct PromptWithOwner {
    #[serde(flatten)]
    pub prompt: prompts::Model,
    pub owner: Option<OwnerInfo>,
    pub url: String,
}

/// Rendered prompt response
#[derive(Debug, Serialize)]
pub struct RenderPromptResponse {
    pub rendered: String,
    pub original: String,
    pub variables_used: Vec<String>,
}
