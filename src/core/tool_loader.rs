// src/core/tool_loader.rs
use anyhow::{anyhow, Context, Result};
use glob::glob;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Tool definition resource loaded from JSON files
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolResource {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub tools: Vec<Value>, // OpenAI function calling schema
}

impl ToolResource {
    /// Validate the tool resource
    pub fn validate(&self) -> Result<()> {
        if self.slug.is_empty() {
            return Err(anyhow!("Tool resource must have a non-empty 'slug'"));
        }

        if self.tools.is_empty() {
            return Err(anyhow!(
                "Tool resource '{}' must define at least one tool",
                self.slug
            ));
        }

        // Validate each tool has required fields
        for (idx, tool) in self.tools.iter().enumerate() {
            if !tool.is_object() {
                return Err(anyhow!(
                    "Tool #{} in '{}' must be an object",
                    idx,
                    self.slug
                ));
            }

            let obj = tool.as_object().unwrap();
            if !obj.contains_key("type") || !obj.contains_key("function") {
                return Err(anyhow!(
                    "Tool #{} in '{}' must have 'type' and 'function' fields",
                    idx,
                    self.slug
                ));
            }
        }

        Ok(())
    }
}

pub struct ToolLoader;

impl ToolLoader {
    /// Load a single tool resource from a JSON file
    pub fn load_from_file(path: &Path) -> Result<ToolResource> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read tool file: {}", path.display()))?;

        let tool: ToolResource = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse tool JSON: {}", path.display()))?;

        tool.validate()
            .with_context(|| format!("Invalid tool file: {}", path.display()))?;

        debug!("Loaded tool '{}' from {}", tool.slug, path.display());
        Ok(tool)
    }

    /// Load multiple tool resources from a glob pattern
    pub fn load_from_glob(pattern: &str, base_dir: &Path) -> Result<Vec<ToolResource>> {
        let pattern_path = if pattern.starts_with('/') {
            PathBuf::from(pattern)
        } else {
            base_dir.join(pattern)
        };

        let pattern_str = pattern_path
            .to_str()
            .ok_or_else(|| anyhow!("Invalid tool path pattern: {}", pattern))?;

        info!("Loading tools from pattern: {}", pattern_str);

        let mut tools = Vec::new();
        let mut count = 0;

        for entry in glob(pattern_str)
            .with_context(|| format!("Invalid glob pattern: {}", pattern_str))?
        {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        match Self::load_from_file(&path) {
                            Ok(tool) => {
                                count += 1;
                                tools.push(tool);
                            }
                            Err(e) => {
                                // Log error but continue loading other files
                                tracing::warn!("Failed to load tool from {}: {}", path.display(), e);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Glob error: {}", e);
                }
            }
        }

        if count == 0 {
            debug!("No tool files found matching pattern: {}", pattern_str);
        } else {
            info!("Loaded {} tool resource(s)", count);
        }

        Ok(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_tool_resource() {
        let valid_tool = ToolResource {
            slug: "test-tools".to_string(),
            name: "Test Tools".to_string(),
            description: Some("Test tool set".to_string()),
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "test_func",
                    "description": "A test function"
                }
            })],
        };

        assert!(valid_tool.validate().is_ok());

        // Test empty slug
        let invalid_slug = ToolResource {
            slug: "".to_string(),
            name: "Test".to_string(),
            description: None,
            tools: vec![json!({"type": "function", "function": {}})],
        };
        assert!(invalid_slug.validate().is_err());

        // Test empty tools
        let empty_tools = ToolResource {
            slug: "test".to_string(),
            name: "Test".to_string(),
            description: None,
            tools: vec![],
        };
        assert!(empty_tools.validate().is_err());
    }
}
