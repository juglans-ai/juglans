// src/repositories/prompts.rs
//
// Prompt repository with cache + DB fallback

use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, IntoActiveModel,
    QueryFilter, QueryOrder, Set,
};
use std::collections::HashMap;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::entities::{agents, prompts, users};
use crate::errors::AppError;
use crate::request::{CreatePromptRequest, PromptFilter, RenderPromptRequest, UpdatePromptRequest};
use crate::response::{OwnerInfo, PromptWithOwner, RenderPromptResponse};
use crate::services::cache::CacheService;

#[derive(Clone)]
pub struct PromptRepository {
    db: DatabaseConnection,
    cache: CacheService,
}

impl PromptRepository {
    pub fn new(db: DatabaseConnection, cache: CacheService) -> Self {
        Self { db, cache }
    }

    /// List all prompts visible to the user
    pub async fn list_for_user(
        &self,
        user: &AuthUser,
        filter: &PromptFilter,
    ) -> Result<Vec<PromptWithOwner>, AppError> {
        let public_only = filter.public_only.unwrap_or(false);

        // Skip cache if search is provided
        if filter.search.is_none() {
            let cache_key = format!("prompts:list:{}:{}:{}", user.org_id, user.id, public_only);
            if let Some(cached) = self.cache.get::<Vec<PromptWithOwner>>(&cache_key).await {
                return Ok(cached);
            }
        }

        // Build query
        let mut query = prompts::Entity::find();
        let mut condition = Condition::any();

        // Own prompts (public + private)
        condition = condition.add(
            Condition::all()
                .add(prompts::Column::OrgId.eq(&user.org_id))
                .add(prompts::Column::UserId.eq(user.id)),
        );

        // Same org public prompts (if requested)
        if public_only {
            condition = condition.add(
                Condition::all()
                    .add(prompts::Column::OrgId.eq(&user.org_id))
                    .add(prompts::Column::IsPublic.eq(true)),
            );
        }

        // Official public prompts
        condition = condition.add(
            Condition::all()
                .add(prompts::Column::OrgId.eq(crate::official_org_slug()))
                .add(prompts::Column::IsPublic.eq(true)),
        );

        query = query.filter(condition);

        // Apply search filter
        if let Some(ref search) = filter.search {
            let pattern = format!("%{}%", search);
            query = query.filter(
                Condition::any()
                    .add(prompts::Column::Name.contains(&pattern))
                    .add(prompts::Column::Slug.contains(&pattern)),
            );
        }

        query = query.order_by_desc(prompts::Column::UsageCount);

        let prompts_list = query.all(&self.db).await?;

        // Build PromptWithOwner
        let result = self.build_prompts_with_owners(prompts_list).await?;

        // Cache if no search
        if filter.search.is_none() {
            let cache_key = format!("prompts:list:{}:{}:{}", user.org_id, user.id, public_only);
            let _ = self.cache.set(&cache_key, &result, 300).await;
        }

        Ok(result)
    }

    /// Get prompt by ID or slug
    pub async fn get_by_id_or_slug(
        &self,
        user: &AuthUser,
        id_or_slug: &str,
    ) -> Result<prompts::Model, AppError> {
        let mut query = prompts::Entity::find();

        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            query = query.filter(prompts::Column::Id.eq(id));
        } else {
            query = query.filter(prompts::Column::Slug.eq(id_or_slug));
        }

        // Access control
        query = query.filter(
            Condition::any()
                // Own prompts
                .add(
                    Condition::all()
                        .add(prompts::Column::OrgId.eq(&user.org_id))
                        .add(prompts::Column::UserId.eq(user.id)),
                )
                // Same org public
                .add(
                    Condition::all()
                        .add(prompts::Column::OrgId.eq(&user.org_id))
                        .add(prompts::Column::IsPublic.eq(true)),
                )
                // Official public
                .add(
                    Condition::all()
                        .add(prompts::Column::OrgId.eq(crate::official_org_slug()))
                        .add(prompts::Column::IsPublic.eq(true)),
                ),
        );

        query
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Prompt '{}' not found", id_or_slug)))
    }

    /// Create prompt
    pub async fn create(
        &self,
        user: &AuthUser,
        req: CreatePromptRequest,
    ) -> Result<prompts::Model, AppError> {
        // Check slug uniqueness
        let exists = prompts::Entity::find()
            .filter(prompts::Column::OrgId.eq(&user.org_id))
            .filter(prompts::Column::UserId.eq(user.id))
            .filter(prompts::Column::Slug.eq(&req.slug))
            .one(&self.db)
            .await?;

        if exists.is_some() {
            return Err(AppError::BadRequest(
                "Slug already exists for this user".to_string(),
            ));
        }

        let is_system = req.is_system.unwrap_or(false);
        if is_system && user.role != "admin" {
            return Err(AppError::Unauthorized(
                "Only admins can create system prompts".to_string(),
            ));
        }

        let new_prompt = prompts::ActiveModel {
            id: Set(Uuid::new_v4()),
            org_id: Set(Some(user.org_id.clone())),
            user_id: Set(Some(user.id)),
            slug: Set(req.slug),
            name: Set(req.name.or(Some("Untitled".to_string()))),
            content: Set(req.content),
            tags: Set(req.tags.map(|t| serde_json::json!(t))),
            is_public: Set(req.is_public.unwrap_or(false)),
            is_system: Set(is_system),
            usage_count: Set(0),
            ..Default::default()
        };

        let saved = new_prompt.insert(&self.db).await?;

        // Invalidate list cache
        self.invalidate_list_cache(&user.org_id).await;

        Ok(saved)
    }

    /// Update prompt
    pub async fn update(
        &self,
        user: &AuthUser,
        id_or_slug: &str,
        req: UpdatePromptRequest,
    ) -> Result<prompts::Model, AppError> {
        let mut query = prompts::Entity::find();
        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            query = query.filter(prompts::Column::Id.eq(id));
        } else {
            query = query.filter(prompts::Column::Slug.eq(id_or_slug));
        }

        let prompt = query
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Prompt not found".to_string()))?;

        // Permission check
        let is_owner =
            prompt.org_id.as_deref() == Some(&user.org_id) && prompt.user_id == Some(user.id);
        let is_system_admin = user.role == "admin" && prompt.is_system;

        if !is_owner && !is_system_admin {
            return Err(AppError::Unauthorized("Access denied".to_string()));
        }

        let mut active = prompt.into_active_model();

        if let Some(v) = req.name {
            active.name = Set(Some(v));
        }
        if let Some(v) = req.content {
            active.content = Set(v);
        }
        if let Some(v) = req.tags {
            active.tags = Set(Some(serde_json::json!(v)));
        }
        if let Some(v) = req.is_public {
            active.is_public = Set(v);
        }

        active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));

        let updated = active.update(&self.db).await?;

        // Invalidate cache
        self.invalidate_list_cache(&user.org_id).await;
        self.invalidate_agent_caches_for_prompt(updated.id).await;

        Ok(updated)
    }

    /// Delete prompt
    pub async fn delete(&self, user: &AuthUser, id_or_slug: &str) -> Result<Uuid, AppError> {
        let mut query = prompts::Entity::find();
        if let Ok(id) = Uuid::parse_str(id_or_slug) {
            query = query.filter(prompts::Column::Id.eq(id));
        } else {
            query = query.filter(prompts::Column::Slug.eq(id_or_slug));
        }

        let prompt = query
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound("Prompt not found".to_string()))?;

        // Permission check
        let is_owner =
            prompt.org_id.as_deref() == Some(&user.org_id) && prompt.user_id == Some(user.id);
        let is_system_admin = user.role == "admin" && prompt.is_system;

        if !is_owner && !is_system_admin {
            return Err(AppError::Unauthorized("Access denied".to_string()));
        }

        let prompt_id = prompt.id;
        prompts::Entity::delete_by_id(prompt_id)
            .exec(&self.db)
            .await?;

        // Invalidate cache
        self.invalidate_list_cache(&user.org_id).await;
        self.invalidate_agent_caches_for_prompt(prompt_id).await;

        Ok(prompt_id)
    }

    /// Render prompt template
    pub async fn render(
        &self,
        user: &AuthUser,
        id_or_slug: &str,
        req: RenderPromptRequest,
    ) -> Result<RenderPromptResponse, AppError> {
        let prompt = self.get_by_id_or_slug(user, id_or_slug).await?;

        let original = prompt.content.clone();
        let variables = req.variables.unwrap_or_default();

        let mut rendered = original.clone();
        let mut variables_used = Vec::new();

        for (var_name, var_value) in &variables {
            let pattern = format!("{{{{{}}}}}", var_name);
            if rendered.contains(&pattern) {
                rendered = rendered.replace(&pattern, var_value);
                variables_used.push(var_name.clone());
            }
        }

        Ok(RenderPromptResponse {
            rendered,
            original,
            variables_used,
        })
    }

    // --- Helper methods ---

    async fn build_prompts_with_owners(
        &self,
        prompts_list: Vec<prompts::Model>,
    ) -> Result<Vec<PromptWithOwner>, AppError> {
        if prompts_list.is_empty() {
            return Ok(vec![]);
        }

        // Batch fetch users
        let user_ids: Vec<Uuid> = prompts_list
            .iter()
            .filter_map(|p| p.user_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let users_map: HashMap<Uuid, users::Model> = if !user_ids.is_empty() {
            users::Entity::find()
                .filter(users::Column::Id.is_in(user_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|u| (u.id, u))
                .collect()
        } else {
            HashMap::new()
        };

        // Build result
        let result = prompts_list
            .into_iter()
            .map(|prompt| {
                let owner = prompt
                    .user_id
                    .and_then(|uid| users_map.get(&uid))
                    .map(|u| OwnerInfo {
                        id: u.id,
                        username: u.username.clone(),
                        name: u.name.clone(),
                    });

                let username = owner
                    .as_ref()
                    .and_then(|o| o.username.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                PromptWithOwner {
                    url: format!("/{}/{}", username, prompt.slug),
                    prompt,
                    owner,
                }
            })
            .collect();

        Ok(result)
    }

    async fn invalidate_list_cache(&self, org_id: &str) {
        if org_id == crate::official_org_slug() {
            // 官方资源对所有 org 可见，清除全部 list 缓存
            let _ = self.cache.del_pattern("prompts:list:*").await;
        } else {
            let keys = self
                .cache
                .scan_keys(&format!("prompts:list:{}:*", org_id))
                .await;
            for key in keys {
                let _ = self.cache.del(&key).await;
            }
        }
    }

    /// 清除引用该 prompt 作为 system_prompt 的 agent 缓存
    async fn invalidate_agent_caches_for_prompt(&self, prompt_id: Uuid) {
        let referencing_agents = agents::Entity::find()
            .filter(agents::Column::SystemPromptId.eq(prompt_id))
            .all(&self.db)
            .await
            .unwrap_or_default();
        for a in referencing_agents {
            let _ = self.cache.del(&format!("agents:detail:{}", a.id)).await;
        }
        // agent list 也包含 system_prompt_content，一并清除
        let _ = self.cache.del_pattern("agents:list:*").await;
    }
}
