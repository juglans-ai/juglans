// src/services/prompt_loader.rs
use anyhow::{anyhow, Result};
use glob::glob;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// Prompt registry
#[derive(Debug, Clone)]
pub struct PromptRegistry {
    // slug -> template content
    templates: HashMap<String, String>,
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Load path patterns defined in config
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

    /// Read a single file and register it
    fn load_file(&mut self, path: &Path) -> Result<()> {
        if path.extension().and_then(|s| s.to_str()) != Some("jgprompt") {
            return Ok(());
        }

        // Use filename as slug
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("Invalid filename: {:?}", path))?
            .to_string();

        let content = fs::read_to_string(path)?;

        info!("📝 Loaded Prompt: [{}] from {:?}", slug, path);
        self.templates.insert(slug, content);

        Ok(())
    }

    /// Get prompt content
    pub fn get(&self, slug: &str) -> Option<&String> {
        self.templates.get(slug)
    }

    /// Get all loaded prompt slugs
    pub fn keys(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }
}
