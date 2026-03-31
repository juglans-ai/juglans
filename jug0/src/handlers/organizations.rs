// src/handlers/organizations.rs
use axum::{extract::State, http::HeaderMap, Json};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use std::sync::Arc;

use crate::auth::hash_key;
use crate::entities::organizations;
use crate::errors::AppError;
use crate::request::SetPublicKeyRequest;
use crate::response::{OrgInfoResponse, SetPublicKeyResponse};
use crate::AppState;

/// POST /api/organizations/public-key
/// 设置组织的公钥（用于验证 JWT）
///
/// 认证方式: X-ORG-ID + X-ORG-KEY headers
pub async fn set_public_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SetPublicKeyRequest>,
) -> Result<Json<SetPublicKeyResponse>, AppError> {
    // 1. 验证 ORG 认证
    let org_id = headers
        .get("X-ORG-ID")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("X-ORG-ID header required".into()))?;

    let org_key = headers
        .get("X-ORG-KEY")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("X-ORG-KEY header required".into()))?;

    // 2. 查询 organization
    let org = organizations::Entity::find_by_id(org_id)
        .one(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::Unauthorized(format!("Organization '{}' not found", org_id)))?;

    // 3. 验证 API Key
    let hashed_input = hash_key(org_key);
    if hashed_input != org.api_key_hash {
        return Err(AppError::Unauthorized("Invalid organization key".into()));
    }

    // 4. 验证公钥格式
    let public_key = payload.public_key.trim();
    if !public_key.starts_with("-----BEGIN PUBLIC KEY-----") {
        return Err(AppError::BadRequest(
            "Invalid public key format. Expected PEM format.".into(),
        ));
    }

    // 5. 验证算法
    let valid_algorithms = ["RS256", "RS384", "RS512", "ES256", "ES384", "EdDSA"];
    if !valid_algorithms.contains(&payload.key_algorithm.as_str()) {
        return Err(AppError::BadRequest(format!(
            "Invalid algorithm. Supported: {:?}",
            valid_algorithms
        )));
    }

    // 6. 更新数据库
    let mut active_model: organizations::ActiveModel = org.into();
    active_model.public_key = Set(Some(public_key.to_string()));
    active_model.key_algorithm = Set(Some(payload.key_algorithm.clone()));

    active_model
        .update(&state.db)
        .await
        .map_err(AppError::Database)?;

    tracing::info!("🔐 [Org] Public key updated for organization: {}", org_id);

    Ok(Json(SetPublicKeyResponse {
        success: true,
        org_id: org_id.to_string(),
        key_algorithm: payload.key_algorithm,
        message: "Public key configured successfully".to_string(),
    }))
}

/// GET /api/organizations/info
/// 获取组织信息（用于检查配置状态）
pub async fn get_org_info(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<OrgInfoResponse>, AppError> {
    let org_id = headers
        .get("X-ORG-ID")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("X-ORG-ID header required".into()))?;

    let org_key = headers
        .get("X-ORG-KEY")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("X-ORG-KEY header required".into()))?;

    let org = organizations::Entity::find_by_id(org_id)
        .one(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::Unauthorized(format!("Organization '{}' not found", org_id)))?;

    let hashed_input = hash_key(org_key);
    if hashed_input != org.api_key_hash {
        return Err(AppError::Unauthorized("Invalid organization key".into()));
    }

    Ok(Json(OrgInfoResponse {
        id: org.id,
        name: org.name,
        has_public_key: org.public_key.is_some(),
        key_algorithm: org.key_algorithm,
    }))
}
