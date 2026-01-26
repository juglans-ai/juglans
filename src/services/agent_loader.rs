// src/services/agent_loader.rs
use anyhow::{anyhow, Result};
use glob::glob;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::core::agent_parser::{AgentParser, AgentResource};

/// Agent 注册表
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    // slug -> (AgentResource, FilePath)
    agents: HashMap<String, (AgentResource, PathBuf)>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// 根据配置的 glob patterns 加载本地 agents
    pub fn load_from_paths(&mut self, patterns: &[String]) -> Result<()> {
        for pattern in patterns {
            let paths =
                glob(pattern).map_err(|e| anyhow!("Invalid glob pattern '{}': {}", pattern, e))?;

            for entry in paths {
                match entry {
                    Ok(path) => {
                        if path.is_file() {
                            let _ = self.load_file(&path);
                        }
                    }
                    Err(e) => warn!("Error reading glob entry: {}", e),
                }
            }
        }
        Ok(())
    }

    /// 读取并解析单个 .jgagent 文件
    fn load_file(&mut self, path: &Path) -> Result<()> {
        if path.extension().and_then(|s| s.to_str()) != Some("jgagent") {
            return Ok(());
        }

        let content = fs::read_to_string(path)?;
        let abs_path = fs::canonicalize(path)?;

        match AgentParser::parse(&content) {
            Ok(agent) => {
                info!("🤖 Loaded Agent: [{}] from {:?}", agent.slug, abs_path);
                self.agents.insert(agent.slug.clone(), (agent, abs_path));
            }
            Err(e) => {
                warn!("Failed to parse agent file {:?}: {}", path, e);
            }
        }

        Ok(())
    }

    /// 获取 Agent 定义
    pub fn get(&self, slug: &str) -> Option<&AgentResource> {
        self.agents.get(slug).map(|(a, _)| a)
    }

    /// 【新增】获取 Agent 定义及其来源文件路径
    pub fn get_with_path(&self, slug: &str) -> Option<(&AgentResource, &PathBuf)> {
        self.agents.get(slug).map(|(a, p)| (a, p))
    }

    /// 获取所有加载的 Slug
    pub fn keys(&self) -> Vec<String> {
        self.agents.keys().cloned().collect()
    }
}
