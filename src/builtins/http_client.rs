// src/builtins/http_client.rs
//
// httpx-style HTTP client builtin.
// Provides http_request() with full-featured HTTP client capabilities:
// query params, auth, timeout, form data, multipart uploads, cookies, redirects.

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::redirect;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct HttpRequest;

#[async_trait]
impl Tool for HttpRequest {
    fn name(&self) -> &str {
        "http_request"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "http_request",
                "description": "Make an HTTP request (httpx-style). Supports all HTTP methods, query params, JSON/form/multipart body, auth, timeout, cookies, and redirect control.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "Request URL (required)"
                        },
                        "method": {
                            "type": "string",
                            "description": "HTTP method: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS. Default: GET"
                        },
                        "params": {
                            "type": "object",
                            "description": "Query parameters as JSON object, appended to URL"
                        },
                        "headers": {
                            "type": "object",
                            "description": "Custom request headers as JSON object"
                        },
                        "json": {
                            "type": "object",
                            "description": "JSON body (auto-sets Content-Type: application/json)"
                        },
                        "data": {
                            "type": "object",
                            "description": "Form data as JSON object (URL-encoded, Content-Type: application/x-www-form-urlencoded)"
                        },
                        "files": {
                            "type": "object",
                            "description": "Multipart file upload: {field_name: file_path}"
                        },
                        "content": {
                            "type": "string",
                            "description": "Raw body content string"
                        },
                        "timeout": {
                            "type": "number",
                            "description": "Request timeout in seconds"
                        },
                        "auth": {
                            "type": "string",
                            "description": "Auth: 'Bearer <token>' for bearer auth, 'user:pass' for basic auth"
                        },
                        "follow_redirects": {
                            "type": "boolean",
                            "description": "Follow redirects. Default: true"
                        },
                        "cookies": {
                            "type": "object",
                            "description": "Cookies as JSON object {name: value}"
                        }
                    },
                    "required": ["url"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Helper: get param, treating "null" as absent (unset optional params from stdlib)
        let get = |key: &str| -> Option<&String> {
            params.get(key).filter(|v| {
                let t = v.trim_matches('"');
                !t.is_empty() && t != "null"
            })
        };

        // --- URL (required) ---
        let raw_url = params
            .get("url")
            .ok_or_else(|| anyhow!("http_request() requires 'url' parameter"))?
            .trim_matches('"');

        // --- Query params → append to URL ---
        let url = if let Some(params_json) = get("params") {
            append_query_params(raw_url, params_json)?
        } else {
            raw_url.to_string()
        };

        // --- Method ---
        let method = get("method")
            .map(|s| s.trim_matches('"').to_uppercase())
            .unwrap_or_else(|| "GET".to_string());

        // --- Follow redirects ---
        let follow = get("follow_redirects")
            .map(|s| {
                let v = s.trim_matches('"');
                v != "false" && v != "0"
            })
            .unwrap_or(true);

        // --- Build client with redirect policy + timeout ---
        let mut client_builder = reqwest::Client::builder();

        if follow {
            client_builder = client_builder.redirect(redirect::Policy::limited(10));
        } else {
            client_builder = client_builder.redirect(redirect::Policy::none());
        }

        if let Some(timeout_str) = get("timeout") {
            if let Ok(secs) = timeout_str.trim_matches('"').parse::<f64>() {
                client_builder = client_builder.timeout(Duration::from_secs_f64(secs));
            }
        }

        let client = client_builder.build()?;

        // --- Request builder ---
        let mut builder = match method.as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            "HEAD" => client.head(&url),
            "OPTIONS" => client.request(reqwest::Method::OPTIONS, &url),
            _ => client.get(&url),
        };

        // --- Auth ---
        if let Some(auth_str) = get("auth") {
            let auth = auth_str.trim_matches('"');
            if let Some(token) = auth
                .strip_prefix("Bearer ")
                .or(auth.strip_prefix("bearer "))
            {
                builder = builder.bearer_auth(token);
            } else if let Some((user, pass)) = auth.split_once(':') {
                builder = builder.basic_auth(user, Some(pass));
            } else {
                // Treat as bearer token directly
                builder = builder.bearer_auth(auth);
            }
        }

        // --- Headers ---
        if let Some(headers_json) = get("headers") {
            if let Ok(headers_map) = serde_json::from_str::<HashMap<String, String>>(headers_json) {
                for (key, value) in headers_map {
                    builder = builder.header(&key, &value);
                }
            }
        }

        // --- Cookies → Cookie header ---
        if let Some(cookies_json) = get("cookies") {
            if let Ok(cookies_map) = serde_json::from_str::<HashMap<String, String>>(cookies_json) {
                let cookie_str: String = cookies_map
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ");
                if !cookie_str.is_empty() {
                    builder = builder.header("Cookie", cookie_str);
                }
            }
        }

        // --- Body (priority: json > data > files > content) ---
        if let Some(json_str) = get("json") {
            builder = builder.header("Content-Type", "application/json");
            builder = builder.body(json_str.clone());
        } else if let Some(data_str) = get("data") {
            if let Ok(form_map) = serde_json::from_str::<HashMap<String, String>>(data_str) {
                builder = builder.form(&form_map);
            } else {
                // Fallback: send as URL-encoded body directly
                builder = builder.header("Content-Type", "application/x-www-form-urlencoded");
                builder = builder.body(data_str.clone());
            }
        } else if let Some(files_str) = get("files") {
            if let Ok(files_map) = serde_json::from_str::<HashMap<String, String>>(files_str) {
                let mut form = reqwest::multipart::Form::new();
                for (field_name, file_path) in &files_map {
                    let path = std::path::Path::new(file_path);
                    let file_bytes = tokio::fs::read(path)
                        .await
                        .map_err(|e| anyhow!("Failed to read file '{}': {}", file_path, e))?;
                    let filename = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "upload".to_string());
                    let part = reqwest::multipart::Part::bytes(file_bytes).file_name(filename);
                    form = form.part(field_name.clone(), part);
                }
                builder = builder.multipart(form);
            }
        } else if let Some(content) = get("content") {
            builder = builder.body(content.trim_matches('"').to_string());
        }

        // --- Send request with timing ---
        let start = Instant::now();
        let res = builder.send().await?;
        let elapsed = start.elapsed().as_secs_f64();

        // --- Parse response ---
        let status = res.status().as_u16();
        let final_url = res.url().to_string();

        // Collect response headers
        let resp_headers: Map<String, Value> = res
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), json!(v.to_str().unwrap_or(""))))
            .collect();

        let content_type = resp_headers
            .get("content-type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let text = res.text().await?;

        // Try to parse as JSON
        let json_value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

        Ok(Some(json!({
            "status_code": status,
            "headers": resp_headers,
            "json": json_value,
            "text": text,
            "url": final_url,
            "is_success": (200..300).contains(&status),
            "elapsed": elapsed,
            "content_type": content_type
        })))
    }
}

/// Append query parameters from a JSON object string to a URL.
fn append_query_params(base_url: &str, params_json: &str) -> Result<String> {
    let params: HashMap<String, Value> =
        serde_json::from_str(params_json).map_err(|e| anyhow!("Invalid params JSON: {}", e))?;

    if params.is_empty() {
        return Ok(base_url.to_string());
    }

    let mut url =
        reqwest::Url::parse(base_url).map_err(|e| anyhow!("Invalid URL '{}': {}", base_url, e))?;

    {
        let mut query_pairs = url.query_pairs_mut();
        for (key, value) in &params {
            let v = match value {
                Value::String(s) => s.clone(),
                Value::Null => continue,
                other => other.to_string(),
            };
            query_pairs.append_pair(key, &v);
        }
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_query_params_basic() {
        let url = append_query_params("https://example.com/api", r#"{"page": 1, "limit": "10"}"#)
            .unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        let pairs: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("page").unwrap(), "1");
        assert_eq!(pairs.get("limit").unwrap(), "10");
    }

    #[test]
    fn test_append_query_params_preserves_existing() {
        let url = append_query_params("https://example.com/api?existing=yes", r#"{"new": "val"}"#)
            .unwrap();
        let parsed = reqwest::Url::parse(&url).unwrap();
        let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        assert!(pairs.iter().any(|(k, v)| k == "existing" && v == "yes"));
        assert!(pairs.iter().any(|(k, v)| k == "new" && v == "val"));
    }

    #[test]
    fn test_append_query_params_skips_null() {
        let url = append_query_params(
            "https://example.com/api",
            r#"{"keep": "yes", "skip": null}"#,
        )
        .unwrap();
        assert!(url.contains("keep=yes"));
        assert!(!url.contains("skip"));
    }

    #[test]
    fn test_append_query_params_empty() {
        let url = append_query_params("https://example.com/api", r#"{}"#).unwrap();
        assert_eq!(url, "https://example.com/api");
    }

    #[test]
    fn test_append_query_params_invalid_json() {
        let result = append_query_params("https://example.com", "not json");
        assert!(result.is_err());
    }
}
