// src/services/jug0.rs
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::core::agent_parser::AgentResource;
use crate::core::graph::WorkflowGraph;
use crate::core::prompt_parser::PromptResource;
use crate::services::config::JuglansConfig;
use crate::services::interface::JuglansRuntime;

/// 定义对话输出类型，区分最终文本和工具调用请求
#[derive(Debug)]
pub enum ChatOutput {
    /// 最终回复文本
    Final { text: String, chat_id: String },
    /// AI 发起的工具调用请求
    ToolCalls { calls: Vec<Value>, chat_id: String },
}

#[derive(Deserialize)]
struct AgentResponse {
    id: Uuid,
    slug: String,
}

#[derive(Deserialize)]
struct PromptResponse {
    id: Uuid,
}

#[derive(Deserialize)]
struct WorkflowResponse {
    id: Uuid,
}

/// Resource info for listing
#[derive(Debug)]
pub struct ResourceInfo {
    pub slug: String,
    pub resource_type: String,
}

#[derive(Clone)]
pub struct Jug0Client {
    http: Client,
    base_url: String,
    api_key: String,
}

impl Jug0Client {
    pub fn new(config: &JuglansConfig) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client for Jug0 communication.");

        let base_url_str = config.jug0.base_url.trim_end_matches('/').to_string();

        let api_key_str = config
            .account
            .api_key
            .clone()
            .or_else(|| std::env::var("JUGLANS_API_KEY").ok())
            .unwrap_or_default();

        Self {
            http: http_client,
            base_url: base_url_str,
            api_key: api_key_str,
        }
    }

    /// Check if slug is in owner/slug format (GitHub-style)
    fn is_owner_slug_format(slug: &str) -> bool {
        slug.contains('/') && !slug.starts_with('/') && !slug.ends_with('/')
    }

    /// Build URL for resource lookup, supporting both formats:
    /// - Simple slug: `/api/prompts/:slug` (legacy)
    /// - Owner/slug: `/api/r/:owner/:slug` (GitHub-style)
    fn build_resource_url(&self, slug: &str, resource_type: &str) -> String {
        if Self::is_owner_slug_format(slug) {
            // GitHub-style: /api/r/:owner/:slug
            format!("{}/api/r/{}", self.base_url, slug)
        } else {
            // Legacy: /api/{resource_type}/:slug
            format!("{}/api/{}/{}", self.base_url, resource_type, slug)
        }
    }

    pub async fn get_prompt_id(&self, slug: &str) -> Result<Uuid> {
        let url = self.build_resource_url(slug, "prompts");
        let res = self
            .http
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!(
                "Remote Resource Not Found: Prompt '{}' is missing.",
                slug
            ));
        }
        let body: PromptResponse = res.json().await?;
        Ok(body.id)
    }

    pub async fn apply_prompt(&self, prompt: &PromptResource) -> Result<String> {
        let url_get = format!("{}/api/prompts/{}", self.base_url, prompt.slug);
        let res_get = self
            .http
            .get(&url_get)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        let payload = json!({
            "name": prompt.name,
            "content": prompt.content,
            "type": prompt.r#type,
            "is_system": prompt.r#type == "system",
            "input_variables": prompt.inputs,
            "tags": json!([prompt.r#type])
        });

        if res_get.status().is_success() {
            let existing: PromptResponse = res_get.json().await?;
            self.http
                .patch(&format!("{}/api/prompts/{}", self.base_url, existing.id))
                .header("X-API-KEY", &self.api_key)
                .json(&payload)
                .send()
                .await?;
            Ok(format!("Successfully synchronized prompt: {}", prompt.slug))
        } else {
            let mut create_payload = payload.clone();
            create_payload["slug"] = json!(prompt.slug);
            self.http
                .post(&format!("{}/api/prompts", self.base_url))
                .header("X-API-KEY", &self.api_key)
                .json(&create_payload)
                .send()
                .await?;
            Ok(format!(
                "Successfully registered new prompt: {}",
                prompt.slug
            ))
        }
    }

    pub async fn apply_agent(&self, agent: &AgentResource) -> Result<String> {
        let sys_prompt_id = if let Some(slug) = &agent.system_prompt_slug {
            self.get_prompt_id(slug).await?
        } else {
            self.get_prompt_id("system-default").await?
        };

        let url_get = format!("{}/api/agents/{}", self.base_url, agent.slug);
        let res_get = self
            .http
            .get(&url_get)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        let payload = json!({
            "name": agent.name,
            "description": agent.description,
            "system_prompt_id": sys_prompt_id,
            "default_model": agent.model,
            "temperature": agent.temperature,
            "skills": agent.skills,
        });

        if res_get.status().is_success() {
            let existing: AgentResponse = res_get.json().await?;
            self.http
                .patch(&format!("{}/api/agents/{}", self.base_url, existing.id))
                .header("X-API-KEY", &self.api_key)
                .json(&payload)
                .send()
                .await?;
            Ok(format!("Successfully synchronized agent: {}", agent.slug))
        } else {
            let mut create_payload = payload.clone();
            create_payload["slug"] = json!(agent.slug);
            self.http
                .post(&format!("{}/api/agents", self.base_url))
                .header("X-API-KEY", &self.api_key)
                .json(&create_payload)
                .send()
                .await?;
            Ok(format!("Successfully registered new agent: {}", agent.slug))
        }
    }

    /// Pull a resource from the server
    pub async fn pull_resource(&self, slug: &str, resource_type: &str) -> Result<(String, String)> {
        let endpoint = match resource_type {
            "prompt" => "prompts",
            "agent" => "agents",
            "workflow" => "workflows",
            _ => return Err(anyhow!("Unknown resource type: {}", resource_type)),
        };

        let url = format!("{}/api/{}/{}", self.base_url, endpoint, slug);
        let res = self
            .http
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!("Resource not found: {} ({})", slug, resource_type));
        }

        let body: Value = res.json().await?;

        let (content, ext) = match resource_type {
            "prompt" => {
                let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let inputs = body.get("input_variables").cloned().unwrap_or(json!({}));
                let name = body.get("name").and_then(|v| v.as_str()).unwrap_or(slug);
                let formatted = format!(
                    "---\nslug: \"{}\"\nname: \"{}\"\ninputs: {}\n---\n{}",
                    slug,
                    name,
                    serde_json::to_string_pretty(&inputs)?,
                    content
                );
                (formatted, "jgprompt")
            }
            "agent" => {
                let name = body.get("name").and_then(|v| v.as_str()).unwrap_or(slug);
                let model = body
                    .get("default_model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("gpt-4o");
                let temp = body
                    .get("temperature")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.7);
                let formatted = format!(
                    "slug: \"{}\"\nname: \"{}\"\nmodel: \"{}\"\ntemperature: {}\nsystem_prompt: \"\"",
                    slug, name, model, temp
                );
                (formatted, "jgagent")
            }
            "workflow" => {
                let definition = body
                    .get("definition")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (definition.to_string(), "jgflow")
            }
            _ => unreachable!(),
        };

        let filename = format!("{}.{}", slug, ext);
        Ok((content, filename))
    }

    /// List resources from the server
    pub async fn list_resources(&self, resource_type: Option<&str>) -> Result<Vec<ResourceInfo>> {
        let mut all_resources = Vec::new();

        let types = if let Some(t) = resource_type {
            vec![t]
        } else {
            vec!["prompt", "agent", "workflow"]
        };

        for rt in types {
            let endpoint = match rt {
                "prompt" => "prompts",
                "agent" => "agents",
                "workflow" => "workflows",
                _ => continue,
            };

            let url = format!("{}/api/{}", self.base_url, endpoint);
            let res = self
                .http
                .get(&url)
                .header("X-API-KEY", &self.api_key)
                .send()
                .await?;

            if res.status().is_success() {
                let items: Vec<Value> = res.json().await?;
                for item in items {
                    if let Some(slug) = item.get("slug").and_then(|v| v.as_str()) {
                        all_resources.push(ResourceInfo {
                            slug: slug.to_string(),
                            resource_type: rt.to_string(),
                        });
                    }
                }
            }
        }

        Ok(all_resources)
    }

    /// Delete a resource from the server
    pub async fn delete_resource(&self, slug: &str, resource_type: &str) -> Result<()> {
        let endpoint = match resource_type {
            "prompt" => "prompts",
            "agent" => "agents",
            "workflow" => "workflows",
            _ => return Err(anyhow!("Unknown resource type: {}", resource_type)),
        };

        let url = format!("{}/api/{}/{}", self.base_url, endpoint, slug);
        let res = self
            .http
            .delete(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to delete {} ({}): {}",
                slug,
                resource_type,
                res.status()
            ));
        }

        Ok(())
    }

    /// 注册 workflow 到 jug0
    pub async fn apply_workflow(
        &self,
        workflow: &WorkflowGraph,
        definition: &str,
        endpoint_url: &str,
    ) -> Result<String> {
        let url_get = format!("{}/api/workflows/{}", self.base_url, workflow.slug);
        let res_get = self
            .http
            .get(&url_get)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        let payload = json!({
            "name": if workflow.name.is_empty() { None } else { Some(&workflow.name) },
            "endpoint_url": endpoint_url,
            "definition": definition,
            "is_active": true,
        });

        if res_get.status().is_success() {
            let existing: WorkflowResponse = res_get.json().await?;
            self.http
                .patch(&format!("{}/api/workflows/{}", self.base_url, existing.id))
                .header("X-API-KEY", &self.api_key)
                .json(&payload)
                .send()
                .await?;
            Ok(format!(
                "Successfully synchronized workflow: {}",
                workflow.slug
            ))
        } else {
            let mut create_payload = payload.clone();
            create_payload["slug"] = json!(workflow.slug);
            self.http
                .post(&format!("{}/api/workflows", self.base_url))
                .header("X-API-KEY", &self.api_key)
                .json(&create_payload)
                .send()
                .await?;
            Ok(format!(
                "Successfully registered new workflow: {}",
                workflow.slug
            ))
        }
    }
}

#[async_trait]
impl JuglansRuntime for Jug0Client {
    async fn fetch_prompt(&self, slug: &str) -> Result<String> {
        let url = self.build_resource_url(slug, "prompts");
        let res = self
            .http
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Jug0 Network Error (Fetch Prompt '{}'): {}",
                slug,
                res.status()
            ));
        }

        let body: Value = res.json().await?;

        // Handle both legacy response and GitHub-style unified resource response
        let content = if Self::is_owner_slug_format(slug) {
            // GitHub-style: response has type field and nested prompt data
            body.get("content")
                .or_else(|| body.get("prompt").and_then(|p| p.get("content")))
                .and_then(|v| v.as_str())
        } else {
            // Legacy: direct content field
            body.get("content").and_then(|v| v.as_str())
        };

        content
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Data Corruption: 'content' field missing in prompt metadata."))
    }

    async fn search_memories(&self, query: &str, limit: u64) -> Result<Vec<Value>> {
        let url = format!("{}/api/memories/search", self.base_url);
        let payload = json!({
            "query": query,
            "limit": limit
        });

        let res = self
            .http
            .post(&url)
            .header("X-API-KEY", &self.api_key)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Jug0 Network Error (Memory Search): {}",
                res.status()
            ));
        }

        let results: Vec<Value> = res.json().await?;
        Ok(results)
    }

    async fn chat(
        &self,
        mut agent_config: Value,
        messages: Vec<Value>,
        tools_def: Option<Vec<Value>>,
        chat_id: Option<&str>,
        token_sender: Option<UnboundedSender<String>>,
    ) -> Result<ChatOutput> {
        let url = format!("{}/api/chat", self.base_url);

        if let Some(t_def) = tools_def {
            if let Some(agent_map) = agent_config.as_object_mut() {
                agent_map.insert("tools".to_string(), json!(t_def));
            }
        }

        let mut payload = json!({
            "agent": agent_config,
            "messages": messages
        });

        if let Some(id) = chat_id {
            if !id.is_empty() {
                payload["chat_id"] = json!(id);
            }
        }

        let res = self
            .http
            .post(&url)
            .header("X-API-KEY", &self.api_key)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res
                .text()
                .await
                .unwrap_or_else(|_| "Unknown Error".to_string());
            return Err(anyhow!("Jug0 API Rejection ({}): {}", status, txt));
        }

        let mut stream = res.bytes_stream().eventsource();
        let mut text_acc = String::new();
        let mut final_id = chat_id.unwrap_or("").to_string();
        let mut tool_calls = Vec::new();

        while let Some(event_res) = stream.next().await {
            let ev = event_res.map_err(|e| anyhow!("Stream Interrupted: {}", e))?;

            if ev.data == "[DONE]" {
                break;
            }

            if ev.event == "meta" {
                if let Ok(m) = serde_json::from_str::<Value>(&ev.data) {
                    if let Some(id) = m.get("chat_id").and_then(|v| v.as_str()) {
                        final_id = id.to_string();
                    }
                }
                continue;
            }

            if ev.event == "tool_call" {
                if let Ok(d) = serde_json::from_str::<Value>(&ev.data) {
                    if let Some(c) = d.get("tools").and_then(|v| v.as_array()) {
                        tool_calls.extend(c.clone());
                    }
                }
                continue;
            }

            if let Ok(d) = serde_json::from_str::<Value>(&ev.data) {
                if let Some(t) = d.get("text").and_then(|s| s.as_str()) {
                    text_acc.push_str(t);

                    // 1. 如果有信道，则发送 Token（用于 Web 端）
                    if let Some(sender) = &token_sender {
                        let _ = sender.send(t.to_string());
                    }

                    // 2. 【补回】CLI 流式输出
                    print!("{}", t);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            }
        }
        println!();

        if !tool_calls.is_empty() {
            Ok(ChatOutput::ToolCalls {
                calls: tool_calls,
                chat_id: final_id,
            })
        } else {
            Ok(ChatOutput::Final {
                text: text_acc,
                chat_id: final_id,
            })
        }
    }
}
