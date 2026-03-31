// src/handlers/workflows.rs
use axum::{
    extract::{Extension, Json, Path},
    Json as AxumJson,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use chrono::Utc;

use crate::auth::AuthUser;
use crate::entities::{users, workflow_runs, workflows};
use crate::errors::AppError;
use crate::request::{CreateWorkflowRequest, ExecuteWorkflowRequest, UpdateWorkflowRequest};
use crate::response::{ExecuteWorkflowResponse, OwnerInfo, WorkflowRunResponse, WorkflowWithOwner};
use crate::AppState;

// --- Helper: build workflow list from DB ---

async fn build_workflow_list(
    state: &AppState,
    user: &AuthUser,
) -> Result<serde_json::Value, AppError> {
    let results = workflows::Entity::find()
        .filter(
            Condition::any()
                // 1. 本机构下，属于自己的（包括私有和公开）
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::UserId.eq(user.id)),
                )
                // 2. 本机构下其他人公开的
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::IsPublic.eq(true)),
                )
                // 3. 官方市场的公开 Workflow
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(crate::official_org_slug()))
                        .add(workflows::Column::IsPublic.eq(true)),
                ),
        )
        .order_by_desc(workflows::Column::CreatedAt)
        .all(&state.db)
        .await?;

    // Batch fetch all users (fix N+1 query)
    let user_ids: Vec<uuid::Uuid> = results
        .iter()
        .filter_map(|w| w.user_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let users_map: std::collections::HashMap<uuid::Uuid, users::Model> = if !user_ids.is_empty() {
        users::Entity::find()
            .filter(users::Column::Id.is_in(user_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|u| (u.id, u))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    // Build response with owner info from map (no N+1 queries)
    let mut workflows_with_owners = Vec::new();
    for wf in results {
        let owner = wf
            .user_id
            .and_then(|uid| users_map.get(&uid))
            .map(|u| OwnerInfo {
                id: u.id,
                username: u.username.clone(),
                name: u.name.clone(),
            });

        let url = owner
            .as_ref()
            .and_then(|o| o.username.as_ref().map(|u| format!("/{}/{}", u, wf.slug)));

        workflows_with_owners.push(WorkflowWithOwner {
            workflow: wf,
            owner,
            url,
        });
    }

    serde_json::to_value(&workflows_with_owners)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Serialization error: {}", e)))
}

// --- Rebuild all workflow caches (called async after mutations) ---

async fn rebuild_all_workflow_caches(state: &Arc<AppState>) {
    let keys = state.cache.scan_keys("jug0:list:workflows:*").await;
    for key in keys {
        // key = "jug0:list:workflows:{org_id}:{user_id}"
        let parts: Vec<&str> = key.split(':').collect();
        if parts.len() == 5 {
            let org_id = parts[3];
            let user_id = match Uuid::parse_str(parts[4]) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let user = AuthUser {
                id: user_id,
                org_id: org_id.to_string(),
                external_id: None,
                name: None,
                role: "user".to_string(),
                is_api_key: false,
            };
            match build_workflow_list(state, &user).await {
                Ok(val) => {
                    let _ = state.cache.set(&key, &val, 300).await;
                }
                Err(_) => {
                    let _ = state.cache.del(&key).await;
                }
            }
        }
    }
}

// --- Handlers ---

/// GET /api/workflows
pub async fn list_workflows(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    // Redis cache lookup
    let redis_key = format!("jug0:list:workflows:{}:{}", user.org_id, user.id);
    if let Some(cached) = state.cache.get::<serde_json::Value>(&redis_key).await {
        return Ok(AxumJson(cached));
    }

    let json_value = build_workflow_list(&state, &user).await?;
    let _ = state.cache.set(&redis_key, &json_value, 300).await;

    Ok(AxumJson(json_value))
}

/// GET /api/workflows/:id_or_slug
pub async fn get_workflow(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_str): Path<String>,
) -> Result<AxumJson<workflows::Model>, AppError> {
    let mut query = workflows::Entity::find();

    if let Ok(id) = Uuid::parse_str(&id_str) {
        query = query.filter(workflows::Column::Id.eq(id));
    } else {
        query = query.filter(workflows::Column::Slug.eq(&id_str));
    }

    query = query.filter(
        Condition::any()
            // 1. Own workflows in same org
            .add(
                Condition::all()
                    .add(workflows::Column::OrgId.eq(&user.org_id))
                    .add(workflows::Column::UserId.eq(user.id)),
            )
            // 2. Public workflows in same org
            .add(
                Condition::all()
                    .add(workflows::Column::OrgId.eq(&user.org_id))
                    .add(workflows::Column::IsPublic.eq(true)),
            )
            // 3. Official public workflows
            .add(
                Condition::all()
                    .add(workflows::Column::OrgId.eq(crate::official_org_slug()))
                    .add(workflows::Column::IsPublic.eq(true)),
            ),
    );

    let workflow = query.one(&state.db).await?.ok_or_else(|| {
        AppError::NotFound(format!("Workflow '{}' not found or access denied", id_str))
    })?;

    Ok(AxumJson(workflow))
}

/// POST /api/workflows
pub async fn create_workflow(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreateWorkflowRequest>,
) -> Result<AxumJson<workflows::Model>, AppError> {
    // 检查 slug 唯一性（每个用户下唯一）
    let slug_exists = workflows::Entity::find()
        .filter(workflows::Column::OrgId.eq(&user.org_id))
        .filter(workflows::Column::UserId.eq(user.id))
        .filter(workflows::Column::Slug.eq(&req.slug))
        .one(&state.db)
        .await?;

    if slug_exists.is_some() {
        return Err(AppError::BadRequest(format!(
            "Workflow slug '{}' already exists for your account",
            req.slug
        )));
    }

    let new_workflow = workflows::ActiveModel {
        id: Set(Uuid::new_v4()),
        slug: Set(req.slug),
        name: Set(req.name),
        description: Set(req.description),
        endpoint_url: Set(req.endpoint_url),
        definition: Set(req.definition),
        org_id: Set(Some(user.org_id)),
        user_id: Set(Some(user.id)),
        trigger_config: Set(req.trigger_config.clone()),
        is_active: Set(req.is_active.or(Some(true))),
        is_public: Set(req.is_public.or(Some(false))), // Default to private
        ..Default::default()
    };

    let saved = new_workflow.insert(&state.db).await?;
    let s = state.clone();
    tokio::spawn(async move { rebuild_all_workflow_caches(&s).await });
    // Notify cron scheduler to reload if trigger_config was set
    if req.trigger_config.is_some() {
        let _ = state.scheduler_reload.send(());
    }
    Ok(AxumJson(saved))
}

/// PATCH /api/workflows/:id_or_slug
pub async fn update_workflow(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_str): Path<String>,
    Json(req): Json<UpdateWorkflowRequest>,
) -> Result<AxumJson<workflows::Model>, AppError> {
    let mut query = workflows::Entity::find();

    if let Ok(id) = Uuid::parse_str(&id_str) {
        query = query.filter(workflows::Column::Id.eq(id));
    } else {
        query = query.filter(workflows::Column::Slug.eq(&id_str));
    }

    // 只能修改自己的
    query = query
        .filter(workflows::Column::OrgId.eq(&user.org_id))
        .filter(workflows::Column::UserId.eq(user.id));

    let workflow = query.one(&state.db).await?.ok_or(AppError::NotFound(
        "Workflow not found or you do not have permission to edit it".to_string(),
    ))?;

    let mut active = workflow.into_active_model();

    if let Some(v) = req.name {
        active.name = Set(Some(v));
    }
    if let Some(v) = req.description {
        active.description = Set(Some(v));
    }
    if let Some(v) = req.endpoint_url {
        active.endpoint_url = Set(Some(v));
    }
    if let Some(v) = req.definition {
        active.definition = Set(Some(v));
    }
    let has_trigger_config_change = req.trigger_config.is_some();
    if let Some(v) = req.trigger_config {
        active.trigger_config = Set(Some(v));
    }
    if let Some(v) = req.is_active {
        active.is_active = Set(Some(v));
    }
    if let Some(v) = req.is_public {
        active.is_public = Set(Some(v));
    }

    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));

    let updated = active.update(&state.db).await?;
    let s = state.clone();
    tokio::spawn(async move { rebuild_all_workflow_caches(&s).await });
    // Notify cron scheduler to reload if trigger_config changed
    if has_trigger_config_change {
        let _ = state.scheduler_reload.send(());
    }
    Ok(AxumJson(updated))
}

/// DELETE /api/workflows/:id_or_slug
pub async fn delete_workflow(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id_str): Path<String>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let mut query = workflows::Entity::find();

    if let Ok(id) = Uuid::parse_str(&id_str) {
        query = query.filter(workflows::Column::Id.eq(id));
    } else {
        query = query.filter(workflows::Column::Slug.eq(&id_str));
    }

    // 只能删除自己的
    query = query
        .filter(workflows::Column::OrgId.eq(&user.org_id))
        .filter(workflows::Column::UserId.eq(user.id));

    let workflow = query.one(&state.db).await?.ok_or(AppError::NotFound(
        "Workflow not found or you do not have permission to delete it".to_string(),
    ))?;

    let id = workflow.id;
    workflows::Entity::delete_by_id(id).exec(&state.db).await?;
    let s = state.clone();
    tokio::spawn(async move { rebuild_all_workflow_caches(&s).await });

    Ok(AxumJson(json!({ "success": true, "id": id })))
}

// --- Execute Workflow ---

/// POST /api/workflows/:id/execute
/// Execute a workflow with the given input
pub async fn execute_workflow(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ExecuteWorkflowRequest>,
) -> Result<AxumJson<ExecuteWorkflowResponse>, AppError> {
    // Find the workflow
    let workflow = workflows::Entity::find_by_id(id)
        .filter(
            Condition::any()
                // Own workflow
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::UserId.eq(user.id)),
                )
                // Public workflow in same org
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::IsPublic.eq(true)),
                )
                // Official public workflow
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(crate::official_org_slug()))
                        .add(workflows::Column::IsPublic.eq(true)),
                ),
        )
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound(
            "Workflow not found or you do not have permission to execute it".to_string(),
        ))?;

    // Check if workflow is active
    if workflow.is_active != Some(true) {
        return Err(AppError::BadRequest("Workflow is not active".to_string()));
    }

    // TODO: Implement actual workflow execution logic
    // For now, return a placeholder response indicating the workflow was triggered
    // In a full implementation, this would:
    // 1. Parse the workflow definition
    // 2. Execute nodes in order (DAG execution)
    // 3. Handle async execution with job queue
    // 4. Return execution ID for status polling

    Ok(AxumJson(ExecuteWorkflowResponse {
        workflow_id: workflow.id,
        status: "triggered".to_string(),
        message: format!(
            "Workflow '{}' execution triggered successfully",
            workflow.name.unwrap_or_else(|| workflow.slug.clone())
        ),
        result: Some(json!({
            "input": req.input,
            "variables": req.variables,
            "definition": workflow.definition,
        })),
    }))
}

// --- Workflow Runs ---

/// GET /api/workflows/:id/runs — list execution history for a workflow
pub async fn list_workflow_runs(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<AxumJson<Vec<WorkflowRunResponse>>, AppError> {
    // Verify user has access to this workflow
    let _workflow = workflows::Entity::find_by_id(id)
        .filter(
            Condition::any()
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::UserId.eq(user.id)),
                )
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::IsPublic.eq(true)),
                ),
        )
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound("Workflow not found".to_string()))?;

    let runs = workflow_runs::Entity::find()
        .filter(workflow_runs::Column::WorkflowId.eq(id))
        .order_by_desc(workflow_runs::Column::CreatedAt)
        .all(&state.db)
        .await?;

    let response: Vec<WorkflowRunResponse> = runs
        .into_iter()
        .map(|r| WorkflowRunResponse {
            id: r.id,
            workflow_id: r.workflow_id,
            trigger: r.trigger,
            status: r.status,
            started_at: r
                .started_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            completed_at: r
                .completed_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            result: r.result,
            error: r.error,
            created_at: r
                .created_at
                .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        })
        .collect();

    Ok(AxumJson(response))
}

/// POST /api/workflows/:id/trigger — manually trigger a workflow execution
pub async fn trigger_workflow(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<AxumJson<WorkflowRunResponse>, AppError> {
    let workflow = workflows::Entity::find_by_id(id)
        .filter(
            Condition::any()
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::UserId.eq(user.id)),
                )
                .add(
                    Condition::all()
                        .add(workflows::Column::OrgId.eq(&user.org_id))
                        .add(workflows::Column::IsPublic.eq(true)),
                ),
        )
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound("Workflow not found".to_string()))?;

    if workflow.is_active != Some(true) {
        return Err(AppError::BadRequest("Workflow is not active".to_string()));
    }

    let endpoint = workflow
        .endpoint_url
        .as_ref()
        .filter(|u| !u.is_empty())
        .ok_or(AppError::BadRequest(
            "Workflow has no endpoint_url configured".to_string(),
        ))?;

    let run_id = Uuid::new_v4();
    let now = Utc::now().naive_utc();

    // Create run record
    let run = workflow_runs::ActiveModel {
        id: Set(run_id),
        workflow_id: Set(id),
        trigger: Set("manual".to_string()),
        status: Set("running".to_string()),
        started_at: Set(Some(now)),
        ..Default::default()
    };
    run.insert(&state.db).await?;

    // POST to endpoint
    let payload = json!({
        "messages": [{
            "role": "user",
            "content": format!("[manual] Triggered workflow '{}'", workflow.slug)
        }],
        "stream": false,
    });

    let result = state
        .http_client
        .post(endpoint)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await;

    let completed_at = Utc::now().naive_utc();

    let (status, result_json, error) = match result {
        Ok(resp) => {
            let status_code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status_code.is_success() {
                let rj = serde_json::from_str::<serde_json::Value>(&body)
                    .unwrap_or(json!({"raw": body}));
                ("success".to_string(), Some(rj), None)
            } else {
                (
                    "failed".to_string(),
                    None,
                    Some(format!(
                        "HTTP {}: {}",
                        status_code,
                        &body[..body.len().min(500)]
                    )),
                )
            }
        }
        Err(e) => (
            "failed".to_string(),
            None,
            Some(format!("Request error: {}", e)),
        ),
    };

    // Update run record
    let update = workflow_runs::ActiveModel {
        id: Set(run_id),
        status: Set(status.clone()),
        completed_at: Set(Some(completed_at)),
        result: Set(result_json.clone()),
        error: Set(error.clone()),
        updated_at: Set(Some(completed_at)),
        ..Default::default()
    };
    let _ = update.update(&state.db).await;

    Ok(AxumJson(WorkflowRunResponse {
        id: run_id,
        workflow_id: id,
        trigger: "manual".to_string(),
        status,
        started_at: Some(now.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        completed_at: Some(completed_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        result: result_json,
        error,
        created_at: Some(now.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    }))
}
