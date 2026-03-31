// src/repositories/agents.rs
//
// Agent repository with cache + DB fallback

use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, IntoActiveModel,
    QueryFilter, QueryOrder, Set,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::entities::{agents, handles, prompts, users};
use crate::errors::AppError;
use crate::handlers::handles::{create_handle, delete_handle_by_target};
use crate::request::{CreateAgentRequest, UpdateAgentRequest};
use crate::response::{AgentDetailResponse, AgentWithOwner, OwnerInfo};
use crate::services::cache::CacheService;

#[derive(Clone)]
pub struct AgentRepository {
    db: DatabaseConnection,
    cache: CacheService,
}

impl AgentRepository {
    pub fn new(db: DatabaseConnection, cache: CacheService) -> Self {
        Self { db, cache }
    }

    /// List all agents visible to the user
    pub async fn list_for_user(&self, user: &AuthUser) -> Result<Vec<AgentWithOwner>, AppError> {
        // 1. Try cache
        let cache_key = format!("agents:list:{}:{}", user.org_id, user.id);
        if let Some(cached) = self.cache.get::<Vec<AgentWithOwner>>(&cache_key).await {
            return Ok(cached);
        }

        // 2. DB query
        let agents_list = agents::Entity::find()
            .filter(
                Condition::any()
                    // Own agents (public + private)
                    .add(
                        Condition::all()
                            .add(agents::Column::OrgId.eq(&user.org_id))
                            .add(agents::Column::UserId.eq(user.id)),
                    )
                    // Same org public agents
                    .add(
                        Condition::all()
                            .add(agents::Column::OrgId.eq(&user.org_id))
                            .add(agents::Column::IsPublic.eq(true)),
                    )
                    // Official public agents
                    .add(
                        Condition::all()
                            .add(agents::Column::OrgId.eq(crate::official_org_slug()))
                            .add(agents::Column::IsPublic.eq(true)),
                    ),
            )
            .order_by_desc(agents::Column::CreatedAt)
            .all(&self.db)
            .await?;

        // 3. Build AgentWithOwner
        let result = self.build_agents_with_owners(agents_list).await?;

        // 4. Cache result
        let _ = self.cache.set(&cache_key, &result, 300).await;

        Ok(result)
    }

    /// Get agent by ID or slug
    pub async fn get_by_id_or_slug(
        &self,
        user: &AuthUser,
        id_or_slug: &str,
    ) -> Result<AgentDetailResponse, AppError> {
        // Try cache first (by ID)
        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            let cache_key = format!("agents:detail:{}", id);
            if let Some(cached) = self.cache.get::<AgentDetailResponse>(&cache_key).await {
                return Ok(cached);
            }
        }

        // DB query
        let mut query = agents::Entity::find();

        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            query = query.filter(agents::Column::Id.eq(id));
        } else {
            query = query.filter(agents::Column::Slug.eq(id_or_slug));
        }

        // Access control
        query = query.filter(
            Condition::any()
                // Own agents
                .add(
                    Condition::all()
                        .add(agents::Column::OrgId.eq(&user.org_id))
                        .add(agents::Column::UserId.eq(user.id)),
                )
                // Same org public
                .add(
                    Condition::all()
                        .add(agents::Column::OrgId.eq(&user.org_id))
                        .add(agents::Column::IsPublic.eq(true)),
                )
                // Official public
                .add(
                    Condition::all()
                        .add(agents::Column::OrgId.eq(crate::official_org_slug()))
                        .add(agents::Column::IsPublic.eq(true)),
                ),
        );

        let agent = query
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Agent '{}' not found", id_or_slug)))?;

        // Get system prompt
        let system_prompt = match agent.system_prompt_id {
            Some(sp_id) => prompts::Entity::find_by_id(sp_id).one(&self.db).await?,
            None => None,
        };

        let result = AgentDetailResponse {
            agent,
            system_prompt,
        };

        // Cache by ID
        let cache_key = format!("agents:detail:{}", result.agent.id);
        let _ = self.cache.set(&cache_key, &result, 300).await;

        Ok(result)
    }

    /// Create agent
    pub async fn create(
        &self,
        user: &AuthUser,
        req: CreateAgentRequest,
    ) -> Result<agents::Model, AppError> {
        // Check slug uniqueness
        let exists = agents::Entity::find()
            .filter(agents::Column::OrgId.eq(&user.org_id))
            .filter(agents::Column::UserId.eq(user.id))
            .filter(agents::Column::Slug.eq(&req.slug))
            .one(&self.db)
            .await?;

        if exists.is_some() {
            return Err(AppError::BadRequest(format!(
                "Agent slug '{}' already exists",
                req.slug
            )));
        }

        // Validate system_prompt_id
        let sp_exists = prompts::Entity::find_by_id(req.system_prompt_id)
            .filter(
                Condition::any()
                    .add(prompts::Column::UserId.eq(user.id))
                    .add(prompts::Column::IsPublic.eq(true))
                    .add(prompts::Column::OrgId.eq(crate::official_org_slug())),
            )
            .one(&self.db)
            .await?;

        if sp_exists.is_none() {
            return Err(AppError::BadRequest("Invalid system_prompt_id".to_string()));
        }

        // Check username uniqueness if provided
        if let Some(ref username) = req.username {
            let existing = handles::Entity::find()
                .filter(handles::Column::OrgId.eq(&user.org_id))
                .filter(handles::Column::Handle.eq(username))
                .one(&self.db)
                .await?;

            if existing.is_some() {
                return Err(AppError::Conflict(format!(
                    "Handle @{} is already taken",
                    username
                )));
            }
        }

        let agent_id = Uuid::new_v4();
        let new_agent = agents::ActiveModel {
            id: Set(agent_id),
            org_id: Set(Some(user.org_id.clone())),
            user_id: Set(Some(user.id)),
            slug: Set(req.slug),
            name: Set(Some(req.name)),
            description: Set(req.description),
            system_prompt_id: Set(Some(req.system_prompt_id)),
            default_model: Set(Some(req.default_model)),
            allowed_models: Set(Some(serde_json::json!(req
                .allowed_models
                .unwrap_or(vec!["gpt-4o".to_string()])))),
            skills: Set(Some(serde_json::json!(req.skills.unwrap_or_default()))),
            mcp_config: Set(req.mcp_config),
            temperature: Set(req.temperature.or(Some(0.7))),
            endpoint_url: Set(req.endpoint_url),
            is_public: Set(req.is_public.or(Some(false))),
            fork_from_id: Set(None),
            username: Set(req.username.clone()),
            avatar: Set(req.avatar.clone()),
            ..Default::default()
        };

        let saved = new_agent.insert(&self.db).await?;

        // Auto-register handle if username is provided
        if let Some(ref username) = req.username {
            if let Err(e) = create_handle(&self.db, &user.org_id, username, "agent", agent_id).await
            {
                tracing::warn!("Failed to create handle @{}: {}", username, e);
                // Don't fail the agent creation, handle is optional
            }
        }

        // Invalidate list cache
        self.invalidate_list_cache(&user.org_id).await;

        Ok(saved)
    }

    /// Update agent
    pub async fn update(
        &self,
        user: &AuthUser,
        id_or_slug: &str,
        req: UpdateAgentRequest,
    ) -> Result<agents::Model, AppError> {
        // Find agent (same org can update)
        let mut query = agents::Entity::find();
        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            query = query.filter(agents::Column::Id.eq(id));
        } else {
            query = query.filter(agents::Column::Slug.eq(id_or_slug));
        }

        let agent = query
            .filter(agents::Column::OrgId.eq(&user.org_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Agent not found or no permission".to_string()))?;

        let agent_id = agent.id;
        let old_username = agent.username.clone();

        let mut active = agent.into_active_model();

        if let Some(v) = req.name {
            active.name = Set(Some(v));
        }
        if let Some(v) = req.description {
            active.description = Set(Some(v));
        }
        if let Some(v) = req.system_prompt_id {
            active.system_prompt_id = Set(Some(v));
        }
        if let Some(v) = req.default_model {
            active.default_model = Set(Some(v));
        }
        if let Some(v) = req.allowed_models {
            active.allowed_models = Set(Some(serde_json::json!(v)));
        }
        if let Some(v) = req.skills {
            active.skills = Set(Some(serde_json::json!(v)));
        }
        if let Some(v) = req.mcp_config {
            active.mcp_config = Set(Some(v));
        }
        if let Some(v) = req.temperature {
            active.temperature = Set(Some(v));
        }
        // endpoint_url: only apply if explicitly provided in the request
        // Some(Some(url)) → set endpoint, Some(None) → clear endpoint, None → don't touch
        if let Some(endpoint_url) = req.endpoint_url {
            active.endpoint_url = Set(endpoint_url);
        }
        if let Some(v) = req.is_public {
            active.is_public = Set(Some(v));
        }

        // Handle avatar update
        if let Some(ref avatar) = req.avatar {
            active.avatar = Set(Some(avatar.clone()));
        }

        // Handle username update
        if let Some(ref new_username) = req.username {
            // Check if username changed
            if old_username.as_ref() != Some(new_username) {
                // Check if new username is available
                let existing = handles::Entity::find()
                    .filter(handles::Column::OrgId.eq(&user.org_id))
                    .filter(handles::Column::Handle.eq(new_username))
                    .one(&self.db)
                    .await?;

                if existing.is_some() {
                    return Err(AppError::Conflict(format!(
                        "Handle @{} is already taken",
                        new_username
                    )));
                }

                // Delete old handle if exists
                if old_username.is_some() {
                    let _ =
                        delete_handle_by_target(&self.db, &user.org_id, "agent", agent_id).await;
                }

                // Create new handle
                if let Err(e) =
                    create_handle(&self.db, &user.org_id, new_username, "agent", agent_id).await
                {
                    tracing::warn!("Failed to create handle @{}: {}", new_username, e);
                }

                active.username = Set(Some(new_username.clone()));
            }
        }

        active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));

        let updated = active.update(&self.db).await?;

        // Invalidate caches
        self.invalidate_list_cache(&user.org_id).await;
        let _ = self
            .cache
            .del(&format!("agents:detail:{}", updated.id))
            .await;

        Ok(updated)
    }

    /// Delete agent
    pub async fn delete(&self, user: &AuthUser, id_or_slug: &str) -> Result<Uuid, AppError> {
        let mut query = agents::Entity::find();
        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            query = query.filter(agents::Column::Id.eq(id));
        } else {
            query = query.filter(agents::Column::Slug.eq(id_or_slug));
        }

        let agent = query
            .filter(agents::Column::OrgId.eq(&user.org_id))
            .filter(agents::Column::UserId.eq(user.id))
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Agent not found or no permission".to_string()))?;

        let agent_id = agent.id;
        let org_id = agent.org_id.clone().unwrap_or_default();

        // Delete associated handle first
        if let Err(e) = delete_handle_by_target(&self.db, &org_id, "agent", agent_id).await {
            tracing::warn!("Failed to delete handle for agent {}: {}", agent_id, e);
        }

        agents::Entity::delete_by_id(agent_id)
            .exec(&self.db)
            .await?;

        // Invalidate caches
        self.invalidate_list_cache(&user.org_id).await;
        let _ = self.cache.del(&format!("agents:detail:{}", agent_id)).await;

        Ok(agent_id)
    }

    // --- Helper methods ---

    async fn build_agents_with_owners(
        &self,
        agents_list: Vec<agents::Model>,
    ) -> Result<Vec<AgentWithOwner>, AppError> {
        if agents_list.is_empty() {
            return Ok(vec![]);
        }

        // Collect IDs for batch fetch
        let user_ids: Vec<Uuid> = agents_list
            .iter()
            .filter_map(|a| a.user_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let prompt_ids: Vec<Uuid> = agents_list
            .iter()
            .filter_map(|a| a.system_prompt_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        // Parallel fetch users and prompts
        let db = &self.db;
        let (users_result, prompts_result) = tokio::join!(
            async {
                if user_ids.is_empty() {
                    Ok::<_, sea_orm::DbErr>(vec![])
                } else {
                    users::Entity::find()
                        .filter(users::Column::Id.is_in(user_ids))
                        .all(db)
                        .await
                }
            },
            async {
                if prompt_ids.is_empty() {
                    Ok::<_, sea_orm::DbErr>(vec![])
                } else {
                    prompts::Entity::find()
                        .filter(prompts::Column::Id.is_in(prompt_ids))
                        .all(db)
                        .await
                }
            }
        );

        let users_map: HashMap<Uuid, users::Model> =
            users_result?.into_iter().map(|u| (u.id, u)).collect();

        let prompts_map: HashMap<Uuid, prompts::Model> =
            prompts_result?.into_iter().map(|p| (p.id, p)).collect();

        // Build result
        let result = agents_list
            .into_iter()
            .map(|agent| {
                let owner = agent
                    .user_id
                    .and_then(|uid| users_map.get(&uid))
                    .map(|u| OwnerInfo {
                        id: u.id,
                        username: u.username.clone(),
                        name: u.name.clone(),
                    });

                let url = owner.as_ref().and_then(|o| {
                    o.username
                        .as_ref()
                        .map(|u| format!("/{}/{}", u, agent.slug))
                });

                let system_prompt_content = agent
                    .system_prompt_id
                    .and_then(|sp_id| prompts_map.get(&sp_id))
                    .map(|p| p.content.clone());

                AgentWithOwner {
                    agent,
                    owner,
                    url,
                    system_prompt_content,
                }
            })
            .collect();

        Ok(result)
    }

    async fn invalidate_list_cache(&self, org_id: &str) {
        if org_id == crate::official_org_slug() {
            // 官方资源对所有 org 可见，清除全部 list 缓存
            let _ = self.cache.del_pattern("agents:list:*").await;
        } else {
            let keys = self
                .cache
                .scan_keys(&format!("agents:list:{}:*", org_id))
                .await;
            for key in keys {
                let _ = self.cache.del(&key).await;
            }
        }
    }
}
