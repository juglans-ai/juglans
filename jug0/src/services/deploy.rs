// src/services/deploy.rs
//
// Serverless deploy service: build Docker image → push ACR → create/update FC function

use anyhow::{Context, Result};
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tracing::{error, info};

/// 阿里云 FC 部署配置 (从环境变量读取)
#[derive(Clone)]
#[allow(dead_code)]
pub struct FcConfig {
    pub region: String,
    pub account_id: String,
    pub access_key_id: String,
    pub access_key_secret: String,
    pub acr_registry: String,
    pub acr_namespace: String,
    pub base_domain: String,
}

impl FcConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            region: std::env::var("FC_REGION").unwrap_or_else(|_| "cn-hangzhou".into()),
            account_id: std::env::var("FC_ACCOUNT_ID").ok()?,
            access_key_id: std::env::var("ALIYUN_ACCESS_KEY_ID").ok()?,
            access_key_secret: std::env::var("ALIYUN_ACCESS_KEY_SECRET").ok()?,
            acr_registry: std::env::var("ACR_REGISTRY")
                .unwrap_or_else(|_| "registry.cn-hangzhou.aliyuncs.com".into()),
            acr_namespace: std::env::var("ACR_NAMESPACE").unwrap_or_else(|_| "juglans".into()),
            base_domain: std::env::var("DEPLOY_BASE_DOMAIN")
                .unwrap_or_else(|_| "juglans.app".into()),
        })
    }

    pub fn image_uri(&self, slug: &str) -> String {
        format!(
            "{}/{}/{}:latest",
            self.acr_registry, self.acr_namespace, slug
        )
    }

    pub fn deploy_url(&self, slug: &str) -> String {
        format!("https://{}.{}", slug, self.base_domain)
    }
}

/// 阿里云 ACS3-HMAC-SHA256 签名
fn sign_fc_request(
    fc_config: &FcConfig,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    action: &str,
) -> HeaderMap {
    let mut headers = HeaderMap::new();

    let host = format!(
        "{}.{}.fc.aliyuncs.com",
        fc_config.account_id, fc_config.region
    );
    let date = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let nonce = uuid::Uuid::new_v4().to_string();

    // Hash body
    let body_hash = hex::encode(Sha256::digest(body));

    headers.insert("host", HeaderValue::from_str(&host).unwrap());
    headers.insert("x-acs-date", HeaderValue::from_str(&date).unwrap());
    headers.insert(
        "x-acs-content-sha256",
        HeaderValue::from_str(&body_hash).unwrap(),
    );
    headers.insert(
        "x-acs-signature-nonce",
        HeaderValue::from_str(&nonce).unwrap(),
    );
    headers.insert(
        "x-acs-version",
        HeaderValue::from_static("2023-03-30"),
    );
    headers.insert("x-acs-action", HeaderValue::from_str(action).unwrap());

    // Build canonical headers (sorted by key)
    let mut signed_header_names: Vec<&str> = vec![
        "host",
        "x-acs-action",
        "x-acs-content-sha256",
        "x-acs-date",
        "x-acs-signature-nonce",
        "x-acs-version",
    ];
    signed_header_names.sort();

    let canonical_headers: String = signed_header_names
        .iter()
        .map(|k| format!("{}:{}\n", k, headers.get(*k).unwrap().to_str().unwrap()))
        .collect();

    let signed_headers = signed_header_names.join(";");

    // Canonical request
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, path, query, canonical_headers, signed_headers, body_hash
    );

    // String to sign
    let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!("ACS3-HMAC-SHA256\n{}", canonical_hash);

    // HMAC-SHA256
    type HmacSha256 = Hmac<Sha256>;
    let mut mac =
        HmacSha256::new_from_slice(fc_config.access_key_secret.as_bytes()).unwrap();
    mac.update(string_to_sign.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Authorization header
    let auth = format!(
        "ACS3-HMAC-SHA256 Credential={},SignedHeaders={},Signature={}",
        fc_config.access_key_id, signed_headers, signature
    );
    headers.insert("authorization", HeaderValue::from_str(&auth).unwrap());

    headers
}

/// FC API endpoint URL
fn fc_endpoint(fc_config: &FcConfig) -> String {
    format!(
        "https://{}.{}.fc.aliyuncs.com",
        fc_config.account_id, fc_config.region
    )
}

/// 验证 slug 合法性
pub fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.len() < 3 || slug.len() > 30 {
        return Err("slug must be 3-30 characters".into());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("slug must contain only lowercase letters, digits, and hyphens".into());
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err("slug must not start or end with a hyphen".into());
    }
    Ok(())
}

/// 构建参数
pub struct BuildParams<'a> {
    pub slug: &'a str,
    pub repo: &'a str,
    pub branch: &'a str,
    pub env_vars: &'a HashMap<String, String>,
    pub jug0_base_url: &'a str,
    pub api_key: &'a str,
}

/// 从 GitHub 下载 repo archive 并构建 Docker 镜像
pub async fn build_and_push(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    params: &BuildParams<'_>,
) -> Result<String> {
    let slug = params.slug;
    let repo = params.repo;
    let branch = params.branch;
    let env_vars = params.env_vars;
    let tmp_dir = std::env::temp_dir().join(format!("juglans-build-{}", slug));
    if tmp_dir.exists() {
        tokio::fs::remove_dir_all(&tmp_dir).await.ok();
    }
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .context("create temp build dir")?;

    // 1. 下载 GitHub repo archive
    info!("[Deploy:{}] Downloading repo {}/{}", slug, repo, branch);
    let archive_url = format!("https://api.github.com/repos/{}/tarball/{}", repo, branch);
    let resp = http_client
        .get(&archive_url)
        .header("User-Agent", "juglans-deploy")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("download repo archive")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API error: {} {}", status, body);
    }

    let archive_path = tmp_dir.join("repo.tar.gz");
    let bytes = resp.bytes().await?;
    tokio::fs::write(&archive_path, &bytes).await?;

    // 2. 解压 (tar -xzf strips the top-level directory)
    let extract_dir = tmp_dir.join("src");
    tokio::fs::create_dir_all(&extract_dir).await?;

    let status = tokio::process::Command::new("tar")
        .args([
            "-xzf",
            archive_path.to_str().unwrap(),
            "--strip-components=1",
            "-C",
            extract_dir.to_str().unwrap(),
        ])
        .status()
        .await
        .context("extract archive")?;

    if !status.success() {
        anyhow::bail!("tar extraction failed");
    }

    // 3. 生成 Dockerfile
    let mut env_lines = String::new();
    env_lines.push_str(&format!(
        "ENV JUG0_BASE_URL={} JUG0_API_KEY={} SERVER_HOST=0.0.0.0 SERVER_PORT=9000\n",
        params.jug0_base_url, params.api_key
    ));
    for (k, v) in env_vars {
        env_lines.push_str(&format!("ENV {}={}\n", k, v));
    }

    let dockerfile = format!(
        r#"FROM juglansai/juglans:latest
COPY src/ /workspace/
WORKDIR /workspace
{}CMD ["juglans", "web", "--host", "0.0.0.0", "--port", "9000"]
EXPOSE 9000
"#,
        env_lines
    );

    tokio::fs::write(tmp_dir.join("Dockerfile"), &dockerfile).await?;

    // 4. Docker build
    let image_uri = fc_config.image_uri(slug);
    info!("[Deploy:{}] Building image: {}", slug, image_uri);

    let build_output = tokio::process::Command::new("docker")
        .args(["build", "--platform", "linux/amd64", "--provenance=false", "-t", &image_uri, tmp_dir.to_str().unwrap()])
        .output()
        .await
        .context("docker build")?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        error!("[Deploy:{}] Docker build failed: {}", slug, stderr);
        anyhow::bail!("docker build failed: {}", stderr);
    }

    // 5. Docker push
    info!("[Deploy:{}] Pushing image: {}", slug, image_uri);

    let push_output = tokio::process::Command::new("docker")
        .args(["push", &image_uri])
        .output()
        .await
        .context("docker push")?;

    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        error!("[Deploy:{}] Docker push failed: {}", slug, stderr);
        anyhow::bail!("docker push failed: {}", stderr);
    }

    // 6. 清理
    tokio::fs::remove_dir_all(&tmp_dir).await.ok();

    info!("[Deploy:{}] Image pushed: {}", slug, image_uri);
    Ok(image_uri)
}

/// 调用阿里云 FC API 创建/更新函数
///
/// 使用 FC 3.0 HTTP API (2023-03-30 版本) + ACS3-HMAC-SHA256 签名
pub async fn create_or_update_fc_function(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    slug: &str,
    image_uri: &str,
    env_vars: &HashMap<String, String>,
) -> Result<String> {
    let endpoint = fc_endpoint(fc_config);

    // 检查函数是否已存在
    let check_path = format!("/2023-03-30/functions/{}", slug);
    let check_headers =
        sign_fc_request(fc_config, "GET", &check_path, "", b"", "GetFunction");
    let check_url = format!("{}{}", endpoint, check_path);
    let check_resp = http_client
        .get(&check_url)
        .headers(check_headers)
        .send()
        .await;

    let function_exists = check_resp.map(|r| r.status().is_success()).unwrap_or(false);

    let body = json!({
        "functionName": slug,
        "runtime": "custom-container",
        "handler": "index.handler",
        "customContainerConfig": {
            "image": image_uri,
            "port": 9000,
            "accelerationType": "None",
        },
        "memorySize": 512,
        "timeout": 300,
        "instanceConcurrency": 10,
        "environmentVariables": env_vars,
    });

    let body_bytes = serde_json::to_vec(&body)?;

    let (method, path, action) = if function_exists {
        info!("[Deploy:{}] Updating existing FC function", slug);
        (
            "PUT",
            format!("/2023-03-30/functions/{}", slug),
            "UpdateFunction",
        )
    } else {
        info!("[Deploy:{}] Creating new FC function", slug);
        ("POST", "/2023-03-30/functions".to_string(), "CreateFunction")
    };

    let mut headers = sign_fc_request(fc_config, method, &path, "", &body_bytes, action);
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let url = format!("{}{}", endpoint, path);
    let resp = if method == "PUT" {
        http_client.put(&url)
    } else {
        http_client.post(&url)
    }
    .headers(headers)
    .body(body_bytes)
    .send()
    .await
    .context("FC API call")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("FC API error: {} {}", status, body);
    }

    let deploy_url = fc_config.deploy_url(slug);
    info!("[Deploy:{}] FC function ready: {}", slug, deploy_url);
    Ok(deploy_url)
}

/// Create HTTP trigger for public URL access.
/// FC 3.0 requires an HTTP trigger before the function can be invoked via URL.
/// Returns the public URL (urlInternet) from the trigger response.
/// Fetches existing trigger URL if the trigger already exists (409 Conflict).
pub async fn ensure_http_trigger(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    slug: &str,
) -> Result<Option<String>> {
    let endpoint = fc_endpoint(fc_config);
    let path = format!("/2023-03-30/functions/{}/triggers", slug);

    let trigger_config = serde_json::json!({
        "methods": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
        "authType": "anonymous"
    });

    let body = json!({
        "triggerName": "http-trigger",
        "triggerType": "http",
        "triggerConfig": trigger_config.to_string(),
        "description": "HTTP trigger for public access"
    });

    let body_bytes = serde_json::to_vec(&body)?;

    let mut headers =
        sign_fc_request(fc_config, "POST", &path, "", &body_bytes, "CreateTrigger");
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let url = format!("{}{}", endpoint, path);
    let resp = http_client
        .post(&url)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await
        .context("FC create HTTP trigger")?;

    if resp.status() == reqwest::StatusCode::CONFLICT {
        info!("[Deploy:{}] HTTP trigger already exists, fetching URL", slug);
        // Fetch existing trigger to get the URL
        return get_trigger_url(http_client, fc_config, slug).await;
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("FC create trigger error: {} {}", status, body);
    }

    let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
    let trigger_url = resp_body["httpTrigger"]["urlInternet"]
        .as_str()
        .map(|s| s.to_string());

    info!(
        "[Deploy:{}] HTTP trigger created, URL: {:?}",
        slug, trigger_url
    );
    Ok(trigger_url)
}

/// Get the public URL of an existing HTTP trigger
pub async fn get_trigger_url(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    slug: &str,
) -> Result<Option<String>> {
    let endpoint = fc_endpoint(fc_config);
    let path = format!(
        "/2023-03-30/functions/{}/triggers/http-trigger",
        slug
    );

    let headers = sign_fc_request(fc_config, "GET", &path, "", b"", "GetTrigger");
    let url = format!("{}{}", endpoint, path);

    let resp = http_client
        .get(&url)
        .headers(headers)
        .send()
        .await
        .context("FC get trigger")?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let resp_body: serde_json::Value = resp.json().await.unwrap_or_default();
    let trigger_url = resp_body["httpTrigger"]["urlInternet"]
        .as_str()
        .map(|s| s.to_string());

    Ok(trigger_url)
}

/// Bind custom domain {slug}.juglans.app → FC function
pub async fn bind_custom_domain(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    slug: &str,
) -> Result<()> {
    let endpoint = fc_endpoint(fc_config);
    let domain_name = format!("{}.{}", slug, fc_config.base_domain);

    let body = json!({
        "domainName": domain_name,
        "protocol": "HTTP",
        "routeConfig": {
            "routes": [{
                "path": "/*",
                "functionName": slug,
                "methods": ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            }]
        }
    });

    let body_bytes = serde_json::to_vec(&body)?;
    let path = "/2023-03-30/custom-domains";

    let mut headers =
        sign_fc_request(fc_config, "POST", path, "", &body_bytes, "CreateCustomDomain");
    headers.insert("content-type", HeaderValue::from_static("application/json"));

    let url = format!("{}{}", endpoint, path);
    let resp = http_client
        .post(&url)
        .headers(headers)
        .body(body_bytes.clone())
        .send()
        .await
        .context("FC custom domain create")?;

    if resp.status() == reqwest::StatusCode::CONFLICT {
        // 已存在，更新路由
        let update_path = format!("/2023-03-30/custom-domains/{}", domain_name);
        let mut headers = sign_fc_request(
            fc_config,
            "PUT",
            &update_path,
            "",
            &body_bytes,
            "UpdateCustomDomain",
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        let update_url = format!("{}{}", endpoint, update_path);
        let resp = http_client
            .put(&update_url)
            .headers(headers)
            .body(body_bytes)
            .send()
            .await
            .context("FC custom domain update")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("FC custom domain update error: {}", body);
        }
    } else if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("FC custom domain create error: {}", body);
    }

    info!(
        "[Deploy:{}] Custom domain bound: {}",
        slug, domain_name
    );
    Ok(())
}

/// 解绑自定义域名
pub async fn unbind_custom_domain(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    slug: &str,
) -> Result<()> {
    let endpoint = fc_endpoint(fc_config);
    let domain_name = format!("{}.{}", slug, fc_config.base_domain);
    let path = format!("/2023-03-30/custom-domains/{}", domain_name);

    let headers =
        sign_fc_request(fc_config, "DELETE", &path, "", b"", "DeleteCustomDomain");
    let url = format!("{}{}", endpoint, path);

    let resp = http_client
        .delete(&url)
        .headers(headers)
        .send()
        .await
        .context("FC custom domain delete")?;

    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("FC custom domain delete error: {}", body);
    }

    info!("[Deploy:{}] Custom domain unbound: {}", slug, domain_name);
    Ok(())
}

/// 删除 FC 函数
pub async fn delete_fc_function(
    http_client: &reqwest::Client,
    fc_config: &FcConfig,
    slug: &str,
) -> Result<()> {
    let endpoint = fc_endpoint(fc_config);
    let path = format!("/2023-03-30/functions/{}", slug);

    let headers =
        sign_fc_request(fc_config, "DELETE", &path, "", b"", "DeleteFunction");
    let url = format!("{}{}", endpoint, path);

    let resp = http_client
        .delete(&url)
        .headers(headers)
        .send()
        .await
        .context("FC delete")?;

    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("FC delete error: {}", body);
    }

    info!("[Deploy:{}] FC function deleted", slug);
    Ok(())
}

/// 查询部署状态摘要
#[allow(dead_code)]
pub fn status_display(status: &str) -> &'static str {
    match status {
        "pending" => "Queued",
        "building" => "Building image...",
        "deploying" => "Deploying to FC...",
        "deployed" => "Live",
        "failed" => "Failed",
        "deleted" => "Deleted",
        _ => "Unknown",
    }
}
