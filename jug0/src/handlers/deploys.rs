// src/handlers/deploys.rs
//
// Deploy API: CRUD + background build & deploy to Alibaba Cloud FC

use axum::{
    extract::{Extension, Path},
    Json as AxumJson,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::entities::deploys;
use crate::errors::AppError;
use crate::request::deploys::{CreateDeployRequest, UpdateDeployRequest};
use crate::services::deploy::{self, FcConfig};
use crate::AppState;

/// GET /api/deploys — list user's deployments
pub async fn list_deploys(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
) -> Result<AxumJson<Vec<deploys::Model>>, AppError> {
    let deploys = deploys::Entity::find()
        .filter(deploys::Column::OrgId.eq(&user.org_id))
        .filter(deploys::Column::UserId.eq(user.id))
        .order_by_desc(deploys::Column::CreatedAt)
        .all(&state.db)
        .await?;

    Ok(AxumJson(deploys))
}

/// GET /api/deploys/:slug — get deployment by slug
pub async fn get_deploy(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<AxumJson<deploys::Model>, AppError> {
    let deploy = find_deploy(&state, &user, &slug).await?;
    Ok(AxumJson(deploy))
}

/// POST /api/deploys — create + trigger deployment
pub async fn create_deploy(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    AxumJson(req): AxumJson<CreateDeployRequest>,
) -> Result<AxumJson<deploys::Model>, AppError> {
    // 验证 slug
    deploy::validate_slug(&req.slug).map_err(AppError::BadRequest)?;

    // 检查 slug 唯一性
    let existing = deploys::Entity::find()
        .filter(deploys::Column::Slug.eq(&req.slug))
        .one(&state.db)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict(format!(
            "Deploy slug '{}' already taken",
            req.slug
        )));
    }

    let fc_config = FcConfig::from_env()
        .ok_or_else(|| AppError::BadRequest("FC deployment not configured on server".into()))?;

    let deploy_url = fc_config.deploy_url(&req.slug);

    let new_deploy = deploys::ActiveModel {
        id: Set(Uuid::new_v4()),
        slug: Set(req.slug.clone()),
        org_id: Set(Some(user.org_id.clone())),
        user_id: Set(Some(user.id)),
        repo: Set(req.repo.clone()),
        branch: Set(Some(req.branch.clone().unwrap_or_else(|| "main".into()))),
        status: Set("pending".into()),
        url: Set(Some(deploy_url)),
        image_uri: Set(None),
        env: Set(json!(req.env)),
        error_message: Set(None),
        ..Default::default()
    };

    let saved = new_deploy.insert(&state.db).await?;
    info!("[Deploy] Created: {} (repo: {})", saved.slug, saved.repo);

    // 触发后台构建
    spawn_build(state, saved.clone(), fc_config, req.env);

    Ok(AxumJson(saved))
}

/// PATCH /api/deploys/:slug — update env/branch (does NOT redeploy)
pub async fn update_deploy(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(slug): Path<String>,
    AxumJson(req): AxumJson<UpdateDeployRequest>,
) -> Result<AxumJson<deploys::Model>, AppError> {
    let deploy = find_deploy(&state, &user, &slug).await?;

    let mut active: deploys::ActiveModel = deploy.into();
    if let Some(branch) = req.branch {
        active.branch = Set(Some(branch));
    }
    if let Some(env) = req.env {
        active.env = Set(json!(env));
    }
    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));

    let updated = active.update(&state.db).await?;
    Ok(AxumJson(updated))
}

/// DELETE /api/deploys/:slug — delete deployment + FC function
pub async fn delete_deploy(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let deploy = find_deploy(&state, &user, &slug).await?;
    let deploy_slug = deploy.slug.clone();

    // 删除 FC 函数 + 解绑域名 (后台)
    let http_client = state.http_client.clone();
    let slug_for_fc = deploy_slug.clone();
    tokio::spawn(async move {
        if let Some(fc_config) = FcConfig::from_env() {
            let _ = deploy::unbind_custom_domain(&http_client, &fc_config, &slug_for_fc).await;
            if let Err(e) = deploy::delete_fc_function(&http_client, &fc_config, &slug_for_fc).await
            {
                error!(
                    "[Deploy] Failed to delete FC function {}: {}",
                    slug_for_fc, e
                );
            }
        }
    });

    // 标记为 deleted
    let mut active: deploys::ActiveModel = deploy.into();
    active.status = Set("deleted".into());
    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
    active.update(&state.db).await?;

    Ok(AxumJson(json!({ "success": true, "slug": deploy_slug })))
}

/// POST /api/deploys/:slug/redeploy — rebuild + redeploy
pub async fn redeploy(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<AxumJson<deploys::Model>, AppError> {
    let deploy = find_deploy(&state, &user, &slug).await?;

    let fc_config = FcConfig::from_env()
        .ok_or_else(|| AppError::BadRequest("FC deployment not configured on server".into()))?;

    // 重置状态
    let mut active: deploys::ActiveModel = deploy.clone().into();
    active.status = Set("pending".into());
    active.error_message = Set(None);
    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
    let updated = active.update(&state.db).await?;

    // 解析 env
    let env: HashMap<String, String> =
        serde_json::from_value(deploy.env.clone()).unwrap_or_default();

    spawn_build(state, updated.clone(), fc_config, env);

    Ok(AxumJson(updated))
}

/// POST /api/deploys/:slug/domain — bind custom domain {slug}.juglans.app
pub async fn bind_domain(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let deploy = find_deploy(&state, &user, &slug).await?;

    let fc_config = FcConfig::from_env()
        .ok_or_else(|| AppError::BadRequest("FC deployment not configured on server".into()))?;

    let domain_name = format!("{}.{}", deploy.slug, fc_config.base_domain);

    deploy::bind_custom_domain(&state.http_client, &fc_config, &deploy.slug)
        .await
        .map_err(|e| AppError::BadRequest(format!("Domain binding failed: {}", e)))?;

    // Update deploy URL to custom domain
    let mut active: deploys::ActiveModel = deploy.into();
    active.url = Set(Some(format!("https://{}", domain_name)));
    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
    active.update(&state.db).await?;

    info!("[Deploy:{}] Domain bound: {}", slug, domain_name);

    Ok(AxumJson(json!({
        "success": true,
        "domain": domain_name,
    })))
}

/// DELETE /api/deploys/:slug/domain — unbind custom domain
pub async fn unbind_domain(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(slug): Path<String>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    let deploy = find_deploy(&state, &user, &slug).await?;

    let fc_config = FcConfig::from_env()
        .ok_or_else(|| AppError::BadRequest("FC deployment not configured on server".into()))?;

    deploy::unbind_custom_domain(&state.http_client, &fc_config, &deploy.slug)
        .await
        .map_err(|e| AppError::BadRequest(format!("Domain unbinding failed: {}", e)))?;

    // Revert URL to trigger URL if available
    let trigger_url = deploy::get_trigger_url(&state.http_client, &fc_config, &deploy.slug).await;
    let fallback_url = trigger_url.unwrap_or_else(|_| None).unwrap_or_else(|| fc_config.deploy_url(&deploy.slug));

    let mut active: deploys::ActiveModel = deploy.into();
    active.url = Set(Some(fallback_url));
    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
    active.update(&state.db).await?;

    info!("[Deploy:{}] Domain unbound", slug);

    Ok(AxumJson(json!({ "success": true })))
}

// ---- helpers ----

async fn find_deploy(
    state: &Arc<AppState>,
    user: &AuthUser,
    slug: &str,
) -> Result<deploys::Model, AppError> {
    // 尝试 UUID 或 slug
    let deploy = if let Ok(id) = Uuid::parse_str(slug) {
        deploys::Entity::find_by_id(id)
            .filter(deploys::Column::OrgId.eq(&user.org_id))
            .one(&state.db)
            .await?
    } else {
        deploys::Entity::find()
            .filter(deploys::Column::Slug.eq(slug))
            .filter(deploys::Column::OrgId.eq(&user.org_id))
            .one(&state.db)
            .await?
    };

    deploy.ok_or_else(|| AppError::NotFound(format!("Deploy '{}' not found", slug)))
}

fn spawn_build(
    state: Arc<AppState>,
    deploy: deploys::Model,
    fc_config: FcConfig,
    env_vars: HashMap<String, String>,
) {
    let slug = deploy.slug.clone();
    let repo = deploy.repo.clone();
    let branch = deploy.branch.clone().unwrap_or_else(|| "main".into());
    let http_client = state.http_client.clone();
    let db = state.db.clone();

    tokio::spawn(async move {
        // 1. Building
        update_status(&db, &slug, "building", None).await;

        let jug0_base_url =
            std::env::var("JUG0_PUBLIC_URL").unwrap_or_else(|_| "https://api.jug0.com".into());
        let api_key = std::env::var("DEPLOY_API_KEY").unwrap_or_default();

        let build_result = deploy::build_and_push(
            &http_client,
            &fc_config,
            &deploy::BuildParams {
                slug: &slug,
                repo: &repo,
                branch: &branch,
                env_vars: &env_vars,
                jug0_base_url: &jug0_base_url,
                api_key: &api_key,
            },
        )
        .await;

        let image_uri = match build_result {
            Ok(uri) => uri,
            Err(e) => {
                error!("[Deploy:{}] Build failed: {}", slug, e);
                update_status(&db, &slug, "failed", Some(&e.to_string())).await;
                return;
            }
        };

        // 2. Deploying
        update_status(&db, &slug, "deploying", None).await;

        match deploy::create_or_update_fc_function(
            &http_client,
            &fc_config,
            &slug,
            &image_uri,
            &env_vars,
        )
        .await
        {
            Ok(url) => {
                // 3. Create HTTP trigger for public URL access
                let trigger_url = match deploy::ensure_http_trigger(&http_client, &fc_config, &slug).await {
                    Ok(Some(u)) => {
                        info!("[Deploy:{}] Trigger URL: {}", slug, u);
                        Some(u)
                    }
                    Ok(None) => {
                        info!("[Deploy:{}] HTTP trigger created but no URL returned", slug);
                        None
                    }
                    Err(e) => {
                        error!("[Deploy:{}] HTTP trigger creation failed (non-fatal): {}", slug, e);
                        None
                    }
                };

                // Use trigger URL if available, otherwise fall back to custom domain URL
                let final_url = trigger_url.unwrap_or(url);
                info!("[Deploy:{}] Deployed: {}", slug, final_url);

                if let Ok(Some(model)) = deploys::Entity::find()
                    .filter(deploys::Column::Slug.eq(&slug))
                    .one(&db)
                    .await
                {
                    let mut active: deploys::ActiveModel = model.into();
                    active.status = Set("deployed".into());
                    active.image_uri = Set(Some(image_uri));
                    active.url = Set(Some(final_url));
                    active.error_message = Set(None);
                    active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
                    let _ = active.update(&db).await;
                }
            }
            Err(e) => {
                error!("[Deploy:{}] FC deploy failed: {}", slug, e);
                update_status(&db, &slug, "failed", Some(&e.to_string())).await;
            }
        }
    });
}

async fn update_status(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    status: &str,
    error_msg: Option<&str>,
) {
    if let Ok(Some(model)) = deploys::Entity::find()
        .filter(deploys::Column::Slug.eq(slug))
        .one(db)
        .await
    {
        let mut active: deploys::ActiveModel = model.into();
        active.status = Set(status.into());
        if let Some(msg) = error_msg {
            active.error_message = Set(Some(msg.into()));
        }
        active.updated_at = Set(Some(chrono::Utc::now().naive_utc()));
        let _ = active.update(db).await;
    }
}
