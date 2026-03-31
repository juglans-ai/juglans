// src/request/organizations.rs
//
// Organization request types

use serde::Deserialize;

fn default_algorithm() -> String {
    "RS256".to_string()
}

/// Set organization public key request
#[derive(Debug, Deserialize)]
pub struct SetPublicKeyRequest {
    pub public_key: String,
    #[serde(default = "default_algorithm")]
    pub key_algorithm: String,
}
