// src/response/resources.rs
//
// Unified resource response types for /:owner/:slug pattern

use super::common::OwnerInfo;
use crate::entities::{agents, prompts, workflows};
use serde::Serialize;

/// Prompt with owner (for resource endpoint)
#[derive(Debug, Serialize)]
pub struct ResourcePrompt {
    #[serde(flatten)]
    pub prompt: prompts::Model,
    pub owner: OwnerInfo,
    pub url: String,
}

/// Agent with owner and system prompt (for resource endpoint)
#[derive(Debug, Serialize)]
pub struct ResourceAgent {
    #[serde(flatten)]
    pub agent: agents::Model,
    pub owner: OwnerInfo,
    pub url: String,
    pub system_prompt: Option<prompts::Model>,
}

/// Workflow with owner (for resource endpoint)
#[derive(Debug, Serialize)]
pub struct ResourceWorkflow {
    #[serde(flatten)]
    pub workflow: workflows::Model,
    pub owner: OwnerInfo,
    pub url: String,
}

/// Unified resource response with type discriminator
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ResourceResponse {
    #[serde(rename = "prompt")]
    Prompt(ResourcePrompt),
    #[serde(rename = "agent")]
    Agent(ResourceAgent),
    #[serde(rename = "workflow")]
    Workflow(ResourceWorkflow),
}
