// src/registry/server.rs
//
// Axum HTTP server for the Juglans package registry.

use axum::{
    extract::{Extension, Multipart, Path, Query},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use super::auth::{require_auth, AuthConfig};
use super::store::RegistryStore;

struct RegistryState {
    store: RegistryStore,
}

// ── Response models ──────────────────────────────────────────────────

#[derive(Serialize)]
struct PackageResponse {
    name: String,
    slug: String,
    description: Option<String>,
    author: Option<String>,
    license: Option<String>,
    versions: Vec<VersionResponse>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct VersionResponse {
    version: String,
    entry: String,
    dependencies: Value,
    checksum: String,
    published_at: String,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

// ── Server entrypoint ────────────────────────────────────────────────

pub async fn start_registry_server(port: u16, data_dir: PathBuf) -> anyhow::Result<()> {
    let store = RegistryStore::open(&data_dir)?;
    let auth = Arc::new(AuthConfig::from_env());
    let state = Arc::new(RegistryState { store });

    // Write routes (require auth)
    let write_routes = Router::new()
        .route(
            "/api/v1/packages/{name}/{version}",
            put(handle_publish),
        )
        .layer(middleware::from_fn(require_auth));

    // Read routes (public)
    let read_routes = Router::new()
        .route("/api/v1/packages/{name}", get(handle_get_package))
        .route(
            "/api/v1/packages/{name}/{version}",
            get(handle_get_version),
        )
        .route(
            "/api/v1/packages/{name}/{version}/download",
            get(handle_download),
        )
        .route("/api/v1/search", get(handle_search))
        .route("/", get(handle_index));

    let app = Router::new()
        .merge(write_routes)
        .merge(read_routes)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(Extension(state))
        .layer(Extension(auth.clone()));

    let auth_status = if auth.tokens.is_empty() {
        "DISABLED (set REGISTRY_TOKEN to enable)"
    } else {
        "enabled"
    };

    info!("──────────────────────────────────────────────────");
    info!("Juglans Package Registry");
    info!("  Listen: http://0.0.0.0:{}", port);
    info!("  Data:   {}", data_dir.display());
    info!("  Auth:   {}", auth_status);
    info!("──────────────────────────────────────────────────");

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ── Handlers ─────────────────────────────────────────────────────────

async fn handle_index() -> Json<Value> {
    Json(json!({
        "service": "Juglans Package Registry",
        "version": env!("CARGO_PKG_VERSION"),
        "api": "/api/v1"
    }))
}

async fn handle_publish(
    Extension(state): Extension<Arc<RegistryState>>,
    Path((name, version)): Path<(String, String)>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut metadata: Option<Value> = None;
    let mut archive_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Multipart error: {}", e)})),
        )
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "metadata" => {
                let text = field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": format!("Failed to read metadata: {}", e)})),
                    )
                })?;
                metadata = Some(serde_json::from_str(&text).map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error": format!("Invalid metadata JSON: {}", e)})),
                    )
                })?);
            }
            "package" => {
                archive_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| {
                            (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error": format!("Failed to read package: {}", e)})),
                            )
                        })?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let metadata = metadata.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing 'metadata' field"})),
        )
    })?;
    let archive_bytes = archive_bytes.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing 'package' field"})),
        )
    })?;

    // Validate URL params match metadata
    let meta_name = metadata["name"].as_str().unwrap_or("");
    let meta_version = metadata["version"].as_str().unwrap_or("");
    if meta_name != name || meta_version != version {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "URL parameters do not match metadata"})),
        ));
    }

    let slug = metadata["slug"]
        .as_str()
        .unwrap_or(&name)
        .to_string();
    let description = metadata["description"].as_str().map(|s| s.to_string());
    let author = metadata["author"].as_str().map(|s| s.to_string());
    let license = metadata["license"].as_str().map(|s| s.to_string());
    let entry = metadata["entry"]
        .as_str()
        .unwrap_or("lib.jg")
        .to_string();
    let dependencies = metadata
        .get("dependencies")
        .cloned()
        .unwrap_or(json!({}));

    let name_c = name.clone();
    let version_c = version.clone();

    let result = tokio::task::spawn_blocking(move || {
        state.store.publish(
            &name_c,
            &version_c,
            &slug,
            description.as_deref(),
            author.as_deref(),
            license.as_deref(),
            &entry,
            &dependencies,
            &archive_bytes,
        )
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Task error: {}", e)})),
        )
    })?;

    match result {
        Ok(()) => Ok(Json(json!({
            "message": format!("Published {}-{}", name, version),
            "name": name,
            "version": version,
        }))),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("already exists") {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            Err((status, Json(json!({"error": msg}))))
        }
    }
}

async fn handle_get_package(
    Extension(state): Extension<Arc<RegistryState>>,
    Path(name): Path<String>,
) -> Result<Json<PackageResponse>, (StatusCode, Json<Value>)> {
    let package = state.store.get_package(&name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let package = package.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Package '{}' not found", name)})),
        )
    })?;

    let versions = state.store.get_versions(&name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(PackageResponse {
        name: package.name,
        slug: package.slug,
        description: package.description,
        author: package.author,
        license: package.license,
        versions: versions
            .into_iter()
            .map(|v| VersionResponse {
                version: v.version,
                entry: v.entry,
                dependencies: serde_json::from_str(&v.dependencies).unwrap_or(json!({})),
                checksum: v.checksum,
                published_at: v.published_at,
            })
            .collect(),
        created_at: package.created_at,
        updated_at: package.updated_at,
    }))
}

async fn handle_get_version(
    Extension(state): Extension<Arc<RegistryState>>,
    Path((name, version)): Path<(String, String)>,
) -> Result<Json<VersionResponse>, (StatusCode, Json<Value>)> {
    let ver = state.store.get_version(&name, &version).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let ver = ver.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("{}@{} not found", name, version)})),
        )
    })?;

    Ok(Json(VersionResponse {
        version: ver.version,
        entry: ver.entry,
        dependencies: serde_json::from_str(&ver.dependencies).unwrap_or(json!({})),
        checksum: ver.checksum,
        published_at: ver.published_at,
    }))
}

async fn handle_download(
    Extension(state): Extension<Arc<RegistryState>>,
    Path((name, version)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    // Verify version exists
    let ver = state.store.get_version(&name, &version).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    ver.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("{}@{} not found", name, version)})),
        )
    })?;

    let archive_path = state.store.archive_path(&name, &version);
    let bytes = tokio::fs::read(&archive_path).await.map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Archive file not found: {}", e)})),
        )
    })?;

    Ok((
        StatusCode::OK,
        [
            ("content-type", "application/gzip".to_string()),
            (
                "content-disposition",
                format!("attachment; filename=\"{}-{}.tar.gz\"", name, version),
            ),
        ],
        bytes,
    ))
}

async fn handle_search(
    Extension(state): Extension<Arc<RegistryState>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<PackageResponse>>, (StatusCode, Json<Value>)> {
    let query = params.q.unwrap_or_default();
    if query.is_empty() {
        return Ok(Json(vec![]));
    }

    let packages = state.store.search(&query).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let mut results = Vec::new();
    for pkg in packages {
        let versions = state.store.get_versions(&pkg.name).unwrap_or_default();
        results.push(PackageResponse {
            name: pkg.name,
            slug: pkg.slug,
            description: pkg.description,
            author: pkg.author,
            license: pkg.license,
            versions: versions
                .into_iter()
                .map(|v| VersionResponse {
                    version: v.version,
                    entry: v.entry,
                    dependencies: serde_json::from_str(&v.dependencies).unwrap_or(json!({})),
                    checksum: v.checksum,
                    published_at: v.published_at,
                })
                .collect(),
            created_at: pkg.created_at,
            updated_at: pkg.updated_at,
        });
    }

    Ok(Json(results))
}
