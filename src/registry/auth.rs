// src/registry/auth.rs
//
// Bearer token authentication for the registry server.

use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

/// Authentication configuration.
#[derive(Clone)]
pub struct AuthConfig {
    /// Valid bearer tokens. If empty, auth is disabled (development mode).
    pub tokens: Vec<String>,
}

impl AuthConfig {
    /// Create from REGISTRY_TOKEN environment variable.
    /// Multiple tokens can be comma-separated.
    pub fn from_env() -> Self {
        let tokens = std::env::var("REGISTRY_TOKEN")
            .map(|val| {
                val.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Self { tokens }
    }

    pub fn is_valid(&self, token: &str) -> bool {
        if self.tokens.is_empty() {
            return true; // Auth disabled
        }
        self.tokens.iter().any(|t| t == token)
    }
}

/// Axum middleware that validates Bearer token for write operations.
pub async fn require_auth(req: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    let auth_config = req
        .extensions()
        .get::<Arc<AuthConfig>>()
        .cloned()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    if auth_config.tokens.is_empty() {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");

    if auth_config.is_valid(token) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tokens_allows_all() {
        let auth = AuthConfig {
            tokens: vec![],
        };
        assert!(auth.is_valid("anything"));
        assert!(auth.is_valid(""));
    }

    #[test]
    fn test_valid_token() {
        let auth = AuthConfig {
            tokens: vec!["secret123".to_string()],
        };
        assert!(auth.is_valid("secret123"));
        assert!(!auth.is_valid("wrong"));
        assert!(!auth.is_valid(""));
    }

    #[test]
    fn test_multiple_tokens() {
        let auth = AuthConfig {
            tokens: vec!["token1".to_string(), "token2".to_string()],
        };
        assert!(auth.is_valid("token1"));
        assert!(auth.is_valid("token2"));
        assert!(!auth.is_valid("token3"));
    }
}
