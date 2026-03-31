use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub base_url: String, // e.g., "http://juglans-api:3000/api/mcp"
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,

    // 内部字段，用于执行时定位
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
                .timeout(Duration::from_secs(60)) // 给工具执行留够时间
                .build()
                .unwrap(),
        }
    }

    /// 1. 工具发现 (Discovery)
    /// 这里的实现假设 MCP Server 提供了一个 HTTP 接口来列出工具。
    /// 在标准的 MCP 中，这应该通过 SSE 握手 -> JSON-RPC 'tools/list' 完成。
    /// 为了适配你的 Node.js Express 架构，我们假设 Node 端暴露了兼容的 REST 端点或 JSON-RPC Over HTTP。
    /// 假设路径：POST {base_url}/messages with payload {"method": "tools/list"}
    pub async fn fetch_tools(&self, config: &McpServerConfig) -> Result<Vec<McpTool>> {
        let url = format!("{}/messages", config.base_url.trim_end_matches('/'));

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": 1
        });

        let mut req = self.http.post(&url).json(&payload);
        if let Some(token) = &config.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let res = req.send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("MCP Server Error ({}): {}", status, text));
        }

        let body: Value = res.json().await?;

        // 解析标准 MCP tools/list 响应: { "result": { "tools": [...] } }
        let tools_array = body
            .pointer("/result/tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid MCP response format: missing result.tools"))?;

        let mut mcp_tools = Vec::new();
        for t in tools_array {
            // 解析 schema，部分实现可能放在 inputSchema，部分可能在 input_schema
            let schema = t
                .get("inputSchema")
                .or(t.get("input_schema"))
                .cloned()
                .unwrap_or(json!({}));

            mcp_tools.push(McpTool {
                name: t["name"].as_str().unwrap_or("unknown").to_string(),
                description: t["description"].as_str().unwrap_or("").to_string(),
                input_schema: schema,
                server_url: url.clone(),
                token: config.token.clone(),
            });
        }

        Ok(mcp_tools)
    }

    /// 2. 工具执行 (Execution)
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

        let res = req.send().await?;
        let body: Value = res.json().await?;

        // 解析 MCP tools/call 响应: { "result": { "content": [{ "type": "text", "text": "..." }] } }
        if let Some(err) = body.get("error") {
            return Err(anyhow::anyhow!("MCP Execution Error: {:?}", err));
        }

        // 提取文本内容
        if let Some(content) = body.pointer("/result/content").and_then(|v| v.as_array()) {
            let mut result_buffer = String::new();
            for part in content {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    result_buffer.push_str(text);
                    result_buffer.push('\n');
                }
            }
            return Ok(result_buffer.trim().to_string());
        }

        Ok("Success (No content returned)".to_string())
    }
}
