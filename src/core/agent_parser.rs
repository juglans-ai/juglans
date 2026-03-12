// src/core/agent_parser.rs
use anyhow::{anyhow, Result};
use pest::Parser;
use pest_derive::Parser;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[grammar = "core/agent.pest"]
struct AgentGrammar;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AgentResource {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub model: String,
    pub temperature: Option<f64>,
    pub system_prompt: String,
    pub system_prompt_slug: Option<String>,
    pub source: Option<String>,
    pub endpoint: Option<String>,
    pub tools: Option<String>,
    pub mcp: Vec<String>,
    pub skills: Vec<String>,
    /// @username for this agent (auto-registers handle in jug0)
    pub username: Option<String>,
    /// Avatar image (local file path or URL)
    pub avatar: Option<String>,
    /// Visibility (default false)
    pub is_public: Option<bool>,
}

pub struct AgentParser;

impl AgentParser {
    pub fn parse(content: &str) -> Result<AgentResource> {
        let mut pairs = AgentGrammar::parse(Rule::agent_def, content)
            .map_err(|e| anyhow!("Agent Syntax Error:\n{}", e))?;

        let mut agent = AgentResource {
            model: "gpt-4o".to_string(),
            temperature: Some(0.7),
            ..Default::default()
        };

        for pair in pairs.next().unwrap().into_inner() {
            match pair.as_rule() {
                Rule::key_slug => agent.slug = Self::parse_string(pair),
                Rule::key_name => agent.name = Self::parse_string(pair),
                Rule::key_desc => agent.description = Some(Self::parse_string(pair)),
                Rule::key_model => agent.model = Self::parse_string(pair),
                Rule::key_source => agent.source = Some(Self::parse_string(pair)),
                Rule::key_endpoint => agent.endpoint = Some(Self::parse_string(pair)),
                Rule::key_tools => {
                    // Supports three formats: JSON array (inline), string (single reference), list (multiple references)
                    let inner = pair.into_inner().next().unwrap();
                    agent.tools = Some(match inner.as_rule() {
                        // Inline JSON array: tools: [{...}, {...}]
                        Rule::json_array => Self::parse_json_value(inner),

                        // Single reference: tools: "web-tools"
                        // Uses @ prefix to mark as reference for runtime identification
                        Rule::string => {
                            let slug = inner.as_str().trim_matches('"');
                            format!("@{}", slug)
                        }

                        // Multiple references: tools: ["web-tools", "data-tools"]
                        // Parsed as string array and serialized to JSON
                        Rule::list => {
                            let slugs = Self::parse_list(inner.clone());
                            serde_json::to_string(&slugs).unwrap_or_else(|_| "[]".to_string())
                        }

                        _ => String::new(),
                    });
                }
                Rule::key_temp => {
                    let val_str = pair.into_inner().next().unwrap().as_str();
                    agent.temperature = Some(val_str.parse().unwrap_or(0.7));
                }
                Rule::key_mcp => agent.mcp = Self::parse_list(pair),
                Rule::key_skills => agent.skills = Self::parse_list(pair),
                Rule::key_username => agent.username = Some(Self::parse_string(pair)),
                Rule::key_avatar => agent.avatar = Some(Self::parse_string(pair)),
                Rule::key_public => {
                    let val = pair.into_inner().next().unwrap().as_str();
                    agent.is_public = Some(val == "true");
                }
                Rule::key_system => {
                    let inner = pair.into_inner().next().unwrap();
                    match inner.as_rule() {
                        Rule::string => {
                            agent.system_prompt = inner.as_str().trim_matches('"').to_string();
                        }
                        Rule::p_func => {
                            let slug_node = inner.into_inner().next().unwrap();
                            agent.system_prompt_slug =
                                Some(slug_node.as_str().trim_matches('"').to_string());
                        }
                        Rule::multiline_string => {
                            // Get the raw text after the "|" marker
                            let raw = inner.as_str();
                            // Skip the leading "|"
                            let body = raw.strip_prefix('|').unwrap_or(raw);

                            // Split into lines, skip the first (empty after "|")
                            let lines: Vec<&str> = body.lines().collect();
                            let content_lines: Vec<&str> =
                                if lines.first().is_some_and(|l| l.is_empty()) {
                                    lines[1..].to_vec()
                                } else {
                                    lines
                                };

                            // Find minimum indentation (ignoring blank lines)
                            let min_indent = content_lines
                                .iter()
                                .filter(|l| !l.trim().is_empty())
                                .map(|l| l.len() - l.trim_start().len())
                                .min()
                                .unwrap_or(0);

                            // Dedent all lines, preserving blank lines
                            let dedented: Vec<&str> = content_lines
                                .iter()
                                .map(|l| {
                                    if l.trim().is_empty() {
                                        ""
                                    } else if l.len() >= min_indent {
                                        &l[min_indent..]
                                    } else {
                                        l.trim_start()
                                    }
                                })
                                .collect();

                            agent.system_prompt = dedented.join("\n");
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if agent.slug.is_empty() {
            return Err(anyhow!("Agent must have a 'slug' field"));
        }
        Ok(agent)
    }

    fn parse_string(pair: pest::iterators::Pair<Rule>) -> String {
        pair.into_inner()
            .next()
            .unwrap()
            .as_str()
            .trim_matches('"')
            .to_string()
    }

    fn parse_list(pair: pest::iterators::Pair<Rule>) -> Vec<String> {
        let mut list = Vec::new();
        let list_node = pair.into_inner().next().unwrap();
        for item in list_node.into_inner() {
            list.push(item.as_str().trim_matches('"').to_string());
        }
        list
    }

    /// Parse JSON value and convert to JSON string
    fn parse_json_value(pair: pest::iterators::Pair<Rule>) -> String {
        use serde_json::json;

        match pair.as_rule() {
            Rule::json_object => {
                let mut map = serde_json::Map::new();
                for inner_pair in pair.into_inner() {
                    if inner_pair.as_rule() == Rule::json_pair {
                        let mut pair_iter = inner_pair.into_inner();
                        let key = pair_iter
                            .next()
                            .unwrap()
                            .as_str()
                            .trim_matches('"')
                            .to_string();
                        let value = Self::parse_json_value(pair_iter.next().unwrap());
                        map.insert(key, serde_json::from_str(&value).unwrap_or(json!(value)));
                    }
                }
                serde_json::to_string(&map).unwrap()
            }
            Rule::json_array => {
                let mut arr = Vec::new();
                for inner_pair in pair.into_inner() {
                    let value = Self::parse_json_value(inner_pair);
                    arr.push(
                        serde_json::from_str::<serde_json::Value>(&value).unwrap_or(json!(value)),
                    );
                }
                serde_json::to_string(&arr).unwrap()
            }
            Rule::string => {
                let s = pair.as_str().trim_matches('"').to_string();
                serde_json::to_string(&s).unwrap()
            }
            Rule::number => pair.as_str().to_string(),
            Rule::boolean => pair.as_str().to_string(),
            Rule::null => "null".to_string(),
            _ => "null".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiline_system_prompt() {
        let input = r#"slug: "test-agent"
name: "Test Agent"
model: "gpt-4o"
system_prompt: |
  You are a professional cryptocurrency AI assistant.

  Your capabilities:
  - Answer questions about market trends, technical analysis, and tokens
  - Use navigate_to_page to navigate to different pages
  - Use get_market_data to fetch candlestick data

  Important: If the user expresses any trading intent in their message,
  you must call the create_trade_suggestion tool.
"#;
        let agent = AgentParser::parse(input).expect("parse should succeed");
        assert_eq!(agent.slug, "test-agent");
        assert!(agent
            .system_prompt
            .contains("You are a professional cryptocurrency AI assistant."));
        assert!(agent.system_prompt.contains("\n\n"));
        assert!(agent.system_prompt.contains("Your capabilities:"));
        assert!(agent.system_prompt.contains("create_trade_suggestion"));
        println!("=== Parsed system_prompt ===\n{}", agent.system_prompt);
    }

    #[test]
    fn test_string_system_prompt() {
        let input = r#"slug: "basic"
name: "Basic"
system_prompt: "You are a helpful assistant."
"#;
        let agent = AgentParser::parse(input).expect("parse should succeed");
        assert_eq!(agent.system_prompt, "You are a helpful assistant.");
    }
}
