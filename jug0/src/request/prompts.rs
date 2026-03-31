// src/request/prompts.rs
//
// Prompt-related request types

use serde::Deserialize;
use std::collections::HashMap;

/// Create prompt request
#[derive(Debug, Deserialize)]
pub struct CreatePromptRequest {
    pub slug: String,
    pub name: Option<String>,
    pub content: String,
    pub tags: Option<Vec<String>>,
    pub is_public: Option<bool>,
    pub is_system: Option<bool>,
}

/// Update prompt request
#[derive(Debug, Deserialize)]
pub struct UpdatePromptRequest {
    pub name: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub is_public: Option<bool>,
    pub is_system: Option<bool>,
}

/// Prompt filter query params
#[derive(Debug, Default, Deserialize)]
pub struct PromptFilter {
    pub search: Option<String>,
    pub public_only: Option<bool>,
}

/// Render prompt request
#[derive(Debug, Deserialize)]
pub struct RenderPromptRequest {
    pub variables: Option<HashMap<String, String>>,
}
