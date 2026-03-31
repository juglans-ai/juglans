// src/response/models.rs
//
// Model list response types

use crate::services::models::{ModelInfo, ProviderStatus};
use serde::Serialize;

/// Models list response
#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
    pub providers: Vec<ProviderStatus>,
}
