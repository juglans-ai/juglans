// src/adapters/discord.rs
//
// Discord Gateway (WebSocket) adapter. Receives MESSAGE_CREATE events,
// hands them to `run_agent_for_message`, and sends replies back via the
// REST API.
//
// Scope (v1):
//   - Gateway opcodes: Hello(10), Identify(2), Heartbeat(1)/Ack(11),
//     Resume(6), Dispatch(0), Reconnect(7), InvalidSession(9)
//   - Dispatch events: READY, RESUMED, MESSAGE_CREATE (others ignored)
//   - REST: send message (POST /channels/{id}/messages), typing indicator
//   - Session persistence at .juglans/discord/gateway.json for resume
//
// Out of scope (deferred):
//   - Slash / interactions API
//   - Message edit / delete / reactions
//   - `dm_policy` / `group_policy` / guild allowlist enforcement
//   - Sharding (only matters beyond 2500 guilds)
//   - `discord_send` builtin (planned separately as part of a unified
//     push-tool story across platforms)
//
// Deployment note: Discord's Gateway is a persistent WebSocket. Serverless
// platforms (Lambda, Cloud Functions) cannot keep the connection alive,
// so `juglans serve` on those platforms will log Discord errors but keep
// the HTTP API up. Run the Discord adapter on a long-lived host.

#![cfg(not(target_arch = "wasm32"))]

use anyhow::{anyhow, Context, Result};
use dashmap::DashSet;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tracing::{debug, error, info, warn};

use super::{Channel, MessageDispatcher, PlatformMessage};
use crate::services::config::JuglansConfig;

// ─── Constants ──────────────────────────────────────────────────────────────

const GATEWAY_VERSION: u8 = 10;
const GATEWAY_ENCODING: &str = "json";
pub(crate) const DISCORD_API: &str = "https://discord.com/api/v10";
pub(crate) const MAX_MESSAGE_LEN: usize = 2000;

/// Intent name → bitmask. Keep in sync with
/// <https://discord.com/developers/docs/topics/gateway#gateway-intents>.
const INTENT_TABLE: &[(&str, u64)] = &[
    ("guilds", 1 << 0),
    ("guild_members", 1 << 1),
    ("guild_moderation", 1 << 2),
    ("guild_emojis_and_stickers", 1 << 3),
    ("guild_integrations", 1 << 4),
    ("guild_webhooks", 1 << 5),
    ("guild_invites", 1 << 6),
    ("guild_voice_states", 1 << 7),
    ("guild_presences", 1 << 8),
    ("guild_messages", 1 << 9),
    ("guild_message_reactions", 1 << 10),
    ("guild_message_typing", 1 << 11),
    ("direct_messages", 1 << 12),
    ("direct_message_reactions", 1 << 13),
    ("direct_message_typing", 1 << 14),
    ("message_content", 1 << 15),
    ("guild_scheduled_events", 1 << 16),
    ("auto_moderation_configuration", 1 << 20),
    ("auto_moderation_execution", 1 << 21),
];

fn intents_to_bitmask(names: &[String]) -> u64 {
    let mut bits: u64 = 0;
    for name in names {
        let key = name.to_ascii_lowercase();
        match INTENT_TABLE.iter().find(|(n, _)| *n == key) {
            Some((_, bit)) => bits |= bit,
            None => warn!("[discord] unknown intent name: {:?} (ignored)", name),
        }
    }
    bits
}

// ─── Close-code classification ──────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum CloseKind {
    /// Try to resume on the stored `resume_gateway_url`.
    Resume,
    /// Session is unusable — clear state, reconnect with fresh Identify.
    FullReconnect,
    /// Do not retry (bad token, disallowed intents, etc.).
    Fatal(u16),
}

fn classify_close(code: u16) -> CloseKind {
    match code {
        // Not fatal; Discord asks us to resume
        4000..=4003 | 4005..=4006 | 4008 => CloseKind::Resume,
        // Session died; fresh identify required
        4007 | 4009 => CloseKind::FullReconnect,
        // Terminal — stop retrying
        4004 | 4010..=4014 => CloseKind::Fatal(code),
        // TCP-level close or unknown — assume transient, try resume
        _ => CloseKind::Resume,
    }
}

fn log_fatal_close(code: u16) {
    if code == 4014 {
        error!(
            "[discord] Gateway rejected intents (close code 4014). \
             Enable 'MESSAGE CONTENT INTENT' in the Discord Developer Portal \
             (https://discord.com/developers/applications), or remove \
             'message_content' from [channels.discord.<id>].intents in juglans.toml."
        );
    } else if code == 4004 {
        error!("[discord] Authentication failed (4004). Check [channels.discord.<id>].token.");
    } else {
        error!("[discord] Fatal gateway close code: {}", code);
    }
}

// ─── Session persistence ────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionFile {
    session_id: Option<String>,
    resume_gateway_url: Option<String>,
    sequence: Option<u64>,
}

impl SessionFile {
    fn path(project_root: &Path) -> PathBuf {
        project_root
            .join(".juglans")
            .join("discord")
            .join("gateway.json")
    }

    fn load(project_root: &Path) -> Self {
        let p = Self::path(project_root);
        fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str::<SessionFile>(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, project_root: &Path) -> Result<()> {
        let p = Self::path(project_root);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(&p, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    fn clear(project_root: &Path) {
        let _ = fs::remove_file(Self::path(project_root));
    }

    fn is_resumable(&self) -> bool {
        self.session_id.is_some() && self.resume_gateway_url.is_some()
    }
}

// ─── Runtime (in-memory, per-process) ───────────────────────────────────────

struct GatewayRuntime {
    token: String,
    intents: u64,
    last_sequence: Mutex<Option<u64>>,
    session_id: Mutex<Option<String>>,
    resume_url: Mutex<Option<String>>,
    bot_user_id: Mutex<Option<String>>,
    processed_message_ids: DashSet<String>,
}

impl GatewayRuntime {
    fn new(token: String, intents: u64) -> Self {
        Self {
            token,
            intents,
            last_sequence: Mutex::new(None),
            session_id: Mutex::new(None),
            resume_url: Mutex::new(None),
            bot_user_id: Mutex::new(None),
            processed_message_ids: DashSet::new(),
        }
    }

    fn record_seq(&self, s: u64) {
        *self.last_sequence.lock().unwrap() = Some(s);
    }

    fn snapshot_seq(&self) -> Option<u64> {
        *self.last_sequence.lock().unwrap()
    }

    fn persist_session(&self, project_root: &Path) {
        let sf = SessionFile {
            session_id: self.session_id.lock().unwrap().clone(),
            resume_gateway_url: self.resume_url.lock().unwrap().clone(),
            sequence: self.snapshot_seq(),
        };
        if let Err(e) = sf.save(project_root) {
            warn!("[discord] failed to persist session: {}", e);
        }
    }
}

// ─── REST helpers ───────────────────────────────────────────────────────────

async fn fetch_gateway_url(http: &reqwest::Client, token: &str) -> Result<String> {
    let resp = http
        .get(format!("{}/gateway/bot", DISCORD_API))
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 401 {
            return Err(anyhow!(
                "Discord rejected bot token (401). Check [channels.discord.<id>].token."
            ));
        }
        return Err(anyhow!("GET /gateway/bot failed: {} {}", status, body));
    }
    let body: Value = resp.json().await?;
    body["url"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow!("/gateway/bot response missing `url`"))
}

pub(crate) async fn send_typing(http: &reqwest::Client, token: &str, channel_id: &str) {
    let url = format!("{}/channels/{}/typing", DISCORD_API, channel_id);
    let _ = http
        .post(&url)
        .header("Authorization", format!("Bot {}", token))
        .timeout(Duration::from_secs(5))
        .send()
        .await;
}

/// Send a message to a channel, chunking at MAX_MESSAGE_LEN characters.
/// Retries once on HTTP 429 using `retry_after` from the body.
pub(crate) async fn send_channel_message(
    http: &reqwest::Client,
    token: &str,
    channel_id: &str,
    text: &str,
) -> Result<()> {
    for chunk in split_message(text, MAX_MESSAGE_LEN) {
        if chunk.is_empty() {
            continue;
        }
        let url = format!("{}/channels/{}/messages", DISCORD_API, channel_id);
        let body = json!({ "content": chunk });

        let mut attempt = 0;
        loop {
            let resp = http
                .post(&url)
                .header("Authorization", format!("Bot {}", token))
                .json(&body)
                .timeout(Duration::from_secs(15))
                .send()
                .await?;

            if resp.status().is_success() {
                break;
            }
            if resp.status().as_u16() == 429 && attempt < 1 {
                let j: Value = resp.json().await.unwrap_or(json!({}));
                let wait = j["retry_after"].as_f64().unwrap_or(1.0);
                tokio::time::sleep(Duration::from_millis((wait * 1000.0) as u64)).await;
                attempt += 1;
                continue;
            }
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "POST /channels/{}/messages failed: {} {}",
                channel_id,
                status,
                err_body
            ));
        }
    }
    Ok(())
}

/// UTF-8 / char-boundary safe chunker. Prefers splitting at a newline within
/// the last 10% of the window; otherwise splits at the last char boundary.
pub(crate) fn split_message(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.chars().count() <= max_chars {
            out.push(remaining.to_string());
            break;
        }
        // Take up to max_chars chars.
        let mut end_byte = remaining.len();
        let mut count = 0;
        for (i, _) in remaining.char_indices() {
            count += 1;
            if count > max_chars {
                end_byte = i;
                break;
            }
        }
        let window = &remaining[..end_byte];
        // Prefer the last newline in the window's last 10% (only when we
        // actually found a natural boundary late in the window — otherwise
        // the prose will be broken awkwardly and we're better off cutting
        // at max_chars directly).
        let mut split_byte = end_byte;
        let cutoff = (window.len() * 9) / 10;
        if let Some(nl) = window[cutoff..].rfind('\n') {
            split_byte = cutoff + nl;
        }
        let chunk = &remaining[..split_byte];
        out.push(chunk.to_string());
        remaining = remaining[split_byte..].trim_start_matches('\n');
    }
    out
}

// ─── Gateway session ────────────────────────────────────────────────────────

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_text_frame(
    ws: &mut futures_util::stream::SplitStream<WsStream>,
) -> Result<Option<Value>> {
    while let Some(msg) = ws.next().await {
        match msg? {
            WsMessage::Text(t) => {
                return serde_json::from_str::<Value>(&t)
                    .map(Some)
                    .map_err(|e| anyhow!("Gateway JSON parse: {}", e));
            }
            WsMessage::Binary(_) => continue,
            WsMessage::Ping(p) => {
                // tokio-tungstenite handles pongs automatically; log for
                // visibility in trace mode.
                debug!("[discord] ws ping ({} bytes)", p.len());
                continue;
            }
            WsMessage::Pong(_) | WsMessage::Frame(_) => continue,
            WsMessage::Close(frame) => {
                let code = frame.map(|f| u16::from(f.code)).unwrap_or(1006);
                return Err(anyhow!("__ws_close__:{}", code));
            }
        }
    }
    Ok(None)
}

fn extract_close_code(err: &anyhow::Error) -> Option<u16> {
    let s = err.to_string();
    if let Some(rest) = s.strip_prefix("__ws_close__:") {
        rest.parse::<u16>().ok()
    } else {
        None
    }
}

/// One connected session. Returns the close kind so the outer loop decides
/// whether to resume, fresh-reconnect, or give up.
async fn run_session(
    ws_url: &str,
    resume: bool,
    dispatcher: Arc<dyn MessageDispatcher>,
    project_root: Arc<PathBuf>,
    rt: Arc<GatewayRuntime>,
    http: reqwest::Client,
) -> Result<CloseKind> {
    use tokio_tungstenite::connect_async;

    let url = format!(
        "{}/?v={}&encoding={}",
        ws_url.trim_end_matches('/'),
        GATEWAY_VERSION,
        GATEWAY_ENCODING
    );
    let (ws, _resp) = connect_async(&url)
        .await
        .with_context(|| format!("Failed to connect to {}", url))?;
    let (mut write, mut read) = ws.split();

    // 1. Hello (op 10)
    let hello = next_text_frame(&mut read)
        .await?
        .ok_or_else(|| anyhow!("Gateway closed before sending Hello"))?;
    let interval = hello["d"]["heartbeat_interval"].as_u64().unwrap_or(41250);

    // 2. Heartbeat ticker on a channel. We don't share the write half with
    // another task — instead we drive both send and recv from the same
    // select! so there's exactly one writer.
    let (tick_tx, mut tick_rx) = mpsc::channel::<()>(8);
    let jitter = {
        let mut rng = rand::rng();
        rng.random_range(0.0..1.0)
    };
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis((interval as f64 * jitter) as u64)).await;
        let _ = tick_tx.send(()).await;
        let mut ticker = tokio::time::interval(Duration::from_millis(interval));
        ticker.tick().await; // consume the immediate tick
        loop {
            ticker.tick().await;
            if tick_tx.send(()).await.is_err() {
                break;
            }
        }
    });

    // 3. Identify or Resume
    if resume {
        let session_id = rt.session_id.lock().unwrap().clone().unwrap_or_default();
        let seq = rt.snapshot_seq();
        let frame = json!({
            "op": 6,
            "d": {
                "token": rt.token,
                "session_id": session_id,
                "seq": seq,
            }
        });
        write.send(WsMessage::Text(frame.to_string())).await?;
        info!(
            "[discord] Resuming gateway session {}",
            &session_id[..session_id.len().min(8)]
        );
    } else {
        let frame = json!({
            "op": 2,
            "d": {
                "token": rt.token,
                "intents": rt.intents,
                "properties": {
                    "os": std::env::consts::OS,
                    "browser": "juglans",
                    "device": "juglans",
                }
            }
        });
        write.send(WsMessage::Text(frame.to_string())).await?;
    }

    // 4. Dispatch loop
    loop {
        tokio::select! {
            _ = tick_rx.recv() => {
                let seq = rt.snapshot_seq();
                write.send(WsMessage::Text(json!({ "op": 1, "d": seq }).to_string())).await?;
            }
            frame = next_text_frame(&mut read) => {
                let v = match frame {
                    Ok(Some(v)) => v,
                    Ok(None) => return Ok(CloseKind::Resume),
                    Err(e) => {
                        if let Some(code) = extract_close_code(&e) {
                            return Ok(classify_close(code));
                        }
                        return Err(e);
                    }
                };

                if let Some(s) = v["s"].as_u64() {
                    rt.record_seq(s);
                }
                match v["op"].as_u64() {
                    Some(0) => {
                        if let Err(e) = handle_dispatch(
                            &v,
                            dispatcher.clone(),
                            project_root.clone(),
                            rt.clone(),
                            http.clone(),
                        ).await {
                            warn!("[discord] dispatch error: {}", e);
                        }
                    }
                    Some(1) => {
                        // Server asked us to heartbeat now
                        let seq = rt.snapshot_seq();
                        write.send(WsMessage::Text(json!({ "op": 1, "d": seq }).to_string())).await?;
                    }
                    Some(7) => {
                        info!("[discord] Gateway requested reconnect (op 7)");
                        return Ok(CloseKind::Resume);
                    }
                    Some(9) => {
                        let resumable = v["d"].as_bool().unwrap_or(false);
                        warn!(
                            "[discord] Invalid session (op 9, resumable={})",
                            resumable
                        );
                        return Ok(if resumable {
                            CloseKind::Resume
                        } else {
                            CloseKind::FullReconnect
                        });
                    }
                    Some(11) => { /* heartbeat ack */ }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_dispatch(
    v: &Value,
    dispatcher: Arc<dyn MessageDispatcher>,
    project_root: Arc<PathBuf>,
    rt: Arc<GatewayRuntime>,
    http: reqwest::Client,
) -> Result<()> {
    let t = v["t"].as_str().unwrap_or("");
    match t {
        "READY" => {
            let d = &v["d"];
            let session_id = d["session_id"].as_str().unwrap_or("").to_string();
            let resume_url = d["resume_gateway_url"].as_str().unwrap_or("").to_string();
            let bot_id = d["user"]["id"].as_str().unwrap_or("").to_string();
            let username = d["user"]["username"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            *rt.session_id.lock().unwrap() = Some(session_id);
            *rt.resume_url.lock().unwrap() = Some(resume_url);
            *rt.bot_user_id.lock().unwrap() = Some(bot_id.clone());
            let id_short = &bot_id[..bot_id.len().min(6)];
            info!(
                "[discord] Gateway READY — bot @{} (id={}…)",
                username, id_short
            );
            rt.persist_session(&project_root);
        }
        "RESUMED" => {
            info!("[discord] Gateway session resumed");
        }
        "MESSAGE_CREATE" => {
            handle_message_create(&v["d"], dispatcher, project_root, rt, http).await?;
        }
        _ => {
            debug!("[discord] ignored dispatch: {}", t);
        }
    }
    Ok(())
}

async fn handle_message_create(
    d: &Value,
    dispatcher: Arc<dyn MessageDispatcher>,
    project_root: Arc<PathBuf>,
    rt: Arc<GatewayRuntime>,
    http: reqwest::Client,
) -> Result<()> {
    let author_id = d["author"]["id"].as_str().unwrap_or("").to_string();

    // Self-filter (our own messages)
    {
        let self_id = rt.bot_user_id.lock().unwrap().clone();
        if Some(&author_id) == self_id.as_ref() {
            return Ok(());
        }
    }
    // Other bots — prevent loops
    if d["author"]["bot"].as_bool() == Some(true) {
        return Ok(());
    }

    let message_id = d["id"].as_str().unwrap_or("").to_string();
    if message_id.is_empty() {
        return Ok(());
    }
    if !rt.processed_message_ids.insert(message_id.clone()) {
        return Ok(());
    }
    // Simple bound on dedupe set growth
    if rt.processed_message_ids.len() > 10_000 {
        rt.processed_message_ids.clear();
    }

    let content = d["content"].as_str().unwrap_or("").to_string();
    if content.is_empty() {
        // Media-only message; v1 has no image handling
        return Ok(());
    }
    let channel_id = d["channel_id"].as_str().unwrap_or("").to_string();
    if channel_id.is_empty() {
        return Ok(());
    }
    let username = d["author"]["username"].as_str().map(String::from);

    let preview: String = content.chars().take(50).collect();
    info!(
        "[discord] {} (id={}…): {}",
        username.as_deref().unwrap_or("?"),
        &author_id[..author_id.len().min(6)],
        preview
    );

    // Persist latest sequence so restart can resume close to where we were.
    rt.persist_session(&project_root);

    // Run the agent + send reply in a detached task so the receive loop
    // keeps reading gateway events in parallel.
    let token = rt.token.clone();
    tokio::spawn(async move {
        let platform_msg = PlatformMessage {
            event_type: "message".into(),
            event_data: json!({ "text": &content }),
            platform_user_id: author_id,
            platform_chat_id: channel_id.clone(),
            text: content,
            username,
            platform: "discord".into(),
        };

        send_typing(&http, &token, &channel_id).await;

        match dispatcher.dispatch(&platform_msg).await {
            Ok(reply) => {
                if reply.text.is_empty() || reply.text == "(No response)" {
                    return;
                }
                if let Err(e) = send_channel_message(&http, &token, &channel_id, &reply.text).await
                {
                    error!("[discord] send failed: {}", e);
                }
            }
            Err(e) => {
                error!("[discord] agent error: {}", e);
                let _ = send_channel_message(&http, &token, &channel_id, &format!("Error: {}", e))
                    .await;
            }
        }
    });

    Ok(())
}

// ─── Connection loop (reconnect with backoff) ───────────────────────────────

async fn connection_loop(
    dispatcher: Arc<dyn MessageDispatcher>,
    project_root: Arc<PathBuf>,
    rt: Arc<GatewayRuntime>,
    http: reqwest::Client,
) -> Result<()> {
    let mut consecutive_failures: u32 = 0;

    loop {
        // Decide resume vs fresh connect.
        let stored = SessionFile::load(&project_root);
        let (should_resume, connect_url) = if stored.is_resumable() {
            // Seed the runtime from stored state.
            *rt.session_id.lock().unwrap() = stored.session_id.clone();
            *rt.resume_url.lock().unwrap() = stored.resume_gateway_url.clone();
            *rt.last_sequence.lock().unwrap() = stored.sequence;
            (true, stored.resume_gateway_url.clone().unwrap())
        } else {
            let url = match fetch_gateway_url(&http, &rt.token).await {
                Ok(u) => u,
                Err(e) => {
                    error!("[discord] GET /gateway/bot: {}", e);
                    consecutive_failures += 1;
                    if consecutive_failures >= 5 {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            (false, url)
        };

        match run_session(
            &connect_url,
            should_resume,
            dispatcher.clone(),
            project_root.clone(),
            rt.clone(),
            http.clone(),
        )
        .await
        {
            Ok(CloseKind::Resume) => {
                consecutive_failures = 0;
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Ok(CloseKind::FullReconnect) => {
                consecutive_failures = 0;
                SessionFile::clear(&project_root);
                *rt.session_id.lock().unwrap() = None;
                *rt.resume_url.lock().unwrap() = None;
                *rt.last_sequence.lock().unwrap() = None;
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Ok(CloseKind::Fatal(code)) => {
                log_fatal_close(code);
                return Err(anyhow!("Discord gateway fatal close: {}", code));
            }
            Err(e) => {
                consecutive_failures += 1;
                warn!(
                    "[discord] session error ({}/5): {} — reconnecting",
                    consecutive_failures, e
                );
                if consecutive_failures >= 5 {
                    return Err(e.context("Discord gateway: 5 consecutive failures"));
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

// ─── Channel + public entry point ───────────────────────────────────────────

/// One Discord gateway connection = one [`DiscordChannel`].
///
/// Holds the bot token + computed intents bitmask + the project root (needed
/// for `SessionFile` resume persistence). `id()` is `"discord:<token_prefix>"`,
/// stable from construction; the bot's `@username` shows up in logs only after
/// `READY`.
pub struct DiscordChannel {
    id: String,
    rt: Arc<GatewayRuntime>,
    project_root: Arc<PathBuf>,
    intents: u64,
    intents_label: String,
}

impl DiscordChannel {
    pub fn new(token: String, intents: u64, intents_label: String, project_root: PathBuf) -> Self {
        let token_prefix = token.split('.').next().unwrap_or("unknown");
        let id = format!("discord:{}", token_prefix);
        Self {
            id,
            rt: Arc::new(GatewayRuntime::new(token, intents)),
            project_root: Arc::new(project_root),
            intents,
            intents_label,
        }
    }
}

#[async_trait::async_trait]
impl crate::core::context::ChannelEgress for DiscordChannel {
    async fn send(&self, conversation: &str, text: &str) -> Result<()> {
        // `conversation` is a Discord channel id (snowflake string).
        send_channel_message(&reqwest::Client::new(), &self.rt.token, conversation, text).await
    }
}

#[async_trait::async_trait]
impl Channel for DiscordChannel {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &str {
        "discord"
    }

    async fn run(self: Arc<Self>, dispatcher: Arc<dyn MessageDispatcher>) -> Result<()> {
        info!(
            "🤖 Discord channel starting — intents 0x{:X} ({})",
            self.intents, self.intents_label
        );
        // Auto-inject ChannelOrigin so workflows reply()-ing inside Discord
        // events route back through this channel's egress.
        let dispatcher = Arc::new(super::OriginAwareDispatcher::new(self.clone(), dispatcher))
            as Arc<dyn MessageDispatcher>;
        connection_loop(
            dispatcher,
            self.project_root.clone(),
            self.rt.clone(),
            reqwest::Client::new(),
        )
        .await
    }
}

/// Build [`DiscordChannel`] instances from `juglans.toml`.
///
/// Reads `[channels.discord.<id>]` entries; tokens are deduplicated so the
/// same bot can't be listed twice. Each pair is `(channel, agent_slug)` —
/// per-channel agent lets different bots route to different workflows.
pub fn discover_channels(
    config: &JuglansConfig,
    project_root: &Path,
) -> Result<Vec<(Arc<DiscordChannel>, String)>> {
    let mut tokens_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<(Arc<DiscordChannel>, String)> = Vec::new();

    let mut emit = |bot_config: &crate::services::config::DiscordChannelConfig| -> Result<()> {
        if bot_config.token.is_empty() {
            return Err(anyhow!(
                "discord channel token is empty — set it in juglans.toml (e.g. `token = \"${{DISCORD_BOT_TOKEN}}\"`)"
            ));
        }
        if !tokens_seen.insert(bot_config.token.clone()) {
            return Ok(());
        }
        if bot_config.dm_policy.is_some()
            || bot_config.group_policy.is_some()
            || !bot_config.guilds.is_empty()
        {
            warn!(
                "[discord] dm_policy / group_policy / guilds allowlist are parsed but not yet enforced (v2)"
            );
        }
        let intents = bot_config
            .intents_bitmask
            .unwrap_or_else(|| intents_to_bitmask(&bot_config.intents));
        let intents_label = bot_config.intents.join(", ");
        out.push((
            Arc::new(DiscordChannel::new(
                bot_config.token.clone(),
                intents,
                intents_label,
                project_root.to_path_buf(),
            )),
            bot_config.agent.clone(),
        ));
        Ok(())
    };

    for cfg in config.channels.discord.values() {
        emit(cfg)?;
    }

    Ok(out)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_to_bitmask_known() {
        let bits = intents_to_bitmask(&[
            "guilds".into(),
            "message_content".into(),
            "direct_messages".into(),
        ]);
        assert_eq!(bits, (1 << 0) | (1 << 15) | (1 << 12));
    }

    #[test]
    fn intents_to_bitmask_unknown_is_skipped() {
        let bits = intents_to_bitmask(&["guilds".into(), "made_up".into()]);
        assert_eq!(bits, 1 << 0);
    }

    #[test]
    fn intents_to_bitmask_case_insensitive() {
        let bits = intents_to_bitmask(&["GUILDS".into(), "Message_Content".into()]);
        assert_eq!(bits, (1 << 0) | (1 << 15));
    }

    #[test]
    fn classify_close_documented_codes() {
        assert_eq!(classify_close(4004), CloseKind::Fatal(4004));
        assert_eq!(classify_close(4010), CloseKind::Fatal(4010));
        assert_eq!(classify_close(4013), CloseKind::Fatal(4013));
        assert_eq!(classify_close(4014), CloseKind::Fatal(4014));
        assert_eq!(classify_close(4007), CloseKind::FullReconnect);
        assert_eq!(classify_close(4009), CloseKind::FullReconnect);
        assert_eq!(classify_close(4000), CloseKind::Resume);
        assert_eq!(classify_close(4008), CloseKind::Resume);
        assert_eq!(classify_close(1006), CloseKind::Resume); // TCP-level
    }

    #[test]
    fn split_message_short_returns_one_chunk() {
        let out = split_message("hi there", 2000);
        assert_eq!(out, vec!["hi there"]);
    }

    #[test]
    fn split_message_prefers_newline_when_available() {
        // Line 1 is 92 'a', then newline, then one short line of 50 'b'.
        // Total 143 chars, limit 100. A naive byte cut would land mid-line;
        // the chunker should split at the newline instead, yielding the
        // first line cleanly.
        let mut s = String::new();
        s.push_str(&"a".repeat(92));
        s.push('\n');
        s.push_str(&"b".repeat(50));
        let out = split_message(&s, 100);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].chars().count(), 92);
        assert!(out[0].chars().all(|c| c == 'a'));
        assert!(out[1].chars().all(|c| c == 'b'));
    }

    #[test]
    fn split_message_utf8_char_boundary_safe() {
        // 1500 Chinese chars (each 3 bytes in UTF-8). Limit is 1000 chars.
        let s: String = std::iter::repeat('字').take(1500).collect();
        let out = split_message(&s, 1000);
        assert!(out.len() >= 2);
        for chunk in &out {
            // Every chunk must be valid UTF-8 (it is by construction since
            // it's a &str) and under the char limit.
            assert!(chunk.chars().count() <= 1000);
        }
        assert_eq!(out.iter().map(|c| c.chars().count()).sum::<usize>(), 1500);
    }

    #[test]
    fn session_file_roundtrip_missing_is_default() {
        let tmp =
            std::env::temp_dir().join(format!("juglans-discord-test-{}", uuid::Uuid::new_v4()));
        let sf = SessionFile::load(&tmp);
        assert!(!sf.is_resumable());
        assert!(sf.sequence.is_none());
    }

    #[test]
    fn session_file_roundtrip_write_then_read() {
        let tmp =
            std::env::temp_dir().join(format!("juglans-discord-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let sf = SessionFile {
            session_id: Some("abc".into()),
            resume_gateway_url: Some("wss://example.discord.gg".into()),
            sequence: Some(42),
        };
        sf.save(&tmp).unwrap();

        let loaded = SessionFile::load(&tmp);
        assert_eq!(loaded.session_id.as_deref(), Some("abc"));
        assert_eq!(
            loaded.resume_gateway_url.as_deref(),
            Some("wss://example.discord.gg")
        );
        assert_eq!(loaded.sequence, Some(42));
        assert!(loaded.is_resumable());

        SessionFile::clear(&tmp);
        let cleared = SessionFile::load(&tmp);
        assert!(!cleared.is_resumable());
    }
}
