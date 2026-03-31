// src/core/prompt_parser.rs
use anyhow::{anyhow, Result};
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
    Tag {
        name: String,
        attributes: Vec<(String, String)>,
        children: Vec<TemplateNode>,
        self_closing: bool,
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
    pub is_public: Option<bool>,
}

pub struct PromptParser;

impl PromptParser {
    pub fn parse(text: &str) -> Result<PromptResource> {
        let mut pairs = JwlPromptGrammar::parse(Rule::prompt_file, text)
            .map_err(|e| anyhow!("Prompt Syntax Error:\n{}", e))?;

        let root = pairs.next().ok_or_else(|| anyhow!("Empty prompt file"))?;
        let mut resource = PromptResource {
            inputs: json!({}),
            ..Default::default()
        };

        for pair in root.into_inner() {
            match pair.as_rule() {
                Rule::frontmatter => {
                    for meta in pair.into_inner() {
                        match meta.as_rule() {
                            Rule::key_slug => resource.slug = Self::parse_raw_string(meta),
                            Rule::key_name => resource.name = Self::parse_raw_string(meta),
                            Rule::key_desc => {
                                resource.description = Some(Self::parse_raw_string(meta))
                            }
                            Rule::key_type => resource.r#type = Self::parse_raw_string(meta),
                            Rule::key_inputs => {
                                if let Some(obj_node) = meta.into_inner().next() {
                                    resource.inputs = Self::parse_object(obj_node);
                                }
                            }
                            Rule::key_public => {
                                let val = meta.into_inner().next().unwrap().as_str();
                                resource.is_public = Some(val == "true");
                            }
                            _ => {}
                        }
                    }
                }
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
                    let expr = inner
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::expression)
                        .unwrap()
                        .as_str()
                        .trim()
                        .to_string();
                    nodes.push(TemplateNode::Interpolation(expr));
                }
                Rule::if_block => {
                    let mut it = inner.into_inner();
                    let header = it.next().unwrap();
                    let cond = header
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::expression)
                        .unwrap()
                        .as_str()
                        .trim()
                        .to_string();
                    let then_nodes = Self::parse_body_to_ast(it.next().unwrap())?;
                    let mut elif_branches = Vec::new();
                    let mut else_nodes = None;

                    while let Some(next) = it.next() {
                        match next.as_rule() {
                            Rule::elif_branch => {
                                let mut elif_it = next.into_inner();
                                let elif_tag = elif_it.next().unwrap();
                                let elif_cond = elif_tag
                                    .into_inner()
                                    .find(|p| p.as_rule() == Rule::expression)
                                    .unwrap()
                                    .as_str()
                                    .trim()
                                    .to_string();
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
                    let var_name = h_inner
                        .find(|p| p.as_rule() == Rule::identifier)
                        .unwrap()
                        .as_str()
                        .to_string();
                    let iterable = h_inner
                        .find(|p| p.as_rule() == Rule::expression)
                        .unwrap()
                        .as_str()
                        .trim()
                        .to_string();
                    let body_nodes = Self::parse_body_to_ast(it.next().unwrap())?;
                    let mut else_nodes = None;
                    while let Some(next) = it.next() {
                        if next.as_rule() == Rule::else_tag {
                            else_nodes = Some(Self::parse_body_to_ast(it.next().unwrap())?);
                        }
                    }
                    nodes.push(TemplateNode::For {
                        var_name,
                        iterable_expr: iterable,
                        body: body_nodes,
                        else_branch: else_nodes,
                    });
                }
                Rule::tag_self_close => {
                    let mut tag_inner = inner.into_inner();
                    let name = tag_inner
                        .find(|p| p.as_rule() == Rule::identifier)
                        .unwrap()
                        .as_str()
                        .to_string();
                    let attributes = Self::parse_tag_attrs(tag_inner);
                    nodes.push(TemplateNode::Tag {
                        name,
                        attributes,
                        children: vec![],
                        self_closing: true,
                    });
                }
                Rule::tag_block => {
                    let mut it = inner.into_inner();
                    // tag_open: extract name + attributes
                    let open = it.next().unwrap();
                    let mut open_inner = open.into_inner();
                    let name = open_inner
                        .find(|p| p.as_rule() == Rule::identifier)
                        .unwrap()
                        .as_str()
                        .to_string();
                    let attributes = Self::parse_tag_attrs(open_inner);
                    // body
                    let children = Self::parse_body_to_ast(it.next().unwrap())?;
                    // tag_close: validate name match
                    let close = it.next().unwrap();
                    let close_name = close
                        .into_inner()
                        .find(|p| p.as_rule() == Rule::identifier)
                        .unwrap()
                        .as_str();
                    if close_name != name {
                        return Err(anyhow!(
                            "Mismatched tag: opening '{}' but closing '{}'",
                            name,
                            close_name
                        ));
                    }
                    nodes.push(TemplateNode::Tag {
                        name,
                        attributes,
                        children,
                        self_closing: false,
                    });
                }
                _ => {}
            }
        }
        Ok(nodes)
    }

    fn parse_tag_attrs<'a>(pairs: impl Iterator<Item = Pair<'a, Rule>>) -> Vec<(String, String)> {
        let mut attrs = Vec::new();
        for pair in pairs {
            match pair.as_rule() {
                Rule::tag_attrs => {
                    for attr_pair in pair.into_inner() {
                        if attr_pair.as_rule() == Rule::tag_attr {
                            let mut inner = attr_pair.into_inner();
                            if let Some(key_pair) = inner.next() {
                                let key = key_pair.as_str().to_string();
                                if let Some(val_pair) = inner.next() {
                                    let val = match val_pair.as_rule() {
                                        Rule::tag_attr_value => {
                                            let inner_val = val_pair.into_inner().next().unwrap();
                                            match inner_val.as_rule() {
                                                Rule::string => {
                                                    inner_val.as_str().trim_matches('"').to_string()
                                                }
                                                _ => inner_val.as_str().trim().to_string(),
                                            }
                                        }
                                        Rule::string => {
                                            val_pair.as_str().trim_matches('"').to_string()
                                        }
                                        _ => val_pair.as_str().trim().to_string(),
                                    };
                                    attrs.push((key, val));
                                }
                            }
                        }
                    }
                }
                Rule::tag_attr => {
                    let mut inner = pair.into_inner();
                    if let Some(key_pair) = inner.next() {
                        let key = key_pair.as_str().to_string();
                        if let Some(val_pair) = inner.next() {
                            let val = match val_pair.as_rule() {
                                Rule::tag_attr_value => {
                                    let inner_val = val_pair.into_inner().next().unwrap();
                                    match inner_val.as_rule() {
                                        Rule::string => {
                                            inner_val.as_str().trim_matches('"').to_string()
                                        }
                                        _ => inner_val.as_str().trim().to_string(),
                                    }
                                }
                                Rule::string => val_pair.as_str().trim_matches('"').to_string(),
                                _ => val_pair.as_str().trim().to_string(),
                            };
                            attrs.push((key, val));
                        }
                    }
                }
                _ => {}
            }
        }
        attrs
    }

    fn parse_raw_string(pair: Pair<Rule>) -> String {
        pair.into_inner()
            .next()
            .map(|s| s.as_str().trim_matches('"').to_string())
            .unwrap_or_default()
    }

    /// Recursively parses any JSON-compatible value from the Pest tree
    fn parse_json_value(pair: Pair<Rule>) -> Value {
        match pair.as_rule() {
            Rule::string => json!(pair.as_str().trim_matches('"')),
            Rule::number => json!(pair.as_str().parse::<f64>().unwrap_or(0.0)),
            Rule::boolean => json!(pair.as_str() == "true"),
            Rule::json_object => {
                let mut map = serde_json::Map::new();
                for p in pair.into_inner() {
                    // json_pair
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_body(template: &str) -> Vec<TemplateNode> {
        let source = format!("---\nslug: \"test\"\n---\n{}", template);
        let resource = PromptParser::parse(&source).unwrap();
        resource.ast
    }

    #[test]
    fn test_self_closing_tag() {
        let ast = parse_body("{% icon name=\"star\" /%}");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            TemplateNode::Tag {
                name,
                attributes,
                self_closing,
                children,
            } => {
                assert_eq!(name, "icon");
                assert_eq!(attributes, &[("name".to_string(), "star".to_string())]);
                assert!(*self_closing);
                assert!(children.is_empty());
            }
            _ => panic!("Expected Tag node"),
        }
    }

    #[test]
    fn test_block_tag() {
        let ast = parse_body("{% callout type=\"warning\" %}\nImportant!\n{% /callout %}");
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            TemplateNode::Tag {
                name,
                attributes,
                self_closing,
                children,
            } => {
                assert_eq!(name, "callout");
                assert_eq!(attributes, &[("type".to_string(), "warning".to_string())]);
                assert!(!*self_closing);
                assert!(!children.is_empty());
            }
            _ => panic!("Expected Tag node"),
        }
    }

    #[test]
    fn test_tag_with_expression_attr() {
        let ast = parse_body("{% partial name=user.name /%}");
        match &ast[0] {
            TemplateNode::Tag {
                name, attributes, ..
            } => {
                assert_eq!(name, "partial");
                assert_eq!(attributes, &[("name".to_string(), "user.name".to_string())]);
            }
            _ => panic!("Expected Tag node"),
        }
    }

    #[test]
    fn test_tag_with_multiple_attrs() {
        let ast = parse_body("{% endpoint method=\"GET\" path=\"/api/users\" /%}");
        match &ast[0] {
            TemplateNode::Tag {
                name, attributes, ..
            } => {
                assert_eq!(name, "endpoint");
                assert_eq!(attributes.len(), 2);
                assert_eq!(attributes[0], ("method".to_string(), "GET".to_string()));
                assert_eq!(
                    attributes[1],
                    ("path".to_string(), "/api/users".to_string())
                );
            }
            _ => panic!("Expected Tag node"),
        }
    }

    #[test]
    fn test_reserved_keyword_not_tag() {
        // "if" should still be parsed as if_block, not tag_block
        let ast = parse_body("{% if true %}\nhello\n{% endif %}");
        match &ast[0] {
            TemplateNode::If { .. } => {} // correct
            _ => panic!("Expected If node, not Tag"),
        }
    }

    #[test]
    fn test_tag_mismatched_close() {
        let source = "---\nslug: \"test\"\n---\n{% foo %}content{% /bar %}";
        let result = PromptParser::parse(source);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Mismatched tag"));
    }

    #[test]
    fn test_tag_with_nested_interpolation() {
        let ast = parse_body("{% callout type=\"info\" %}\nHello {{ name }}!\n{% /callout %}");
        match &ast[0] {
            TemplateNode::Tag { children, .. } => {
                assert!(children.len() >= 2); // Text + Interpolation + Text
                assert!(children
                    .iter()
                    .any(|n| matches!(n, TemplateNode::Interpolation(_))));
            }
            _ => panic!("Expected Tag node"),
        }
    }

    #[test]
    fn test_tag_renders_children_transparently() {
        use crate::core::renderer::JwlRenderer;
        use serde_json::json;

        let ast = vec![TemplateNode::Tag {
            name: "callout".into(),
            attributes: vec![("type".into(), "warning".into())],
            children: vec![TemplateNode::Text("Important!".into())],
            self_closing: false,
        }];
        let renderer = JwlRenderer::new();
        let result = renderer.render(&ast, &json!({})).unwrap();
        assert_eq!(result, "Important!");
    }

    #[test]
    fn test_self_closing_tag_renders_empty() {
        use crate::core::renderer::JwlRenderer;
        use serde_json::json;

        let ast = vec![TemplateNode::Tag {
            name: "hr".into(),
            attributes: vec![],
            children: vec![],
            self_closing: true,
        }];
        let renderer = JwlRenderer::new();
        let result = renderer.render(&ast, &json!({})).unwrap();
        assert_eq!(result, "");
    }
}
