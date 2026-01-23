// src/builtins/network.rs
use super::Tool;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use async_trait::async_trait;
use crate::core::context::WorkflowContext;

pub struct FetchUrl;

#[async_trait]
impl Tool for FetchUrl {
    fn name(&self) -> &str { "fetch_url" }

    async fn execute(&self, params: &HashMap<String, String>, _context: &WorkflowContext) -> Result<Option<Value>> {
        let url = params.get("url").ok_or_else(|| anyhow!("Missing param: url"))?;
        let method = params.get("method").map(|s| s.as_str()).unwrap_or("GET");
        
        let client = reqwest::Client::new();
        let res = match method {
            "POST" => client.post(url).send().await?,
            _ => client.get(url).send().await?,
        };
        
        let status = res.status().as_u16();
        let content = res.text().await?;

        Ok(Some(json!({
            "status": status,
            "method": method,
            "url": url,
            "content": content
        })))
    }
}