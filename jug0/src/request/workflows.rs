// src/request/workflows.rs
//
// Workflow-related request types

use serde::Deserialize;

/// Create workflow request
#[derive(Debug, Deserialize)]
pub struct CreateWorkflowRequest {
    pub slug: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub endpoint_url: Option<String>,
    pub definition: Option<serde_json::Value>,
    pub trigger_config: Option<serde_json::Value>,
    pub is_active: Option<bool>,
    pub is_public: Option<bool>,
}

/// Update workflow request
#[derive(Debug, Deserialize)]
pub struct UpdateWorkflowRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub endpoint_url: Option<String>,
    pub definition: Option<serde_json::Value>,
    pub trigger_config: Option<serde_json::Value>,
    pub is_active: Option<bool>,
    pub is_public: Option<bool>,
}

/// Execute workflow request
#[derive(Debug, Deserialize)]
pub struct ExecuteWorkflowRequest {
    pub input: Option<serde_json::Value>,
    pub variables: Option<serde_json::Value>,
}
