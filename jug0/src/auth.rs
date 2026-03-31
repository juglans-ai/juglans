// src/auth.rs
use axum::{
    async_trait,
    extract::{FromRequestParts, State},
    http::{request::Parts, Request},
    middleware::Next,
    response::Response,
    RequestPartsExt,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use jsonwebtoken::{
    decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use lazy_static::lazy_static;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::{api_keys, organizations, users};
use crate::errors::AppError;
use crate::AppState;

/// Cache TTL in seconds (5 minutes)
const CACHE_TTL_SECS: u64 = 300;

lazy_static! {
    static ref JWT_SECRET: String = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub org_id: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub role: String,
    #[serde(skip)]
    pub is_api_key: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub org_id: String,
    pub role: String,
}

/// ORG 签发的 JWT Claims（用于前端直连 jug0）
#[derive(Debug, Serialize, Deserialize)]
pub struct OrgSignedClaims {
    pub sub: String, // user_id (external_id from org's system)
    pub org: String, // org_id
    pub iat: usize,  // issued at
    pub exp: usize,  // expiration
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

// =============================================================================
// Execution Token - 用于 jug0 → juglans-agent → jug0 的请求链追踪
// =============================================================================

type HmacSha256 = Hmac<Sha256>;

/// Execution Token Claims - 包含调用链信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionClaims {
    pub caller_user_id: Uuid,  // 实际调用者 (发起请求的用户)
    pub caller_org_id: String, // 调用者所属组织
    pub author_user_id: Uuid,  // agent/workflow 作者
    pub agent_id: Uuid,        // 被调用的 agent
    pub chat_id: Option<Uuid>, // 关联的 chat session
    pub iat: i64,              // issued at (unix timestamp)
    pub exp: i64,              // expires at (unix timestamp)
}

impl ExecutionClaims {
    /// 创建新的 ExecutionClaims
    pub fn new(
        caller_user_id: Uuid,
        caller_org_id: String,
        author_user_id: Uuid,
        agent_id: Uuid,
        chat_id: Option<Uuid>,
        ttl_seconds: i64,
    ) -> Self {
        let now = Utc::now().timestamp();
        Self {
            caller_user_id,
            caller_org_id,
            author_user_id,
            agent_id,
            chat_id,
            iat: now,
            exp: now + ttl_seconds,
        }
    }

    /// 编码为签名 token: base64(json).base64(hmac)
    pub fn encode(&self, signing_key: &[u8]) -> String {
        let json =
            serde_json::to_string(self).expect("ExecutionClaims serialization should not fail");
        let payload = URL_SAFE_NO_PAD.encode(json.as_bytes());

        let mut mac =
            HmacSha256::new_from_slice(signing_key).expect("HMAC can accept any key length");
        mac.update(payload.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

        format!("{}.{}", payload, signature)
    }

    /// 解码并验证签名
    pub fn decode(token: &str, signing_key: &[u8]) -> Result<Self, String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 2 {
            return Err("Invalid token format".to_string());
        }

        let payload = parts[0];
        let signature = parts[1];

        // 验证签名
        let mut mac =
            HmacSha256::new_from_slice(signing_key).expect("HMAC can accept any key length");
        mac.update(payload.as_bytes());

        let expected_sig = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| "Invalid signature encoding")?;

        mac.verify_slice(&expected_sig)
            .map_err(|_| "Signature verification failed")?;

        // 解码 payload
        let json_bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| "Invalid payload encoding")?;

        let claims: ExecutionClaims =
            serde_json::from_slice(&json_bytes).map_err(|e| format!("Invalid claims: {}", e))?;

        // 检查过期
        let now = Utc::now().timestamp();
        if claims.exp < now {
            return Err("Token expired".to_string());
        }

        Ok(claims)
    }
}

/// 验证 Execution Token 并返回 AuthUser
pub fn verify_execution_token(
    token: &str,
    signing_key: &[u8],
) -> Result<(ExecutionClaims, AuthUser), String> {
    let claims = ExecutionClaims::decode(token, signing_key)?;

    // 构建 AuthUser，使用 caller 信息
    let auth_user = AuthUser {
        id: claims.caller_user_id,
        org_id: claims.caller_org_id.clone(),
        external_id: None, // execution token 不携带 external_id
        name: None,
        role: "user".to_string(),
        is_api_key: false,
    };

    Ok((claims, auth_user))
}

pub fn generate_token(user_id: Uuid, org_id: String, role: String) -> Result<String, AppError> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::days(7))
        .expect("valid timestamp")
        .timestamp();
    let claims = Claims {
        sub: user_id.to_string(),
        exp: expiration as usize,
        org_id,
        role,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Token generation failed: {}", e)))
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// 验证 ORG 签发的 JWT（使用 ORG 的公钥）- 使用 Redis 缓存
async fn verify_org_signed_token(
    state: &AppState,
    token: &str,
) -> Result<(OrgSignedClaims, organizations::Model), AppError> {
    // 1. 先解码 header 获取算法
    let header = decode_header(token)
        .map_err(|e| AppError::Unauthorized(format!("Invalid token header: {}", e)))?;

    // 2. 不验证地解码 payload 获取 org_id
    let mut validation = Validation::default();
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;

    let token_data = decode::<OrgSignedClaims>(token, &DecodingKey::from_secret(&[]), &validation)
        .map_err(|e| AppError::Unauthorized(format!("Invalid token payload: {}", e)))?;

    let org_id = &token_data.claims.org;

    // 3. 查询 ORG (使用 Redis 缓存)
    let org = get_org_cached(state, org_id)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::Unauthorized(format!("Organization '{}' not found", org_id)))?;

    let public_key = org.public_key.as_ref().ok_or_else(|| {
        AppError::Unauthorized("Organization has no public key configured".into())
    })?;

    // 4. 根据算法构建解码密钥
    let algorithm = match org.key_algorithm.as_deref().unwrap_or("RS256") {
        "RS256" => Algorithm::RS256,
        "RS384" => Algorithm::RS384,
        "RS512" => Algorithm::RS512,
        "ES256" => Algorithm::ES256,
        "ES384" => Algorithm::ES384,
        "EdDSA" => Algorithm::EdDSA,
        alg => {
            return Err(AppError::Unauthorized(format!(
                "Unsupported algorithm: {}",
                alg
            )))
        }
    };

    let decoding_key = match algorithm {
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            DecodingKey::from_rsa_pem(public_key.as_bytes())
                .map_err(|e| AppError::Unauthorized(format!("Invalid RSA public key: {}", e)))?
        }
        Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(public_key.as_bytes())
            .map_err(|e| AppError::Unauthorized(format!("Invalid EC public key: {}", e)))?,
        Algorithm::EdDSA => DecodingKey::from_ed_pem(public_key.as_bytes())
            .map_err(|e| AppError::Unauthorized(format!("Invalid Ed25519 public key: {}", e)))?,
        _ => return Err(AppError::Unauthorized("Unsupported algorithm".into())),
    };

    // 5. 正式验证 token
    let mut validation = Validation::new(algorithm);
    validation.validate_exp = true;

    let verified_token = decode::<OrgSignedClaims>(token, &decoding_key, &validation)
        .map_err(|e| AppError::Unauthorized(format!("Token verification failed: {}", e)))?;

    Ok((verified_token.claims, org))
}

/// Admin auth middleware - validates X-Admin-Key header against ADMIN_API_KEY env var
pub async fn admin_auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let admin_key = req
        .headers()
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok());
    match (admin_key, &state.admin_key) {
        (Some(k), Some(expected)) if k == expected => Ok(next.run(req).await),
        (_, None) => Err(AppError::Forbidden("Admin API not configured".to_string())),
        _ => Err(AppError::Forbidden("Invalid admin key".to_string())),
    }
}

/// Optional auth middleware - doesn't fail on missing auth, just continues without user context
pub async fn optional_auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    match try_authenticate(state, req).await {
        Ok((parts, body)) => {
            let req = Request::from_parts(parts, body);
            next.run(req).await
        }
        Err((parts, body)) => {
            // Authentication failed, but we continue without user context
            let req = Request::from_parts(parts, body);
            next.run(req).await
        }
    }
}

/// Internal helper to attempt authentication
async fn try_authenticate(
    state: Arc<AppState>,
    req: Request<axum::body::Body>,
) -> Result<(Parts, axum::body::Body), (Parts, axum::body::Body)> {
    let (mut parts, body) = req.into_parts();

    // Try execution token first (highest priority)
    if let Some(token_header) = parts.headers.get("X-Execution-Token") {
        if let Ok(token_str) = token_header.to_str() {
            if let Ok((claims, auth_user)) = verify_execution_token(token_str, &state.signing_key) {
                parts.extensions.insert(auth_user);
                parts.extensions.insert(claims);
                return Ok((parts, body));
            }
        }
    }

    // Try org proxy auth (use cached lookup)
    let org_id_header = parts.headers.get("X-ORG-ID").and_then(|v| v.to_str().ok());
    let org_key_header = parts.headers.get("X-ORG-KEY").and_then(|v| v.to_str().ok());

    if let (Some(org_id), Some(org_key)) = (org_id_header, org_key_header) {
        if let Ok(Some(org)) = get_org_cached(&state, org_id).await {
            let hashed_input = hash_key(org_key);
            if hashed_input == org.api_key_hash {
                if let Some(ext_id) = parts.headers.get("X-USER-ID").and_then(|v| v.to_str().ok()) {
                    if let Ok(user) = get_shadow_user_cached(&state, org_id, ext_id).await {
                        let auth_user = AuthUser {
                            id: user.id,
                            org_id: user.org_id.unwrap_or_else(|| org_id.to_string()),
                            external_id: user.external_id,
                            name: user.name,
                            role: user.role,
                            is_api_key: true,
                        };
                        parts.extensions.insert(auth_user);
                        return Ok((parts, body));
                    }
                }
            }
        }
    }

    // Try API key auth (cached)
    if let Some(key_header) = parts.headers.get("X-API-KEY") {
        if let Ok(key_str) = key_header.to_str() {
            let hashed = hash_key(key_str);
            if let Ok(Some(user)) = get_user_by_api_key_cached(&state, &hashed).await {
                let auth_user = AuthUser {
                    id: user.id,
                    org_id: user
                        .org_id
                        .unwrap_or_else(|| crate::official_org_slug().to_string()),
                    external_id: user.external_id,
                    name: user.name,
                    role: user.role,
                    is_api_key: true,
                };
                parts.extensions.insert(auth_user);
                return Ok((parts, body));
            }
        }
    }

    // Try JWT auth - 先检查 algorithm 决定验证路径
    if let Ok(TypedHeader(auth_header)) =
        parts.extract::<TypedHeader<Authorization<Bearer>>>().await
    {
        let token = auth_header.token();

        if let Ok(header) = decode_header(token) {
            match header.alg {
                // HS256 = 内部 JWT（快速路径）
                Algorithm::HS256 => {
                    if let Ok(token_data) = decode::<Claims>(
                        token,
                        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
                        &Validation::default(),
                    ) {
                        let user_id = Uuid::parse_str(&token_data.claims.sub).unwrap_or_default();
                        let auth_user = AuthUser {
                            id: user_id,
                            org_id: token_data.claims.org_id,
                            external_id: None,
                            name: None,
                            role: token_data.claims.role,
                            is_api_key: false,
                        };
                        parts.extensions.insert(auth_user);
                        return Ok((parts, body));
                    }
                }
                // RS256/ES256/EdDSA = ORG-signed JWT
                _ => {
                    if let Ok((claims, org)) = verify_org_signed_token(&state, token).await {
                        if let Ok(user) = get_shadow_user_cached(&state, &org.id, &claims.sub).await
                        {
                            let auth_user = AuthUser {
                                id: user.id,
                                org_id: org.id,
                                external_id: user.external_id,
                                name: claims.name.or(user.name),
                                role: claims.role.unwrap_or_else(|| user.role),
                                is_api_key: false,
                            };
                            parts.extensions.insert(auth_user);
                            return Ok((parts, body));
                        }
                    }
                }
            }
        }
    }

    Err((parts, body))
}

/// Helper to get or create shadow user
async fn get_or_create_shadow_user(
    db: &sea_orm::DatabaseConnection,
    org_id: &str,
    ext_id: &str,
) -> Result<users::Model, sea_orm::DbErr> {
    let user_record = users::Entity::find()
        .filter(users::Column::OrgId.eq(org_id))
        .filter(users::Column::ExternalId.eq(ext_id))
        .one(db)
        .await?;

    match user_record {
        Some(u) => Ok(u),
        None => {
            let new_user = users::ActiveModel {
                id: Set(Uuid::new_v4()),
                org_id: Set(Some(org_id.to_string())),
                external_id: Set(Some(ext_id.to_string())),
                name: Set(Some(format!("User {}", &ext_id[0..4.min(ext_id.len())]))),
                role: Set("user".to_string()),
                ..Default::default()
            };
            new_user.insert(db).await
        }
    }
}

/// Cached organization lookup (Redis)
async fn get_org_cached(
    state: &AppState,
    org_id: &str,
) -> Result<Option<organizations::Model>, sea_orm::DbErr> {
    let key = format!("jug0:org:{}", org_id);

    // Check Redis first
    if let Some(org) = state.cache.get::<organizations::Model>(&key).await {
        return Ok(Some(org));
    }

    // Cache miss, fallback to DB
    tracing::warn!("⚠️ Cache MISS: org:{} - querying DB", org_id);
    let org = organizations::Entity::find_by_id(org_id)
        .one(&state.db)
        .await?;

    if let Some(ref o) = org {
        let _ = state.cache.set(&key, o, CACHE_TTL_SECS).await;
    }

    Ok(org)
}

/// Cached shadow user lookup (Redis)
async fn get_shadow_user_cached(
    state: &AppState,
    org_id: &str,
    ext_id: &str,
) -> Result<users::Model, sea_orm::DbErr> {
    let key = format!("jug0:user:{}:{}", org_id, ext_id);

    // Check Redis first
    if let Some(user) = state.cache.get::<users::Model>(&key).await {
        return Ok(user);
    }

    // Cache miss, fallback to DB
    tracing::warn!("⚠️ Cache MISS: user:{}:{} - querying DB", org_id, ext_id);
    let user = get_or_create_shadow_user(&state.db, org_id, ext_id).await?;

    let _ = state.cache.set(&key, &user, CACHE_TTL_SECS).await;

    Ok(user)
}

/// Cached API key → user lookup (Redis)
async fn get_user_by_api_key_cached(
    state: &AppState,
    key_hash: &str,
) -> Result<Option<users::Model>, sea_orm::DbErr> {
    let cache_key = format!("jug0:apikey:{}", key_hash);

    // Check Redis first
    if let Some(user) = state.cache.get::<users::Model>(&cache_key).await {
        return Ok(Some(user));
    }

    // Cache miss, fallback to DB
    tracing::warn!("⚠️ Cache MISS: apikey:{} - querying DB", &key_hash[..8]);
    let record = api_keys::Entity::find()
        .filter(api_keys::Column::KeyHash.eq(key_hash))
        .one(&state.db)
        .await?;

    if let Some(record) = record {
        let expired = record
            .expires_at
            .map(|exp| exp < chrono::Utc::now().naive_utc())
            .unwrap_or(false);
        if !expired {
            if let Some(user) = users::Entity::find_by_id(record.user_id)
                .one(&state.db)
                .await?
            {
                let _ = state.cache.set(&cache_key, &user, CACHE_TTL_SECS).await;
                return Ok(Some(user));
            }
        }
    }

    Ok(None)
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();

    // ---------------------------------------------------------
    // 路径 0: Execution Token (X-Execution-Token)
    // 用于 juglans-agent 回调 jug0，携带原始调用者信息
    // 最高优先级，因为这是可信的服务端签名
    // ---------------------------------------------------------
    if let Some(token_header) = parts.headers.get("X-Execution-Token") {
        if let Ok(token_str) = token_header.to_str() {
            match verify_execution_token(token_str, &state.signing_key) {
                Ok((claims, auth_user)) => {
                    tracing::debug!(
                        "🔐 [Auth] Execution token verified: caller={}, agent={}",
                        claims.caller_user_id,
                        claims.agent_id
                    );
                    parts.extensions.insert(auth_user);
                    parts.extensions.insert(claims); // 也存入 claims 供后续使用
                    let req = Request::from_parts(parts, body);
                    return Ok(next.run(req).await);
                }
                Err(e) => {
                    tracing::warn!("🔐 [Auth] Execution token invalid: {}", e);
                    return Err(AppError::Unauthorized(format!(
                        "Invalid execution token: {}",
                        e
                    )));
                }
            }
        }
    }

    // ---------------------------------------------------------
    // 路径 A: 机构代理认证 (X-ORG-ID + X-ORG-KEY + X-USER-ID)
    // ---------------------------------------------------------
    let org_id_header = parts.headers.get("X-ORG-ID").and_then(|v| v.to_str().ok());
    let org_key_header = parts.headers.get("X-ORG-KEY").and_then(|v| v.to_str().ok());

    if let (Some(org_id), Some(org_key)) = (org_id_header, org_key_header) {
        // Use cached organization lookup
        let org = get_org_cached(&state, org_id)
            .await
            .map_err(AppError::Database)?;

        let org = match org {
            Some(o) => o,
            None => return Err(AppError::Unauthorized("Invalid Organization ID".into())),
        };

        let hashed_input = hash_key(org_key);
        if hashed_input != org.api_key_hash {
            return Err(AppError::Unauthorized("Invalid Organization Key".into()));
        }

        let external_user_id = parts.headers.get("X-USER-ID").and_then(|v| v.to_str().ok());

        let user = if let Some(ext_id) = external_user_id {
            // Use cached shadow user lookup
            get_shadow_user_cached(&state, org_id, ext_id)
                .await
                .map_err(AppError::Database)?
        } else {
            return Err(AppError::BadRequest(
                "X-USER-ID header is required for Org Proxy mode".into(),
            ));
        };

        // 【修复】确保 org_id 不为空
        let final_org_id = user.org_id.clone().unwrap_or_else(|| org_id.to_string());

        let auth_user = AuthUser {
            id: user.id,
            org_id: final_org_id,
            external_id: user.external_id,
            name: user.name,
            role: user.role,
            is_api_key: true,
        };

        parts.extensions.insert(auth_user);
        let req = Request::from_parts(parts, body);
        return Ok(next.run(req).await);
    }

    // ---------------------------------------------------------
    // 路径 B: 个人 API Key (X-API-KEY) - 使用 Redis 缓存
    // ---------------------------------------------------------
    if let Some(key_header) = parts.headers.get("X-API-KEY") {
        if let Ok(key_str) = key_header.to_str() {
            let hashed = hash_key(key_str);

            match get_user_by_api_key_cached(&state, &hashed).await {
                Ok(Some(user)) => {
                    let final_org_id = user
                        .org_id
                        .unwrap_or_else(|| crate::official_org_slug().to_string());

                    let auth_user = AuthUser {
                        id: user.id,
                        org_id: final_org_id,
                        external_id: user.external_id,
                        name: user.name,
                        role: user.role,
                        is_api_key: true,
                    };
                    parts.extensions.insert(auth_user);
                    let req = Request::from_parts(parts, body);
                    return Ok(next.run(req).await);
                }
                Ok(None) => {
                    // Key not found or expired, fall through
                }
                Err(e) => {
                    return Err(AppError::Database(e));
                }
            }
        }
    }

    // ---------------------------------------------------------
    // 路径 C: JWT (Authorization: Bearer ...)
    // 优化: 先检查 algorithm 决定验证路径
    //   HS256 = 内部 JWT (路径 C2)
    //   RS256/ES256/EdDSA = ORG-signed JWT (路径 C1)
    // ---------------------------------------------------------
    if let Some(auth_header) = parts
        .extract::<TypedHeader<Authorization<Bearer>>>()
        .await
        .ok()
    {
        let token = auth_header.token();

        // 先检查 token 的 algorithm
        if let Ok(header) = decode_header(token) {
            match header.alg {
                // HS256 = 内部 JWT，直接用 JWT_SECRET 验证（快速路径）
                Algorithm::HS256 => {
                    if let Ok(token_data) = decode::<Claims>(
                        token,
                        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
                        &Validation::default(),
                    ) {
                        let user_id = Uuid::parse_str(&token_data.claims.sub).unwrap_or_default();

                        let auth_user = AuthUser {
                            id: user_id,
                            org_id: token_data.claims.org_id,
                            external_id: None,
                            name: None,
                            role: token_data.claims.role,
                            is_api_key: false,
                        };

                        parts.extensions.insert(auth_user);
                        let req = Request::from_parts(parts, body);
                        return Ok(next.run(req).await);
                    }
                }
                // RS256/ES256/EdDSA = ORG-signed JWT（需要公钥验证）
                _ => {
                    if let Ok((claims, org)) = verify_org_signed_token(&state, token).await {
                        tracing::debug!("🔐 [Auth] ORG-signed token verified for org: {}", org.id);

                        // 获取或创建 shadow user (使用缓存)
                        let user = get_shadow_user_cached(&state, &org.id, &claims.sub)
                            .await
                            .map_err(AppError::Database)?;

                        let auth_user = AuthUser {
                            id: user.id,
                            org_id: org.id,
                            external_id: user.external_id,
                            name: claims.name.or(user.name),
                            role: claims.role.unwrap_or_else(|| user.role),
                            is_api_key: false,
                        };

                        parts.extensions.insert(auth_user);
                        let req = Request::from_parts(parts, body);
                        return Ok(next.run(req).await);
                    }
                }
            }
        }

        return Err(AppError::Unauthorized("Invalid token".into()));
    }

    Err(AppError::Unauthorized(
        "Authentication failed: Missing credentials".into(),
    ))
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or(AppError::Unauthorized("User context missing".to_string()))
    }
}

/// Optional authentication - returns None if no auth provided, Some(user) if authenticated
#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<AuthUser>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalAuthUser(
            parts.extensions.get::<AuthUser>().cloned(),
        ))
    }
}
