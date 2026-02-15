// src/core/parser.rs
use crate::core::graph::{Action, Edge, Node, NodeType, SwitchCase, SwitchRoute, WorkflowGraph};
use anyhow::{anyhow, Context, Result};
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use petgraph::graph::DiGraph;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Parser)]
#[grammar = "core/jwl.pest"]
struct JwlGrammar;

pub struct GraphParser;

impl GraphParser {
    /// 静态辅助方法：解析任务参数字符串
    /// 处理形如 `key1=value1, key2=[nested]` 的字符串
    pub fn parse_arguments_str(args_str: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();
        let mut buffer = String::new();
        let mut key = String::new();
        let mut depth = 0;
        let mut in_quote = false;
        let mut parsing_key = true;

        let chars: Vec<char> = args_str.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            let c = chars[i];

            match c {
                // 处理 key 和 value 的分隔符 '='
                '=' if depth == 0 && !in_quote && parsing_key => {
                    key = buffer.trim().to_string();
                    buffer.clear();
                    parsing_key = false;
                }
                // 处理参数之间的分隔符 ','
                ',' if depth == 0 && !in_quote => {
                    if !key.is_empty() {
                        params.insert(key.clone(), buffer.trim().to_string());
                    }
                    buffer.clear();
                    key.clear();
                    parsing_key = true;
                }
                // 处理引号
                '"' if depth == 0 => {
                    in_quote = !in_quote;
                    buffer.push(c);
                }
                // 处理转义引号 (在引号内部)
                '\\' if in_quote && i + 1 < len && chars[i + 1] == '"' => {
                    buffer.push(c);
                    buffer.push(chars[i + 1]);
                    i += 1; // 跳过下一个字符
                }
                // 处理嵌套结构：小括号、中括号、大括号
                '(' | '{' | '[' if !in_quote => {
                    depth += 1;
                    buffer.push(c);
                }
                ')' | '}' | ']' if !in_quote => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    buffer.push(c);
                }
                // 常规字符
                _ => {
                    // 如果正在解析 Key，跳过空白
                    // 如果正在解析 Value，保留空白（除非是值的前导空白，buffer.trim() 会处理）
                    // 这里的逻辑稍微宽松一点，把字符都放进去，最后 trim
                    buffer.push(c);
                }
            }
            i += 1;
        }

        // 处理最后一个参数
        if !key.is_empty() {
            params.insert(key, buffer.trim().to_string());
        }
        params
    }

    pub fn parse(content: &str) -> Result<WorkflowGraph> {
        let mut pairs = JwlGrammar::parse(Rule::workflow, content)
            .map_err(|e| anyhow!("JWL Compilation Syntax Error:\n{}", e))?;

        let workflow_pair = pairs
            .next()
            .ok_or_else(|| anyhow!("Compilation Error: The input workflow source is empty."))?;

        let mut workflow_instance = WorkflowGraph::default();

        if workflow_pair.as_rule() == Rule::workflow {
            Self::parse_block(workflow_pair, &mut workflow_instance)?;
        }

        if workflow_instance.entry_node.is_empty() {
            if let Some(first_node_index) = workflow_instance.graph.node_indices().next() {
                workflow_instance.entry_node = workflow_instance.graph[first_node_index].id.clone();
            } else {
                return Err(anyhow!("Architecture Error: Workflow must define an 'entry' node or contain at least one valid node."));
            }
        }

        Ok(workflow_instance)
    }

    fn parse_block(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::metadata => Self::parse_metadata(inner, workflow)?,
                Rule::node_def => Self::parse_node(inner, workflow)?,
                Rule::chain_edge_def => Self::parse_chain_edge(inner, workflow)?,
                Rule::complex_edge_def => Self::parse_complex_edge(inner, workflow)?,
                Rule::switch_edge_def => Self::parse_switch_edge(inner, workflow)?,
                Rule::EOI => {}
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_metadata(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut inner_it = pair.into_inner();
        let key_node = inner_it
            .next()
            .ok_or_else(|| anyhow!("Metadata Parsing Error: Missing key."))?;
        let key_str = key_node.as_str();

        let val_node = inner_it.next().ok_or_else(|| {
            anyhow!(
                "Metadata Parsing Error: Missing value for key '{}'",
                key_str
            )
        })?;

        match key_str {
            "slug" => workflow.slug = Self::parse_text_value_raw(val_node),
            "name" => workflow.name = Self::parse_text_value_raw(val_node),
            "version" => workflow.version = Self::parse_text_value_raw(val_node),
            "author" => workflow.author = Self::parse_text_value_raw(val_node),
            "description" => workflow.description = Self::parse_text_value_raw(val_node),
            "flows" => {
                // 解析 flows: { alias: "path", ... } 对象映射
                if val_node.as_rule() == Rule::meta_val_map {
                    for pair in val_node.into_inner() {
                        if pair.as_rule() == Rule::meta_map_pair {
                            let mut it = pair.into_inner();
                            let alias = it.next().unwrap().as_str().to_string();
                            let path = it.next().unwrap().as_str().trim_matches('"').to_string();
                            workflow.flow_imports.insert(alias, path);
                        }
                    }
                }
            }
            "entry" | "exit" | "libs" | "prompts" | "agents" | "tools" | "python" => {
                let string_vec = Self::parse_string_list_helper(val_node)?;
                match key_str {
                    "entry" => workflow.entry_node = string_vec.get(0).cloned().unwrap_or_default(),
                    "exit" => workflow.exit_nodes = string_vec,
                    "libs" => workflow.libs = string_vec,
                    "prompts" => workflow.prompt_patterns = string_vec,
                    "agents" => workflow.agent_patterns = string_vec,
                    "tools" => workflow.tool_patterns = string_vec,
                    "python" => workflow.python_imports = string_vec,
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_text_value_raw(pair: Pair<Rule>) -> String {
        match pair.as_rule() {
            Rule::string => pair.as_str().trim_matches('"').to_string(),
            Rule::identifier => pair.as_str().to_string(),
            _ => pair.as_str().to_string(),
        }
    }

    fn parse_string_list_helper(pair: Pair<Rule>) -> Result<Vec<String>> {
        let mut results_list = Vec::new();
        if pair.as_rule() == Rule::meta_val_list {
            for inner in pair.into_inner() {
                if inner.as_rule() == Rule::meta_list_item {
                    let item_node = inner.into_inner().next().ok_or_else(|| {
                        anyhow!("Inconsistent metadata: Found an empty list item.")
                    })?;
                    results_list.push(Self::parse_text_value_raw(item_node));
                }
            }
        } else {
            results_list.push(Self::parse_text_value_raw(pair));
        }
        Ok(results_list)
    }

    fn parse_node(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut inner_parts = pair.into_inner();
        let id_node = inner_parts
            .next()
            .ok_or_else(|| anyhow!("Node Syntax Error: Node is missing an ID."))?;
        let id_inner = id_node
            .into_inner()
            .next()
            .ok_or_else(|| anyhow!("Node ID Error: Invalid ID format."))?;
        let node_id_str = id_inner.as_str().to_string();

        let content_node = inner_parts.next().ok_or_else(|| {
            anyhow!(
                "Node Content Error: Node '{}' has no executable body.",
                node_id_str
            )
        })?;

        let node_type_res = match content_node.as_rule() {
            Rule::task_def => {
                let mut task_inner = content_node.into_inner();
                let tool_name = task_inner
                    .next()
                    .ok_or_else(|| anyhow!("Task Error in [{}]: Missing tool name.", node_id_str))?
                    .as_str()
                    .to_string();

                let mut param_map = HashMap::new();
                for p_pair in task_inner {
                    let mut p_it = p_pair.into_inner();
                    let pk = p_it.next().unwrap().as_str().to_string();
                    let pv = p_it.next().unwrap().as_str().trim().to_string();
                    if param_map.contains_key(&pk) {
                        return Err(anyhow!(
                            "Duplicate parameter '{}' in node [{}]",
                            pk, node_id_str
                        ));
                    }
                    param_map.insert(pk, pv);
                }
                NodeType::Task(Action {
                    name: tool_name,
                    params: param_map,
                })
            }
            Rule::while_def => {
                let mut w_it = content_node.into_inner();
                let cond_text = w_it.next().unwrap().as_str().trim().to_string();
                let body_node = w_it.next().unwrap();

                let mut inner_graph = WorkflowGraph::default();
                Self::parse_block(body_node, &mut inner_graph)?;
                NodeType::Loop {
                    condition: cond_text,
                    body: Box::new(inner_graph),
                }
            }
            Rule::foreach_def => {
                let mut f_it = content_node.into_inner();
                let item_v = f_it
                    .next()
                    .unwrap()
                    .as_str()
                    .trim_start_matches('$')
                    .to_string();
                let list_v = f_it
                    .next()
                    .unwrap()
                    .as_str()
                    .trim_start_matches('$')
                    .to_string();
                let body_node = f_it.next().unwrap();

                let mut inner_graph = WorkflowGraph::default();
                Self::parse_block(body_node, &mut inner_graph)?;
                NodeType::Foreach {
                    item: item_v,
                    list: list_v,
                    body: Box::new(inner_graph),
                }
            }
            Rule::json_object
            | Rule::json_array
            | Rule::string
            | Rule::boolean
            | Rule::number
            | Rule::null => {
                let raw_content = content_node.as_str();
                let val_obj: Value = serde_json::from_str(raw_content)
                    .unwrap_or(Value::String(raw_content.to_string()));
                NodeType::Literal(val_obj)
            }
            _ => {
                return Err(anyhow!(
                    "Compiler Error: Unknown rule type '{:?}' in node '{}'",
                    content_node.as_rule(),
                    node_id_str
                ))
            }
        };

        let final_node = Node {
            id: node_id_str.clone(),
            node_type: node_type_res,
        };
        let node_idx = workflow.graph.add_node(final_node);
        workflow.node_map.insert(node_id_str, node_idx);

        Ok(())
    }

    fn parse_chain_edge(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut it = pair.into_inner();
        let start_node = it
            .next()
            .ok_or_else(|| anyhow!("Connection Error: Empty chain found."))?;
        let mut last_id = start_node.into_inner().next().unwrap().as_str();

        while let Some(item) = it.next() {
            if item.as_rule() == Rule::simple_arrow {
                if let Some(target_node) = it.next() {
                    let target_id = target_node.into_inner().next().unwrap().as_str();
                    Self::commit_edge_to_graph(workflow, last_id, target_id, Edge::default())?;
                    last_id = target_id;
                }
            }
        }
        Ok(())
    }

    fn parse_complex_edge(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut it = pair.into_inner();
        let from_id = it.next().unwrap().into_inner().next().unwrap().as_str();

        let mut next = it.next().unwrap();
        let mut edge_data = Edge::default();

        if next.as_rule() == Rule::edge_error {
            edge_data.is_error_path = true;
            next = it.next().unwrap();
        } else if next.as_rule() == Rule::edge_condition {
            let cond_str = next.as_str();
            edge_data.condition = Some(cond_str.trim_start_matches("if").trim().to_string());
            next = it.next().unwrap();
        }

        let to_id = next.into_inner().next().unwrap().as_str();
        Self::commit_edge_to_graph(workflow, from_id, to_id, edge_data)?;

        Ok(())
    }

    /// Parse switch edge definition: [node] -> switch $var { "case1": [target1], default: [target2] }
    fn parse_switch_edge(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut it = pair.into_inner();

        // First is the source node_id
        let from_node = it.next().ok_or_else(|| anyhow!("Switch edge missing source node"))?;
        let from_id = from_node.into_inner().next().unwrap().as_str().to_string();

        // Next is the switch subject (optional) and body
        let mut subject = String::new();
        let mut cases: Vec<SwitchCase> = Vec::new();

        for item in it {
            match item.as_rule() {
                Rule::switch_subject => {
                    subject = item.as_str().trim().to_string();
                }
                Rule::switch_body => {
                    for case_item in item.into_inner() {
                        if case_item.as_rule() == Rule::switch_case {
                            let mut case_it = case_item.into_inner();
                            let case_value_or_default = case_it.next().unwrap();

                            let case_value = if case_value_or_default.as_rule() == Rule::switch_default {
                                None
                            } else {
                                // It's a switch_case_value (string, number, boolean, or variable_ref)
                                let val_str = case_value_or_default.as_str().trim();
                                // Remove quotes from strings
                                let clean_val = if val_str.starts_with('"') && val_str.ends_with('"') {
                                    val_str[1..val_str.len()-1].to_string()
                                } else {
                                    val_str.to_string()
                                };
                                Some(clean_val)
                            };

                            let target_node = case_it.next().ok_or_else(|| {
                                anyhow!("Switch case missing target node for value: {:?}", case_value)
                            })?;
                            let target_id = target_node.into_inner().next()
                                .ok_or_else(|| anyhow!("Invalid target node in switch case"))?
                                .as_str().to_string();

                            cases.push(SwitchCase {
                                value: case_value.clone(),
                                target: target_id.clone(),
                            });

                            // Create edge with switch_case marker
                            let edge = Edge {
                                condition: None,
                                is_error_path: false,
                                switch_case: case_value,
                            };
                            Self::commit_edge_to_graph(workflow, &from_id, &target_id, edge)?;
                        }
                    }
                }
                _ => {}
            }
        }

        // Store the switch route
        workflow.switch_routes.insert(from_id.clone(), SwitchRoute {
            subject,
            cases,
        });

        Ok(())
    }

    fn commit_edge_to_graph(
        workflow: &mut WorkflowGraph,
        f_id: &str,
        t_id: &str,
        e_obj: Edge,
    ) -> Result<()> {
        // 命名空间节点（含 '.'）在 parse 阶段还不存在，需要延迟到 flow 合并后再 commit
        let f_is_namespaced = f_id.contains('.');
        let t_is_namespaced = t_id.contains('.');

        if f_is_namespaced || t_is_namespaced {
            workflow.pending_edges.push((f_id.to_string(), t_id.to_string(), e_obj));
            return Ok(());
        }

        let f_idx = *workflow.node_map.get(f_id).ok_or_else(|| {
            anyhow!(
                "Graph Error: Attempted to link from undefined node '{}'.",
                f_id
            )
        })?;
        let t_idx = *workflow.node_map.get(t_id).ok_or_else(|| {
            anyhow!(
                "Graph Error: Attempted to link to undefined node '{}'.",
                t_id
            )
        })?;

        workflow.graph.add_edge(f_idx, t_idx, e_obj);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_imports_parsing() {
        let content = r#"
name: "Python Workflow"
python: ["pandas", "sklearn.ensemble", "./utils.py"]
entry: [load]

[load]: pandas.read_csv(path="data.csv")
[train]: sklearn.ensemble.RandomForestClassifier()

[load] -> [train]
"#;
        let graph = GraphParser::parse(content).unwrap();

        assert_eq!(graph.python_imports.len(), 3);
        assert!(graph.python_imports.contains(&"pandas".to_string()));
        assert!(graph.python_imports.contains(&"sklearn.ensemble".to_string()));
        assert!(graph.python_imports.contains(&"./utils.py".to_string()));
    }

    #[test]
    fn test_scoped_identifier_call() {
        let content = r#"
name: "Scoped Call Test"
python: ["pandas"]
entry: [load]

[load]: pandas.read_csv(path="data.csv", encoding="utf-8")
"#;
        let graph = GraphParser::parse(content).unwrap();

        // Verify the node was parsed correctly
        let node = graph.graph.node_weights().next().unwrap();
        assert_eq!(node.id, "load");

        if let NodeType::Task(action) = &node.node_type {
            assert_eq!(action.name, "pandas.read_csv");
            assert_eq!(action.params.get("path"), Some(&"\"data.csv\"".to_string()));
            assert_eq!(action.params.get("encoding"), Some(&"\"utf-8\"".to_string()));
        } else {
            panic!("Expected Task node type");
        }
    }

    #[test]
    fn test_switch_syntax_parsing() {
        let content = r#"
name: "Switch Test"
entry: [start]

[start]: notify(message="start")
[case_a]: notify(message="A")
[case_b]: notify(message="B")
[fallback]: notify(message="default")

[start] -> switch $type {
    "a": [case_a]
    "b": [case_b]
    default: [fallback]
}
"#;
        let graph = GraphParser::parse(content).unwrap();

        // Verify switch route was created
        assert!(graph.switch_routes.contains_key("start"));
        let switch_route = graph.switch_routes.get("start").unwrap();
        assert_eq!(switch_route.subject.trim(), "$type");
        assert_eq!(switch_route.cases.len(), 3);

        // Verify cases
        assert_eq!(switch_route.cases[0].value, Some("a".to_string()));
        assert_eq!(switch_route.cases[0].target, "case_a");
        assert_eq!(switch_route.cases[1].value, Some("b".to_string()));
        assert_eq!(switch_route.cases[1].target, "case_b");
        assert_eq!(switch_route.cases[2].value, None); // default
        assert_eq!(switch_route.cases[2].target, "fallback");
    }

    #[test]
    fn test_missing_comma_detected() {
        let content = r#"
name: "Test"
entry: [start]
[start]: notify(message="hello" status="ok")
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_err(),
            "Missing comma between parameters should cause parse error"
        );
    }

    #[test]
    fn test_valid_comma_separated_params() {
        let content = r#"
name: "Test"
entry: [start]
[start]: notify(message="hello", status="ok")
"#;
        let result = GraphParser::parse(content);
        assert!(result.is_ok(), "Comma-separated params should parse: {:?}", result.err());
    }

    #[test]
    fn test_comparison_in_expression() {
        // == in edge conditions should still work
        let content = r#"
name: "Test"
entry: [start]
[start]: notify(message="test")
[a]: notify(message="a")
[b]: notify(message="b")
[start] if $output.category == "technical" -> [a]
[start] -> [b]
"#;
        let result = GraphParser::parse(content);
        assert!(result.is_ok(), "Comparison operators should be valid: {:?}", result.err());
    }

    #[test]
    fn test_duplicate_param_detected() {
        let content = r#"
name: "Test"
entry: [start]
[start]: notify(message="first", message="second")
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_err(),
            "Duplicate parameter keys should cause parse error"
        );
    }

    #[test]
    fn test_multiline_params_with_commas() {
        let content = r#"
name: "Test"
entry: [start]
[start]: chat(
  agent="helper",
  message=$input.query
)
"#;
        let result = GraphParser::parse(content);
        assert!(result.is_ok(), "Multiline params should parse: {:?}", result.err());
    }

    #[test]
    fn test_string_concat_expression() {
        let content = r#"
name: "Test"
entry: [start]
[start]: chat(agent="helper", message="[Expert] " + $input.query)
"#;
        let result = GraphParser::parse(content);
        assert!(result.is_ok(), "String concat should parse: {:?}", result.err());
    }
}
