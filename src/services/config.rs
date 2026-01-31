// src/services/config.rs
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

#[derive(Debug, Deserialize, Clone)]
pub struct AccountConfig {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub base_url: String,
    pub alias: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Jug0Config {
    pub base_url: String,
}

// 【新增】Server 配置部分
#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_server_host")]
    pub host: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
}

fn default_server_host() -> String {
    "127.0.0.1".to_string()
}
fn default_server_port() -> u16 {
    3000
}

#[derive(Debug, Deserialize, Clone)]
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
                server: ServerConfig::default(), // 使用默认
                mcp_servers: vec![],
                env: Default::default(),
            });
        }

        let content = fs::read_to_string(path).context("Failed to read juglans.toml")?;

        let config: JuglansConfig =
            toml::from_str(&content).context("Failed to parse juglans.toml")?;

        debug!("✓ Config loaded for user: {}", config.account.name);
        Ok(config)
    }
}
