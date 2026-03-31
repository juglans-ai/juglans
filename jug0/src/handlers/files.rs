// src/handlers/files.rs
//
// File upload handler

use axum::{extract::Multipart, Extension, Json};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::errors::AppError;
use crate::AppState;

const MAX_FILE_SIZE: usize = 2 * 1024 * 1024; // 2MB
const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg"];

#[derive(Serialize)]
pub struct UploadResponse {
    pub url: String,
    pub filename: String,
    pub size: usize,
    pub content_type: String,
}

/// POST /api/upload — upload a file (multipart form)
pub async fn upload(
    Extension(state): Extension<Arc<AppState>>,
    _user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Failed to read multipart field: {}", e)))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        if field_name != "file" {
            continue;
        }

        let original_filename = field.file_name().unwrap_or("upload").to_string();

        let content_type = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        // Extract extension from original filename
        let ext = original_filename
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_lowercase();

        if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
            return Err(AppError::BadRequest(format!(
                "File type '{}' not allowed. Allowed: {}",
                ext,
                ALLOWED_EXTENSIONS.join(", ")
            )));
        }

        // Read file data
        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("Failed to read file data: {}", e)))?;

        if data.len() > MAX_FILE_SIZE {
            return Err(AppError::BadRequest(format!(
                "File too large ({} bytes). Maximum: {} bytes",
                data.len(),
                MAX_FILE_SIZE
            )));
        }

        // Generate unique filename
        let uuid = Uuid::new_v4();
        let stored_filename = format!("{}.{}", uuid, ext);

        // Ensure upload directory exists
        let upload_dir = PathBuf::from(&state.upload_dir);
        tokio::fs::create_dir_all(&upload_dir).await.map_err(|e| {
            AppError::BadRequest(format!("Failed to create upload directory: {}", e))
        })?;

        // Write file
        let file_path = upload_dir.join(&stored_filename);
        tokio::fs::write(&file_path, &data)
            .await
            .map_err(|e| AppError::BadRequest(format!("Failed to write file: {}", e)))?;

        let url = format!("/uploads/{}", stored_filename);
        let size = data.len();

        tracing::info!(
            "File uploaded: {} -> {} ({} bytes)",
            original_filename,
            url,
            size
        );

        return Ok(Json(UploadResponse {
            url,
            filename: original_filename,
            size,
            content_type,
        }));
    }

    Err(AppError::BadRequest(
        "No 'file' field found in multipart form".to_string(),
    ))
}
