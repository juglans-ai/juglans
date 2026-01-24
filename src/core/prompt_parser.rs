// src/core/prompt_parser.rs
use anyhow::{Result, anyhow};
use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};

#[derive(Parser)]
#[grammar = "core/prompt.pest"]
struct JwlPromptGrammar;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TemplateNode {
    Text(String),
    Interpolation(String),
    If {
        condition: String,
        then_branch: Vec<TemplateNode>,
        elif_branches: Vec<(String, Vec<TemplateNode>)>,
        else_branch: Option<Vec<TemplateNode>>,
    },
    For {
        var_name: String,
        iterable_expr: String,
        body: Vec<TemplateNode>,
        else_branch: Option<Vec<TemplateNode>>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PromptResource {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub r#type: String, 
    pub inputs: Value, 
    pub ast: Vec<TemplateNode>,
    pub content: String,
}

pub struct PromptParser;

impl PromptParser {
    pub fn parse(text: &str) -> Result<PromptResource> {
        let mut pairs = JwlPromptGrammar::parse(Rule::prompt_file, text)
            .map_err(|e| anyhow!("Prompt Syntax Error:\n{}", e))?;

        let root = pairs.next().ok_or_else(|| anyhow!("Empty prompt file"))?;
        let mut resource = PromptResource::default();
        resource.inputs = json!({});

        for pair in root.into_inner() {
            match pair.as_rule() {
                Rule::frontmatter => {
                    for meta in pair.into_inner() {
                        match meta.as_rule() {
                            Rule::key_slug => resource.slug = Self::parse_raw_string(meta),
                            Rule::key_name => resource.name = Self::parse_raw_string(meta),
                            Rule::key_desc => resource.description = Some(Self::parse_raw_string(meta)),
                            Rule::key_type => resource.r#type = Self::parse_raw_string(meta),
                            Rule::key_inputs => {
                                if let Some(obj_node) = meta.into_inner().next() {
                                    resource.inputs = Self::parse_object(obj_node);
                                }
                            },
                            _ => {}
                        }
                    }
                },
                Rule::body => {
                    resource.content = pair.as_str().to_string();
                    resource.ast = Self::parse_body_to_ast(pair)?;
                }
                _ => {}
            }
        }

        Ok(resource)
    }

    fn parse_body_to_ast(pair: Pair<Rule>) -> Result<Vec<TemplateNode>> {
        let mut nodes = Vec::new();
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::raw_text => nodes.push(TemplateNode::Text(inner.as_str().to_string())),
                Rule::interpolation => {
                    let expr = inner.into_inner()
                        .find(|p| p.as_rule() == Rule::expression)
                        .unwrap().as_str().trim().to_string();
                    nodes.push(TemplateNode::Interpolation(expr));
                }
                Rule::if_block => {
                    let mut it = inner.into_inner();
                    let header = it.next().unwrap();
                    let cond = header.into_inner()
                        .find(|p| p.as_rule() == Rule::expression)
                        .unwrap().as_str().trim().to_string();
                    let then_nodes = Self::parse_body_to_ast(it.next().unwrap())?;
                    let mut elif_branches = Vec::new();
                    let mut else_nodes = None;

                    while let Some(next) = it.next() {
                        match next.as_rule() {
                            Rule::elif_branch => {
                                let mut elif_it = next.into_inner();
                                let elif_tag = elif_it.next().unwrap();
                                let elif_cond = elif_tag.into_inner()
                                    .find(|p| p.as_rule() == Rule::expression)
                                    .unwrap().as_str().trim().to_string();
                                let elif_body = Self::parse_body_to_ast(elif_it.next().unwrap())?;
                                elif_branches.push((elif_cond, elif_body));
                            }
                            Rule::else_tag => {
                                else_nodes = Some(Self::parse_body_to_ast(it.next().unwrap())?);
                            }
                            _ => {}
                        }
                    }
                    nodes.push(TemplateNode::If {
                        condition: cond,
                        then_branch: then_nodes,
                        elif_branches,
                        else_branch: else_nodes,
                    });
                }
                Rule::for_block => {
                    let mut it = inner.into_inner();
                    let header = it.next().unwrap(); 
                    let mut h_inner = header.into_inner();
                    let var_name = h_inner.find(|p| p.as_rule() == Rule::identifier).unwrap().as_str().to_string();
                    let iterable = h_inner.find(|p| p.as_rule() == Rule::expression).unwrap().as_str().trim().to_string();
                    let body_nodes = Self::parse_body_to_ast(it.next().unwrap())?;
                    let mut else_nodes = None;
                    while let Some(next) = it.next() {
                        if next.as_rule() == Rule::else_tag {
                            else_nodes = Some(Self::parse_body_to_ast(it.next().unwrap())?);
                        }
                    }
                    nodes.push(TemplateNode::For { var_name, iterable_expr: iterable, body: body_nodes, else_branch: else_nodes });
                }
                _ => {}
            }
        }
        Ok(nodes)
    }

    fn parse_raw_string(pair: Pair<Rule>) -> String {
        pair.into_inner().next().map(|s| s.as_str().trim_matches('"').to_string()).unwrap_or_default()
    }

    /// Recursively parses any JSON-compatible value from the Pest tree
    fn parse_json_value(pair: Pair<Rule>) -> Value {
        match pair.as_rule() {
            Rule::string => json!(pair.as_str().trim_matches('"')),
            Rule::number => json!(pair.as_str().parse::<f64>().unwrap_or(0.0)),
            Rule::boolean => json!(pair.as_str() == "true"),
            Rule::json_object => {
                let mut map = serde_json::Map::new();
                for p in pair.into_inner() { // json_pair
                    let mut inner = p.into_inner();
                    let key = inner.next().unwrap().as_str().trim_matches('"').to_string();
                    let val = Self::parse_json_value(inner.next().unwrap());
                    map.insert(key, val);
                }
                Value::Object(map)
            }
            Rule::json_array => {
                let mut vec = Vec::new();
                for p in pair.into_inner() {
                    vec.push(Self::parse_json_value(p));
                }
                Value::Array(vec)
            }
            _ => Value::Null,
        }
    }

    fn parse_object(pair: Pair<Rule>) -> Value {
        match pair.as_rule() {
            Rule::json_object => Self::parse_json_value(pair),
            Rule::json_array => Self::parse_json_value(pair),
            _ => {
                // Handle the obj_pair* list format (YAML-like)
                let mut map = serde_json::Map::new();
                for p in pair.into_inner() {
                    if p.as_rule() == Rule::obj_pair {
                        let mut inner = p.into_inner();
                        let k = inner.next().unwrap().as_str().to_string();
                        let v_node = inner.next().unwrap();
                        map.insert(k, Self::parse_json_value(v_node));
                    }
                }
                Value::Object(map)
            }
        }
    }
}