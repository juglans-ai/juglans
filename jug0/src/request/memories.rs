// src/request/memories.rs
//
// Memory request types

use serde::Deserialize;

/// List memories query params
#[derive(Debug, Deserialize)]
pub struct ListMemoryQuery {
    pub agent_id: Option<String>,
    pub limit: Option<u32>,
}

/// Search memories request
#[derive(Debug, Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
    pub agent_id: Option<String>,
    pub limit: Option<u64>,
}
