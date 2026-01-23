// src/core/parser.rs
use std::collections::HashMap;
use anyhow::{Result, anyhow, Context};
use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;
use petgraph::graph::DiGraph;
use serde_json::Value;
use crate::core::graph::{WorkflowGraph, Node, NodeType, Action, Edge};

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
                '\\' if in_quote && i + 1 < len && chars[i+1] == '"' => {
                    buffer.push(c);
                    buffer.push(chars[i+1]);
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
        
        let workflow_pair = pairs.next()
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
                Rule::EOI => {},
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_metadata(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut inner_it = pair.into_inner();
        let key_node = inner_it.next().ok_or_else(|| anyhow!("Metadata Parsing Error: Missing key."))?;
        let key_str = key_node.as_str();
        
        let val_node = inner_it.next().ok_or_else(|| anyhow!("Metadata Parsing Error: Missing value for key '{}'", key_str))?;
        
        match key_str {
            "slug" => workflow.slug = Self::parse_text_value_raw(val_node),
            "name" => workflow.name = Self::parse_text_value_raw(val_node),
            "version" => workflow.version = Self::parse_text_value_raw(val_node),
            "author" => workflow.author = Self::parse_text_value_raw(val_node),
            "description" => workflow.description = Self::parse_text_value_raw(val_node),
            "entry" | "exit" | "libs" | "prompts" | "agents" => {
                let string_vec = Self::parse_string_list_helper(val_node)?;
                match key_str {
                    "entry" => workflow.entry_node = string_vec.get(0).cloned().unwrap_or_default(),
                    "exit" => workflow.exit_nodes = string_vec,
                    "libs" => workflow.libs = string_vec,
                    "prompts" => workflow.prompt_patterns = string_vec,
                    "agents" => workflow.agent_patterns = string_vec,
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
                    let item_node = inner.into_inner().next()
                        .ok_or_else(|| anyhow!("Inconsistent metadata: Found an empty list item."))?;
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
        let id_node = inner_parts.next().ok_or_else(|| anyhow!("Node Syntax Error: Node is missing an ID."))?;
        let id_inner = id_node.into_inner().next().ok_or_else(|| anyhow!("Node ID Error: Invalid ID format."))?;
        let node_id_str = id_inner.as_str().to_string(); 

        let content_node = inner_parts.next()
            .ok_or_else(|| anyhow!("Node Content Error: Node '{}' has no executable body.", node_id_str))?;
        
        let node_type_res = match content_node.as_rule() {
            Rule::task_def => {
                let mut task_inner = content_node.into_inner();
                let tool_name = task_inner.next()
                    .ok_or_else(|| anyhow!("Task Error in [{}]: Missing tool name.", node_id_str))?
                    .as_str().to_string();
                
                let mut param_map = HashMap::new();
                for p_pair in task_inner {
                    let mut p_it = p_pair.into_inner();
                    let pk = p_it.next().unwrap().as_str().to_string();
                    let pv = p_it.next().unwrap().as_str().trim().to_string(); 
                    param_map.insert(pk, pv);
                }
                NodeType::Task(Action { name: tool_name, params: param_map })
            },
            Rule::while_def => {
                let mut w_it = content_node.into_inner();
                let cond_text = w_it.next().unwrap().as_str().trim().to_string();
                let body_node = w_it.next().unwrap();
                
                let mut inner_graph = WorkflowGraph::default();
                Self::parse_block(body_node, &mut inner_graph)?;
                NodeType::Loop { condition: cond_text, body: Box::new(inner_graph) }
            },
            Rule::foreach_def => {
                let mut f_it = content_node.into_inner();
                let item_v = f_it.next().unwrap().as_str().trim_start_matches('$').to_string();
                let list_v = f_it.next().unwrap().as_str().trim_start_matches('$').to_string();
                let body_node = f_it.next().unwrap();

                let mut inner_graph = WorkflowGraph::default();
                Self::parse_block(body_node, &mut inner_graph)?;
                NodeType::Foreach { item: item_v, list: list_v, body: Box::new(inner_graph) }
            },
            Rule::json_object | Rule::json_array | Rule::string | Rule::boolean | Rule::number | Rule::null => {
                let raw_content = content_node.as_str();
                let val_obj: Value = serde_json::from_str(raw_content)
                    .unwrap_or(Value::String(raw_content.to_string()));
                NodeType::Literal(val_obj)
            },
            _ => return Err(anyhow!("Compiler Error: Unknown rule type '{:?}' in node '{}'", content_node.as_rule(), node_id_str)),
        };

        let final_node = Node { id: node_id_str.clone(), node_type: node_type_res };
        let node_idx = workflow.graph.add_node(final_node);
        workflow.node_map.insert(node_id_str, node_idx);

        Ok(())
    }

    fn parse_chain_edge(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut it = pair.into_inner();
        let start_node = it.next().ok_or_else(|| anyhow!("Connection Error: Empty chain found."))?;
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

    fn commit_edge_to_graph(workflow: &mut WorkflowGraph, f_id: &str, t_id: &str, e_obj: Edge) -> Result<()> {
        let f_idx = *workflow.node_map.get(f_id)
            .ok_or_else(|| anyhow!("Graph Error: Attempted to link from undefined node '{}'.", f_id))?;
        let t_idx = *workflow.node_map.get(t_id)
            .ok_or_else(|| anyhow!("Graph Error: Attempted to link to undefined node '{}'.", t_id))?;
        
        workflow.graph.add_edge(f_idx, t_idx, e_obj);
        Ok(())
    }
}