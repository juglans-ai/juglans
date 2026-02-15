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
    /// External call (e.g., Python module.function)
    ExternalCall {
        /// Full call path (e.g., "pandas.read_csv" or "$df.describe")
        call_path: String,
        /// Positional arguments
        args: Vec<String>,
        /// Named arguments
        kwargs: HashMap<String, String>,
    },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub node_type: NodeType,
}

/// A single case in a switch expression
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// The value to match (None for default case)
    pub value: Option<String>,
    /// The target node ID
    pub target: String,
}

/// Switch routing from a single source node
#[derive(Debug, Clone)]
pub struct SwitchRoute {
    /// The subject expression to evaluate (e.g., "$output.intent")
    pub subject: String,
    /// The cases to match
    pub cases: Vec<SwitchCase>,
}

#[derive(Debug, Clone, Default)]
pub struct Edge {
    pub condition: Option<String>,
    pub is_error_path: bool,
    /// Switch routing (if this edge is part of a switch)
    pub switch_case: Option<String>,
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
    // 【新增】Python 模块导入列表 (支持系统模块、.py 文件、glob 模式)
    pub python_imports: Vec<String>,
    // 【新增】Switch 路由表 (source_node_id -> SwitchRoute)
    pub switch_routes: HashMap<String, SwitchRoute>,
    // 【新增】Flow 导入映射 (alias -> relative_path)，用于跨工作流图合并
    pub flow_imports: HashMap<String, String>,
    // 【新增】待解析的边（引用了命名空间节点，需要在 flow 合并后才能 commit）
    pub pending_edges: Vec<(String, String, Edge)>,
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
            python_imports: Vec::new(),
            switch_routes: HashMap::new(),
            flow_imports: HashMap::new(),
            pending_edges: Vec::new(),
        }
    }
}
