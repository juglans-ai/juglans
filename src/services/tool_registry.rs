// src/services/tool_registry.rs
use crate::core::tool_loader::ToolResource;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Registry for managing tool resources
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolResource>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool resource
    pub fn register(&mut self, tool: ToolResource) {
        let slug = tool.slug.clone();
        if self.tools.contains_key(&slug) {
            warn!("Tool resource '{}' already registered, overwriting", slug);
        }
        debug!("Registered tool resource: {}", slug);
        self.tools.insert(slug, tool);
    }

    /// Register multiple tool resources
    pub fn register_all(&mut self, tools: Vec<ToolResource>) {
        for tool in tools {
            self.register(tool);
        }
    }

    /// Get a tool resource by slug
    pub fn get(&self, slug: &str) -> Option<&ToolResource> {
        self.tools.get(slug)
    }

    /// Check if a tool resource exists
    pub fn contains(&self, slug: &str) -> bool {
        self.tools.contains_key(slug)
    }

    /// Get all registered tool slugs
    pub fn list_slugs(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Resolve multiple tool resources by slugs and merge their tools
    ///
    /// This function:
    /// 1. Looks up each slug in the registry
    /// 2. Merges all tools from all referenced tool resources
    /// 3. Deduplicates by tool function name (last one wins)
    ///
    /// Example:
    /// ```
    /// let tools = registry.resolve_tools(&["web-tools", "data-tools"])?;
    /// ```
    pub fn resolve_tools(&self, slugs: &[String]) -> Result<Vec<Value>> {
        let mut merged_tools: HashMap<String, Value> = HashMap::new();

        for slug in slugs {
            let tool_resource = self
                .tools
                .get(slug)
                .ok_or_else(|| anyhow!("Tool resource '{}' not found", slug))?;

            debug!(
                "Resolving tools from '{}': {} tool(s)",
                slug,
                tool_resource.tools.len()
            );

            for tool_value in &tool_resource.tools {
                // Extract tool function name for deduplication
                if let Some(function_name) = tool_value
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                {
                    if merged_tools.contains_key(function_name) {
                        debug!(
                            "Tool function '{}' already exists, overwriting with version from '{}'",
                            function_name, slug
                        );
                    }
                    merged_tools.insert(function_name.to_string(), tool_value.clone());
                } else {
                    // If we can't extract function name, just add it
                    // Use a unique key based on the tool JSON
                    let key = format!("tool_{}", merged_tools.len());
                    merged_tools.insert(key, tool_value.clone());
                }
            }
        }

        let result: Vec<Value> = merged_tools.into_values().collect();
        debug!("Resolved {} tool(s) from {} slug(s)", result.len(), slugs.len());
        Ok(result)
    }

    /// Get the total number of registered tool resources
    pub fn count(&self) -> usize {
        self.tools.len()
    }

    /// Clear all registered tools
    pub fn clear(&mut self) {
        self.tools.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_tool(slug: &str, tool_name: &str) -> ToolResource {
        ToolResource {
            slug: slug.to_string(),
            name: format!("{} Tools", slug),
            description: Some(format!("Test tools for {}", slug)),
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": format!("Test function {}", tool_name)
                }
            })],
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut registry = ToolRegistry::new();
        let tool = create_test_tool("web-tools", "fetch_url");

        registry.register(tool.clone());

        assert!(registry.contains("web-tools"));
        assert_eq!(registry.get("web-tools").unwrap().slug, "web-tools");
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_resolve_single_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(create_test_tool("web-tools", "fetch_url"));

        let resolved = registry.resolve_tools(&["web-tools".to_string()]).unwrap();
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn test_resolve_multiple_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(create_test_tool("web-tools", "fetch_url"));
        registry.register(create_test_tool("data-tools", "calculate"));

        let resolved = registry
            .resolve_tools(&["web-tools".to_string(), "data-tools".to_string()])
            .unwrap();
        assert_eq!(resolved.len(), 2);
    }

    #[test]
    fn test_resolve_nonexistent_tool() {
        let registry = ToolRegistry::new();
        let result = registry.resolve_tools(&["nonexistent".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_deduplication() {
        let mut registry = ToolRegistry::new();

        // Both have a tool named "fetch_url"
        let tool1 = ToolResource {
            slug: "web-tools-v1".to_string(),
            name: "Web Tools V1".to_string(),
            description: None,
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "fetch_url",
                    "description": "Old version"
                }
            })],
        };

        let tool2 = ToolResource {
            slug: "web-tools-v2".to_string(),
            name: "Web Tools V2".to_string(),
            description: None,
            tools: vec![json!({
                "type": "function",
                "function": {
                    "name": "fetch_url",
                    "description": "New version"
                }
            })],
        };

        registry.register(tool1);
        registry.register(tool2);

        let resolved = registry
            .resolve_tools(&["web-tools-v1".to_string(), "web-tools-v2".to_string()])
            .unwrap();

        // Should only have 1 tool (deduplicated)
        assert_eq!(resolved.len(), 1);

        // Should be the "New version" (last one wins)
        assert_eq!(
            resolved[0]["function"]["description"].as_str().unwrap(),
            "New version"
        );
    }
}
