// src/services/config.rs
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::debug;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AccountConfig {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub api_key: Option<String>,
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Jug0Config {
    pub base_url: String,
}

// Server configuration section
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_server_host")]
    pub host: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
    /// Public endpoint URL, written to jug0 when applying workflows.
    /// Example: "https://agent.juglans.ai"
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

// Bot configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BotConfig {
    pub telegram: Option<TelegramBotConfig>,
    pub feishu: Option<FeishuBotConfig>,
    pub wechat: Option<WechatBotConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TelegramBotConfig {
    pub token: String,
    #[serde(default = "default_bot_agent")]
    pub agent: String,
    /// Execution mode: "local" (local execution) or "jug0" (SSE client), auto-detected from jug0.base_url by default
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FeishuBotConfig {
    /// Event subscription mode (bidirectional)
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    /// Webhook mode (one-way push)
    pub webhook_url: Option<String>,
    #[serde(default = "default_bot_agent")]
    pub agent: String,
    #[serde(default = "default_feishu_port")]
    pub port: u16,
    /// API base URL: "https://open.feishu.cn" (default) or "https://open.larksuite.com" (Lark international)
    #[serde(default = "default_feishu_base_url")]
    pub base_url: String,
    /// List of approvers (open_id)
    #[serde(default)]
    pub approvers: Vec<String>,
    /// Execution mode: "local" (local execution) or "jug0" (SSE client), auto-detected from jug0.base_url by default
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WechatBotConfig {
    #[serde(default = "default_bot_agent")]
    pub agent: String,
}

fn default_bot_agent() -> String {
    "default".to_string()
}
fn default_feishu_port() -> u16 {
    9000
}
fn default_feishu_base_url() -> String {
    "https://open.feishu.cn".to_string()
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

    #[serde(default = "default_jug0_config")]
    pub jug0: Jug0Config,

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

    // Bot configuration
    pub bot: Option<BotConfig>,

    // Path alias configuration
    #[serde(default)]
    pub paths: PathsConfig,

    // Package Registry configuration
    pub registry: Option<RegistryConfig>,

    // AI provider configuration
    #[serde(default)]
    pub ai: AiConfig,
}

fn default_env_file() -> Vec<String> {
    vec![".env".to_string()]
}

fn default_jug0_config() -> Jug0Config {
    Jug0Config {
        base_url: "https://api.jug0.com".to_string(),
    }
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
                    api_key: None,
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
                jug0: default_jug0_config(),
                server: ServerConfig::default(),
                env_file: default_env_file(),
                env: Default::default(),
                debug: DebugConfig::default(),
                limits: RuntimeLimits::default(),
                bot: None,
                paths: PathsConfig::default(),
                registry: None,
                ai: AiConfig::default(),
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

    /// Override config fields with environment variables (for FC/Lambda and other serverless environments)
    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("JUG0_BASE_URL") {
            self.jug0.base_url = v;
        }
        if let Ok(v) = std::env::var("JUG0_API_KEY") {
            self.account.api_key = Some(v);
        }
        if let Ok(v) = std::env::var("SERVER_HOST") {
            self.server.host = v;
        }
        if let Ok(Ok(v)) = std::env::var("SERVER_PORT").map(|s| s.parse::<u16>()) {
            self.server.port = v;
        }
        // Feishu bot config
        let feishu_app_id = std::env::var("FEISHU_APP_ID").ok();
        let feishu_app_secret = std::env::var("FEISHU_APP_SECRET").ok();
        if feishu_app_id.is_some() || feishu_app_secret.is_some() {
            let bot = self.bot.get_or_insert(BotConfig {
                telegram: None,
                feishu: None,
                wechat: None,
            });
            let feishu = bot.feishu.get_or_insert_with(|| FeishuBotConfig {
                app_id: None,
                app_secret: None,
                webhook_url: None,
                agent: default_bot_agent(),
                port: default_feishu_port(),
                base_url: default_feishu_base_url(),
                approvers: vec![],
                mode: None,
            });
            if let Some(v) = feishu_app_id {
                feishu.app_id = Some(v);
            }
            if let Some(v) = feishu_app_secret {
                feishu.app_secret = Some(v);
            }
        }
        // Telegram bot config
        if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
            let bot = self.bot.get_or_insert(BotConfig {
                telegram: None,
                feishu: None,
                wechat: None,
            });
            let tg = bot.telegram.get_or_insert_with(|| TelegramBotConfig {
                token: String::new(),
                agent: default_bot_agent(),
                mode: None,
            });
            tg.token = token;
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
