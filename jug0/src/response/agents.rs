// src/response/agents.rs
//
// Agent-related response types

use super::common::OwnerInfo;
use crate::entities::{agents, prompts};
use serde::{Deserialize, Serialize};

/// Agent with owner information for list responses
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentWithOwner {
    #[serde(flatten)]
    pub agent: agents::Model,
    pub owner: Option<OwnerInfo>,
    pub url: Option<String>,
    pub system_prompt_content: Option<String>,
}

/// Agent detail response with full system prompt
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentDetailResponse {
    #[serde(flatten)]
    pub agent: agents::Model,
    pub system_prompt: Option<prompts::Model>,
}
