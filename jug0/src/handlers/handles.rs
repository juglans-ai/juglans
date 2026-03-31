// src/handlers/handles.rs
use crate::auth::AuthUser;
use crate::entities::handles;
use crate::errors::AppError;
use crate::AppState;
use axum::{
    extract::{Extension, Path},
    Json as AxumJson,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct HandleResponse {
    pub handle: String,
    pub target_type: String,
    pub target_id: uuid::Uuid,
}

#[derive(Debug, Serialize)]
pub struct HandleAvailableResponse {
    pub handle: String,
    pub available: bool,
}

/// List all handles in the user's org
pub async fn list_handles(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
) -> Result<AxumJson<Vec<handles::Model>>, AppError> {
    let handles = handles::Entity::find()
        .filter(handles::Column::OrgId.eq(&user.org_id))
        .all(&state.db)
        .await
        .map_err(AppError::Database)?;

    Ok(AxumJson(handles))
}

/// Check if a handle is available
pub async fn check_handle(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(handle): Path<String>,
) -> Result<AxumJson<HandleAvailableResponse>, AppError> {
    let existing = handles::Entity::find()
        .filter(handles::Column::OrgId.eq(&user.org_id))
        .filter(handles::Column::Handle.eq(&handle))
        .one(&state.db)
        .await
        .map_err(AppError::Database)?;

    Ok(AxumJson(HandleAvailableResponse {
        handle,
        available: existing.is_none(),
    }))
}

/// Get handle details
pub async fn get_handle(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(handle): Path<String>,
) -> Result<AxumJson<handles::Model>, AppError> {
    let handle_record = handles::Entity::find()
        .filter(handles::Column::OrgId.eq(&user.org_id))
        .filter(handles::Column::Handle.eq(&handle))
        .one(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound(format!("Handle @{} not found", handle)))?;

    Ok(AxumJson(handle_record))
}

/// Delete a handle (only if user owns the target)
pub async fn delete_handle(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(handle): Path<String>,
) -> Result<AxumJson<serde_json::Value>, AppError> {
    use crate::entities::agents;

    let handle_record = handles::Entity::find()
        .filter(handles::Column::OrgId.eq(&user.org_id))
        .filter(handles::Column::Handle.eq(&handle))
        .one(&state.db)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound(format!("Handle @{} not found", handle)))?;

    // Check ownership based on target type
    let can_delete = match handle_record.target_type.as_str() {
        "agent" => {
            // Check if user owns the agent
            let agent = agents::Entity::find_by_id(handle_record.target_id)
                .one(&state.db)
                .await
                .map_err(AppError::Database)?;
            agent.map(|a| a.user_id == Some(user.id)).unwrap_or(false)
        }
        "user" => {
            // Users can only delete their own handle
            handle_record.target_id == user.id
        }
        _ => false,
    };

    if !can_delete {
        return Err(AppError::Forbidden(
            "You don't have permission to delete this handle".to_string(),
        ));
    }

    // Delete the handle
    handles::Entity::delete_by_id(handle_record.id)
        .exec(&state.db)
        .await
        .map_err(AppError::Database)?;

    Ok(AxumJson(serde_json::json!({
        "success": true,
        "handle": handle
    })))
}

/// Resolve a handle to its target (used internally by chat handler)
pub async fn resolve_handle(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    handle: &str,
) -> Result<Option<handles::Model>, AppError> {
    handles::Entity::find()
        .filter(handles::Column::OrgId.eq(org_id))
        .filter(handles::Column::Handle.eq(handle))
        .one(db)
        .await
        .map_err(AppError::Database)
}

/// Create a handle (used internally when creating agents or users join org)
pub async fn create_handle(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    handle: &str,
    target_type: &str,
    target_id: uuid::Uuid,
) -> Result<handles::Model, AppError> {
    use sea_orm::ActiveModelTrait;
    use sea_orm::Set;

    // Check if handle already exists
    let existing = handles::Entity::find()
        .filter(handles::Column::OrgId.eq(org_id))
        .filter(handles::Column::Handle.eq(handle))
        .one(db)
        .await
        .map_err(AppError::Database)?;

    if existing.is_some() {
        return Err(AppError::Conflict(format!(
            "Handle @{} is already taken",
            handle
        )));
    }

    let new_handle = handles::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        org_id: Set(org_id.to_string()),
        handle: Set(handle.to_string()),
        target_type: Set(target_type.to_string()),
        target_id: Set(target_id),
        ..Default::default()
    };

    let saved = new_handle.insert(db).await.map_err(AppError::Database)?;
    Ok(saved)
}

/// Delete handle by target (used when deleting agents)
pub async fn delete_handle_by_target(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    target_type: &str,
    target_id: uuid::Uuid,
) -> Result<(), AppError> {
    handles::Entity::delete_many()
        .filter(handles::Column::OrgId.eq(org_id))
        .filter(handles::Column::TargetType.eq(target_type))
        .filter(handles::Column::TargetId.eq(target_id))
        .exec(db)
        .await
        .map_err(AppError::Database)?;

    Ok(())
}
