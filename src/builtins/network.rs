// src/builtins/network.rs
use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct FetchUrl;

#[async_trait]
impl Tool for FetchUrl {
    fn name(&self) -> &str {
        "fetch_url"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let url = params
            .get("url")
            .ok_or_else(|| anyhow!("Missing param: url"))?;
        let method = params.get("method").map(|s| s.as_str()).unwrap_or("GET");

        let client = reqwest::Client::new();
        let mut builder = match method.to_uppercase().as_str() {
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => client.get(url),
        };

        // Add custom headers if provided
        if let Some(headers_json) = params.get("headers") {
            let headers_str = headers_json.trim_matches('"');
            if let Ok(headers_map) = serde_json::from_str::<HashMap<String, String>>(headers_str) {
                for (key, value) in headers_map {
                    builder = builder.header(&key, &value);
                }
            }
        }

        // Add request body if provided
        if let Some(body) = params.get("body") {
            let body_str = body.trim_matches('"');
            builder = builder.body(body_str.to_string());
        }

        let res = builder.send().await?;

        let status = res.status().as_u16();
        let content = res.text().await?;

        // Try to parse as JSON, otherwise return as string
        let content_value: Value = serde_json::from_str(&content).unwrap_or(json!(content));

        Ok(Some(json!({
            "status": status,
            "method": method,
            "url": url,
            "content": content_value,
            "ok": status >= 200 && status < 300
        })))
    }
}

/// Simple fetch function: fetch(url="...", method="GET", body={...}, headers={...})
pub struct Fetch;

#[async_trait]
impl Tool for Fetch {
    fn name(&self) -> &str {
        "fetch"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let url = params
            .get("url")
            .ok_or_else(|| anyhow!("fetch() requires 'url' parameter"))?
            .trim_matches('"');

        let method = params.get("method").map(|s| s.trim_matches('"')).unwrap_or("GET");

        let client = reqwest::Client::new();
        let mut builder = match method.to_uppercase().as_str() {
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => client.get(url),
        };

        // Add custom headers if provided
        if let Some(headers_json) = params.get("headers") {
            if let Ok(headers_map) = serde_json::from_str::<HashMap<String, String>>(headers_json) {
                for (key, value) in headers_map {
                    builder = builder.header(&key, &value);
                }
            }
        }

        // Add request body if provided (for POST/PUT/PATCH)
        if let Some(body) = params.get("body") {
            builder = builder.header("Content-Type", "application/json");
            builder = builder.body(body.clone());
        }

        let res = builder.send().await?;
        let status = res.status().as_u16();
        let content = res.text().await?;

        // Try to parse as JSON
        let data: Value = serde_json::from_str(&content).unwrap_or(json!(content));

        Ok(Some(json!({
            "status": status,
            "ok": status >= 200 && status < 300,
            "data": data
        })))
    }
}
