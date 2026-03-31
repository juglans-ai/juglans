// src/request/models.rs
//
// Models query types

use serde::Deserialize;

/// Models list query params
#[derive(Debug, Deserialize)]
pub struct ModelsQuery {
    pub provider: Option<String>,
    pub refresh: Option<bool>,
}
