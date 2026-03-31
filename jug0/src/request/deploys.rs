use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct CreateDeployRequest {
    pub slug: String,
    /// GitHub repo (owner/repo)
    pub repo: String,
    pub branch: Option<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDeployRequest {
    pub branch: Option<String>,
    pub env: Option<HashMap<String, String>>,
}
