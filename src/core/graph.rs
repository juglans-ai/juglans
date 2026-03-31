// src/core/graph.rs
use petgraph::graph::{DiGraph, NodeIndex};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
    /// Method call on an instance (legacy — $ syntax removed, kept for runtime compat)
    #[allow(dead_code)]
    MethodCall {
        instance_path: String,
        method_name: String,
        args: HashMap<String, String>,
    },
    /// return err { kind: "...", message: "..." } — explicit typed error (Rust-style)
    ReturnErr(Value),
    /// yield expression — emit value as SSE event during execution
    Yield(String),
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
    pub description: String,
    pub graph: DiGraph<Node, Edge>,
    pub node_map: HashMap<String, NodeIndex>,
    pub entry_node: String,
    pub libs: Vec<String>,
    pub prompt_patterns: Vec<String>,

    pub tool_patterns: Vec<String>,
    pub python_imports: Vec<String>,
    pub switch_routes: HashMap<String, SwitchRoute>,
    pub flow_imports: HashMap<String, String>,
    pub pending_edges: Vec<(String, String, Edge)>,
    /// Wildcard edges (from_pattern, to_pattern, edge), expanded during resolver phase
    pub pending_wildcard_edges: Vec<(String, String, Edge)>,
    pub functions: HashMap<String, FunctionDef>,
    pub lib_imports: HashMap<String, String>,
    pub lib_auto_namespaces: HashSet<String>,
    // Class definitions (class_name -> Arc<ClassDef>)
    pub classes: HashMap<String, Arc<ClassDef>>,
    // External method definitions, merged into ClassDef.methods after parsing (type_name, method_name, FunctionDef)
    pub pending_methods: Vec<(String, String, FunctionDef)>,
}

/// .jgflow Manifest — pure configuration struct, no DAG
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub slug: String,
    pub name: String,
    pub version: String,
    pub source: String,
    pub author: String,
    pub description: String,
    pub is_public: Option<bool>,
    pub schedule: Option<String>,
    pub entry_node: String,
    pub exit_nodes: Vec<String>,
    // Import-related fields
    pub libs: Vec<String>,
    pub lib_imports: HashMap<String, String>,
    pub lib_auto_namespaces: HashSet<String>,
    pub prompt_patterns: Vec<String>,

    pub tool_patterns: Vec<String>,
    pub python_imports: Vec<String>,
    pub flow_imports: HashMap<String, String>,
}

impl Manifest {
    /// Apply manifest metadata onto a WorkflowGraph (non-empty fields override)
    pub fn apply_to(&self, wf: &mut WorkflowGraph) {
        if !self.slug.is_empty() {
            wf.slug = self.slug.clone();
        }
        if !self.name.is_empty() {
            wf.name = self.name.clone();
        }
        if !self.version.is_empty() {
            wf.version = self.version.clone();
        }
        if !self.description.is_empty() {
            wf.description = self.description.clone();
        }
        if !self.libs.is_empty() {
            wf.libs = self.libs.clone();
            wf.lib_imports = self.lib_imports.clone();
            wf.lib_auto_namespaces = self.lib_auto_namespaces.clone();
        }
        if !self.entry_node.is_empty() {
            wf.entry_node = self.entry_node.clone();
        }
        if !self.prompt_patterns.is_empty() {
            wf.prompt_patterns = self.prompt_patterns.clone();
        }
        if !self.tool_patterns.is_empty() {
            wf.tool_patterns = self.tool_patterns.clone();
        }
    }
}

/// Function node definition: reusable node with parameters
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub params: Vec<String>,
    pub body: Arc<WorkflowGraph>,
}

/// Class field definition: name + optional type annotation + optional default value expression
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClassField {
    pub name: String,
    /// Type annotation (set in Pydantic-style body field declarations, e.g. "str", "int", "float")
    pub type_hint: Option<String>,
    /// Default value raw expression (evaluated at runtime)
    pub default: Option<String>,
}

/// Class definition: field list + method collection
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub fields: Vec<ClassField>,
    pub methods: HashMap<String, FunctionDef>,
    /// Field name → Vec index mapping, instances stored as `__fields__: [val0, val1, ...]`
    pub field_index: HashMap<String, usize>,
}

impl ClassDef {
    /// Build ClassDef and auto-generate field_index
    pub fn new(fields: Vec<ClassField>, methods: HashMap<String, FunctionDef>) -> Self {
        let field_index = fields
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect();
        Self {
            fields,
            methods,
            field_index,
        }
    }
}

impl WorkflowGraph {
    /// Create an empty graph (for testing)
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self::default()
    }
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
            description: String::new(),
            graph: DiGraph::new(),
            node_map: HashMap::new(),
            entry_node: String::new(),
            libs: Vec::new(),
            prompt_patterns: Vec::new(),
            tool_patterns: Vec::new(),
            python_imports: Vec::new(),
            switch_routes: HashMap::new(),
            flow_imports: HashMap::new(),
            pending_edges: Vec::new(),
            pending_wildcard_edges: Vec::new(),
            functions: HashMap::new(),
            lib_imports: HashMap::new(),
            lib_auto_namespaces: HashSet::new(),
            classes: HashMap::new(),
            pending_methods: Vec::new(),
        }
    }
}
