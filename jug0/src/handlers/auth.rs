// src/handlers/auth.rs
use axum::{
    extract::{Extension, Json},
    Json as AxumJson,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::generate_token;
use crate::entities::users;
use crate::errors::AppError;
use crate::request::{LoginRequest, RegisterRequest};
use crate::response::{AuthResponse, MeResponse, UserDto};
use crate::AppState;

// POST /api/auth/login
pub async fn login(
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<AxumJson<AuthResponse>, AppError> {
    let user = users::Entity::find()
        .filter(users::Column::Email.eq(&req.email))
        .one(&state.db)
        .await?
        .ok_or(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ))?;

    // 验证密码
    let valid = if let Some(hash) = &user.password_hash {
        verify(&req.password, hash).unwrap_or(false)
    } else {
        false
    };

    if !valid {
        return Err(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ));
    }

    // 签发 Token
    let token = generate_token(
        user.id,
        user.org_id.clone().unwrap_or_default(),
        user.role.clone(),
    )?;

    Ok(AxumJson(AuthResponse {
        token,
        user: UserDto {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
        },
    }))
}

// POST /api/auth/register
pub async fn register(
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<AxumJson<AuthResponse>, AppError> {
    // 检查邮箱是否存在
    let exists = users::Entity::find()
        .filter(users::Column::Email.eq(&req.email))
        .one(&state.db)
        .await?;

    if exists.is_some() {
        return Err(AppError::BadRequest("Email already exists".to_string()));
    }

    // 哈希密码
    let hash = hash(&req.password, DEFAULT_COST)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("Password hashing error")))?;

    let new_user = users::ActiveModel {
        id: Set(Uuid::new_v4()),
        org_id: Set(Some(req.org_id.clone())),
        email: Set(Some(req.email.clone())),
        password_hash: Set(Some(hash)),
        name: Set(req.name.clone()),
        role: Set("user".to_string()), // 默认普通用户
        ..Default::default()
    };

    let saved = new_user.insert(&state.db).await?;

    // 注册成功自动登录
    let token = generate_token(
        saved.id,
        saved.org_id.unwrap_or_default(),
        saved.role.clone(),
    )?;

    Ok(AxumJson(AuthResponse {
        token,
        user: UserDto {
            id: saved.id,
            email: saved.email,
            name: saved.name,
            role: saved.role,
        },
    }))
}

// GET /api/auth/me - 获取当前用户信息
use crate::auth::AuthUser;
use crate::entities::organizations;

pub async fn me(
    Extension(state): Extension<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<AxumJson<MeResponse>, AppError> {
    // 查询用户详细信息
    let user = users::Entity::find_by_id(auth_user.id)
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    // 查询组织信息（如果有）
    let org_name = if let Some(ref org_id) = user.org_id {
        organizations::Entity::find_by_id(org_id.clone())
            .one(&state.db)
            .await?
            .map(|org| org.name)
    } else {
        None
    };

    Ok(AxumJson(MeResponse {
        id: user.id.to_string(),
        username: user
            .name
            .clone()
            .unwrap_or_else(|| user.email.clone().unwrap_or_else(|| user.id.to_string())),
        email: user.email,
        role: Some(user.role),
        org_id: user.org_id,
        org_name,
    }))
}
