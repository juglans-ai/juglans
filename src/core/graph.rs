// src/core/graph.rs
use petgraph::graph::{DiGraph, NodeIndex};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub params: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum NodeType {
    Task(Action),
    /// Assert keyword: evaluates expression, fails if falsy
    Assert(String),
    /// Assign call: executes tool and stores result in variable
    AssignCall {
        var: String,
        action: Action,
    },
    Foreach {
        item: String,
        list: String,
        body: Box<WorkflowGraph>,
        parallel: bool,
    },
    Literal(Value),
    Loop {
        condition: String,
        body: Box<WorkflowGraph>,
    },
    /// External call (e.g., Python module.function)
    _ExternalCall {
        /// Full call path (e.g., "pandas.read_csv" or "$df.describe")
        call_path: String,
        /// Positional arguments
        args: Vec<String>,
        /// Named arguments
        kwargs: HashMap<String, String>,
    },
    /// Class instantiation: new ClassName(field=value, ...)
    NewInstance {
        class_name: String,
        args: HashMap<String, String>,
    },
    /// Method call on an instance: $instance.method(args)
    MethodCall {
        instance_path: String,
        method_name: String,
        args: HashMap<String, String>,
    },
    /// return err { kind: "...", message: "..." } — explicit typed error (Rust-style)
    ReturnErr(Value),
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
    /// Result routing: ok case (node succeeded)
    pub is_ok: bool,
    /// Result routing: err case (node failed)
    pub is_err: bool,
    /// Error kind filter for err cases (e.g., err "timeout" → Some("timeout"))
    pub err_kind: Option<String>,
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
    pub source: String,
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
    // 【新增】函数节点定义（带参数的可复用节点）
    pub functions: HashMap<String, FunctionDef>,
    // 【新增】库导入映射 (namespace → path)
    pub lib_imports: HashMap<String, String>,
    // 【新增】列表形式导入的自动命名空间集合（resolver 中允许被 slug 覆盖）
    pub lib_auto_namespaces: HashSet<String>,
    // 资源可见性
    pub is_public: Option<bool>,
    // Cron 调度表达式 (e.g. "0 9 * * *")
    pub schedule: Option<String>,
    // Class 定义 (class_name -> ClassDef)
    pub classes: HashMap<String, ClassDef>,
}

/// 函数节点定义：带参数的可复用节点
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub params: Vec<String>,
    pub body: Box<WorkflowGraph>,
}

/// Class 字段定义：名称 + 可选类型注解 + 可选默认值表达式
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClassField {
    pub name: String,
    /// 类型注解（Pydantic 风格 body 字段声明时设置，如 "str", "int", "float"）
    pub type_hint: Option<String>,
    /// 默认值原始表达式（运行时求值）
    pub default: Option<String>,
}

/// Class 定义：字段列表 + 方法集合
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub fields: Vec<ClassField>,
    pub methods: HashMap<String, FunctionDef>,
}

/// Check if a node ID belongs to the test framework (`test_*` prefix)
pub fn is_test_node_id(id: &str) -> bool {
    id.starts_with("test_")
}

impl Default for WorkflowGraph {
    fn default() -> Self {
        WorkflowGraph {
            slug: String::new(),
            name: String::new(),
            version: String::new(),
            source: String::new(),
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
            functions: HashMap::new(),
            lib_imports: HashMap::new(),
            lib_auto_namespaces: HashSet::new(),
            is_public: None,
            schedule: None,
            classes: HashMap::new(),
        }
    }
}
