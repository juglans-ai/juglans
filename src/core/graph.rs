// src/core/graph.rs
use petgraph::graph::{DiGraph, NodeIndex};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub params: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum NodeType {
    Task(Action),
    Foreach {
        item: String,
        list: String,
        body: Box<WorkflowGraph>,
    },
    Literal(Value),
    Loop {
        condition: String,
        body: Box<WorkflowGraph>,
    },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub node_type: NodeType,
}

#[derive(Debug, Clone, Default)]
pub struct Edge {
    pub condition: Option<String>,
    pub is_error_path: bool,
}

#[derive(Debug, Clone)]
pub struct WorkflowGraph {
    pub slug: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub graph: DiGraph<Node, Edge>,
    pub node_map: HashMap<String, NodeIndex>,
    pub entry_node: String,
    pub exit_nodes: Vec<String>,
    pub libs: Vec<String>,
    pub prompt_patterns: Vec<String>,
    // 【新增】Agent 导入路径模式，用于自动加载
    pub agent_patterns: Vec<String>,
    // 【新增】Tool 导入路径模式，用于自动加载
    pub tool_patterns: Vec<String>,
}

impl Default for WorkflowGraph {
    fn default() -> Self {
        WorkflowGraph {
            slug: String::new(),
            name: String::new(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            graph: DiGraph::new(),
            node_map: HashMap::new(),
            entry_node: String::new(),
            exit_nodes: Vec::new(),
            libs: Vec::new(),
            prompt_patterns: Vec::new(),
            // 【新增】
            agent_patterns: Vec::new(),
            tool_patterns: Vec::new(),
        }
    }
}
