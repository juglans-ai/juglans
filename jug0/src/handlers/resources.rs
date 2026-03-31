// src/handlers/resources.rs
//
// Unified resource lookup handler for GitHub-style /:owner/:slug pattern
// Searches prompts -> agents -> workflows in order

use axum::{
    extract::{Extension, Path},
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::sync::Arc;

use crate::auth::OptionalAuthUser;
use crate::entities::{agents, prompts, users, workflows};
use crate::errors::AppError;
use crate::response::{
    OwnerInfo, PublicUserProfile, ResourceAgent, ResourcePrompt, ResourceResponse, ResourceWorkflow,
};
use crate::AppState;

/// Check if user has access to a prompt
fn can_access_prompt(prompt: &prompts::Model, user: &Option<crate::auth::AuthUser>) -> bool {
    // Public prompts are accessible to everyone
    if prompt.is_public {
        return true;
    }

    // Private prompts require authentication and ownership
    if let Some(u) = user {
        // Owner can access their own prompts
        if prompt.user_id == Some(u.id) {
            return true;
        }
        // Same org can access org prompts
        if prompt.org_id.as_ref() == Some(&u.org_id) && prompt.is_public {
            return true;
        }
    }

    false
}

/// Check if user has access to an agent
fn can_access_agent(agent: &agents::Model, user: &Option<crate::auth::AuthUser>) -> bool {
    // Public agents are accessible to everyone
    if agent.is_public == Some(true) {
        return true;
    }

    // Private agents require authentication and ownership
    if let Some(u) = user {
        // Owner can access their own agents
        if agent.user_id == Some(u.id) {
            return true;
        }
        // Same org members can access org agents if public
        if agent.org_id.as_ref() == Some(&u.org_id) && agent.is_public == Some(true) {
            return true;
        }
    }

    false
}

/// Check if user has access to a workflow
fn can_access_workflow(workflow: &workflows::Model, user: &Option<crate::auth::AuthUser>) -> bool {
    // Public workflows are accessible to everyone
    if workflow.is_public == Some(true) {
        return true;
    }

    // Private workflows require authentication and ownership
    if let Some(u) = user {
        // Owner can access their own workflows
        if workflow.user_id == Some(u.id) {
            return true;
        }
        // Same org members can access org workflows if public
        if workflow.org_id.as_ref() == Some(&u.org_id) && workflow.is_public == Some(true) {
            return true;
        }
    }

    false
}

/// GET /api/r/:owner/:slug
///
/// Unified resource lookup by owner username and slug.
/// Returns ALL matching resources (prompts, agents, workflows) with the same slug.
pub async fn get_resource_by_owner_slug(
    Extension(state): Extension<Arc<AppState>>,
    user: OptionalAuthUser,
    Path((owner, slug)): Path<(String, String)>,
) -> Result<Json<Vec<ResourceResponse>>, AppError> {
    // 1. Look up owner by username
    let owner_user = users::Entity::find()
        .filter(users::Column::Username.eq(&owner))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User '{}' not found", owner)))?;

    let owner_info = OwnerInfo {
        id: owner_user.id,
        username: owner_user.username.clone(),
        name: owner_user.name.clone(),
    };

    let mut resources = Vec::new();

    // 2. Check for prompts
    if let Some(prompt) = prompts::Entity::find()
        .filter(prompts::Column::UserId.eq(owner_user.id))
        .filter(prompts::Column::Slug.eq(&slug))
        .one(&state.db)
        .await?
    {
        if can_access_prompt(&prompt, &user.0) {
            resources.push(ResourceResponse::Prompt(ResourcePrompt {
                url: format!("/{}/{}", owner, prompt.slug),
                prompt,
                owner: owner_info.clone(),
            }));
        }
    }

    // 3. Check for agents
    if let Some(agent) = agents::Entity::find()
        .filter(agents::Column::UserId.eq(owner_user.id))
        .filter(agents::Column::Slug.eq(&slug))
        .one(&state.db)
        .await?
    {
        if can_access_agent(&agent, &user.0) {
            // Fetch associated system prompt if exists
            let system_prompt = if let Some(sp_id) = agent.system_prompt_id {
                prompts::Entity::find_by_id(sp_id).one(&state.db).await?
            } else {
                None
            };

            resources.push(ResourceResponse::Agent(ResourceAgent {
                url: format!("/{}/{}", owner, agent.slug),
                agent,
                owner: owner_info.clone(),
                system_prompt,
            }));
        }
    }

    // 4. Check for workflows
    if let Some(workflow) = workflows::Entity::find()
        .filter(workflows::Column::UserId.eq(owner_user.id))
        .filter(workflows::Column::Slug.eq(&slug))
        .one(&state.db)
        .await?
    {
        if can_access_workflow(&workflow, &user.0) {
            resources.push(ResourceResponse::Workflow(ResourceWorkflow {
                url: format!("/{}/{}", owner, workflow.slug),
                workflow,
                owner: owner_info.clone(),
            }));
        }
    }

    // 5. Return all found resources, or error if none
    if resources.is_empty() {
        Err(AppError::NotFound(format!(
            "Resource '{}' not found for user '{}' or access denied",
            slug, owner
        )))
    } else {
        Ok(Json(resources))
    }
}

/// GET /api/users/by-username/:username
///
/// Look up user by username (public profile).
pub async fn get_user_by_username(
    Extension(state): Extension<Arc<AppState>>,
    Path(username): Path<String>,
) -> Result<Json<PublicUserProfile>, AppError> {
    let user = users::Entity::find()
        .filter(users::Column::Username.eq(&username))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User '{}' not found", username)))?;

    Ok(Json(PublicUserProfile {
        id: user.id,
        username: user.username.unwrap_or_default(),
        name: user.name,
    }))
}
