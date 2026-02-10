// src/services/jug0.rs
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};
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

/// User info from Jug0 server
#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub org_name: Option<String>,
}

/// Handle lookup response from jug0 (GET /api/handles/:handle)
#[derive(Debug, Clone, Deserialize)]
pub struct HandleLookup {
    pub handle: String,
    pub target_type: String,
    pub target_id: Uuid,
}

/// Remote agent detail from jug0 (GET /api/agents/:id_or_slug)
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteAgentDetail {
    pub id: Uuid,
    pub slug: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub default_model: Option<String>,
    pub temperature: Option<f64>,
    pub username: Option<String>,
    pub system_prompt: Option<RemotePromptDetail>,
}

/// Remote prompt detail (embedded in agent response)
#[derive(Debug, Clone, Deserialize)]
pub struct RemotePromptDetail {
    pub id: Uuid,
    pub slug: String,
    pub content: String,
}

#[derive(Clone)]
pub struct Jug0Client {
    http: Client,
    base_url: String,
    api_key: String,
    /// Execution token injected from jug0 when forwarding workflow requests.
    /// When set, this takes priority over api_key for authentication.
    execution_token: Arc<RwLock<Option<String>>>,
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
            execution_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Set execution token (called by web_server when receiving forwarded requests from jug0).
    /// When set, this token takes priority over api_key for authenticating with jug0.
    pub fn set_execution_token(&self, token: Option<String>) {
        if let Ok(mut guard) = self.execution_token.write() {
            *guard = token;
        }
    }

    /// Get the current execution token (if set).
    pub fn get_execution_token(&self) -> Option<String> {
        self.execution_token.read().ok().and_then(|guard| guard.clone())
    }

    /// Build a request with the appropriate authentication header.
    /// Priority: execution_token > api_key
    fn build_auth_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut builder = self.http.request(method, url);

        if let Some(token) = self.get_execution_token() {
            // Use execution token (forwarded from jug0, represents original caller)
            tracing::debug!("🔐 Using X-Execution-Token for jug0 request");
            builder = builder.header("X-Execution-Token", token);
        } else {
            // Use api_key (local development or CLI mode)
            builder = builder.header("X-API-KEY", &self.api_key);
        }

        builder
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

    /// Get agent details by handle or slug
    /// @handle → resolve via handles API → GET /api/agents/:uuid
    /// slug   → GET /api/agents/:slug directly
    pub async fn get_agent(&self, handle_or_slug: &str) -> Result<RemoteAgentDetail> {
        // @handle: resolve through handles table first
        let agent_identifier = if let Some(handle) = handle_or_slug.strip_prefix('@') {
            let url = format!("{}/api/handles/{}", self.base_url, handle);
            let res = self
                .build_auth_request(reqwest::Method::GET, &url)
                .send()
                .await?;

            if !res.status().is_success() {
                return Err(anyhow!("Handle @{} not found", handle));
            }

            let lookup: HandleLookup = res.json().await?;
            if lookup.target_type != "agent" {
                return Err(anyhow!("@{} is not an agent (type: {})", handle, lookup.target_type));
            }
            lookup.target_id.to_string()
        } else {
            handle_or_slug.to_string()
        };

        let url = format!("{}/api/agents/{}", self.base_url, agent_identifier);
        let res = self
            .build_auth_request(reqwest::Method::GET, &url)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to get agent '{}': {} - {}",
                handle_or_slug, status, error_text
            ));
        }

        let agent: RemoteAgentDetail = res.json().await?;
        Ok(agent)
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

    pub async fn get_workflow_id(&self, slug: &str) -> Result<Uuid> {
        let url = self.build_resource_url(slug, "workflows");
        let res = self
            .http
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!(
                "Remote Resource Not Found: Workflow '{}' is missing.",
                slug
            ));
        }
        let body: WorkflowResponse = res.json().await?;
        Ok(body.id)
    }

    pub async fn apply_prompt(&self, prompt: &PromptResource, force: bool) -> Result<String> {
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
            let url = if force {
                format!("{}/api/prompts/{}?force=true", self.base_url, existing.id)
            } else {
                format!("{}/api/prompts/{}", self.base_url, existing.id)
            };
            let res = self.http
                .patch(&url)
                .header("X-API-KEY", &self.api_key)
                .json(&payload)
                .send()
                .await?;

            // If PATCH fails with 404, fall through to create
            if res.status() == 404 {
                // Prompt exists but we don't own it, create our own
            } else if !res.status().is_success() {
                let status = res.status();
                let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                return Err(anyhow!(
                    "Failed to update prompt '{}': {} - {}",
                    prompt.slug,
                    status,
                    error_text
                ));
            } else {
                return Ok(format!("Successfully synchronized prompt: {}", prompt.slug));
            }
        }

        // Create new prompt
        {
            let mut create_payload = payload.clone();
            create_payload["slug"] = json!(prompt.slug);
            let url = if force {
                format!("{}/api/prompts?force=true", self.base_url)
            } else {
                format!("{}/api/prompts", self.base_url)
            };
            let res = self.http
                .post(&url)
                .header("X-API-KEY", &self.api_key)
                .json(&create_payload)
                .send()
                .await?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to create prompt '{}': {} - {}",
                prompt.slug,
                status,
                error_text
            ));
        }

        Ok(format!(
            "Successfully registered new prompt: {}",
            prompt.slug
        ))
        }
    }

    pub async fn apply_agent(&self, agent: &AgentResource, force: bool) -> Result<String> {
        let sys_prompt_id = if let Some(slug) = &agent.system_prompt_slug {
            self.get_prompt_id(slug).await?
        } else {
            self.get_prompt_id("system-default").await?
        };

        // Resolve workflow_id if workflow reference is provided
        // Supports both file path format ("../workflows/trading.jgflow") and slug format ("trading")
        let workflow_id = if let Some(workflow_ref) = &agent.workflow {
            let slug = if workflow_ref.ends_with(".jgflow")
                || workflow_ref.contains('/')
                || workflow_ref.contains('\\')
            {
                std::path::Path::new(workflow_ref)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(workflow_ref)
            } else {
                workflow_ref.as_str()
            };

            match self.get_workflow_id(slug).await {
                Ok(id) => Some(id),
                Err(_) => {
                    tracing::warn!(
                        "Agent '{}': workflow '{}' not found on server, skipping binding.",
                        agent.slug, slug
                    );
                    None
                }
            }
        } else {
            None
        };

        let url_get = format!("{}/api/agents/{}", self.base_url, agent.slug);
        let res_get = self
            .http
            .get(&url_get)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        let mut payload = json!({
            "name": agent.name,
            "description": agent.description,
            "system_prompt_id": sys_prompt_id,
            "default_model": agent.model,
            "temperature": agent.temperature,
            "skills": agent.skills,
        });

        // Add username if present (auto-registers @handle in jug0)
        if let Some(ref username) = agent.username {
            payload["username"] = json!(username);
        }

        // Add workflow_id if present
        let workflow_suffix = if let Some(wf_id) = workflow_id {
            payload["workflow_id"] = json!(wf_id);
            let wf_ref = agent.workflow.as_deref().unwrap_or("?");
            format!(" (workflow: {} → {})", wf_ref, wf_id)
        } else if agent.workflow.is_some() {
            let wf_ref = agent.workflow.as_deref().unwrap_or("?");
            format!(" (⚠️ workflow '{}' not bound - not found on server)", wf_ref)
        } else {
            String::new()
        };

        if res_get.status().is_success() {
            let existing: AgentResponse = res_get.json().await?;
            let url = if force {
                format!("{}/api/agents/{}?force=true", self.base_url, existing.id)
            } else {
                format!("{}/api/agents/{}", self.base_url, existing.id)
            };
            let res = self.http
                .patch(&url)
                .header("X-API-KEY", &self.api_key)
                .json(&payload)
                .send()
                .await?;

            // If PATCH by ID fails with 404, GET may have returned a different org's agent
            // Retry PATCH by slug (which will match our own agent in the same org)
            if res.status() == 404 {
                let url_retry = format!("{}/api/agents/{}", self.base_url, agent.slug);
                let res_retry = self.http
                    .patch(&url_retry)
                    .header("X-API-KEY", &self.api_key)
                    .json(&payload)
                    .send()
                    .await?;

                if res_retry.status().is_success() {
                    return Ok(format!("Successfully synchronized agent: {}{}", agent.slug, workflow_suffix));
                }
                // Still failed, fall through to create
            } else if !res.status().is_success() {
                let status = res.status();
                let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                return Err(anyhow!(
                    "Failed to update agent '{}': {} - {}",
                    agent.slug,
                    status,
                    error_text
                ));
            } else {
                return Ok(format!("Successfully synchronized agent: {}{}", agent.slug, workflow_suffix));
            }
        }

        // Create new agent (either GET failed or PATCH returned 404)
        {
            let mut create_payload = payload.clone();
            create_payload["slug"] = json!(agent.slug);
            let url = if force {
                format!("{}/api/agents?force=true", self.base_url)
            } else {
                format!("{}/api/agents", self.base_url)
            };
            let res = self.http
                .post(&url)
                .header("X-API-KEY", &self.api_key)
                .json(&create_payload)
                .send()
                .await?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to create agent '{}': {} - {}",
                agent.slug,
                status,
                error_text
            ));
        }

        Ok(format!("Successfully registered new agent: {}{}", agent.slug, workflow_suffix))
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

    /// 获取当前用户信息
    pub async fn get_current_user(&self) -> Result<UserInfo> {
        let url = format!("{}/api/auth/me", self.base_url);
        let res = self
            .http
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to get user info: {} (Check your API key)",
                res.status()
            ));
        }

        let user_info: UserInfo = res.json().await?;
        Ok(user_info)
    }

    /// 注册 workflow 到 jug0
    pub async fn apply_workflow(
        &self,
        workflow: &WorkflowGraph,
        definition: &str,
        endpoint_url: &str,
        force: bool,
    ) -> Result<String> {
        let url_get = format!("{}/api/workflows/{}", self.base_url, workflow.slug);
        let res_get = self
            .http
            .get(&url_get)
            .header("X-API-KEY", &self.api_key)
            .send()
            .await?;

        // Parse definition as JSON for JSONB column
        let definition_json: Value = serde_json::from_str(definition)
            .unwrap_or_else(|_| json!({"raw": definition}));

        let payload = json!({
            "name": if workflow.name.is_empty() { None } else { Some(&workflow.name) },
            "endpoint_url": endpoint_url,
            "definition": definition_json,
            "is_active": true,
        });

        if res_get.status().is_success() {
            let existing: WorkflowResponse = res_get.json().await?;
            let url = if force {
                format!("{}/api/workflows/{}?force=true", self.base_url, existing.id)
            } else {
                format!("{}/api/workflows/{}", self.base_url, existing.id)
            };
            let res = self.http
                .patch(&url)
                .header("X-API-KEY", &self.api_key)
                .json(&payload)
                .send()
                .await?;

            // If PATCH fails with 404, fall through to create
            if res.status() == 404 {
                // Workflow exists but we don't own it, create our own
            } else if !res.status().is_success() {
                let status = res.status();
                let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                return Err(anyhow!(
                    "Failed to update workflow '{}': {} - {}",
                    workflow.slug,
                    status,
                    error_text
                ));
            } else {
                return Ok(format!(
                    "Successfully synchronized workflow: {}",
                    workflow.slug
                ));
            }
        }

        // Create new workflow
        {
            let mut create_payload = payload.clone();
            create_payload["slug"] = json!(workflow.slug);
            let url = if force {
                format!("{}/api/workflows?force=true", self.base_url)
            } else {
                format!("{}/api/workflows", self.base_url)
            };
            let res = self.http
                .post(&url)
                .header("X-API-KEY", &self.api_key)
                .json(&create_payload)
                .send()
                .await?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to create workflow '{}': {} - {}",
                workflow.slug,
                status,
                error_text
            ));
        }

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
            .build_auth_request(reqwest::Method::GET, &url)
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
            .build_auth_request(reqwest::Method::POST, &url)
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

    async fn fetch_chat_history(&self, chat_id: &str, include_all: bool) -> Result<Vec<Value>> {
        let mut url = format!("{}/api/chats/{}/history", self.base_url, chat_id);
        if include_all {
            url.push_str("?include_all=true");
        }

        let res = self
            .build_auth_request(reqwest::Method::GET, &url)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Jug0 Network Error (Fetch Chat History '{}'): {} - {}",
                chat_id,
                status,
                txt
            ));
        }

        #[derive(Deserialize)]
        struct HistoryResponse {
            messages: Vec<Value>,
        }

        let resp: HistoryResponse = res.json().await?;
        Ok(resp.messages)
    }

    async fn create_message(
        &self,
        chat_id: &str,
        role: &str,
        content: &str,
        state: &str,
    ) -> Result<()> {
        let url = format!("{}/api/chats/{}/messages", self.base_url, chat_id);
        let payload = json!({
            "role": role,
            "message_type": "chat",
            "state": state,
            "parts": [{ "type": "text", "content": content }]
        });

        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            tracing::warn!("Failed to persist reply message: {}", status);
        }

        Ok(())
    }

    async fn update_message_state(
        &self,
        chat_id: &str,
        message_id: i32,
        state: &str,
    ) -> Result<()> {
        let url = format!("{}/api/chats/{}/messages/{}", self.base_url, chat_id, message_id);
        let payload = json!({ "state": state });

        let res = self
            .build_auth_request(reqwest::Method::PATCH, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            tracing::warn!("Failed to update user message state: {}", status);
        }

        Ok(())
    }

    async fn chat(
        &self,
        mut agent_config: Value,
        messages: Vec<Value>,
        tools_def: Option<Vec<Value>>,
        chat_id: Option<&str>,
        token_sender: Option<UnboundedSender<String>>,
        state: Option<&str>,
        history: Option<&str>,
    ) -> Result<ChatOutput> {
        // 验证 agent_config 包含必须字段
        if let Some(agent_obj) = agent_config.as_object() {
            let required_fields = ["slug", "model"];
            let mut missing_fields = Vec::new();

            for field in required_fields {
                if !agent_obj.contains_key(field) {
                    missing_fields.push(field);
                }
            }

            if !missing_fields.is_empty() {
                return Err(anyhow!(
                    "Agent configuration is incomplete. Missing required fields: {}\n\n\
                    💡 This usually means:\n\
                       1. The agent file (.jgagent) is missing required fields: slug and model\n\
                       2. Or the jug0 server endpoint is incorrect\n\
                       3. Current jug0 endpoint: {}\n\
                       4. For local development, add [jug0] section in juglans.toml:\n\
                          [jug0]\n\
                          base_url = \"http://localhost:3000\"",
                    missing_fields.join(", "),
                    self.base_url
                ));
            }
        }

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

        if let Some(s) = state {
            payload["state"] = json!(s);
        }
        if let Some(h) = history {
            // 尝试解析为 JSON（false 或 数组），否则忽略
            if let Ok(parsed) = serde_json::from_str::<Value>(h) {
                payload["history"] = parsed;
            }
        }

        // Use execution token if available (forwarded from jug0), otherwise fall back to api_key
        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res
                .text()
                .await
                .unwrap_or_else(|_| "Unknown Error".to_string());

            // 422 错误通常是请求体字段缺失或格式错误
            if status == 422 {
                let mut error_msg = format!("Jug0 API Validation Error (422): {}", txt);

                // 如果错误提到 missing field，提供配置建议
                if txt.contains("missing field") {
                    error_msg.push_str("\n\n💡 Possible causes:");
                    error_msg.push_str(
                        "\n   1. Agent configuration is incomplete (missing required fields)",
                    );
                    error_msg.push_str("\n   2. Check your jug0 server configuration:");
                    error_msg.push_str(&format!("\n      - Current endpoint: {}", self.base_url));
                    error_msg.push_str("\n      - Ensure [jug0] section exists in juglans.toml");
                    error_msg.push_str("\n   3. Verify agent file has all required fields: slug, name, model, system_prompt");
                }

                return Err(anyhow!(error_msg));
            }

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
