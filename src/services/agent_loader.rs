// src/services/agent_loader.rs
use anyhow::{anyhow, Result};
use glob::glob;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use crate::core::agent_parser::{AgentParser, AgentResource};

/// Agent registry
#[derive(Debug, Clone)]
pub struct AgentRegistry {
    // slug -> (AgentResource, FilePath)
    agents: HashMap<String, (AgentResource, PathBuf)>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Load local agents from configured glob patterns
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

    /// Read and parse a single .jgagent file
    fn load_file(&mut self, path: &Path) -> Result<()> {
        if path.extension().and_then(|s| s.to_str()) != Some("jgagent") {
            return Ok(());
        }

        let content = fs::read_to_string(path)?;
        let abs_path = fs::canonicalize(path)?;

        match AgentParser::parse(&content) {
            Ok(agent) => {
                debug!("  ✓ Agent loaded: {} from {:?}", agent.slug, abs_path);
                self.agents.insert(agent.slug.clone(), (agent, abs_path));
            }
            Err(e) => {
                warn!("  ✗ Failed to parse agent: {:?} - {}", path, e);
            }
        }

        Ok(())
    }

    /// Get agent definition
    pub fn get(&self, slug: &str) -> Option<&AgentResource> {
        self.agents.get(slug).map(|(a, _)| a)
    }

    /// Get agent definition and its source file path
    pub fn get_with_path(&self, slug: &str) -> Option<(&AgentResource, &PathBuf)> {
        self.agents.get(slug).map(|(a, p)| (a, p))
    }

    /// Get all loaded slugs
    pub fn keys(&self) -> Vec<String> {
        self.agents.keys().cloned().collect()
    }

    /// Manually register an agent
    pub fn register(&mut self, agent: AgentResource, path: PathBuf) {
        debug!("  ✓ Agent registered: {} from {:?}", agent.slug, path);
        self.agents.insert(agent.slug.clone(), (agent, path));
    }

    /// Find agent by username (@handle)
    pub fn get_by_username(&self, username: &str) -> Option<&AgentResource> {
        self.agents
            .values()
            .find(|(agent, _)| agent.username.as_deref() == Some(username))
            .map(|(a, _)| a)
    }
}
