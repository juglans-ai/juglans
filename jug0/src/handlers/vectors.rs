// src/handlers/vectors.rs
//
// Generic vector storage & search API with per-user space isolation.
// Qdrant collections are partitioned by embedding dimension (vectors_1536, vectors_1024).
// Isolation is enforced via payload filters: org_id + user_id + space.

use axum::{
    extract::{Extension, Path},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::errors::AppError;
use crate::request::vectors::*;
use crate::services::memory::utils::{json_to_qdrant_value, qdrant_payload_to_map};
use crate::AppState;

/// Get collection name for given embedding model
fn collection_for_model(model: &str) -> &'static str {
    let m = model.to_lowercase();
    if m.contains("qwen") || m.contains("text-embedding") {
        "vectors_1024"
    } else {
        "vectors_1536"
    }
}

/// Get dimension for given embedding model
fn dim_for_model(model: &str) -> u64 {
    let m = model.to_lowercase();
    if m.contains("qwen") || m.contains("text-embedding") {
        1024
    } else {
        1536
    }
}

/// Resolve embedding model name: request > space default > env default
fn resolve_model(request_model: &Option<String>, space_model: Option<&str>) -> String {
    if let Some(m) = request_model {
        return m.clone();
    }
    if let Some(m) = space_model {
        return m.to_string();
    }
    std::env::var("MEMORY_EMBEDDING_MODEL").unwrap_or_else(|_| "default".to_string())
}

/// Generate deterministic point UUID from scope + id (SHA256-based)
fn point_uuid(org_id: &str, user_id: &Uuid, space: &str, point_id: &str) -> Uuid {
    let input = format!("vec:{}:{}:{}:{}", org_id, user_id, space, point_id);
    let hash = Sha256::digest(input.as_bytes());
    // Use first 16 bytes of SHA256 as UUID
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // Set version 4 and variant bits for valid UUID format
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Generate space meta point UUID
fn space_meta_uuid(org_id: &str, user_id: &Uuid, space: &str) -> Uuid {
    point_uuid(org_id, user_id, space, "__meta__")
}

// ─── Space Management ───────────────────────────────────────

/// POST /api/vectors/spaces — Create a new vector space
pub async fn create_space(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<CreateSpaceRequest>,
) -> Result<impl IntoResponse, AppError> {
    let model_name = resolve_model(&req.model, None);
    let collection = collection_for_model(&model_name);
    let dim = dim_for_model(&model_name);

    // Ensure collection exists
    state
        .vector_db
        .ensure_collection(collection, dim)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Create index fields for space isolation (idempotent)
    for field in &["org_id", "user_id", "space", "_type"] {
        let _ = state
            .vector_db
            .client
            .create_field_index(
                collection,
                field,
                qdrant_client::qdrant::FieldType::Keyword,
                None,
                None,
            )
            .await;
    }

    // Store space metadata as a special point
    let meta_id = space_meta_uuid(&user.org_id, &user.id, &req.space);
    let mut payload = HashMap::new();
    payload.insert(
        "_type".to_string(),
        json_to_qdrant_value(json!("space_meta")),
    );
    payload.insert(
        "org_id".to_string(),
        json_to_qdrant_value(json!(user.org_id)),
    );
    payload.insert(
        "user_id".to_string(),
        json_to_qdrant_value(json!(user.id.to_string())),
    );
    payload.insert("space".to_string(), json_to_qdrant_value(json!(req.space)));
    payload.insert("model".to_string(), json_to_qdrant_value(json!(model_name)));
    payload.insert(
        "public".to_string(),
        json_to_qdrant_value(json!(req.public.unwrap_or(false))),
    );
    payload.insert(
        "description".to_string(),
        json_to_qdrant_value(json!(req.description.as_deref().unwrap_or(""))),
    );
    payload.insert(
        "created_at".to_string(),
        json_to_qdrant_value(json!(Utc::now().to_rfc3339())),
    );
    payload.insert(
        "collection".to_string(),
        json_to_qdrant_value(json!(collection)),
    );

    // Dummy zero vector for meta point
    let zero_vec = vec![0.0f32; dim as usize];
    state
        .vector_db
        .upsert_points(collection, vec![(meta_id, zero_vec, payload)])
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    tracing::info!(
        "[Vectors] Space '{}' created by user {} (model: {}, collection: {})",
        req.space,
        user.id,
        model_name,
        collection
    );

    Ok(Json(json!({
        "space": req.space,
        "model": model_name,
        "collection": collection,
        "created": true,
    })))
}

/// GET /api/vectors/spaces — List spaces owned by the current user
pub async fn list_spaces(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let mut all_spaces = Vec::new();

    // Search in both collections
    for collection in &["vectors_1536", "vectors_1024"] {
        if !state
            .vector_db
            .client
            .collection_exists(collection)
            .await
            .unwrap_or(false)
        {
            continue;
        }

        let filter = json!({
            "org_id": user.org_id,
            "user_id": user.id.to_string(),
            "_type": "space_meta",
        });

        let points = state
            .vector_db
            .scroll(collection, Some(100), Some(filter))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        for point in points {
            let payload = qdrant_payload_to_map(point.payload);
            all_spaces.push(json!({
                "space": payload.get("space").and_then(|v| v.as_str()).unwrap_or(""),
                "model": payload.get("model").and_then(|v| v.as_str()).unwrap_or("default"),
                "public": payload.get("public").and_then(|v| v.as_bool()).unwrap_or(false),
                "description": payload.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                "created_at": payload.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
            }));
        }
    }

    Ok(Json(json!(all_spaces)))
}

/// DELETE /api/vectors/spaces/:space — Delete a space and all its vectors
pub async fn delete_space(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Path(space): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let mut deleted = 0u64;

    for collection in &["vectors_1536", "vectors_1024"] {
        if !state
            .vector_db
            .client
            .collection_exists(collection)
            .await
            .unwrap_or(false)
        {
            continue;
        }

        // Scroll all points in this space owned by user
        let filter = json!({
            "org_id": user.org_id,
            "user_id": user.id.to_string(),
            "space": space,
        });

        let points = state
            .vector_db
            .scroll(collection, Some(10000), Some(filter))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if points.is_empty() {
            continue;
        }

        let ids: Vec<Uuid> = points
            .into_iter()
            .filter_map(|p| {
                p.id.and_then(|id| match id.point_id_options {
                    Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => {
                        Uuid::parse_str(&u).ok()
                    }
                    _ => None,
                })
            })
            .collect();

        deleted += ids.len() as u64;
        if !ids.is_empty() {
            state
                .vector_db
                .delete_points(collection, ids)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
    }

    tracing::info!(
        "[Vectors] Space '{}' deleted by user {} ({} points)",
        space,
        user.id,
        deleted
    );

    Ok(Json(json!({ "status": "ok", "deleted": deleted })))
}

// ─── Vector Operations ──────────────────────────────────────

/// Look up space metadata to get model and public flag.
/// First tries exact match (org + user), then falls back to org-only match
/// so that public spaces created by one user are visible to all org members.
async fn get_space_meta(
    state: &AppState,
    org_id: &str,
    user_id: &Uuid,
    space: &str,
) -> Option<HashMap<String, Value>> {
    for collection in &["vectors_1536", "vectors_1024"] {
        if !state
            .vector_db
            .client
            .collection_exists(collection)
            .await
            .unwrap_or(false)
        {
            continue;
        }

        // Try exact match first (user's own space)
        let filter = json!({
            "org_id": org_id,
            "user_id": user_id.to_string(),
            "space": space,
            "_type": "space_meta",
        });

        if let Ok(points) = state
            .vector_db
            .scroll(collection, Some(1), Some(filter))
            .await
        {
            if let Some(point) = points.into_iter().next() {
                return Some(qdrant_payload_to_map(point.payload));
            }
        }

        // Fallback: org-level match (find public spaces created by other org members)
        let org_filter = json!({
            "org_id": org_id,
            "space": space,
            "_type": "space_meta",
        });

        if let Ok(points) = state
            .vector_db
            .scroll(collection, Some(1), Some(org_filter))
            .await
        {
            if let Some(point) = points.into_iter().next() {
                return Some(qdrant_payload_to_map(point.payload));
            }
        }
    }
    None
}

/// POST /api/vectors/upsert — Upsert vectors into a space
pub async fn upsert_vectors(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<VectorUpsertRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Look up space meta for default model
    let space_meta = get_space_meta(&state, &user.org_id, &user.id, &req.space).await;
    let space_model = space_meta
        .as_ref()
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str());
    let model_name = resolve_model(&req.model, space_model);
    let collection = collection_for_model(&model_name);
    let dim = dim_for_model(&model_name);

    // Ensure collection exists
    state
        .vector_db
        .ensure_collection(collection, dim)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Separate points needing embedding from those with pre-computed vectors
    let mut texts_to_embed: Vec<(usize, String)> = Vec::new();
    let mut point_data: Vec<(Uuid, Option<Vec<f32>>, HashMap<String, Value>)> = Vec::new();

    for (i, point) in req.points.iter().enumerate() {
        let uid = point_uuid(&user.org_id, &user.id, &req.space, &point.id);

        // Build payload with isolation fields
        let mut payload_json = point.payload.clone();
        payload_json.insert("org_id".to_string(), json!(user.org_id));
        payload_json.insert("user_id".to_string(), json!(user.id.to_string()));
        payload_json.insert("space".to_string(), json!(req.space));
        payload_json.insert("_point_id".to_string(), json!(point.id));
        if let Some(ref text) = point.text {
            payload_json.insert("_text".to_string(), json!(text));
        }

        if let Some(ref embedding) = point.embedding {
            point_data.push((uid, Some(embedding.clone()), payload_json));
        } else if let Some(ref text) = point.text {
            texts_to_embed.push((i, text.clone()));
            point_data.push((uid, None, payload_json));
        } else {
            return Err(AppError::BadRequest(format!(
                "Point '{}' must have either 'text' or 'embedding'",
                point.id
            )));
        }
    }

    // Batch embed texts
    if !texts_to_embed.is_empty() {
        let texts: Vec<String> = texts_to_embed.iter().map(|(_, t)| t.clone()).collect();
        let provider = state.embedding_factory.get_provider(&model_name);
        let embeddings = provider
            .embed_batch(texts)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        for ((idx, _), embedding) in texts_to_embed.iter().zip(embeddings) {
            point_data[*idx].1 = Some(embedding);
        }
    }

    // Convert to Qdrant format and upsert
    let qdrant_points: Vec<(
        Uuid,
        Vec<f32>,
        HashMap<String, qdrant_client::qdrant::Value>,
    )> = point_data
        .into_iter()
        .map(|(id, vec, payload)| {
            let qdrant_payload: HashMap<String, qdrant_client::qdrant::Value> = payload
                .into_iter()
                .map(|(k, v)| (k, json_to_qdrant_value(v)))
                .collect();
            (id, vec.unwrap_or_default(), qdrant_payload)
        })
        .collect();

    let count = qdrant_points.len();
    state
        .vector_db
        .upsert_points(collection, qdrant_points)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    tracing::info!(
        "[Vectors] Upserted {} points into space '{}' (user: {}, model: {})",
        count,
        req.space,
        user.id,
        model_name
    );

    Ok(Json(json!({
        "status": "ok",
        "upserted": count,
        "space": req.space,
        "model": model_name,
    })))
}

/// POST /api/vectors/search — Search vectors in a space
pub async fn search_vectors(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<VectorSearchRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Look up space meta for model and public flag
    let space_meta = get_space_meta(&state, &user.org_id, &user.id, &req.space).await;
    let space_model = space_meta
        .as_ref()
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str());
    let is_public = space_meta
        .as_ref()
        .and_then(|m| m.get("public"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let model_name = resolve_model(&req.model, space_model);
    let collection = collection_for_model(&model_name);

    // Check collection exists
    if !state
        .vector_db
        .client
        .collection_exists(collection)
        .await
        .unwrap_or(false)
    {
        return Ok(Json(json!([])));
    }

    // Embed query
    let provider = state.embedding_factory.get_provider(&model_name);
    let query_vec = provider
        .embed(&req.query)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Build isolation filter
    let mut filter_map = serde_json::Map::new();
    filter_map.insert("space".to_string(), json!(req.space));
    if is_public {
        // Public spaces: only filter by space name (accessible to all)
        filter_map.insert("org_id".to_string(), json!(user.org_id));
    } else {
        // Private spaces: restrict to owner only
        filter_map.insert("org_id".to_string(), json!(user.org_id));
        filter_map.insert("user_id".to_string(), json!(user.id.to_string()));
    }
    // Merge additional filters
    if let Some(ref extra) = req.filters {
        if let Some(obj) = extra.as_object() {
            for (k, v) in obj {
                filter_map.insert(k.clone(), v.clone());
            }
        }
    }

    let limit = req.limit.unwrap_or(10);
    let threshold = req.threshold.unwrap_or(0.3);

    let scored_points = state
        .vector_db
        .search(
            collection,
            query_vec,
            limit,
            Some(threshold),
            Some(Value::Object(filter_map)),
        )
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    // Convert results
    let results: Vec<Value> = scored_points
        .into_iter()
        .filter_map(|p| {
            let payload = qdrant_payload_to_map(p.payload);
            // Skip space_meta points
            if payload.get("_type").and_then(|v| v.as_str()) == Some("space_meta") {
                return None;
            }
            let point_id = payload
                .get("_point_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Strip internal fields from response
            let clean_payload: HashMap<String, Value> = payload
                .into_iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "org_id" | "user_id" | "space" | "_type" | "_point_id" | "_text"
                    )
                })
                .collect();

            Some(json!({
                "id": point_id,
                "score": p.score,
                "payload": clean_payload,
            }))
        })
        .collect();

    Ok(Json(json!(results)))
}

/// POST /api/vectors/delete — Delete vectors from a space
pub async fn delete_vectors(
    Extension(state): Extension<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<VectorDeleteRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Determine which collections to check
    let mut deleted = 0usize;

    for collection in &["vectors_1536", "vectors_1024"] {
        if !state
            .vector_db
            .client
            .collection_exists(collection)
            .await
            .unwrap_or(false)
        {
            continue;
        }

        let ids: Vec<Uuid> = req
            .ids
            .iter()
            .map(|id| point_uuid(&user.org_id, &user.id, &req.space, id))
            .collect();

        // We only delete points we can verify belong to this user+space
        // (UUID v5 is deterministic from org+user+space+id, so ownership is implicit)
        let count = ids.len();
        state
            .vector_db
            .delete_points(collection, ids)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        deleted += count;
    }

    Ok(Json(json!({
        "status": "ok",
        "deleted": deleted,
    })))
}
