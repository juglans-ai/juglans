// src/main.rs
mod auth;
mod entities;
mod errors;
mod handlers;
mod providers;
pub mod repositories;
pub mod request;
pub mod response;
mod scheduler;
mod services;

use axum::{
    http::{header, Method},
    middleware,
    routing::{delete, get, patch, post},
    Extension, Router,
};
use dashmap::DashMap;
use dotenvy::dotenv;
use log::LevelFilter;
use sea_orm::{ConnectOptions, Database, EntityTrait};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

use lazy_static::lazy_static;

lazy_static! {
    /// The "official" organization slug, configurable via OFFICIAL_ORG_SLUG env var.
    /// All places that previously hardcoded "juglans_official" now use this value.
    pub static ref OFFICIAL_ORG_SLUG: String =
        std::env::var("OFFICIAL_ORG_SLUG").unwrap_or_else(|_| "juglans_official".to_string());
}

/// Convenience accessor so other modules can do `crate::official_org_slug()`.
pub fn official_org_slug() -> &'static str {
    &OFFICIAL_ORG_SLUG
}

use crate::auth::{admin_auth_middleware, auth_middleware, optional_auth_middleware};
use crate::entities::{api_keys, organizations, users};
use crate::handlers::chat::types::ToolResultWithTools;
use crate::providers::search::tavily::TavilySearch;
use crate::providers::EmbeddingFactory;
use crate::providers::ProviderFactory;
use crate::providers::SearchProvider;
use crate::repositories::{AgentRepository, PromptRepository};
use crate::services::cache::CacheService;
use crate::services::mcp::McpClient;
use crate::services::memory::service::MemoryService;
use crate::services::models::ModelsService;
use crate::services::qdrant::VectorDbService;

// AppState 定义
pub struct AppState {
    pub db: sea_orm::DatabaseConnection,
    pub providers: ProviderFactory,
    pub active_requests: DashMap<Uuid, CancellationToken>,
    pub mcp_client: McpClient,
    pub embedding_factory: EmbeddingFactory,
    pub vector_db: VectorDbService,
    pub memory_service: MemoryService,
    pub models_service: ModelsService,
    /// Signing key for Execution Tokens (jug0 → juglans-agent → jug0 chain)
    pub signing_key: Vec<u8>,
    /// Redis cache service
    pub cache: CacheService,
    /// Repositories
    pub agent_repo: AgentRepository,
    pub prompt_repo: PromptRepository,
    /// Channels for tool-result → SSE stream communication
    pub tool_result_channels: DashMap<Uuid, mpsc::Sender<ToolResultWithTools>>,
    /// MCP tool sessions for Claude Code (session_id → session state)
    pub tool_sessions: Arc<DashMap<String, handlers::mcp_endpoint::McpSession>>,
    /// Workflow forward mapping: chat_id → endpoint info (for tool_result routing)
    pub workflow_forwards: DashMap<Uuid, handlers::chat::types::WorkflowForwardInfo>,
    /// Shared HTTP client with connection pooling (for workflow forwarding etc.)
    pub http_client: reqwest::Client,
    /// Search provider (Tavily etc.)
    pub search: Arc<dyn SearchProvider>,
    /// Upload directory for file storage
    pub upload_dir: String,
    /// Admin API key for admin endpoints
    pub admin_key: Option<String>,
    /// Channel to notify cron scheduler to reload workflows
    pub scheduler_reload: tokio::sync::watch::Sender<()>,
}

/// Load auth-related data to Redis at startup
/// TTL = 0 means permanent (until restart or explicit delete)
/// Note: Agent/Prompt lists are handled by repositories with cache + DB fallback
async fn load_auth_data_to_redis(state: &AppState) {
    let db = &state.db;
    let cache = &state.cache;

    // 1. Users (by id, by org:external_id for shadow user auth)
    let all_users = users::Entity::find().all(db).await.unwrap_or_default();
    for u in &all_users {
        let _ = cache.set(&format!("jug0:user:id:{}", u.id), u, 0).await;
        if let (Some(oid), Some(eid)) = (&u.org_id, &u.external_id) {
            let _ = cache.set(&format!("jug0:user:{}:{}", oid, eid), u, 0).await;
        }
    }
    tracing::info!("Redis: {} users", all_users.len());

    // 2. Organizations (by id)
    let all_orgs = organizations::Entity::find()
        .all(db)
        .await
        .unwrap_or_default();
    for o in &all_orgs {
        let _ = cache.set(&format!("jug0:org:{}", o.id), o, 0).await;
    }
    tracing::info!("Redis: {} orgs", all_orgs.len());

    // 3. API Keys (by key_hash -> user)
    let all_keys = api_keys::Entity::find().all(db).await.unwrap_or_default();
    let mut key_count = 0;
    for k in &all_keys {
        let expired = k
            .expires_at
            .map(|e| e < chrono::Utc::now().naive_utc())
            .unwrap_or(false);
        if !expired {
            if let Some(u) = all_users.iter().find(|u| u.id == k.user_id) {
                let _ = cache
                    .set(&format!("jug0:apikey:{}", k.key_hash), u, 0)
                    .await;
                key_count += 1;
            }
        }
    }
    tracing::info!("Redis: {} api_keys", key_count);

    tracing::info!("✅ Auth data loaded to Redis");
}

/// Warmup commonly accessed data at startup (agents/prompts for official org)
/// This ensures the first user request hits cache instead of cold DB queries
async fn warmup_cache(state: &AppState) {
    use crate::request::PromptFilter;

    // Create a synthetic "official org" user for warmup
    // This will cache the public agents/prompts that ALL users can see
    let warmup_user = crate::auth::AuthUser {
        id: Uuid::nil(),
        org_id: official_org_slug().to_string(),
        external_id: None,
        name: None,
        role: "system".to_string(),
        is_api_key: false,
    };

    // Warmup agents list (triggers DB query + cache write)
    match state.agent_repo.list_for_user(&warmup_user).await {
        Ok(agents) => tracing::info!("Cache warmup: {} agents", agents.len()),
        Err(e) => tracing::warn!("Cache warmup agents failed: {}", e),
    }

    // Warmup prompts list (triggers DB query + cache write)
    match state
        .prompt_repo
        .list_for_user(&warmup_user, &PromptFilter::default())
        .await
    {
        Ok(prompts) => tracing::info!("Cache warmup: {} prompts", prompts.len()),
        Err(e) => tracing::warn!("Cache warmup prompts failed: {}", e),
    }

    tracing::info!("✅ Cache warmup complete");
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "jug0=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is not set");

    let mut opt = ConnectOptions::new(db_url);
    opt.max_connections(20)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(10))
        .sqlx_logging(true)
        .sqlx_logging_level(LevelFilter::Debug);

    let db = Database::connect(opt)
        .await
        .expect("Failed to connect to DB");

    // Pre-warm database connection pool with parallel queries
    // This ensures all min_connections are established before serving requests
    {
        use sea_orm::ConnectionTrait;
        let warmup_queries: Vec<_> = (0..5)
            .map(|_| {
                db.execute(sea_orm::Statement::from_string(
                    sea_orm::DatabaseBackend::Postgres,
                    "SELECT 1".to_string(),
                ))
            })
            .collect();
        let results = futures::future::join_all(warmup_queries).await;
        let success_count = results.iter().filter(|r| r.is_ok()).count();
        tracing::info!(
            "Database pool warmup: {}/5 connections ready",
            success_count
        );
    }

    let tool_sessions: Arc<DashMap<String, handlers::mcp_endpoint::McpSession>> = Arc::new(DashMap::new());
    let providers = ProviderFactory::new_with_mcp(tool_sessions.clone());
    let active_requests = DashMap::new();
    let mcp_client = McpClient::new();

    let vector_db = VectorDbService::new().expect("Failed to initialize Qdrant client.");
    let embedding_factory = EmbeddingFactory::new();

    let memory_service = MemoryService::new(
        embedding_factory.clone(),
        vector_db.clone(),
        providers.clone(),
    );

    if let Err(e) = memory_service.init().await {
        tracing::error!("Memory Service initialization failed: {}", e);
    }

    // Initialize Models Service
    let models_service = ModelsService::new(
        db.clone(),
        std::env::var("OPENAI_API_KEY").ok(),
        std::env::var("DEEPSEEK_API_KEY").ok(),
        std::env::var("GEMINI_API_KEY").ok(),
        std::env::var("QWEN_API_KEY").ok(),
        std::env::var("ARK_API_KEY").ok(),
        std::env::var("XAI_API_KEY").ok(),
    );

    // Load execution signing key from environment
    let signing_key = std::env::var("EXECUTION_SIGNING_KEY")
        .unwrap_or_else(|_| {
            // Generate a default key for development (should be set in production)
            tracing::warn!("EXECUTION_SIGNING_KEY not set, using default development key");
            "jug0_dev_signing_key_change_in_production".to_string()
        })
        .into_bytes();

    // Connect to Redis
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let cache = CacheService::new(&redis_url)
        .await
        .expect("Failed to connect to Redis");
    tracing::info!("Connected to Redis at {}", redis_url);

    // Create repositories
    let agent_repo = AgentRepository::new(db.clone(), cache.clone());
    let prompt_repo = PromptRepository::new(db.clone(), cache.clone());

    // Shared HTTP client with connection pooling (for workflow forwarding etc.)
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create HTTP client");

    let search: Arc<dyn SearchProvider> = Arc::new(TavilySearch::new(http_client.clone()));

    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string());

    let (scheduler_reload_tx, _) = tokio::sync::watch::channel(());

    let app_state = Arc::new(AppState {
        db,
        providers,
        active_requests,
        mcp_client,
        embedding_factory,
        vector_db,
        memory_service,
        models_service: models_service.clone(),
        signing_key,
        cache,
        agent_repo,
        prompt_repo,
        tool_result_channels: DashMap::new(),
        tool_sessions: tool_sessions.clone(),
        workflow_forwards: DashMap::new(),
        http_client,
        search,
        upload_dir: upload_dir.clone(),
        admin_key: std::env::var("ADMIN_API_KEY").ok(),
        scheduler_reload: scheduler_reload_tx,
    });

    // Load auth data to Redis (users, orgs, api_keys)
    load_auth_data_to_redis(&app_state).await;

    // Warmup cache with commonly accessed data (agents/prompts)
    warmup_cache(&app_state).await;

    // Start cron scheduler
    {
        let state = app_state.clone();
        tokio::spawn(async move {
            scheduler::CronScheduler::start(state).await;
        });
    }

    // Spawn background model sync task (runs every hour)
    {
        let ms = models_service.clone();
        tokio::spawn(async move {
            // Initial sync on startup
            if let Err(e) = ms.refresh().await {
                tracing::warn!("Initial model sync failed: {}", e);
            }
            // Periodic sync
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                if let Err(e) = ms.refresh().await {
                    tracing::warn!("Periodic model sync failed: {}", e);
                }
            }
        });
    }

    // Periodic Redis sync (every 30 min, refresh auth data only)
    {
        let state = app_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1800));
            loop {
                interval.tick().await;
                load_auth_data_to_redis(&state).await;
            }
        });
    }

    let public_routes = Router::new()
        .route("/", get(|| async { "Jug0 is running!" }))
        .route("/health", get(|| async { "OK" }))
        .route("/api/auth/login", post(handlers::auth::login))
        .route("/api/auth/register", post(handlers::auth::register))
        // Organization management (uses X-ORG-ID + X-ORG-KEY auth)
        .route(
            "/api/organizations/public-key",
            post(handlers::organizations::set_public_key),
        )
        .route(
            "/api/organizations/info",
            get(handlers::organizations::get_org_info),
        )
        // MCP endpoint for Claude Code tool bridge
        .route("/mcp/:session_id", post(handlers::mcp_endpoint::mcp_handler).get(handlers::mcp_endpoint::mcp_get_handler))
        // Models API (public, no auth required)
        .route("/api/models", get(handlers::models::list_models))
        .route("/api/models/sync", post(handlers::models::sync_models))
        .with_state(app_state.clone());

    // Routes with optional authentication (public resources accessible without auth)
    let optional_auth_routes = Router::new()
        .route(
            "/api/r/:owner/:slug",
            get(handlers::resources::get_resource_by_owner_slug),
        )
        .route(
            "/api/users/by-username/:username",
            get(handlers::resources::get_user_by_username),
        )
        // Chat — optional auth: 无 API key 时返回友好引导消息
        .route("/api/chat", post(handlers::chat::chat_handler))
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            optional_auth_middleware,
        ))
        .with_state(app_state.clone());

    let protected_routes = Router::new()
        // Auth
        .route("/api/auth/me", get(handlers::auth::me))
        // Chat 核心（除 /api/chat POST 外，其余仍需强制 auth）
        .route("/api/chat/stop", post(handlers::chat::stop_chat_handler))
        .route(
            "/api/chat/tool-result",
            post(handlers::chat::tool_result_handler),
        )
        .route("/api/chats", get(handlers::chat::list_chats_handler))
        .route(
            "/api/chat/:id",
            get(handlers::context::get_history)
                .patch(handlers::context::update_chat)
                .delete(handlers::context::delete_chat),
        )
        .route(
            "/api/chat/:id/messages",
            delete(handlers::context::clear_chat_messages),
        )
        .route(
            "/api/chat/:id/clear",
            post(handlers::context::clear_chat_history),
        )
        .route("/api/chat/:id/branch", post(handlers::context::branch_chat))
        .route(
            "/api/chat/:id/regenerate",
            post(handlers::context::regenerate_chat),
        )
        // Chat 消息管理（新增）
        .route(
            "/api/chats/:chat_id/context",
            get(handlers::messages::get_context),
        )
        .route(
            "/api/chats/:chat_id/history",
            get(handlers::messages::get_history),
        )
        .route(
            "/api/chats/:chat_id/messages",
            get(handlers::messages::list_messages).post(handlers::messages::create_message),
        )
        .route(
            "/api/chats/:chat_id/messages/:message_id",
            get(handlers::messages::get_message_by_id)
                .patch(handlers::messages::update_message_by_id)
                .delete(handlers::messages::delete_message_by_id),
        )
        .route(
            "/api/chats/:chat_id/messages/batch-delete",
            post(handlers::messages::batch_delete_messages),
        )
        .route(
            "/api/chats/:chat_id/messages/truncate",
            post(handlers::messages::truncate_messages),
        )
        // 消息管理（按 UUID）
        .route(
            "/api/messages/:id",
            get(handlers::messages::get_message)
                .patch(handlers::messages::update_message)
                .delete(handlers::messages::delete_message),
        )
        .route(
            "/api/prompts",
            get(handlers::prompts::list_prompts).post(handlers::prompts::create_prompt),
        )
        .route(
            "/api/prompts/:key",
            get(handlers::prompts::get_prompt)
                .patch(handlers::prompts::update_prompt)
                .put(handlers::prompts::update_prompt) // PUT as alias for PATCH
                .delete(handlers::prompts::delete_prompt),
        )
        .route(
            "/api/prompts/:key/render",
            post(handlers::prompts::render_prompt),
        )
        .route(
            "/api/agents",
            get(handlers::agents::list_agents).post(handlers::agents::create_agent),
        )
        .route(
            "/api/agents/:id",
            get(handlers::agents::get_agent)
                .patch(handlers::agents::update_agent)
                .delete(handlers::agents::delete_agent),
        )
        .route(
            "/api/workflows",
            get(handlers::workflows::list_workflows).post(handlers::workflows::create_workflow),
        )
        .route(
            "/api/workflows/:id",
            get(handlers::workflows::get_workflow)
                .patch(handlers::workflows::update_workflow)
                .delete(handlers::workflows::delete_workflow),
        )
        .route(
            "/api/workflows/:id/execute",
            post(handlers::workflows::execute_workflow),
        )
        .route(
            "/api/workflows/:id/runs",
            get(handlers::workflows::list_workflow_runs),
        )
        .route(
            "/api/workflows/:id/trigger",
            post(handlers::workflows::trigger_workflow),
        )
        .route(
            "/api/keys",
            get(handlers::api_keys::list_keys).post(handlers::api_keys::create_key),
        )
        .route("/api/keys/:id", delete(handlers::api_keys::delete_key))
        .route("/api/handles", get(handlers::handles::list_handles))
        .route(
            "/api/handles/:handle",
            get(handlers::handles::get_handle).delete(handlers::handles::delete_handle),
        )
        .route(
            "/api/handles/:handle/check",
            get(handlers::handles::check_handle),
        )
        .route("/api/memories", get(handlers::memories::list_memories))
        .route(
            "/api/memories/:id",
            delete(handlers::memories::delete_memory),
        )
        .route(
            "/api/memories/search",
            post(handlers::memories::search_memories),
        )
        .route("/api/search", post(handlers::search::web_search))
        .route("/api/upload", post(handlers::files::upload))
        .route(
            "/api/embeddings",
            post(handlers::embeddings::create_embedding),
        )
        // Vector storage & search API
        .route(
            "/api/vectors/spaces",
            post(handlers::vectors::create_space).get(handlers::vectors::list_spaces),
        )
        .route(
            "/api/vectors/spaces/:space",
            delete(handlers::vectors::delete_space),
        )
        .route(
            "/api/vectors/upsert",
            post(handlers::vectors::upsert_vectors),
        )
        .route(
            "/api/vectors/search",
            post(handlers::vectors::search_vectors),
        )
        .route(
            "/api/vectors/delete",
            post(handlers::vectors::delete_vectors),
        )
        // Deploys
        .route(
            "/api/deploys",
            get(handlers::deploys::list_deploys).post(handlers::deploys::create_deploy),
        )
        .route(
            "/api/deploys/:slug",
            get(handlers::deploys::get_deploy)
                .patch(handlers::deploys::update_deploy)
                .delete(handlers::deploys::delete_deploy),
        )
        .route(
            "/api/deploys/:slug/redeploy",
            post(handlers::deploys::redeploy),
        )
        .route(
            "/api/deploys/:slug/domain",
            post(handlers::deploys::bind_domain).delete(handlers::deploys::unbind_domain),
        )
        // Usage statistics
        .route("/api/usage/stats", get(handlers::usage::get_usage_stats))
        // Internal APIs for juglans-api user sync
        .route("/api/internal/sync-user", post(handlers::users::sync_user))
        .route(
            "/api/internal/sync-users",
            post(handlers::users::batch_sync_users),
        )
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            auth_middleware,
        ))
        .with_state(app_state.clone());

    let admin_routes = Router::new()
        .route("/api/admin/usage", get(handlers::admin::global_usage))
        .route("/api/admin/users", get(handlers::admin::user_usage_list))
        .route("/api/admin/chats", get(handlers::admin::admin_chats))
        .route(
            "/api/admin/users/:user_id/quota",
            get(handlers::admin::get_user_quota).patch(handlers::admin::set_user_quota),
        )
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            admin_auth_middleware,
        ))
        .with_state(app_state.clone());

    // CORS 配置 - 允许浏览器直连
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::HeaderName::from_static("x-api-key"),
            header::HeaderName::from_static("x-user-id"),
            header::HeaderName::from_static("x-org-id"),
            header::HeaderName::from_static("x-org-key"),
        ])
        .expose_headers([header::CONTENT_TYPE]);

    let app = Router::new()
        .merge(public_routes)
        .merge(optional_auth_routes)
        .merge(protected_routes)
        .merge(admin_routes)
        .nest_service("/uploads", ServeDir::new(&upload_dir))
        .nest_service("/admin", ServeDir::new("static"))
        .layer(cors)
        .layer(Extension(app_state));

    let host: [u8; 4] = std::env::var("HOST")
        .unwrap_or_else(|_| "0.0.0.0".to_string())
        .split('.')
        .map(|s| s.parse::<u8>().unwrap_or(0))
        .collect::<Vec<u8>>()
        .try_into()
        .unwrap_or([0, 0, 0, 0]);
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .unwrap_or(3000);
    let addr = SocketAddr::from((host, port));
    tracing::info!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
