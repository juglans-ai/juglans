// src/services/prompt_loader.rs
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use anyhow::{Result, anyhow};
use glob::glob;
use tracing::{info, warn};

/// 提示词注册表
#[derive(Debug, Clone)]
pub struct PromptRegistry {
    // slug -> template content
    templates: HashMap<String, String>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// 加载配置文件中定义的路径列表
    pub fn load_from_paths(&mut self, patterns: &[String]) -> Result<()> {
        for pattern in patterns {
            let paths = glob(pattern).map_err(|e| anyhow!("Invalid glob pattern '{}': {}", pattern, e))?;

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

    /// 读取单个文件并注册
    fn load_file(&mut self, path: &Path) -> Result<()> {
        if path.extension().and_then(|s| s.to_str()) != Some("jgprompt") {
            return Ok(());
        }

        // 使用文件名作为 Slug
        let slug = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("Invalid filename: {:?}", path))?
            .to_string();

        let content = fs::read_to_string(path)?;

        info!("📝 Loaded Prompt: [{}] from {:?}", slug, path);
        self.templates.insert(slug, content);

        Ok(())
    }

    /// 获取提示词内容
    pub fn get(&self, slug: &str) -> Option<&String> {
        self.templates.get(slug)
    }

    /// 【新增】获取所有已加载的 Prompt Slug
    pub fn keys(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }
}