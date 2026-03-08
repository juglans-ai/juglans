// src/services/jug0.rs
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use uuid::Uuid;

use crate::core::agent_parser::AgentResource;
use crate::core::graph::WorkflowGraph;
use crate::core::jvalue::JValue;
use crate::core::prompt_parser::PromptResource;
use crate::services::config::JuglansConfig;
use crate::services::interface::{ChatRequest, JuglansRuntime};
use tracing::{error, info};

/// 定义对话输出类型，区分最终文本和工具调用请求
#[derive(Debug)]
pub enum ChatOutput {
    /// 最终回复文本
    Final { text: String, chat_id: String },
    /// AI 发起的工具调用请求
    ToolCalls {
        _calls: Vec<Value>,
        _chat_id: String,
    },
}

// Response types for get_prompt_id / get_workflow_id
#[derive(Deserialize)]
struct IdResponse {
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
    #[serde(rename = "handle")]
    pub _handle: String,
    pub target_type: String,
    pub target_id: Uuid,
}

/// Remote agent detail from jug0 (GET /api/agents/:id_or_slug)
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteAgentDetail {
    #[serde(rename = "id")]
    pub _id: Uuid,
    pub slug: String,
    pub name: Option<String>,
    #[serde(rename = "description")]
    pub _description: Option<String>,
    pub default_model: Option<String>,
    pub temperature: Option<f64>,
    #[serde(rename = "username")]
    pub _username: Option<String>,
    #[serde(rename = "avatar")]
    pub _avatar: Option<String>,
    pub system_prompt: Option<RemotePromptDetail>,
}

/// Remote prompt detail (embedded in agent response)
#[derive(Debug, Clone, Deserialize)]
pub struct RemotePromptDetail {
    #[serde(rename = "id")]
    pub _id: Uuid,
    #[serde(rename = "slug")]
    pub _slug: String,
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
            .timeout(Duration::from_secs(config.limits.http_timeout_secs))
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
        self.execution_token
            .read()
            .ok()
            .and_then(|guard| guard.clone())
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
                return Err(anyhow!(
                    "@{} is not an agent (type: {})",
                    handle,
                    lookup.target_type
                ));
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
            let error_text = res
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to get agent '{}': {} - {}",
                handle_or_slug,
                status,
                error_text
            ));
        }

        let agent: RemoteAgentDetail = res.json().await?;
        Ok(agent)
    }

    pub async fn get_prompt_id(&self, slug: &str) -> Result<Uuid> {
        let url = self.build_resource_url(slug, "prompts");
        let res = self
            .build_auth_request(reqwest::Method::GET, &url)
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(anyhow!(
                "Remote Resource Not Found: Prompt '{}' is missing.",
                slug
            ));
        }
        let body: IdResponse = res.json().await?;
        Ok(body.id)
    }

    /// Generic upsert: GET by slug → PATCH by id → fallback POST to create.
    /// Returns a success message string.
    async fn upsert_resource(
        &self,
        slug: &str,
        endpoint: &str,
        payload: Value,
        force: bool,
    ) -> Result<String> {
        let url_get = format!("{}/api/{}/{}", self.base_url, endpoint, slug);
        let res_get = self
            .build_auth_request(reqwest::Method::GET, &url_get)
            .send()
            .await?;

        if res_get.status().is_success() {
            // Extract id from response (works for prompts, agents, workflows)
            let existing: Value = res_get.json().await?;
            let id = existing
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'id' in {} response for '{}'", endpoint, slug))?;

            let url = if force {
                format!("{}/api/{}?force=true", self.base_url, id)
            } else {
                format!("{}/api/{}/{}", self.base_url, endpoint, id)
            };
            let res = self
                .build_auth_request(reqwest::Method::PATCH, &url)
                .json(&payload)
                .send()
                .await?;

            // If PATCH fails with 404, fall through to create (resource belongs to different org)
            if res.status() == 404 {
                // Fall through to create
            } else if !res.status().is_success() {
                let status = res.status();
                let error_text = res
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(anyhow!(
                    "Failed to update {} '{}': {} - {}",
                    endpoint,
                    slug,
                    status,
                    error_text
                ));
            } else {
                return Ok(format!(
                    "Successfully synchronized {}: {}",
                    endpoint.trim_end_matches('s'),
                    slug
                ));
            }
        }

        // Create new resource
        let mut create_payload = payload.clone();
        create_payload["slug"] = json!(slug);
        let url = if force {
            format!("{}/api/{}?force=true", self.base_url, endpoint)
        } else {
            format!("{}/api/{}", self.base_url, endpoint)
        };
        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&create_payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!(
                "Failed to create {} '{}': {} - {}",
                endpoint.trim_end_matches('s'),
                slug,
                status,
                error_text
            ));
        }

        Ok(format!(
            "Successfully registered new {}: {}",
            endpoint.trim_end_matches('s'),
            slug
        ))
    }

    /// Upload a local file to jug0 and return the URL
    pub async fn upload_file(&self, file_path: &std::path::Path) -> Result<String> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("upload")
            .to_string();

        let data = tokio::fs::read(file_path)
            .await
            .map_err(|e| anyhow!("Failed to read file '{}': {}", file_path.display(), e))?;

        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mime_type = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };

        let part = reqwest::multipart::Part::bytes(data)
            .file_name(file_name.clone())
            .mime_str(mime_type)?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let url = format!("{}/api/upload", self.base_url);
        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .multipart(form)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res.text().await.unwrap_or_default();
            return Err(anyhow!("Upload failed ({}): {}", status, error_text));
        }

        #[derive(Deserialize)]
        struct UploadResponse {
            url: String,
        }

        let resp: UploadResponse = res.json().await?;
        info!("Uploaded {} → {}", file_name, resp.url);
        Ok(resp.url)
    }

    pub async fn apply_prompt(&self, prompt: &PromptResource, force: bool) -> Result<String> {
        let mut payload = json!({
            "name": prompt.name,
            "content": prompt.content,
            "type": prompt.r#type,
            "is_system": prompt.r#type == "system",
            "input_variables": prompt.inputs,
            "tags": json!([prompt.r#type])
        });

        if let Some(public) = prompt.is_public {
            payload["is_public"] = json!(public);
        }

        self.upsert_resource(&prompt.slug, "prompts", payload, force)
            .await
    }

    pub async fn apply_agent(&self, agent: &AgentResource, force: bool) -> Result<String> {
        let sys_prompt_id = if let Some(slug) = &agent.system_prompt_slug {
            self.get_prompt_id(slug).await?
        } else {
            self.get_prompt_id("system-default").await?
        };

        let mut payload = json!({
            "name": agent.name,
            "description": agent.description,
            "system_prompt_id": sys_prompt_id,
            "default_model": agent.model,
            "temperature": agent.temperature,
            "skills": agent.skills,
        });

        if let Some(public) = agent.is_public {
            payload["is_public"] = json!(public);
        }

        // Add username if present (auto-registers @handle in jug0)
        if let Some(ref username) = agent.username {
            payload["username"] = json!(username);
        }

        // Handle avatar: upload local file or use URL directly
        if let Some(ref avatar_value) = agent.avatar {
            let avatar_url = if avatar_value.starts_with("http") || avatar_value.starts_with("/") {
                // Already a URL, use directly
                avatar_value.clone()
            } else {
                // Local file path — upload it
                let path = std::path::Path::new(avatar_value);
                match self.upload_file(path).await {
                    Ok(url) => url,
                    Err(e) => {
                        tracing::warn!(
                            "Agent '{}': failed to upload avatar '{}': {}",
                            agent.slug,
                            avatar_value,
                            e
                        );
                        avatar_value.clone()
                    }
                }
            };
            payload["avatar"] = json!(avatar_url);
        }

        // Add endpoint_url if provided (tells jug0 where to forward requests)
        let endpoint_suffix = if let Some(ref ep) = agent.endpoint {
            let base = ep.trim_end_matches('/');
            let url = if base.ends_with("/api/chat") {
                base.to_string()
            } else {
                format!("{}/api/chat", base)
            };
            payload["endpoint_url"] = json!(url);
            format!(" (endpoint: {})", url)
        } else {
            String::new()
        };

        let result = self
            .upsert_resource(&agent.slug, "agents", payload, force)
            .await?;
        Ok(format!("{}{}", result, endpoint_suffix))
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
            .build_auth_request(reqwest::Method::GET, &url)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!("Resource not found: {} ({})", slug, resource_type));
        }

        let body: Value = res.json().await?;

        let jb = JValue::from(body.clone());
        let (content, ext) = match resource_type {
            "prompt" => {
                let content_jv = jb.get("content");
                let content = content_jv.str_or("");
                let inputs = body.get("input_variables").cloned().unwrap_or(json!({}));
                let name_jv = jb.get("name");
                let name = name_jv.str_or(slug);
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
                let name_jv = jb.get("name");
                let name = name_jv.str_or(slug);
                let model_jv = jb.get("default_model");
                let model = model_jv.str_or("gpt-4o");
                let temp = jb.get("temperature").f64().unwrap_or(0.7);
                let formatted = format!(
                    "slug: \"{}\"\nname: \"{}\"\nmodel: \"{}\"\ntemperature: {}\nsystem_prompt: \"\"",
                    slug, name, model, temp
                );
                (formatted, "jgagent")
            }
            "workflow" => {
                let def_jv = jb.get("definition");
                let definition = def_jv.str_or("");
                (definition.to_string(), "jg")
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
                .build_auth_request(reqwest::Method::GET, &url)
                .send()
                .await?;

            if res.status().is_success() {
                let items: Vec<Value> = res.json().await?;
                for item in items {
                    let ji = JValue::from(item.clone());
                    if let Some(slug) = ji.get("slug").str() {
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
            .build_auth_request(reqwest::Method::DELETE, &url)
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
            .build_auth_request(reqwest::Method::GET, &url)
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
        // Parse definition as JSON for JSONB column
        let definition_json: Value =
            serde_json::from_str(definition).unwrap_or_else(|_| json!({"raw": definition}));

        let mut payload = json!({
            "name": if workflow.name.is_empty() { None } else { Some(&workflow.name) },
            "endpoint_url": endpoint_url,
            "definition": definition_json,
            "is_active": true,
        });

        if let Some(public) = workflow.is_public {
            payload["is_public"] = json!(public);
        }

        if let Some(ref schedule) = workflow.schedule {
            payload["trigger_config"] = json!({
                "type": "cron",
                "schedule": schedule,
            });
        }

        self.upsert_resource(&workflow.slug, "workflows", payload, force)
            .await
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
        let url = format!(
            "{}/api/chats/{}/messages/{}",
            self.base_url, chat_id, message_id
        );
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

    // ─── Vector Storage & Search ─────────────────────────────

    async fn vector_create_space(
        &self,
        space: &str,
        model: Option<&str>,
        public: bool,
    ) -> Result<Value> {
        let url = format!("{}/api/vectors/spaces", self.base_url);
        let mut payload = json!({ "space": space, "public": public });
        if let Some(m) = model {
            payload["model"] = json!(m);
        }

        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!("Vector create_space failed: {} - {}", status, txt));
        }

        let result: Value = res.json().await?;
        Ok(result)
    }

    async fn vector_upsert(
        &self,
        space: &str,
        points: Vec<Value>,
        model: Option<&str>,
    ) -> Result<Value> {
        let url = format!("{}/api/vectors/upsert", self.base_url);
        let mut payload = json!({ "space": space, "points": points });
        if let Some(m) = model {
            payload["model"] = json!(m);
        }

        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!("Vector upsert failed: {} - {}", status, txt));
        }

        let result: Value = res.json().await?;
        Ok(result)
    }

    async fn vector_search(
        &self,
        space: &str,
        query: &str,
        limit: u64,
        model: Option<&str>,
    ) -> Result<Vec<Value>> {
        let url = format!("{}/api/vectors/search", self.base_url);
        let mut payload = json!({ "space": space, "query": query, "limit": limit });
        if let Some(m) = model {
            payload["model"] = json!(m);
        }

        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!("Vector search failed: {} - {}", status, txt));
        }

        let results: Vec<Value> = res.json().await?;
        Ok(results)
    }

    async fn vector_list_spaces(&self) -> Result<Vec<Value>> {
        let url = format!("{}/api/vectors/spaces", self.base_url);
        let res = self
            .build_auth_request(reqwest::Method::GET, &url)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!("Vector list_spaces failed: {} - {}", status, txt));
        }

        let results: Vec<Value> = res.json().await?;
        Ok(results)
    }

    async fn vector_delete_space(&self, space: &str) -> Result<Value> {
        let url = format!("{}/api/vectors/spaces/{}", self.base_url, space);
        let res = self
            .build_auth_request(reqwest::Method::DELETE, &url)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!("Vector delete_space failed: {} - {}", status, txt));
        }

        let result: Value = res.json().await?;
        Ok(result)
    }

    async fn vector_delete(&self, space: &str, ids: Vec<String>) -> Result<Value> {
        let url = format!("{}/api/vectors/delete", self.base_url);
        let payload = json!({ "space": space, "ids": ids });
        let res = self
            .build_auth_request(reqwest::Method::POST, &url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().await.unwrap_or_default();
            return Err(anyhow!("Vector delete failed: {} - {}", status, txt));
        }

        let result: Value = res.json().await?;
        Ok(result)
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatOutput> {
        let ChatRequest {
            mut agent_config,
            messages,
            tools: tools_def,
            chat_id,
            token_sender,
            meta_sender,
            state,
            history,
            tool_handler,
        } = req;

        // 验证 agent_config 包含必须字段（slug 必须，model 可选 — server 端可从 agent 配置中获取）
        if let Some(agent_obj) = agent_config.as_object() {
            if !agent_obj.contains_key("slug") {
                return Err(anyhow!(
                    "Agent configuration is incomplete. Missing required field: slug\n\n\
                    💡 This usually means:\n\
                       1. The agent file (.jgagent) is missing required field: slug\n\
                       2. Or the jug0 server endpoint is incorrect\n\
                       3. Current jug0 endpoint: {}\n\
                       4. For local development, add [jug0] section in juglans.toml:\n\
                          [jug0]\n\
                          base_url = \"http://localhost:3000\"",
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

        if let Some(ref id) = chat_id {
            if !id.is_empty() {
                payload["chat_id"] = json!(id);
            }
        }

        if let Some(ref s) = state {
            payload["state"] = json!(s);
        }
        if let Some(ref h) = history {
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
        let mut final_id = chat_id.unwrap_or_default();
        let mut tool_calls = Vec::new();

        while let Some(event_res) = stream.next().await {
            let ev = event_res.map_err(|e| anyhow!("Stream Interrupted: {}", e))?;

            if ev.data == "[DONE]" {
                break;
            }

            if ev.event == "meta" {
                if let Ok(m) = serde_json::from_str::<Value>(&ev.data) {
                    if let Some(id) = JValue::from(m.clone()).get("chat_id").str() {
                        final_id = id.to_string();
                    }
                    // 转发完整 meta 事件到前端（chat_id, user_message_id 等）
                    if let Some(sender) = &meta_sender {
                        let _ = sender.send(m);
                    }
                }
                continue;
            }

            if ev.event == "tool_call" {
                if let Ok(d) = serde_json::from_str::<Value>(&ev.data) {
                    let tools = d
                        .get("tools")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();

                    if let Some(ref handler) = tool_handler {
                        // SSE 统一流：执行工具 → POST /tool-result → 继续读流
                        let call_id = d
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let mut results = Vec::new();

                        // Execute all tool calls concurrently
                        let mut tool_tasks = Vec::new();
                        for tool in &tools {
                            let name = tool["name"]
                                .as_str()
                                .or_else(|| tool.pointer("/function/name").and_then(|v| v.as_str()))
                                .unwrap_or("unknown")
                                .to_string();
                            let args = tool["arguments"]
                                .as_str()
                                .or_else(|| {
                                    tool.pointer("/function/arguments").and_then(|v| v.as_str())
                                })
                                .unwrap_or("{}")
                                .to_string();
                            let tool_call_id = tool["id"].as_str().unwrap_or("").to_string();
                            let handler_clone = handler.clone();

                            tool_tasks.push(tokio::spawn(async move {
                                info!("🔧 [Tool Handler] Executing: {}({:.80})", name, args);
                                let content =
                                    match handler_clone.handle_tool_call(&name, &args).await {
                                        Ok(r) => r,
                                        Err(e) => {
                                            error!("🔧 [Tool Handler] {} failed: {}", name, e);
                                            format!("Error: {}", e)
                                        }
                                    };
                                info!(
                                    "🔧 [Tool Handler] {} → {:.80}",
                                    name,
                                    content.replace('\n', " ")
                                );
                                json!({"tool_call_id": tool_call_id, "content": content})
                            }));
                        }
                        for task in tool_tasks {
                            if let Ok(result) = task.await {
                                results.push(result);
                            }
                        }

                        // POST /api/chat/tool-result → jug0 channel 接收 → SSE 流恢复
                        let post_result = self
                            .build_auth_request(
                                reqwest::Method::POST,
                                &format!("{}/api/chat/tool-result", self.base_url),
                            )
                            .json(&json!({"call_id": call_id, "results": results}))
                            .send()
                            .await;

                        match post_result {
                            Ok(r) => {
                                if r.status().is_success() {
                                    info!("🔧 [Tool Handler] tool-result POST: {}", r.status());
                                } else {
                                    let status = r.status();
                                    let err_text = r
                                        .text()
                                        .await
                                        .unwrap_or_else(|_| "Unknown error".to_string());
                                    error!(
                                        "🔧 [Tool Handler] tool-result POST failed: {} - {}",
                                        status, err_text
                                    );
                                    return Err(anyhow!(
                                        "Tool result submission failed ({}): {}",
                                        status,
                                        err_text
                                    ));
                                }
                            }
                            Err(e) => {
                                error!("🔧 [Tool Handler] tool-result POST failed: {}", e);
                                return Err(anyhow!("Tool result submission failed: {}", e));
                            }
                        }
                        continue; // SSE 流恢复，继续读
                    } else {
                        // 无 handler → break 返回 ToolCalls（兼容旧调用方）
                        tool_calls.extend(tools);
                        break;
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

                    // 2. CLI 流式输出（仅在无 token_sender 时，避免 TUI 模式下污染终端）
                    if token_sender.is_none() {
                        print!("{}", t);
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                    }
                }
            }
        }
        if token_sender.is_none() {
            println!();
        }

        if !tool_calls.is_empty() {
            Ok(ChatOutput::ToolCalls {
                _calls: tool_calls,
                _chat_id: final_id,
            })
        } else {
            Ok(ChatOutput::Final {
                text: text_acc,
                chat_id: final_id,
            })
        }
    }
}
