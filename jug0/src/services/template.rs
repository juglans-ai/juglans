// src/services/template.rs
use std::collections::HashMap;
use anyhow::Result;
use regex::{Captures, Regex};
use lazy_static::lazy_static;

// 【核心变更】: 正则表达式支持可选的描述/fallback 部分.
// - `([a-zA-Z0-9_]+)`: Captures the variable key (group 1).
// - `(?:\{([\s\S]*?)\})?`: An optional non-capturing group for the description.
//   - `\{([\s\S]*?)\}`: Captures the description text (group 2).
//   - `?` makes the whole description group optional.
lazy_static! {
    static ref TEMPLATE_RE: Regex = Regex::new(r"\{\{\s*([a-zA-Z0-9_]+)\s*(?:\{([\s\S]*?)\})?\s*\}\}").unwrap();
}

pub struct TemplateEngine;

impl TemplateEngine {
    /// Renders a template string with multi-level variable resolution.
    ///
    /// The rendering follows this priority:
    /// 1. Variables provided by the client (`client_vars`).
    /// 2. Variables resolved by the server (e.g., `market_snapshot`).
    /// 3. Fallback description text provided in the template itself (e.g., `{{key}{fallback}}`).
    /// 4. A placeholder indicating a missing variable.
    pub async fn render(
        template: &str,
        client_vars: &HashMap<String, String>,
        user_id: Option<&str>,
        // In the future, this can take &AppState for DB access etc.
    ) -> Result<String> {
        
        // --- Step 1: Collect all server-side keys that need to be resolved ---
        let mut server_keys_to_resolve = Vec::new();
        for cap in TEMPLATE_RE.captures_iter(template) {
            if let Some(key_match) = cap.get(1) {
                let key = key_match.as_str();
                // If the key is not provided by the client, it might need server-side resolution.
                if !client_vars.contains_key(key) {
                    server_keys_to_resolve.push(key.to_string());
                }
            }
        }
        
        // --- Step 2: Asynchronously resolve all server-side variables ---
        let mut resolved_server_vars = HashMap::new();
        for key in server_keys_to_resolve {
            // This is where you would call async functions to get data.
            // For now, we simulate it with a match statement.
            let value = match key.as_str() {
                "market_snapshot" => "BTC: $88,000 (+1.5%); ETH: $3,000 (-0.5%)".to_string(),
                "user_memory" => {
                    if let Some(uid) = user_id {
                        format!("(Retrieved 3 memories for user {})", uid)
                    } else {
                        "No user context available.".to_string()
                    }
                },
                "server_time_utc" => chrono::Utc::now().to_rfc3339(),
                _ => continue, // Skip unknown server variables
            };
            resolved_server_vars.insert(key, value);
        }

        // --- Step 3: Perform the final replacement using all available data ---
        let final_rendered_text = TEMPLATE_RE.replace_all(template, |caps: &Captures| {
            let key = caps.get(1).map_or("", |m| m.as_str());

            // Priority 1: Client variables
            if let Some(value) = client_vars.get(key) {
                return value.clone();
            }

            // Priority 2: Server-resolved variables
            if let Some(value) = resolved_server_vars.get(key) {
                return value.clone();
            }

            // Priority 3: Fallback text from template `{{key}{fallback}}`
            if let Some(fallback) = caps.get(2) {
                return fallback.as_str().to_string();
            }

            // Priority 4: Indicate missing variable
            format!("[Missing: {}]", key)
        });

        Ok(final_rendered_text.to_string())
    }
}