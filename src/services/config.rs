// src/services/config.rs
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{debug, info};

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
    // 【新增】资源路径配置
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
pub struct McpServerConfig {
    pub name: String,
    pub base_url: String,
    pub alias: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Jug0Config {
    pub base_url: String,
}

// 【新增】Server 配置部分
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_server_host")]
    pub host: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
}

// 【新增】Debug 配置部分
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DebugConfig {
    /// 显示节点执行信息
    #[serde(default)]
    pub show_nodes: bool,

    /// 显示上下文变量
    #[serde(default)]
    pub show_context: bool,

    /// 显示条件评估详情
    #[serde(default)]
    pub show_conditions: bool,

    /// 显示变量解析过程
    #[serde(default)]
    pub show_variables: bool,
}

// Bot 配置
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BotConfig {
    pub telegram: Option<TelegramBotConfig>,
    pub feishu: Option<FeishuBotConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TelegramBotConfig {
    pub token: String,
    #[serde(default = "default_bot_agent")]
    pub agent: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FeishuBotConfig {
    /// 事件订阅模式（双向）
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    /// Webhook 模式（单向推送）
    pub webhook_url: Option<String>,
    #[serde(default = "default_bot_agent")]
    pub agent: String,
    #[serde(default = "default_feishu_port")]
    pub port: u16,
    /// API base URL: "https://open.feishu.cn" (默认) 或 "https://open.larksuite.com" (Lark 国际版)
    #[serde(default = "default_feishu_base_url")]
    pub base_url: String,
    /// 审批人列表（open_id）
    #[serde(default)]
    pub approvers: Vec<String>,
    /// 执行模式: "local" (本地执行) 或 "jug0" (SSE 客户端)，默认根据 jug0.base_url 自动判断
    #[serde(default)]
    pub mode: Option<String>,
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

    // 【新增】Web Server 配置
    #[serde(default)]
    pub server: ServerConfig,

    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,

    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    // 【新增】Debug 配置
    #[serde(default)]
    pub debug: DebugConfig,

    // Bot 配置
    pub bot: Option<BotConfig>,
}

fn default_jug0_config() -> Jug0Config {
    Jug0Config {
        base_url: "https://api.jug0.com".to_string(),
    }
}

// 为 ServerConfig 提供默认实现，以便在配置文件缺失该段时正常工作
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_server_host(),
            port: default_server_port(),
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
                mcp_servers: vec![],
                env: Default::default(),
                debug: DebugConfig::default(),
                bot: None,
            });
        }

        let content = fs::read_to_string(path).context("Failed to read juglans.toml")?;

        let config: JuglansConfig =
            toml::from_str(&content).context("Failed to parse juglans.toml")?;

        debug!("✓ Config loaded for user: {}", config.account.name);
        Ok(config)
    }
}
