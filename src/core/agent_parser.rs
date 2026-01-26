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
    pub workflow: Option<String>, // 【新增】
    pub mcp: Vec<String>,
    pub skills: Vec<String>,
}

pub struct AgentParser;

impl AgentParser {
    pub fn parse(content: &str) -> Result<AgentResource> {
        let mut pairs = AgentGrammar::parse(Rule::agent_def, content)
            .map_err(|e| anyhow!("Agent Syntax Error:\n{}", e))?;

        let mut agent = AgentResource::default();
        agent.model = "gpt-4o".to_string();
        agent.temperature = Some(0.7);

        for pair in pairs.next().unwrap().into_inner() {
            match pair.as_rule() {
                Rule::key_slug => agent.slug = Self::parse_string(pair),
                Rule::key_name => agent.name = Self::parse_string(pair),
                Rule::key_desc => agent.description = Some(Self::parse_string(pair)),
                Rule::key_model => agent.model = Self::parse_string(pair),
                Rule::key_workflow => agent.workflow = Some(Self::parse_string(pair)), // 【新增】
                Rule::key_temp => {
                    let val_str = pair.into_inner().next().unwrap().as_str();
                    agent.temperature = Some(val_str.parse().unwrap_or(0.7));
                }
                Rule::key_mcp => agent.mcp = Self::parse_list(pair),
                Rule::key_skills => agent.skills = Self::parse_list(pair),
                Rule::key_system => {
                    let inner = pair.into_inner().next().unwrap();
                    if inner.as_rule() == Rule::string {
                        agent.system_prompt = inner.as_str().trim_matches('"').to_string();
                    } else if inner.as_rule() == Rule::p_func {
                        let slug_node = inner.into_inner().next().unwrap();
                        agent.system_prompt_slug =
                            Some(slug_node.as_str().trim_matches('"').to_string());
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
}
