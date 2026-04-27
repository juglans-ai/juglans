// src/adapters/wechat.rs
//! WeChat adapter using iLink Bot API (getUpdates long-poll + sendMessage).
//!
//! Protocol reverse-engineered from @tencent-weixin/openclaw-weixin plugin.

use anyhow::{anyhow, Result};
use base64::Engine;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::{LocalDispatcher, MessageDispatcher, PlatformMessage};
use crate::services::config::JuglansConfig;

// ── Constants ────────────────────────────────────────────────────────────────

const FIXED_QR_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const BOT_TYPE: &str = "3";
const CHANNEL_VERSION: &str = "2.1.1";
const DEFAULT_LONG_POLL_TIMEOUT_MS: u64 = 35_000;
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
const BACKOFF_DELAY_MS: u64 = 30_000;
const RETRY_DELAY_MS: u64 = 2_000;
const MAX_QR_REFRESH_COUNT: u32 = 3;
const QR_POLL_TIMEOUT_MS: u64 = 35_000;
const SESSION_EXPIRED_ERRCODE: i64 = -14;

// ── API Types ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct QrCodeResponse {
    qrcode: Option<String>,
    qrcode_img_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrStatusResponse {
    status: Option<String>,
    bot_token: Option<String>,
    ilink_bot_id: Option<String>,
    baseurl: Option<String>,
    ilink_user_id: Option<String>,
    redirect_host: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetUpdatesResp {
    ret: Option<i64>,
    errcode: Option<i64>,
    errmsg: Option<String>,
    msgs: Option<Vec<WeixinMessage>>,
    get_updates_buf: Option<String>,
    longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WeixinMessage {
    from_user_id: Option<String>,
    context_token: Option<String>,
    item_list: Option<Vec<MessageItem>>,
}

#[derive(Debug, Deserialize)]
struct MessageItem {
    #[serde(rename = "type")]
    item_type: Option<i32>,
    text_item: Option<TextItem>,
    image_item: Option<ImageItem>,
    voice_item: Option<VoiceItem>,
    file_item: Option<FileItem>,
}

#[derive(Debug, Deserialize)]
struct TextItem {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CdnMedia {
    encrypt_query_param: Option<String>,
    aes_key: Option<String>,
    full_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageItem {
    media: Option<CdnMedia>,
    /// Raw AES-128 key as hex string; preferred over media.aes_key for inbound decryption.
    aeskey: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VoiceItem {
    #[allow(dead_code)]
    media: Option<CdnMedia>,
    /// Voice-to-text transcription (done server-side by WeChat).
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileItem {
    media: Option<CdnMedia>,
    file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetConfigResp {
    #[allow(dead_code)]
    ret: Option<i64>,
    typing_ticket: Option<String>,
}

// ── Login Result ─────────────────────────────────────────────────────────────

/// Output of a successful WeChat login (either fresh QR scan or restored from
/// disk). Public so external orchestrators (juglans-wallet) can drive the
/// login flow themselves and feed the result into [`message_loop`].
pub struct LoginResult {
    pub bot_token: String,
    pub base_url: String,
    pub account_id: String,
    pub user_id: Option<String>,
}

// ── Account Persistence ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct AccountData {
    token: String,
    base_url: String,
    user_id: Option<String>,
    saved_at: String,
}

fn wechat_state_dir(project_root: &Path) -> PathBuf {
    project_root.join(".juglans").join("wechat")
}

fn normalize_account_id(raw: &str) -> String {
    raw.replace(['@', '.'], "-")
}

fn save_account(project_root: &Path, account_id: &str, data: &AccountData) -> Result<()> {
    let dir = wechat_state_dir(project_root);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", account_id));
    let content = serde_json::to_string_pretty(data)?;
    fs::write(&path, content)?;
    info!("[wechat] Account saved: {}", path.display());
    Ok(())
}

fn load_account(project_root: &Path) -> Option<(String, AccountData)> {
    let dir = wechat_state_dir(project_root);
    if !dir.exists() {
        return None;
    }
    // Find the first .json account file
    let entries = fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && !path
                .file_name()
                .and_then(|f| f.to_str())
                .map(|f| f.contains("sync") || f.contains("context"))
                .unwrap_or(false)
        {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(data) = serde_json::from_str::<AccountData>(&content) {
                    let account_id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    return Some((account_id, data));
                }
            }
        }
    }
    None
}

fn save_sync_buf(project_root: &Path, account_id: &str, buf: &str) -> Result<()> {
    let dir = wechat_state_dir(project_root);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.sync.json", account_id));
    fs::write(&path, buf)?;
    Ok(())
}

fn load_sync_buf(project_root: &Path, account_id: &str) -> Option<String> {
    let path = wechat_state_dir(project_root).join(format!("{}.sync.json", account_id));
    fs::read_to_string(path).ok()
}

// ── Pending-QR persistence (external controller hook) ────────────────────────
//
// While a QR login is in flight we write two files so external tools (e.g.
// juglans-wallet) can show the QR to an end-user and observe progress:
//   .juglans/wechat/qr.pending.txt   — raw QR payload (the string the WeChat
//                                      app scans); external tools can render
//                                      it as an image
//   .juglans/wechat/qr.pending.json  — { qrcode_id, status, created_at,
//                                      expires_at }
// Files are deleted once login is confirmed. Expiry is a soft hint — the
// source of truth is the iLink poll loop.

const QR_PENDING_LIFETIME_SECS: i64 = 240;

#[derive(Debug, Serialize, Deserialize)]
struct PendingQrMeta {
    qrcode_id: String,
    status: String,
    created_at: String,
    expires_at: String,
}

fn write_pending_qr(
    project_root: &Path,
    qrcode_id: &str,
    qrcode_payload: &str,
    status: &str,
) -> Result<()> {
    let dir = wechat_state_dir(project_root);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("qr.pending.txt"), qrcode_payload)?;
    let now = chrono::Utc::now();
    let meta = PendingQrMeta {
        qrcode_id: qrcode_id.to_string(),
        status: status.to_string(),
        created_at: now.to_rfc3339(),
        expires_at: (now + chrono::Duration::seconds(QR_PENDING_LIFETIME_SECS)).to_rfc3339(),
    };
    fs::write(
        dir.join("qr.pending.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;
    Ok(())
}

fn update_pending_qr_status(project_root: &Path, status: &str) -> Result<()> {
    let path = wechat_state_dir(project_root).join("qr.pending.json");
    let body = fs::read_to_string(&path)?;
    let mut meta: PendingQrMeta = serde_json::from_str(&body)?;
    meta.status = status.to_string();
    fs::write(&path, serde_json::to_string_pretty(&meta)?)?;
    Ok(())
}

fn clear_pending_qr(project_root: &Path) {
    let dir = wechat_state_dir(project_root);
    let _ = fs::remove_file(dir.join("qr.pending.txt"));
    let _ = fs::remove_file(dir.join("qr.pending.json"));
}

// ── Context Token Store ──────────────────────────────────────────────────────

type ContextTokenStore = Arc<RwLock<HashMap<String, String>>>;

fn context_token_key(account_id: &str, user_id: &str) -> String {
    format!("{}:{}", account_id, user_id)
}

// ── HTTP Helpers ─────────────────────────────────────────────────────────────

fn random_wechat_uin() -> String {
    let n: u32 = rand::rng().random();
    let decimal = n.to_string();
    base64::engine::general_purpose::STANDARD.encode(decimal.as_bytes())
}

fn build_base_info() -> Value {
    json!({ "channel_version": CHANNEL_VERSION })
}

fn build_headers(token: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert(
        "AuthorizationType",
        HeaderValue::from_static("ilink_bot_token"),
    );
    headers.insert(
        "X-WECHAT-UIN",
        HeaderValue::from_str(&random_wechat_uin()).unwrap_or(HeaderValue::from_static("")),
    );
    if let Some(t) = token {
        if !t.is_empty() {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", t)) {
                headers.insert("Authorization", val);
            }
        }
    }
    headers
}

fn ensure_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{}/", url)
    }
}

// ── CDN Media Download + AES-128-ECB Decrypt ────────────────────────────────

const CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";

/// Parse AES key from base64. Two encodings in the wild:
/// - base64 → 16 raw bytes (images)
/// - base64 → 32 hex ASCII chars → hex decode → 16 bytes (file/voice/video)
fn parse_aes_key(aes_key_base64: &str) -> Result<[u8; 16]> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(aes_key_base64)
        .map_err(|e| anyhow!("AES key base64 decode error: {}", e))?;

    let decoded_len = decoded.len();

    if decoded_len == 16 {
        let mut key = [0u8; 16];
        key.copy_from_slice(&decoded);
        return Ok(key);
    }
    if decoded_len == 32 {
        // hex-encoded: base64 → hex string → raw bytes
        let hex_str =
            String::from_utf8(decoded).map_err(|e| anyhow!("AES key hex decode error: {}", e))?;
        let raw = hex::decode(&hex_str).map_err(|e| anyhow!("AES key hex parse error: {}", e))?;
        if raw.len() == 16 {
            let mut key = [0u8; 16];
            key.copy_from_slice(&raw);
            return Ok(key);
        }
    }
    Err(anyhow!(
        "Invalid AES key: expected 16 or 32 bytes after base64 decode, got {}",
        decoded_len
    ))
}

/// Decrypt AES-128-ECB with PKCS7 padding.
fn decrypt_aes_ecb(ciphertext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    use aes::Aes128;
    use ecb::cipher::block_padding::Pkcs7;
    use ecb::cipher::BlockDecryptMut;
    use ecb::cipher::KeyInit;
    use ecb::Decryptor;

    type Aes128EcbDec = Decryptor<Aes128>;

    let decryptor = Aes128EcbDec::new(key.into());
    let mut buf = ciphertext.to_vec();
    let plaintext = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("AES-ECB decrypt error: {}", e))?;
    Ok(plaintext.to_vec())
}

/// Download from CDN and decrypt with AES-128-ECB.
async fn download_and_decrypt_media(
    http: &reqwest::Client,
    media: &CdnMedia,
    aes_key_base64: &str,
    label: &str,
) -> Result<Vec<u8>> {
    let url = if let Some(ref u) = media.full_url {
        u.clone()
    } else if let Some(ref param) = media.encrypt_query_param {
        format!(
            "{}/download?encrypted_query_param={}",
            CDN_BASE_URL,
            urlencoding::encode(param)
        )
    } else {
        return Err(anyhow!("{}: no download URL available", label));
    };

    debug!(
        "[wechat] {} downloading: {}",
        label,
        &url[..url.len().min(80)]
    );

    let resp = http
        .get(&url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| anyhow!("{}: CDN download failed: {}", label, e))?;

    if !resp.status().is_success() {
        return Err(anyhow!("{}: CDN download HTTP {}", label, resp.status()));
    }

    let encrypted = resp.bytes().await?;
    debug!(
        "[wechat] {} downloaded {} bytes, decrypting",
        label,
        encrypted.len()
    );

    let key = parse_aes_key(aes_key_base64)?;
    let decrypted = decrypt_aes_ecb(&encrypted, &key)?;
    debug!("[wechat] {} decrypted {} bytes", label, decrypted.len());

    Ok(decrypted)
}

/// Save media bytes to .juglans/wechat/media/ and return the file path.
fn save_media_file(project_root: &Path, data: &[u8], filename: &str) -> Result<String> {
    let dir = wechat_state_dir(project_root).join("media");
    fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    fs::write(&path, data)?;
    Ok(path.to_string_lossy().to_string())
}

// ── QR Login ─────────────────────────────────────────────────────────────────

/// Drive the iLink QR login flow: fetch the QR, write it to
/// `<workspace>/.juglans/wechat/qr.pending.{txt,json}` so external observers
/// can render it, and poll until the user confirms on their phone.
/// Returns the resulting bot token / base URL / account id.
pub async fn qr_login(http: &reqwest::Client, workspace: &Path) -> Result<LoginResult> {
    // Step 1: Get QR code
    let qr_url = format!(
        "{}ilink/bot/get_bot_qrcode?bot_type={}",
        ensure_trailing_slash(FIXED_QR_BASE_URL),
        BOT_TYPE
    );

    let resp = http
        .get(&qr_url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    let qr_resp: QrCodeResponse = resp.json().await?;

    let qrcode = qr_resp
        .qrcode
        .ok_or_else(|| anyhow!("No qrcode in response"))?;
    let qrcode_url = qr_resp
        .qrcode_img_content
        .ok_or_else(|| anyhow!("No qrcode_img_content in response"))?;

    // Render QR code in terminal
    render_qr_terminal(&qrcode_url);

    // Persist QR to workspace so external controllers (e.g. juglans-wallet)
    // can surface it to end-users without sharing the terminal.
    if let Err(e) = write_pending_qr(workspace, &qrcode, &qrcode_url, "awaiting_scan") {
        warn!("[wechat] could not persist pending QR: {}", e);
    }

    // Step 2: Poll for QR status
    let mut current_base_url = FIXED_QR_BASE_URL.to_string();
    let mut qrcode = qrcode;
    #[allow(unused_assignments)]
    let mut qrcode_url = qrcode_url;
    let mut scanned_printed = false;
    let mut qr_refresh_count: u32 = 1;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(480);

    loop {
        if std::time::Instant::now() > deadline {
            return Err(anyhow!("Login timed out"));
        }

        let status_url = format!(
            "{}ilink/bot/get_qrcode_status?qrcode={}",
            ensure_trailing_slash(&current_base_url),
            urlencoding::encode(&qrcode)
        );

        let status_resp = http
            .get(&status_url)
            .timeout(std::time::Duration::from_millis(QR_POLL_TIMEOUT_MS + 5_000))
            .send()
            .await;

        let status: QrStatusResponse = match status_resp {
            Ok(r) => match r.json().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("[wechat] QR status parse error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            },
            Err(e) => {
                if e.is_timeout() {
                    debug!("[wechat] QR poll timeout, retrying");
                    continue;
                }
                warn!("[wechat] QR poll error: {}", e);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };

        match status.status.as_deref() {
            Some("wait") => {
                // Still waiting
            }
            Some("scaned") => {
                if !scanned_printed {
                    info!("[wechat] QR code scanned, waiting for confirmation...");
                    println!("👀 已扫码，在微信继续操作...");
                    scanned_printed = true;
                    let _ = update_pending_qr_status(workspace, "scanned");
                }
            }
            Some("scaned_but_redirect") => {
                if let Some(host) = &status.redirect_host {
                    current_base_url = format!("https://{}", host);
                    info!("[wechat] IDC redirect to {}", current_base_url);
                }
            }
            Some("expired") => {
                qr_refresh_count += 1;
                if qr_refresh_count > MAX_QR_REFRESH_COUNT {
                    return Err(anyhow!("QR code expired too many times"));
                }
                println!(
                    "⏳ 二维码已过期，正在刷新...({}/{})",
                    qr_refresh_count, MAX_QR_REFRESH_COUNT
                );

                // Refresh QR
                let resp = http
                    .get(format!(
                        "{}ilink/bot/get_bot_qrcode?bot_type={}",
                        ensure_trailing_slash(FIXED_QR_BASE_URL),
                        BOT_TYPE
                    ))
                    .timeout(std::time::Duration::from_secs(10))
                    .send()
                    .await?;
                let new_qr: QrCodeResponse = resp.json().await?;
                qrcode = new_qr.qrcode.ok_or_else(|| anyhow!("No qrcode"))?;
                qrcode_url = new_qr
                    .qrcode_img_content
                    .ok_or_else(|| anyhow!("No qrcode_img_content"))?;
                scanned_printed = false;
                println!("🔄 新二维码已生成，请重新扫描\n");
                render_qr_terminal(&qrcode_url);
                if let Err(e) = write_pending_qr(workspace, &qrcode, &qrcode_url, "awaiting_scan") {
                    warn!("[wechat] could not persist refreshed QR: {}", e);
                }
            }
            Some("confirmed") => {
                let bot_id = status
                    .ilink_bot_id
                    .ok_or_else(|| anyhow!("confirmed but no ilink_bot_id"))?;
                let bot_token = status
                    .bot_token
                    .ok_or_else(|| anyhow!("confirmed but no bot_token"))?;
                let base_url = status
                    .baseurl
                    .unwrap_or_else(|| FIXED_QR_BASE_URL.to_string());

                println!("\n✅ 与微信连接成功！");
                info!("[wechat] Login confirmed: account_id={}", bot_id);
                let _ = update_pending_qr_status(workspace, "confirmed");
                clear_pending_qr(workspace);

                return Ok(LoginResult {
                    bot_token,
                    base_url,
                    account_id: bot_id,
                    user_id: status.ilink_user_id,
                });
            }
            other => {
                warn!("[wechat] Unknown QR status: {:?}", other);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

fn render_qr_terminal(url: &str) {
    use qrcode::QrCode;

    println!("\n请使用微信扫描以下二维码：\n");

    match QrCode::new(url) {
        Ok(code) => {
            let string = code
                .render::<char>()
                .quiet_zone(false)
                .module_dimensions(2, 1)
                .build();
            println!("{}", string);
        }
        Err(e) => {
            warn!("[wechat] QR render error: {}", e);
        }
    }

    println!("\n如果二维码无法展示，请用浏览器打开以下链接扫码：");
    println!("{}\n", url);
}

// ── Message Extraction ───────────────────────────────────────────────────────

struct ExtractedMessage {
    text: String,
    media_path: Option<String>,
    media_type: Option<String>,
    file_name: Option<String>,
}

async fn extract_message(
    msg: &WeixinMessage,
    http: &reqwest::Client,
    project_root: &Path,
) -> ExtractedMessage {
    let items = match &msg.item_list {
        Some(items) => items,
        None => {
            return ExtractedMessage {
                text: String::new(),
                media_path: None,
                media_type: None,
                file_name: None,
            }
        }
    };

    for item in items {
        match item.item_type {
            // TEXT
            Some(1) => {
                if let Some(ref ti) = item.text_item {
                    if let Some(ref text) = ti.text {
                        return ExtractedMessage {
                            text: text.clone(),
                            media_path: None,
                            media_type: None,
                            file_name: None,
                        };
                    }
                }
            }
            // IMAGE
            Some(2) => {
                if let Some(ref img) = item.image_item {
                    if let Some(ref media) = img.media {
                        // Resolve AES key: prefer image_item.aeskey (hex) over media.aes_key (base64)
                        let aes_key_b64 = if let Some(ref hex_key) = img.aeskey {
                            // hex → raw bytes → base64
                            match hex::decode(hex_key) {
                                Ok(raw) => base64::engine::general_purpose::STANDARD.encode(&raw),
                                Err(e) => {
                                    warn!("[wechat] image aeskey hex decode error: {}", e);
                                    media.aes_key.clone().unwrap_or_default()
                                }
                            }
                        } else {
                            media.aes_key.clone().unwrap_or_default()
                        };

                        if !aes_key_b64.is_empty() {
                            match download_and_decrypt_media(http, media, &aes_key_b64, "image")
                                .await
                            {
                                Ok(data) => {
                                    let ts = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_millis())
                                        .unwrap_or(0);
                                    let filename = format!("{}.jpg", ts);
                                    match save_media_file(project_root, &data, &filename) {
                                        Ok(path) => {
                                            return ExtractedMessage {
                                                text: "[图片]".into(),
                                                media_path: Some(path),
                                                media_type: Some("image".into()),
                                                file_name: Some(filename),
                                            };
                                        }
                                        Err(e) => warn!("[wechat] save image error: {}", e),
                                    }
                                }
                                Err(e) => warn!("[wechat] image download/decrypt error: {}", e),
                            }
                        }
                    }
                    // Fallback: image without decryptable media
                    return ExtractedMessage {
                        text: "[图片]".into(),
                        media_path: None,
                        media_type: Some("image".into()),
                        file_name: None,
                    };
                }
            }
            // VOICE
            Some(3) => {
                if let Some(ref voice) = item.voice_item {
                    // Prefer voice-to-text transcription (no CDN needed)
                    let text = voice
                        .text
                        .as_deref()
                        .filter(|t| !t.is_empty())
                        .unwrap_or("[语音]");
                    return ExtractedMessage {
                        text: text.to_string(),
                        media_path: None,
                        media_type: Some("voice".into()),
                        file_name: None,
                    };
                }
            }
            // FILE
            Some(4) => {
                if let Some(ref fi) = item.file_item {
                    let fname = fi.file_name.clone().unwrap_or_else(|| "file.bin".into());
                    if let Some(ref media) = fi.media {
                        if let Some(ref aes_key) = media.aes_key {
                            match download_and_decrypt_media(http, media, aes_key, "file").await {
                                Ok(data) => match save_media_file(project_root, &data, &fname) {
                                    Ok(path) => {
                                        return ExtractedMessage {
                                            text: format!("[文件: {}]", fname),
                                            media_path: Some(path),
                                            media_type: Some("file".into()),
                                            file_name: Some(fname),
                                        };
                                    }
                                    Err(e) => warn!("[wechat] save file error: {}", e),
                                },
                                Err(e) => warn!("[wechat] file download/decrypt error: {}", e),
                            }
                        }
                    }
                    return ExtractedMessage {
                        text: format!("[文件: {}]", fname),
                        media_path: None,
                        media_type: Some("file".into()),
                        file_name: Some(fname),
                    };
                }
            }
            _ => {}
        }
    }

    ExtractedMessage {
        text: String::new(),
        media_path: None,
        media_type: None,
        file_name: None,
    }
}

// ── Main Entry Point ─────────────────────────────────────────────────────────

/// Returns a saved [`LoginResult`] if `<workspace>/.juglans/wechat/` already
/// has an account file, otherwise drives a fresh QR login (writing the
/// pending QR for external observers) and persists the result.
pub async fn ensure_login(http: &reqwest::Client, workspace: &Path) -> Result<LoginResult> {
    if let Some((id, data)) = load_account(workspace) {
        info!("[wechat] Loaded saved account: {}", id);
        return Ok(LoginResult {
            account_id: id,
            base_url: data.base_url,
            bot_token: data.token,
            user_id: data.user_id,
        });
    }
    info!("[wechat] No saved account, starting QR login...");
    let result = qr_login(http, workspace).await?;
    let normalized_id = normalize_account_id(&result.account_id);
    save_account(
        workspace,
        &normalized_id,
        &AccountData {
            token: result.bot_token.clone(),
            base_url: result.base_url.clone(),
            user_id: result.user_id.clone(),
            saved_at: chrono::Utc::now().to_rfc3339(),
        },
    )?;
    Ok(LoginResult {
        account_id: normalized_id,
        ..result
    })
}

pub async fn start(
    config: JuglansConfig,
    workspace: PathBuf,
    agent_slug: Option<String>,
) -> Result<()> {
    let agent_slug = agent_slug
        .or_else(|| {
            config
                .bot
                .as_ref()
                .and_then(|b| b.wechat.as_ref())
                .map(|w| w.agent.clone())
        })
        .unwrap_or_else(|| "default".to_string());

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let login = ensure_login(&http, &workspace).await?;
    println!("📱 微信连接就绪 (account: {})", login.account_id);
    let dispatcher: Arc<dyn MessageDispatcher> = Arc::new(LocalDispatcher {
        config,
        project_root: workspace.clone(),
        agent_slug: agent_slug.clone(),
    });
    println!(
        "🤖 微信 Bot 已启动 (agent: {}, account: {})",
        agent_slug, login.account_id
    );
    message_loop(&http, &workspace, login, dispatcher).await
}

/// Long-poll iLink for new messages and dispatch each one through
/// `dispatcher`. Reply text is sent back via the same iLink session. Runs
/// forever; cancel by aborting the task. Both CLI mode (via [`start`]) and
/// orchestrator mode (juglans-wallet's per-channel task) share this body.
pub async fn message_loop(
    http: &reqwest::Client,
    workspace: &Path,
    login: LoginResult,
    dispatcher: Arc<dyn MessageDispatcher>,
) -> Result<()> {
    let LoginResult {
        account_id,
        base_url,
        bot_token: token,
        ..
    } = login;

    let context_tokens: ContextTokenStore = Arc::new(RwLock::new(HashMap::new()));

    // Load saved sync_buf
    let mut get_updates_buf = load_sync_buf(workspace, &account_id).unwrap_or_default();
    if !get_updates_buf.is_empty() {
        info!(
            "[wechat] Resumed sync buf ({} bytes)",
            get_updates_buf.len()
        );
    }

    let mut next_timeout_ms = DEFAULT_LONG_POLL_TIMEOUT_MS;
    let mut consecutive_failures: u32 = 0;

    info!("[wechat] Starting message loop: base_url={}", base_url);

    loop {
        let body = json!({
            "get_updates_buf": &get_updates_buf,
            "base_info": build_base_info(),
        });

        let resp = http
            .post(format!(
                "{}ilink/bot/getupdates",
                ensure_trailing_slash(&base_url)
            ))
            .headers(build_headers(Some(&token)))
            .json(&body)
            .timeout(std::time::Duration::from_millis(next_timeout_ms + 10_000))
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    debug!("[wechat] getUpdates timeout, retrying");
                    continue;
                }
                consecutive_failures += 1;
                error!(
                    "[wechat] getUpdates error ({}/{}): {}",
                    consecutive_failures, MAX_CONSECUTIVE_FAILURES, e
                );
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    consecutive_failures = 0;
                    tokio::time::sleep(std::time::Duration::from_millis(BACKOFF_DELAY_MS)).await;
                } else {
                    tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                }
                continue;
            }
        };

        let updates: GetUpdatesResp = match resp.json().await {
            Ok(u) => u,
            Err(e) => {
                consecutive_failures += 1;
                error!("[wechat] getUpdates parse error: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                continue;
            }
        };

        // Check server-suggested timeout
        if let Some(t) = updates.longpolling_timeout_ms {
            if t > 0 {
                next_timeout_ms = t;
            }
        }

        // Check for API errors
        let is_error = updates.ret.map(|r| r != 0).unwrap_or(false)
            || updates.errcode.map(|e| e != 0).unwrap_or(false);

        if is_error {
            let is_session_expired = updates.errcode == Some(SESSION_EXPIRED_ERRCODE)
                || updates.ret == Some(SESSION_EXPIRED_ERRCODE);

            if is_session_expired {
                error!("[wechat] Session expired! Please re-login with `juglans bot wechat`");
                // Delete saved account to force re-login next time
                let account_path = wechat_state_dir(workspace).join(format!("{}.json", account_id));
                let _ = fs::remove_file(&account_path);
                return Err(anyhow!(
                    "WeChat session expired. Run `juglans bot wechat` to re-login."
                ));
            }

            consecutive_failures += 1;
            error!(
                "[wechat] getUpdates failed: ret={:?} errcode={:?} errmsg={:?}",
                updates.ret, updates.errcode, updates.errmsg
            );
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                consecutive_failures = 0;
                tokio::time::sleep(std::time::Duration::from_millis(BACKOFF_DELAY_MS)).await;
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
            }
            continue;
        }

        consecutive_failures = 0;

        // Save sync buf
        if let Some(ref buf) = updates.get_updates_buf {
            if !buf.is_empty() {
                get_updates_buf = buf.clone();
                let _ = save_sync_buf(workspace, &account_id, buf);
            }
        }

        // Process messages
        let msgs = updates.msgs.unwrap_or_default();
        for msg in &msgs {
            let from_user_id = msg.from_user_id.as_deref().unwrap_or("");

            // Extract message content (text, voice-to-text, or media)
            let extracted = extract_message(msg, http, workspace).await;

            if extracted.text.is_empty() && extracted.media_path.is_none() {
                debug!("[wechat] Skipping empty message from {}", from_user_id);
                continue;
            }

            info!(
                "[wechat] Message from {}: {} (media: {:?})",
                from_user_id, &extracted.text, &extracted.media_type
            );

            // Cache context_token
            if let Some(ref ct) = msg.context_token {
                let key = context_token_key(&account_id, from_user_id);
                context_tokens.write().await.insert(key, ct.clone());
            }

            // Send typing indicator
            let context_token_for_typing = msg.context_token.as_deref();
            let typing_ticket = get_typing_ticket(
                &http,
                &base_url,
                &token,
                from_user_id,
                context_token_for_typing,
            )
            .await;
            if let Some(ref ticket) = typing_ticket {
                send_typing(&http, &base_url, &token, from_user_id, ticket, 1).await;
            }

            // Build event_data with media info
            let mut event_data = json!({ "text": &extracted.text });
            if let Some(ref path) = extracted.media_path {
                event_data["media_path"] = json!(path);
            }
            if let Some(ref mtype) = extracted.media_type {
                event_data["media_type"] = json!(mtype);
            }
            if let Some(ref fname) = extracted.file_name {
                event_data["file_name"] = json!(fname);
            }

            // Build PlatformMessage and execute workflow
            let platform_msg = PlatformMessage {
                event_type: "message".into(),
                event_data,
                platform_user_id: from_user_id.to_string(),
                platform_chat_id: from_user_id.to_string(),
                text: extracted.text.clone(),
                username: None,
                platform: "wechat".into(),
            };

            let reply = match dispatcher.dispatch(&platform_msg).await {
                Ok(r) => r.text,
                Err(e) => {
                    error!("[wechat] Workflow error: {}", e);
                    format!("Error: {}", e)
                }
            };

            // Send reply
            let context_token = {
                let key = context_token_key(&account_id, from_user_id);
                context_tokens.read().await.get(&key).cloned()
            };

            // Cancel typing before sending reply
            if let Some(ref ticket) = typing_ticket {
                send_typing(&http, &base_url, &token, from_user_id, ticket, 2).await;
            }

            if let Err(e) = send_text_message(
                &http,
                &base_url,
                &token,
                from_user_id,
                &reply,
                context_token.as_deref(),
            )
            .await
            {
                error!("[wechat] sendMessage error: {}", e);
            }
        }
    }
}

// ── Session accessor (for builtins) ──────────────────────────────────────────

/// Session info loaded from `.juglans/wechat/{account}.json`, used by
/// `wechat.*` builtin tools so they can push without going through QR login.
pub(crate) struct SessionInfo {
    pub token: String,
    pub base_url: String,
    #[allow(dead_code)]
    pub account_id: String,
    #[allow(dead_code)]
    pub user_id: Option<String>,
}

/// Load the most recent saved WeChat account. Returns `None` if no session
/// file exists — callers should prompt the user to run `juglans bot wechat`
/// once to perform the QR login.
pub(crate) fn load_session(project_root: &Path) -> Option<SessionInfo> {
    let (account_id, data) = load_account(project_root)?;
    Some(SessionInfo {
        token: data.token,
        base_url: data.base_url,
        account_id,
        user_id: data.user_id,
    })
}

// ── Send Message ─────────────────────────────────────────────────────────────

pub(crate) async fn send_text_message(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    to: &str,
    text: &str,
    context_token: Option<&str>,
) -> Result<()> {
    // Split into chunks of 4000 chars (WeChat limit)
    let chunks: Vec<&str> = if text.len() <= 4000 {
        vec![text]
    } else {
        text.as_bytes()
            .chunks(4000)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect()
    };

    for chunk in chunks {
        if chunk.is_empty() {
            continue;
        }

        let body = json!({
            "msg": {
                "from_user_id": "",
                "to_user_id": to,
                "client_id": format!("juglans-{}", rand::rng().random::<u32>()),
                "message_type": 2,  // BOT
                "message_state": 2, // FINISH
                "item_list": [{
                    "type": 1,
                    "text_item": { "text": chunk }
                }],
                "context_token": context_token,
            },
            "base_info": build_base_info(),
        });

        let url = format!("{}ilink/bot/sendmessage", ensure_trailing_slash(base_url));

        let resp = http
            .post(&url)
            .headers(build_headers(Some(token)))
            .json(&body)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("sendMessage failed: {} {}", status, text));
        }

        debug!("[wechat] Message sent to {}", to);
    }

    Ok(())
}

// ── Typing Indicator ─────────────────────────────────────────────────────────

/// Fetch typing_ticket for a user via getConfig API.
async fn get_typing_ticket(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    user_id: &str,
    context_token: Option<&str>,
) -> Option<String> {
    let body = json!({
        "ilink_user_id": user_id,
        "context_token": context_token,
        "base_info": build_base_info(),
    });

    let url = format!("{}ilink/bot/getconfig", ensure_trailing_slash(base_url));

    let resp = match http
        .post(&url)
        .headers(build_headers(Some(token)))
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            warn!("[wechat] getconfig request failed: {}", e);
            return None;
        }
    };

    let raw = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            warn!("[wechat] getconfig response read failed: {}", e);
            return None;
        }
    };

    debug!(
        "[wechat] getconfig response: {}",
        &raw[..raw.len().min(200)]
    );

    let config_resp: GetConfigResp = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            warn!("[wechat] getconfig parse failed: {}", e);
            return None;
        }
    };

    if let Some(ref ticket) = config_resp.typing_ticket {
        debug!("[wechat] got typing_ticket ({} bytes)", ticket.len());
    } else {
        debug!("[wechat] getconfig returned no typing_ticket");
    }

    config_resp.typing_ticket
}

/// Send typing status (1 = typing, 2 = cancel).
async fn send_typing(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    user_id: &str,
    typing_ticket: &str,
    status: i32,
) {
    let label = if status == 1 { "start" } else { "cancel" };
    let body = json!({
        "ilink_user_id": user_id,
        "typing_ticket": typing_ticket,
        "status": status,
        "base_info": build_base_info(),
    });

    let url = format!("{}ilink/bot/sendtyping", ensure_trailing_slash(base_url));

    match http
        .post(&url)
        .headers(build_headers(Some(token)))
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => {
            debug!(
                "[wechat] sendtyping {} → {} (status={})",
                label,
                resp.status(),
                status
            );
        }
        Err(e) => {
            warn!("[wechat] sendtyping {} failed: {}", label, e);
        }
    }
}
