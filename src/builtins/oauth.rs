// src/builtins/oauth.rs
//
// OAuth2 token exchange builtin.
// Supports client_credentials, password, refresh_token, authorization_code grants.
// Built-in providers: github, google (auto-configure token_url + headers).

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Built-in OAuth provider presets.
struct ProviderPreset {
    token_url: &'static str,
    default_grant_type: &'static str,
    extra_headers: Vec<(&'static str, &'static str)>,
}

fn get_provider(name: &str) -> Option<ProviderPreset> {
    match name {
        "github" => Some(ProviderPreset {
            token_url: "https://github.com/login/oauth/access_token",
            default_grant_type: "authorization_code",
            // GitHub returns form-encoded by default; we need JSON
            extra_headers: vec![("Accept", "application/json")],
        }),
        "google" => Some(ProviderPreset {
            token_url: "https://oauth2.googleapis.com/token",
            default_grant_type: "authorization_code",
            extra_headers: vec![],
        }),
        _ => None,
    }
}

pub struct OAuthToken;

#[async_trait]
impl Tool for OAuthToken {
    fn name(&self) -> &str {
        "oauth_token"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "oauth_token",
                "description": "Exchange OAuth2 tokens. Supports client_credentials, password, refresh_token, authorization_code grants. Built-in providers: github, google.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "grant_type": { "type": "string", "description": "OAuth2 grant type: client_credentials, password, refresh_token, authorization_code" },
                        "token_url": { "type": "string", "description": "Token endpoint URL (auto-set by provider)" },
                        "provider": { "type": "string", "description": "Built-in provider: github, google. Auto-sets token_url and headers." },
                        "client_id": { "type": "string" },
                        "client_secret": { "type": "string" },
                        "scope": { "type": "string" },
                        "username": { "type": "string", "description": "For password grant" },
                        "password": { "type": "string", "description": "For password grant" },
                        "refresh_token": { "type": "string", "description": "For refresh_token grant" },
                        "code": { "type": "string", "description": "For authorization_code grant" },
                        "redirect_uri": { "type": "string", "description": "For authorization_code grant" },
                        "extra_params": { "type": "object", "description": "Additional form params (JSON object)" }
                    },
                    "required": []
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let get = |key: &str| -> Option<&String> {
            params.get(key).filter(|v| {
                let t = v.trim_matches('"');
                !t.is_empty() && t != "null"
            })
        };

        // Resolve provider preset
        let preset = get("provider").and_then(|p| get_provider(p.trim_matches('"')));

        // Resolve token_url (explicit > provider default)
        let token_url = get("token_url")
            .map(|s| s.trim_matches('"').to_string())
            .or_else(|| preset.as_ref().map(|p| p.token_url.to_string()))
            .ok_or_else(|| anyhow!("oauth_token() requires 'token_url' or 'provider' parameter"))?;

        // Resolve grant_type (explicit > provider default)
        let grant_type = get("grant_type")
            .map(|s| s.trim_matches('"').to_string())
            .or_else(|| preset.as_ref().map(|p| p.default_grant_type.to_string()))
            .ok_or_else(|| {
                anyhow!("oauth_token() requires 'grant_type' or 'provider' parameter")
            })?;

        // Build form body
        let mut form: HashMap<String, String> = HashMap::new();
        form.insert("grant_type".to_string(), grant_type.clone());

        // Add params based on grant_type
        let optional_fields: &[&str] = match grant_type.as_str() {
            "client_credentials" => &["client_id", "client_secret", "scope"],
            "password" => &[
                "client_id",
                "client_secret",
                "username",
                "password",
                "scope",
            ],
            "refresh_token" => &["client_id", "client_secret", "refresh_token", "scope"],
            "authorization_code" => &[
                "client_id",
                "client_secret",
                "code",
                "redirect_uri",
                "scope",
            ],
            _ => &[
                "client_id",
                "client_secret",
                "scope",
                "username",
                "password",
                "refresh_token",
                "code",
                "redirect_uri",
            ],
        };

        for &field in optional_fields {
            if let Some(val) = get(field) {
                form.insert(field.to_string(), val.trim_matches('"').to_string());
            }
        }

        // Merge extra_params if provided
        if let Some(extra_str) = get("extra_params") {
            if let Ok(extra_map) = serde_json::from_str::<HashMap<String, String>>(extra_str) {
                form.extend(extra_map);
            }
        }

        // Build request
        let client = reqwest::Client::new();
        let mut builder = client.post(&token_url).form(&form);

        // Add provider-specific headers
        if let Some(ref preset) = preset {
            for &(key, value) in &preset.extra_headers {
                builder = builder.header(key, value);
            }
        }

        // Send
        let res = builder.send().await?;
        let status = res.status().as_u16();
        let body_text = res.text().await?;

        if !(200..300).contains(&status) {
            return Err(anyhow!(
                "OAuth token request failed ({}): {}",
                status,
                body_text
            ));
        }

        // Parse response JSON
        let raw: Value = serde_json::from_str(&body_text).map_err(|e| {
            anyhow!(
                "Failed to parse OAuth response as JSON: {} — body: {}",
                e,
                body_text
            )
        })?;

        // Extract standard fields, keep raw for vendor-specific ones
        Ok(Some(json!({
            "access_token": raw.get("access_token").unwrap_or(&Value::Null),
            "token_type": raw.get("token_type").unwrap_or(&json!("Bearer")),
            "expires_in": raw.get("expires_in").unwrap_or(&Value::Null),
            "refresh_token": raw.get("refresh_token").unwrap_or(&Value::Null),
            "scope": raw.get("scope").unwrap_or(&Value::Null),
            "raw": raw
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_provider() {
        let preset = get_provider("github").unwrap();
        assert_eq!(
            preset.token_url,
            "https://github.com/login/oauth/access_token"
        );
        assert_eq!(preset.default_grant_type, "authorization_code");
        assert!(preset
            .extra_headers
            .iter()
            .any(|&(k, v)| k == "Accept" && v == "application/json"));
    }

    #[test]
    fn test_google_provider() {
        let preset = get_provider("google").unwrap();
        assert_eq!(preset.token_url, "https://oauth2.googleapis.com/token");
        assert_eq!(preset.default_grant_type, "authorization_code");
    }

    #[test]
    fn test_unknown_provider() {
        assert!(get_provider("unknown").is_none());
    }
}
