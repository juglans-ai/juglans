// src/services/mcp.rs
use crate::services::config::McpServerConfig;
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,

    #[serde(skip)]
    pub server_url: String,
    #[serde(skip)]
    pub token: Option<String>,
}

#[derive(Clone)]
pub struct McpClient {
    http: Client,
}

impl McpClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }

    pub async fn fetch_tools(&self, config: &McpServerConfig) -> Result<Vec<McpTool>> {
        let post_url = if config.base_url.ends_with("/sse") {
            config.base_url.replace("/sse", "/messages")
        } else {
            format!("{}/messages", config.base_url.trim_end_matches('/'))
        };

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": "list_1"
        });

        let mut req = self.http.post(&post_url).json(&payload);
        if let Some(token) = &config.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let res = req
            .send()
            .await
            .map_err(|e| anyhow!("Connection failed: {}", e))?;
        let status = res.status();

        if !status.is_success() {
            let err_body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "MCP Server ({} /tools/list) returned {}: {}",
                config.name,
                status,
                err_body
            ));
        }

        let body: Value = res.json().await?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!("MCP Server Error: {:?}", err));
        }

        let tools_array = body
            .pointer("/result/tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                anyhow!(
                    "Invalid MCP response from {}: missing result.tools",
                    config.name
                )
            })?;

        let mut mcp_tools = Vec::new();
        for t in tools_array {
            let schema = t
                .get("inputSchema")
                .or(t.get("input_schema"))
                .cloned()
                .unwrap_or(json!({}));
            mcp_tools.push(McpTool {
                name: t["name"].as_str().unwrap_or("unknown").to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                input_schema: schema,
                server_url: post_url.clone(),
                token: config.token.clone(),
            });
        }
        Ok(mcp_tools)
    }

    pub async fn execute_tool(&self, tool: &McpTool, args: Value) -> Result<String> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": tool.name,
                "arguments": args
            },
            "id": uuid::Uuid::new_v4().to_string()
        });

        let mut req = self.http.post(&tool.server_url).json(&payload);
        if let Some(token) = &tool.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let res = req
            .send()
            .await
            .map_err(|e| anyhow!("Request failed: {}", e))?;
        let status = res.status();

        // Handle potential non-JSON errors before parsing
        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow!("MCP Server returned error {}: {}", status, text));
        }

        let body: Value = res
            .json()
            .await
            .map_err(|_| anyhow!("MCP Server returned non-JSON response (Status {})", status))?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!("MCP Execution Error: {:?}", err));
        }

        if let Some(result_node) = body.get("result") {
            if let Some(content) = result_node.get("content").and_then(|v| v.as_array()) {
                let mut result_buffer = String::new();
                for part in content {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        result_buffer.push_str(text);
                    }
                }
                return Ok(result_buffer);
            }
            return Ok(serde_json::to_string(result_node)?);
        }

        Ok("Success (No content returned)".to_string())
    }
}
