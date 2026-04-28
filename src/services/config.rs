// src/services/config.rs
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::debug;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountConfig {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    // Identity slot — future juglans-issued agent ID will live here.
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkspaceConfig {
    pub id: String,
    pub name: String,
    pub members: Option<Vec<String>>,
    // Resource path configuration
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub workflows: Vec<String>,
    #[serde(default)]
    pub prompts: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

// Server configuration section
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_server_host")]
    pub host: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
    /// Public endpoint URL for this server. Example: "https://agent.juglans.ai"
    pub endpoint_url: Option<String>,
}

// Debug configuration section
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DebugConfig {
    /// Show node execution info
    #[serde(default)]
    pub show_nodes: bool,

    /// Show context variables
    #[serde(default)]
    pub show_context: bool,

    /// Show condition evaluation details
    #[serde(default)]
    pub show_conditions: bool,

    /// Show variable resolution process
    #[serde(default)]
    pub show_variables: bool,
}

// Runtime limits configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RuntimeLimits {
    /// Max loop iterations (default: 100)
    #[serde(default = "default_max_loop_iterations")]
    pub max_loop_iterations: usize,

    /// Max nested execution depth (default: 10)
    #[serde(default = "default_max_execution_depth")]
    pub max_execution_depth: usize,

    /// HTTP request timeout in seconds (default: 120)
    #[serde(default = "default_http_timeout_secs")]
    pub http_timeout_secs: u64,

    /// Number of Python workers (default: 1)
    #[serde(default = "default_python_workers")]
    pub python_workers: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            max_loop_iterations: default_max_loop_iterations(),
            max_execution_depth: default_max_execution_depth(),
            http_timeout_secs: default_http_timeout_secs(),
            python_workers: default_python_workers(),
        }
    }
}

fn default_max_loop_iterations() -> usize {
    100
}
fn default_max_execution_depth() -> usize {
    10
}
fn default_http_timeout_secs() -> u64 {
    120
}
fn default_python_workers() -> usize {
    1
}

// AI provider configuration
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct AiConfig {
    /// Default model for chat() when not specified (e.g. "deepseek/deepseek-chat")
    pub default_model: Option<String>,
    /// Per-provider configuration (key = provider name: openai, anthropic, deepseek, etc.)
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

impl AiConfig {
    /// Check if any provider has a non-empty api_key configured.
    pub fn has_providers(&self) -> bool {
        self.providers
            .values()
            .any(|p| p.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false))
    }
}

// Path alias configuration
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct PathsConfig {
    /// Base directory for @ path aliases (relative to project root).
    /// None = feature disabled, Some(".") = @ points to project root
    pub base: Option<String>,
}

/// Multi-instance channel config: `[channels.<kind>.<instance_id>]`.
///
/// One section per channel instance. Each platform module reads its own
/// subsection in `discover_channels` and produces `Arc<dyn Channel>`s.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ChannelsConfig {
    /// One entry per Telegram bot, keyed by an arbitrary instance id. Each
    /// instance picks polling vs webhook automatically (webhook when
    /// `server.endpoint_url` is set, polling otherwise) unless `mode` overrides.
    #[serde(default)]
    pub telegram: HashMap<String, TelegramChannelConfig>,

    /// One entry per Discord gateway connection.
    #[serde(default)]
    pub discord: HashMap<String, DiscordChannelConfig>,

    /// WeChat: accounts are auto-discovered from disk; this section sets
    /// defaults applied to all of them. No per-instance subkeys.
    #[serde(default)]
    pub wechat: Option<WechatChannelConfig>,

    /// One entry per Feishu channel — either a bidirectional event-subscription
    /// (set `app_id` + `app_secret`) or an egress-only incoming webhook
    /// (set `incoming_webhook_url`). The two variants share one config struct;
    /// the discovery code picks which `Channel` impl to instantiate.
    #[serde(default)]
    pub feishu: HashMap<String, FeishuChannelConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TelegramChannelConfig {
    pub token: String,
    #[serde(default = "default_channel_agent")]
    pub agent: String,
    /// Ingress mode override. Auto-detected from `server.endpoint_url` when
    /// not set: webhook (passive) if endpoint_url is configured, polling (active)
    /// otherwise. Set to `"polling"` or `"webhook"` to force one mode.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Feishu channel config. Two flavors share one struct; the discovery code
/// picks an impl based on which fields are set:
///
/// - `app_id` + `app_secret` → [`FeishuEventChannel`] (bidirectional event subscription)
/// - `incoming_webhook_url` → [`FeishuWebhookChannel`] (egress-only push to a Feishu group)
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FeishuChannelConfig {
    /// Event-subscription mode credentials.
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    /// Egress-only mode: incoming webhook URL bound to a single Feishu group.
    pub incoming_webhook_url: Option<String>,
    #[serde(default = "default_channel_agent")]
    pub agent: String,
    /// API base URL: "https://open.feishu.cn" (default) or
    /// "https://open.larksuite.com" (Lark international).
    #[serde(default = "default_feishu_base_url")]
    pub base_url: String,
    /// Approvers (open_id) — used by approval-flow workflows.
    #[serde(default)]
    pub approvers: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WechatChannelConfig {
    #[serde(default = "default_channel_agent")]
    pub agent: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DiscordChannelConfig {
    /// Discord bot token (use `${DISCORD_BOT_TOKEN}` in TOML to pull from `.env` / env).
    pub token: String,
    #[serde(default = "default_channel_agent")]
    pub agent: String,
    /// Gateway intents by name. Default: guilds, guild_messages,
    /// message_content, direct_messages.
    #[serde(default = "default_discord_intents")]
    pub intents: Vec<String>,
    /// Raw intents bitmask; wins over `intents` when set (for users copy-pasting
    /// from the Discord developer portal).
    pub intents_bitmask: Option<u64>,
    /// Execution mode (reserved, currently always local).
    #[serde(default)]
    pub mode: Option<String>,
    // ── v2 placeholders: parsed so copy-pasted openclaw-style TOML doesn't
    //    fail to deserialize, but not enforced by v1. Setting any of them
    //    emits a warning at startup. See plans/discord-virtual-waffle.md.
    #[serde(default)]
    pub dm_policy: Option<String>,
    #[serde(default)]
    pub group_policy: Option<String>,
    #[serde(default)]
    pub guilds: Vec<String>,
}

fn default_channel_agent() -> String {
    "default".to_string()
}
fn default_discord_intents() -> Vec<String> {
    vec![
        "guilds".into(),
        "guild_messages".into(),
        "message_content".into(),
        "direct_messages".into(),
    ]
}
fn default_feishu_base_url() -> String {
    "https://open.feishu.cn".to_string()
}

// Conversation history configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HistoryConfig {
    /// Master switch. When false, chat_id is still accepted as a field but no
    /// storage is read or written.
    #[serde(default = "default_history_enabled")]
    pub enabled: bool,

    /// Storage backend: "jsonl" | "sqlite" | "memory" | "none".
    #[serde(default = "default_history_backend")]
    pub backend: String,

    /// Directory for JSONL backend (one file per chat_id).
    pub dir: Option<String>,

    /// Database path for SQLite backend.
    pub path: Option<String>,

    /// Hard upper bound on messages auto-loaded per chat() call.
    #[serde(default = "default_history_max_messages")]
    pub max_messages: usize,

    /// Soft token budget for auto-loaded history (rough estimate).
    #[serde(default = "default_history_max_tokens")]
    pub max_tokens: u32,

    /// Days after which old messages are eligible for GC. 0 disables.
    #[serde(default = "default_history_retention_days")]
    pub retention_days: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_history_enabled(),
            backend: default_history_backend(),
            dir: None,
            path: None,
            max_messages: default_history_max_messages(),
            max_tokens: default_history_max_tokens(),
            retention_days: default_history_retention_days(),
        }
    }
}

fn default_history_enabled() -> bool {
    true
}
fn default_history_backend() -> String {
    "jsonl".to_string()
}
fn default_history_max_messages() -> usize {
    20
}
fn default_history_max_tokens() -> u32 {
    8000
}
fn default_history_retention_days() -> u32 {
    30
}

// Package Registry configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RegistryConfig {
    /// Registry URL for client commands (default: https://jgr.juglans.ai)
    #[serde(default = "default_registry_url")]
    pub url: String,
    /// Server port when running `juglans registry` (optional, CLI arg takes precedence)
    pub port: Option<u16>,
    /// Server data directory (optional, CLI arg takes precedence)
    pub data_dir: Option<String>,
}

fn default_registry_url() -> String {
    "https://jgr.juglans.ai".to_string()
}

fn default_server_host() -> String {
    "127.0.0.1".to_string()
}
fn default_server_port() -> u16 {
    3000
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JuglansConfig {
    pub account: AccountConfig,
    pub workspace: Option<WorkspaceConfig>,

    // Web Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Env files to load (pydantic-settings style), loaded in order, later overrides earlier.
    /// Default: [".env"]
    #[serde(default = "default_env_file")]
    pub env_file: Vec<String>,

    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    // Debug configuration
    #[serde(default)]
    pub debug: DebugConfig,

    // Runtime limits configuration
    #[serde(default)]
    pub limits: RuntimeLimits,

    // Channel configuration: `[channels.<kind>.<instance_id>]`. Each subsection
    //
    // NOTE: legacy `[bot.*]` config is no longer parsed. Use `[channels.*]`
    // exclusively. If serde encounters `[bot.*]` in juglans.toml, it is silently
    // ignored (serde default behavior); bot config will simply not start.
    //
    // (continuation below)
    // declares one platform endpoint (one Telegram bot, one Discord gateway,
    // one Feishu event subscription, etc.). Multiple instances per kind are
    // allowed:
    //
    //   [channels.telegram.main]
    //   token = "..."
    //   agent = "support"
    //
    //   [channels.telegram.beta]
    //   token = "..."
    //   agent = "beta_test"
    //
    //   [channels.discord.community]
    //   token = "..."
    //
    //   [channels.feishu.events]      # bidirectional: event subscription
    //   app_id = "..."
    //   app_secret = "..."
    //
    //   [channels.feishu.alerts]      # egress-only: incoming webhook
    //   incoming_webhook_url = "https://open.feishu.cn/...hook/..."
    //
    // WeChat is special: accounts are auto-discovered from disk, so
    // `[channels.wechat]` (no instance_id) sets defaults that apply to all
    // discovered accounts.
    #[serde(default)]
    pub channels: ChannelsConfig,

    // Path alias configuration
    #[serde(default)]
    pub paths: PathsConfig,

    // Package Registry configuration
    pub registry: Option<RegistryConfig>,

    // AI provider configuration
    #[serde(default)]
    pub ai: AiConfig,

    // Conversation history configuration
    #[serde(default)]
    pub history: HistoryConfig,
}

fn default_env_file() -> Vec<String> {
    vec![".env".to_string()]
}

// Default implementation for ServerConfig, used when the config file is missing this section
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_server_host(),
            port: default_server_port(),
            endpoint_url: None,
        }
    }
}

impl JuglansConfig {
    pub fn load() -> Result<Self> {
        let path = Path::new("juglans.toml");

        if !path.exists() {
            debug!("⚠️ juglans.toml not found, using defaults");
            return Ok(JuglansConfig {
                account: AccountConfig {
                    id: "dev_user".to_string(),
                    name: "Developer".to_string(),
                    role: Some("admin".to_string()),
                },
                workspace: Some(WorkspaceConfig {
                    id: "default_ws".to_string(),
                    name: "Default Workspace".to_string(),
                    members: Some(vec!["dev_user".to_string()]),
                    agents: vec![],
                    workflows: vec![],
                    prompts: vec![],
                    tools: vec![],
                    exclude: vec![],
                }),
                server: ServerConfig::default(),
                env_file: default_env_file(),
                env: Default::default(),
                debug: DebugConfig::default(),
                limits: RuntimeLimits::default(),
                channels: ChannelsConfig::default(),
                paths: PathsConfig::default(),
                registry: None,
                ai: AiConfig::default(),
                history: HistoryConfig::default(),
            });
        }

        let content = fs::read_to_string(path).context("Failed to read juglans.toml")?;

        // Phase 1: Pre-parse to extract env_file list
        let pre: PreConfig = toml::from_str(&content).unwrap_or_default();

        // Phase 2: Load env files in order (later overrides earlier)
        for env_path in &pre.env_file {
            if let Ok(p) = dotenvy::from_filename(env_path) {
                debug!("✓ Loaded env file: {:?}", p);
            }
        }

        // Phase 3: Interpolate ${VAR} patterns with env values
        let content = interpolate_env_vars(&content);

        // Phase 4: Full parse
        let mut config: JuglansConfig =
            toml::from_str(&content).context("Failed to parse juglans.toml")?;

        // Environment variable overrides (serverless deployment)
        config.apply_env_overrides();

        debug!("✓ Config loaded for user: {}", config.account.name);
        Ok(config)
    }

    /// Override config fields with environment variables (for FC/Lambda and
    /// other serverless deployments). Env-var overrides drop into a synthetic
    /// `[channels.<kind>.default]` instance so containers without TOML can
    /// still bring up a channel.
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("SERVER_HOST") {
            self.server.host = v;
        }
        if let Ok(Ok(v)) = std::env::var("SERVER_PORT").map(|s| s.parse::<u16>()) {
            self.server.port = v;
        }

        // Feishu event-mode credentials → channels.feishu.default
        let feishu_app_id = std::env::var("FEISHU_APP_ID").ok();
        let feishu_app_secret = std::env::var("FEISHU_APP_SECRET").ok();
        if feishu_app_id.is_some() || feishu_app_secret.is_some() {
            let entry = self
                .channels
                .feishu
                .entry("default".to_string())
                .or_insert_with(|| FeishuChannelConfig {
                    app_id: None,
                    app_secret: None,
                    incoming_webhook_url: None,
                    agent: default_channel_agent(),
                    base_url: default_feishu_base_url(),
                    approvers: vec![],
                });
            if let Some(v) = feishu_app_id {
                entry.app_id = Some(v);
            }
            if let Some(v) = feishu_app_secret {
                entry.app_secret = Some(v);
            }
        }

        // History config overrides
        if let Ok(v) = std::env::var("JUGLANS_HISTORY_BACKEND") {
            self.history.backend = v;
        }
        if let Ok(v) = std::env::var("JUGLANS_HISTORY_DIR") {
            self.history.dir = Some(v);
        }
        if let Ok(v) = std::env::var("JUGLANS_HISTORY_PATH") {
            self.history.path = Some(v);
        }
        if let Ok(Ok(v)) = std::env::var("JUGLANS_HISTORY_MAX_MESSAGES").map(|s| s.parse::<usize>())
        {
            self.history.max_messages = v;
        }
        if let Ok(Ok(v)) = std::env::var("JUGLANS_HISTORY_MAX_TOKENS").map(|s| s.parse::<u32>()) {
            self.history.max_tokens = v;
        }
        if let Ok(Ok(v)) = std::env::var("JUGLANS_HISTORY_ENABLED").map(|s| s.parse::<bool>()) {
            self.history.enabled = v;
        }

        // Telegram token → channels.telegram.default
        if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
            let entry = self
                .channels
                .telegram
                .entry("default".to_string())
                .or_insert_with(|| TelegramChannelConfig {
                    token: String::new(),
                    agent: default_channel_agent(),
                    mode: None,
                });
            entry.token = token;
        }
    }
}

/// Pre-parse config to extract env_file before full deserialization.
#[derive(Deserialize, Default)]
struct PreConfig {
    #[serde(default = "default_env_file")]
    env_file: Vec<String>,
}

/// Replace `${VAR}` patterns in TOML content with environment variable values.
fn interpolate_env_vars(content: &str) -> String {
    let re = Regex::new(r"\$\{([^}]+)\}").unwrap();
    re.replace_all(content, |caps: &regex::Captures| {
        let var_name = &caps[1];
        std::env::var(var_name).unwrap_or_default()
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_basic() {
        std::env::set_var("TEST_INTERP_VAR", "hello");
        let result = interpolate_env_vars("key = \"${TEST_INTERP_VAR}\"");
        assert_eq!(result, "key = \"hello\"");
        std::env::remove_var("TEST_INTERP_VAR");
    }

    #[test]
    fn test_interpolate_missing_var() {
        let result = interpolate_env_vars("key = \"${NONEXISTENT_VAR_XYZ}\"");
        assert_eq!(result, "key = \"\"");
    }

    #[test]
    fn test_interpolate_no_pattern() {
        let input = "key = \"plain value\"";
        assert_eq!(interpolate_env_vars(input), input);
    }

    #[test]
    fn test_interpolate_multiple() {
        std::env::set_var("TEST_A", "aaa");
        std::env::set_var("TEST_B", "bbb");
        let result = interpolate_env_vars("a = \"${TEST_A}\"\nb = \"${TEST_B}\"");
        assert_eq!(result, "a = \"aaa\"\nb = \"bbb\"");
        std::env::remove_var("TEST_A");
        std::env::remove_var("TEST_B");
    }

    #[test]
    fn test_pre_config_default() {
        let pre: PreConfig = toml::from_str("").unwrap();
        assert_eq!(pre.env_file, vec![".env".to_string()]);
    }

    #[test]
    fn test_pre_config_custom() {
        let pre: PreConfig = toml::from_str("env_file = [\".env\", \".env.deploy\"]").unwrap();
        assert_eq!(pre.env_file, vec![".env", ".env.deploy"]);
    }
}
