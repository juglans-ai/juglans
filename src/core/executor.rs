// src/core/executor.rs
#![cfg(not(target_arch = "wasm32"))]

use anyhow::{anyhow, Result};
use futures::future::join_all;
use lazy_static::lazy_static;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use regex::{Captures, Regex};
use rhai::{Dynamic, Engine, Scope};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

use crate::builtins::BuiltinRegistry;
use crate::core::context::WorkflowContext;
use crate::core::graph::{NodeType, WorkflowGraph};
use crate::core::parser::GraphParser;
use crate::runtime::python::PythonRuntime;
use crate::services::agent_loader::AgentRegistry;
use crate::services::config::{DebugConfig, JuglansConfig};
use crate::services::interface::JuglansRuntime;
use crate::services::mcp::{McpClient, McpTool};
use crate::services::prompt_loader::PromptRegistry;
use crate::services::tool_registry::ToolRegistry;

lazy_static! {
    static ref CONTEXT_VAR_RE: Regex = Regex::new(r"\$([a-zA-Z0-9_.]+)").unwrap();
    static ref FUNC_CALL_RE: Regex =
        Regex::new(r"(?s)^([a-zA-Z0-9_.]+)\((.*)\)(\.[a-zA-Z0-9_]+)?$").unwrap();
}

pub struct WorkflowExecutor {
    builtin_registry: Arc<BuiltinRegistry>,
    mcp_client: McpClient,
    mcp_tools_map: HashMap<String, McpTool>,
    tool_registry: Arc<ToolRegistry>,
    rhai_engine: Engine,
    debug_config: DebugConfig,
    /// Python runtime for executing external Python calls
    python_runtime: Option<Arc<Mutex<PythonRuntime>>>,
    /// Imported Python modules (from workflow python: [...] declaration)
    python_imports: Vec<String>,
}

impl WorkflowExecutor {
    pub async fn new(
        prompt_registry: Arc<PromptRegistry>,
        agent_registry: Arc<AgentRegistry>,
        runtime: Arc<dyn JuglansRuntime>,
    ) -> Self {
        Self::new_with_debug(prompt_registry, agent_registry, runtime, DebugConfig::default()).await
    }

    pub async fn new_with_debug(
        prompt_registry: Arc<PromptRegistry>,
        agent_registry: Arc<AgentRegistry>,
        runtime: Arc<dyn JuglansRuntime>,
        debug_config: DebugConfig,
    ) -> Self {
        let mut engine = Engine::new_raw();
        engine.set_max_operations(1_000_000);
        engine.set_max_call_levels(10);

        let registry_arc = BuiltinRegistry::new(prompt_registry, agent_registry, runtime);

        // 将 devtools schema 自动注册到 ToolRegistry（slug: "devtools"）
        let mut tool_registry = ToolRegistry::new();
        registry_arc.register_devtools_to_registry(&mut tool_registry);

        Self {
            builtin_registry: registry_arc,
            mcp_client: McpClient::new(),
            mcp_tools_map: HashMap::new(),
            tool_registry: Arc::new(tool_registry),
            rhai_engine: engine,
            debug_config,
            python_runtime: None,
            python_imports: Vec::new(),
        }
    }

    /// 获取 builtin registry 的引用（用于注入 executor）
    pub fn get_registry(&self) -> &Arc<BuiltinRegistry> {
        &self.builtin_registry
    }

    /// Load tool definitions from workflow patterns
    pub async fn load_tools(&mut self, workflow: &WorkflowGraph) {
        use crate::core::tool_loader::ToolLoader;
        use std::path::Path;

        if workflow.tool_patterns.is_empty() {
            return;
        }

        info!(
            "📦 Loading tool definitions from {} pattern(s)...",
            workflow.tool_patterns.len()
        );

        let workflow_base_dir = Path::new("."); // 可以从 workflow 文件路径推导
        let mut loaded_count = 0;

        for pattern in &workflow.tool_patterns {
            match ToolLoader::load_from_glob(pattern, workflow_base_dir) {
                Ok(tools) => {
                    loaded_count += tools.len();
                    // 需要获取可变引用，所以使用 Arc::get_mut 或 Mutex
                    // 这里暂时创建一个新的 registry 并替换
                    let mut registry = (*self.tool_registry).clone();
                    registry.register_all(tools);
                    self.tool_registry = Arc::new(registry);
                }
                Err(e) => {
                    warn!("Failed to load tools from pattern '{}': {}", pattern, e);
                }
            }
        }

        if loaded_count > 0 {
            info!(
                "  ✅ Loaded {} tool resource(s) with {} total tools",
                self.tool_registry.count(),
                loaded_count
            );
        }
    }

    /// Get a reference to the tool registry
    pub fn get_tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Replace the tool registry
    pub fn set_tool_registry(&mut self, registry: Arc<ToolRegistry>) {
        self.tool_registry = registry;
    }

    pub async fn load_mcp_tools(&mut self, config: &JuglansConfig) {
        if config.mcp_servers.is_empty() {
            return;
        }

        info!(
            "🔌 Connecting to {} MCP servers...",
            config.mcp_servers.len()
        );

        for server_conf in &config.mcp_servers {
            match self.mcp_client.fetch_tools(server_conf).await {
                Ok(tools) => {
                    info!(
                        "  ✅ Connected to [{}], found {} tools.",
                        server_conf.name,
                        tools.len()
                    );
                    let namespace = server_conf.alias.as_deref().unwrap_or(&server_conf.name);
                    for tool in tools {
                        let namespaced_key = format!("{}.{}", namespace, tool.name);
                        debug!("    Registered tool: {}", namespaced_key);
                        self.mcp_tools_map.insert(namespaced_key, tool);
                    }
                }
                Err(e) => warn!("  ❌ Failed to connect to [{}]: {}", server_conf.name, e),
            }
        }
    }

    /// Initialize Python runtime if the workflow has python imports
    pub fn init_python_runtime(&mut self, workflow: &WorkflowGraph) -> Result<()> {
        if workflow.python_imports.is_empty() {
            return Ok(());
        }

        info!(
            "🐍 Initializing Python runtime with {} import(s)...",
            workflow.python_imports.len()
        );

        // Store the imports for later resolution
        self.python_imports = workflow.python_imports.clone();

        // Create Python runtime with 1 worker for now
        // TODO: Make worker count configurable
        let mut runtime = PythonRuntime::new(1)?;
        runtime.set_imports(workflow.python_imports.clone());
        self.python_runtime = Some(Arc::new(Mutex::new(runtime)));

        info!("  ✅ Python runtime initialized with imports: {:?}", workflow.python_imports);
        Ok(())
    }

    /// Check if a tool name is a Python module call
    fn is_python_call(&self, name: &str) -> bool {
        if self.python_imports.is_empty() {
            return false;
        }

        // Check if the tool name starts with any imported module
        for import in &self.python_imports {
            // Handle file imports (e.g., "./utils.py" -> "utils")
            let module_name = if import.ends_with(".py") {
                import.rsplit('/').next()
                    .map(|f| f.trim_end_matches(".py"))
                    .unwrap_or(import)
            } else {
                import.as_str()
            };

            if name == module_name || name.starts_with(&format!("{}.", module_name)) {
                return true;
            }
        }
        false
    }

    /// Execute a Python function call
    fn execute_python_call(
        &self,
        name: &str,
        params: &HashMap<String, String>,
    ) -> Result<Option<Value>> {
        let runtime = self.python_runtime.as_ref()
            .ok_or_else(|| anyhow!("Python runtime not initialized"))?;

        let mut rt = runtime.lock()
            .map_err(|e| anyhow!("Failed to lock Python runtime: {}", e))?;

        // Convert params to kwargs
        let mut kwargs: HashMap<String, Value> = HashMap::new();
        for (k, v) in params {
            // Try to parse as JSON, fall back to string
            let val = serde_json::from_str(v).unwrap_or(Value::String(v.clone()));
            kwargs.insert(k.clone(), val);
        }

        let result = rt.call(name, Vec::new(), kwargs)?;
        Ok(Some(result))
    }

    async fn process_parameter(&self, param_str: &str, context: &WorkflowContext) -> Result<Value> {
        let clean_param = param_str.trim();

        if let Some(caps) = FUNC_CALL_RE.captures(clean_param) {
            let tool_name = &caps[1];
            let args_str = &caps[2];
            let field_access = caps.get(3).map(|m| m.as_str().trim_start_matches('.'));

            let raw_args = GraphParser::parse_arguments_str(args_str);
            let mut resolved_args = HashMap::new();

            for (k, v) in raw_args {
                let resolved_val = Box::pin(self.process_parameter(&v, context)).await?;
                let val_str = match resolved_val {
                    Value::String(s) => s,
                    Value::Null => "".to_string(),
                    other => other.to_string(),
                };
                resolved_args.insert(k, val_str);
            }

            let result_val = self
                .execute_tool_internal(tool_name, &resolved_args, context)
                .await?;

            if let Some(field) = field_access {
                if let Some(obj) = result_val.as_ref().and_then(|v| v.as_object()) {
                    let field_val = obj.get(field).cloned().unwrap_or(Value::Null);
                    return Ok(field_val);
                }
            }
            return Ok(result_val.unwrap_or(Value::Null));
        }

        let clean_param_no_quotes = if clean_param.starts_with('"') && clean_param.ends_with('"') {
            &clean_param[1..clean_param.len() - 1]
        } else {
            clean_param
        };

        if CONTEXT_VAR_RE.is_match(clean_param_no_quotes) {
            // 检查是否是纯变量引用（没有运算符/表达式）
            let is_pure_variable = clean_param_no_quotes.starts_with('$') &&
                !clean_param_no_quotes.contains("==") &&
                !clean_param_no_quotes.contains("!=") &&
                !clean_param_no_quotes.contains(">=") &&
                !clean_param_no_quotes.contains("<=") &&
                !clean_param_no_quotes.contains('>') &&
                !clean_param_no_quotes.contains('<') &&
                !clean_param_no_quotes.contains('+') &&
                !clean_param_no_quotes.contains('-') &&
                !clean_param_no_quotes.contains('*') &&
                !clean_param_no_quotes.contains('/') &&
                !clean_param_no_quotes.contains("&&") &&
                !clean_param_no_quotes.contains("||") &&
                !clean_param_no_quotes.contains('(') &&
                !clean_param_no_quotes.contains(' ');

            if is_pure_variable {
                // 纯变量引用：返回原始 JSON 类型（保留 boolean, number 等）
                // 如果变量不存在，返回 null（而不是报错），使条件路由更直观
                let path = &clean_param_no_quotes[1..]; // 去掉 $
                let path = if path.starts_with("ctx.") {
                    &path[4..]
                } else {
                    path
                };
                let resolved = context.resolve_path(path)?.unwrap_or(Value::Null);
                if self.debug_config.show_variables {
                    info!("🔍 [Debug] Resolve: ${} → {:?}", path, resolved);
                }
                return Ok(resolved);
            }

            // 包含表达式：替换变量后用 Rhai 评估
            let rendered = CONTEXT_VAR_RE.replace_all(clean_param_no_quotes, |caps: &Captures| {
                let raw_path = &caps[1];
                let path = if raw_path.starts_with("ctx.") {
                    &raw_path[4..]
                } else {
                    raw_path
                };
                context
                    .resolve_path(path)
                    .ok()
                    .flatten()
                    .map(|v| match v {
                        // 保留类型的字符串表示（用于 Rhai 评估）
                        Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")), // 转义引号
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        Value::Null => "()".to_string(), // Rhai 用 () 表示 null/unit
                        Value::Array(_) | Value::Object(_) => v.to_string(),
                    })
                    .unwrap_or_else(|| "()".to_string()) // 变量不存在时也返回 Rhai 的 unit
            });

            // 用 Rhai 评估表达式
            let mut scope = Scope::new();
            let context_val = context.get_as_value()?;
            let dynamic_ctx = rhai::serde::to_dynamic(context_val)?;
            if let Some(map) = dynamic_ctx.try_cast::<rhai::Map>() {
                scope.push("ctx", map);
            }

            match self.rhai_engine.eval_with_scope::<Dynamic>(&mut scope, &rendered.to_string()) {
                Ok(result) => {
                    let json_result = rhai::serde::from_dynamic::<Value>(&result)?;
                    return Ok(json_result);
                }
                Err(e) => {
                    return Err(anyhow!("Failed to evaluate expression '{}': {}", rendered, e));
                }
            }
        }

        let mut scope = Scope::new();
        let context_val = context.get_as_value()?;
        let dynamic_ctx = rhai::serde::to_dynamic(context_val)?;
        if let Some(map) = dynamic_ctx.try_cast::<rhai::Map>() {
            scope.push("ctx", map);
        }

        match self
            .rhai_engine
            .eval_with_scope::<Dynamic>(&mut scope, clean_param)
        {
            Ok(result) => {
                let json_result = rhai::serde::from_dynamic::<Value>(&result)?;
                Ok(json_result)
            }
            Err(_) => Ok(json!(clean_param_no_quotes)),
        }
    }

    pub async fn execute_tool_internal(
        &self,
        name: &str,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // 1. Check built-in tools first
        if let Some(tool) = self.builtin_registry.get(name) {
            return tool.execute(params, context).await;
        }

        // 2. Check MCP tools
        if let Some(mcp_tool) = self.mcp_tools_map.get(name) {
            let mut args = serde_json::Map::new();
            for (k, v) in params {
                let val = serde_json::from_str(v).unwrap_or(Value::String(v.clone()));
                args.insert(k.clone(), val);
            }
            let output_str = self
                .mcp_client
                .execute_tool(mcp_tool, Value::Object(args))
                .await?;
            let parsed_val = serde_json::from_str(&output_str).unwrap_or(Value::String(output_str));
            return Ok(Some(parsed_val));
        }

        // 3. Check Python imports
        if self.is_python_call(name) {
            debug!("🐍 Executing Python call: {}", name);
            return self.execute_python_call(name, params);
        }

        Err(anyhow!("Function/Tool '{}' not found", name))
    }

    /// 尝试执行 MCP tool（供 Chat builtin 的 tool call loop 使用）
    /// 如果 tool 不在 mcp_tools_map 中，返回 None
    pub async fn execute_mcp_tool(&self, name: &str, args_json_str: &str) -> Option<String> {
        let mcp_tool = self.mcp_tools_map.get(name)?;
        let args: Value = serde_json::from_str(args_json_str).unwrap_or(json!({}));
        match self.mcp_client.execute_tool(mcp_tool, args).await {
            Ok(output) => Some(output),
            Err(e) => Some(format!("MCP tool error: {}", e)),
        }
    }

    async fn evaluate_condition_async(
        &self,
        script: &str,
        context: &WorkflowContext,
    ) -> Result<bool> {
        let val = self.process_parameter(script, context).await?;
        let result = val.as_bool().unwrap_or(false);
        if self.debug_config.show_conditions {
            info!("🔀 [Debug] Condition '{}' → {} (raw: {:?})", script, result, val);
        }
        Ok(result)
    }

    async fn run_single_node(
        self: Arc<Self>,
        node_idx: NodeIndex,
        workflow: &Arc<WorkflowGraph>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let node = &workflow.graph[node_idx];
        let status_suffix = if let Ok(Some(s)) = context.resolve_path("reply.status") {
            format!(" | Status: {}", s.as_str().unwrap_or(""))
        } else {
            "".to_string()
        };

        debug!("│ → [{}]{}", node.id, status_suffix);

        if self.debug_config.show_nodes {
            info!("📦 [Debug] Node [{}]: {:?}", node.id, node.node_type);
        }

        match &node.node_type {
            NodeType::Literal(val) => {
                debug!("│   Literal value assigned");
                Ok(Some(val.clone()))
            }
            NodeType::Task(action) => {
                let mut rendered_params = HashMap::new();
                for (key, val_template) in &action.params {
                    let processed_val = self.process_parameter(val_template, context).await?;
                    let val_str = match processed_val {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    rendered_params.insert(key.clone(), val_str);
                }
                debug!("  Arguments: {:?}", rendered_params);
                self.execute_tool_internal(&action.name, &rendered_params, context)
                    .await
            }
            NodeType::Foreach { item, list, body } => {
                let clean_path = if list.starts_with("ctx.") {
                    &list[4..]
                } else {
                    list
                };
                debug!(
                    "│   Foreach: {} in {} ({})",
                    item, list, clean_path
                );
                let list_val = context
                    .resolve_path(clean_path)?
                    .ok_or_else(|| anyhow!("Foreach list variable '{}' not found", clean_path))?;
                let array = list_val
                    .as_array()
                    .ok_or_else(|| anyhow!("Variable '{}' is not an array.", list))?;
                for (i, val) in array.iter().enumerate() {
                    debug!("│   ├─ Iteration {}/{}", i + 1, array.len());
                    context.set(item.clone(), val.clone())?;
                    let body_arc = Arc::new(*body.clone());
                    if let Err(e) = self.clone().execute_graph(body_arc, context).await {
                        return Err(anyhow!("Error inside foreach body at index {}: {}", i, e));
                    }
                }
                Ok(None)
            }
            NodeType::Loop { condition, body } => {
                info!(
                    "  [Control] Entering while loop with condition: '{}'",
                    condition
                );
                let mut loop_count = 0;
                loop {
                    if loop_count > 100 {
                        return Err(anyhow!("Loop limit exceeded."));
                    }
                    if !self.evaluate_condition_async(condition, context).await? {
                        info!("  [Control] Loop condition is false, exiting loop.");
                        break;
                    }
                    let body_arc = Arc::new(*body.clone());
                    if let Err(e) = self.clone().execute_graph(body_arc, context).await {
                        return Err(anyhow!("Error inside loop body: {}", e));
                    }
                    loop_count += 1;
                }
                Ok(None)
            }
            NodeType::ExternalCall { call_path, args, kwargs } => {
                // TODO: Implement Python worker call
                // For now, this is a placeholder until we implement the Python runtime
                debug!("│   ExternalCall: {} args={:?} kwargs={:?}", call_path, args, kwargs);
                Err(anyhow!(
                    "Python runtime not yet implemented. Cannot execute external call: {}",
                    call_path
                ))
            }
        }
    }

    pub async fn run(
        self: Arc<Self>,
        workflow: Arc<WorkflowGraph>,
        config: &JuglansConfig,
    ) -> Result<()> {
        self.run_with_input(workflow, config, None).await
    }

    pub async fn run_with_input(
        self: Arc<Self>,
        workflow: Arc<WorkflowGraph>,
        config: &JuglansConfig,
        input: Option<Value>,
    ) -> Result<()> {
        info!(
            "🚀 Starting Execution: {} (v{})",
            workflow.name, workflow.version
        );
        debug!("👤 User: {}", config.account.name);
        let context = WorkflowContext::new();

        // 设置输入数据到 ctx.input
        if let Some(input_val) = input {
            if let Some(obj) = input_val.as_object() {
                for (key, val) in obj {
                    context.set(format!("input.{}", key), val.clone())?;
                }
            }
            // 同时设置完整的 input 对象
            context.set("input".to_string(), input_val)?;
        }

        info!("\n--- Execution Log ---");
        self.execute_graph(workflow, &context).await?;
        info!("🎉 Workflow finished successfully.");
        Ok(())
    }

    /// 清理不可达节点：检查并跳过那些所有前驱都已完成但仍有入度的节点
    /// 这些节点永远无法执行（因为前驱都没有激活通往它们的边）
    fn cleanup_unreachable_nodes(
        workflow: &Arc<WorkflowGraph>,
        in_degrees: &Arc<Mutex<HashMap<NodeIndex, usize>>>,
        ready_queue: &Arc<Mutex<VecDeque<NodeIndex>>>,
        completed_nodes: &Arc<Mutex<HashSet<NodeIndex>>>,
    ) {
        let unreachable_nodes = Arc::new(Mutex::new(HashSet::new()));
        let completed = completed_nodes.lock().unwrap().clone();
        let mut degrees = in_degrees.lock().unwrap();

        // 找出所有不可达的节点
        let unreachable: Vec<NodeIndex> = degrees
            .iter()
            .filter(|(idx, &degree)| {
                if degree == 0 || completed.contains(idx) {
                    return false;
                }

                // 检查所有前驱是否都已完成
                let all_predecessors_done = workflow
                    .graph
                    .edges_directed(**idx, Direction::Incoming)
                    .all(|e| completed.contains(&e.source()));

                all_predecessors_done
            })
            .map(|(idx, _)| *idx)
            .collect();

        drop(degrees);
        drop(completed);

        // 递归处理不可达节点
        for node_idx in unreachable {
            Self::mark_unreachable_recursive(
                node_idx,
                workflow,
                in_degrees,
                ready_queue,
                completed_nodes,
                &unreachable_nodes,
            );
        }
    }

    /// 递归标记节点及其后继为不可达
    fn mark_unreachable_recursive(
        node_idx: NodeIndex,
        workflow: &Arc<WorkflowGraph>,
        in_degrees: &Arc<Mutex<HashMap<NodeIndex, usize>>>,
        ready_queue: &Arc<Mutex<VecDeque<NodeIndex>>>,
        completed_nodes: &Arc<Mutex<HashSet<NodeIndex>>>,
        unreachable_nodes: &Arc<Mutex<HashSet<NodeIndex>>>,
    ) {
        // 检查是否已经处理过
        if completed_nodes.lock().unwrap().contains(&node_idx) {
            return;
        }

        info!(
            "  -> Node [{}] is unreachable (skipping)",
            workflow.graph[node_idx].id
        );

        // 标记为已完成（虽然没有执行）
        completed_nodes.lock().unwrap().insert(node_idx);
        // 同时标记为不可达
        unreachable_nodes.lock().unwrap().insert(node_idx);

        // 处理所有后继节点
        for edge in workflow.graph.edges(node_idx) {
            let successor_idx = edge.target();

            let mut degrees = in_degrees.lock().unwrap();
            if let Some(degree) = degrees.get_mut(&successor_idx) {
                *degree -= 1;
                let new_degree = *degree;
                drop(degrees);

                if new_degree == 0 {
                    // 检查该后继的所有前驱是否都是不可达的
                    let unreachable = unreachable_nodes.lock().unwrap();
                    let all_preds_unreachable = workflow
                        .graph
                        .edges_directed(successor_idx, Direction::Incoming)
                        .all(|e| unreachable.contains(&e.source()));
                    drop(unreachable);

                    if all_preds_unreachable {
                        // 所有前驱都不可达，继续递归标记
                        Self::mark_unreachable_recursive(
                            successor_idx,
                            workflow,
                            in_degrees,
                            ready_queue,
                            completed_nodes,
                            unreachable_nodes,
                        );
                    } else {
                        // 有前驱是正常执行的，加入队列
                        info!(
                            "Node [{}] is now ready to run.",
                            workflow.graph[successor_idx].id
                        );
                        ready_queue.lock().unwrap().push_back(successor_idx);
                    }
                }
            }
        }
    }

    pub fn execute_graph<'a>(
        self: Arc<Self>,
        workflow: Arc<WorkflowGraph>,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let total_nodes = workflow.graph.node_count();
            if total_nodes == 0 {
                return Ok(());
            }
            let in_degrees: Arc<Mutex<HashMap<NodeIndex, usize>>> = Arc::new(Mutex::new(
                workflow
                    .graph
                    .node_indices()
                    .map(|idx| {
                        (
                            idx,
                            workflow
                                .graph
                                .edges_directed(idx, Direction::Incoming)
                                .count(),
                        )
                    })
                    .collect(),
            ));
            let completed_nodes = Arc::new(Mutex::new(HashSet::new()));
            let ready_queue: Arc<Mutex<VecDeque<NodeIndex>>> = Arc::new(Mutex::new({
                let guard = in_degrees.lock().unwrap();
                guard
                    .iter()
                    .filter(|(_, &degree)| degree == 0)
                    .map(|(&idx, _)| idx)
                    .collect()
            }));

            while completed_nodes.lock().unwrap().len() < total_nodes {
                let mut tasks = vec![];
                let current_batch: Vec<NodeIndex> =
                    { ready_queue.lock().unwrap().drain(..).collect() };
                if current_batch.is_empty() {
                    let completed = completed_nodes.lock().unwrap().len();
                    if completed < total_nodes {
                        // 尝试清理不可达节点
                        info!("Detecting unreachable nodes...");
                        Self::cleanup_unreachable_nodes(
                            &workflow,
                            &in_degrees,
                            &ready_queue,
                            &completed_nodes,
                        );

                        // 检查是否有新的节点加入队列
                        if ready_queue.lock().unwrap().is_empty() {
                            info!("Workflow graph execution finished early/deadlocked. ({} / {} nodes ran)", completed, total_nodes);
                            break;
                        }
                        // 否则继续执行新加入的节点
                        continue;
                    }
                    break;
                }
                info!(
                    "--- Starting execution batch of {} parallel nodes ---",
                    current_batch.len()
                );
                for node_idx in current_batch {
                    let self_clone = self.clone();
                    let workflow_clone = workflow.clone();
                    let context_clone = context.clone();
                    let in_degrees_clone = in_degrees.clone();
                    let ready_queue_clone = ready_queue.clone();
                    let completed_nodes_clone = completed_nodes.clone();

                    tasks.push(tokio::spawn(async move {
                        let node = &workflow_clone.graph[node_idx];
                        let node_result = self_clone
                            .clone()
                            .run_single_node(node_idx, &workflow_clone, &context_clone)
                            .await;
                        let node_succeeded = node_result.is_ok();
                        match node_result {
                            Ok(Some(output)) => {
                                debug!(
                                    "  [Output] Result for [{}]: {}",
                                    node.id,
                                    serde_json::to_string(&output).unwrap_or_default()
                                );
                                info!("  ✅ Success");
                                context_clone
                                    .set(format!("{}.output", node.id), output)
                                    .unwrap();
                            }
                            Ok(None) => {
                                debug!("  [Output] No primary output for [{}].", node.id);
                                info!("  ✅ Success");
                                context_clone
                                    .set(format!("{}.output", node.id), Value::Null)
                                    .unwrap();
                            }
                            Err(e) => {
                                warn!("  [Output] Error for [{}]: {}", node.id, e);
                                error!("  ❌ Failed: {}", e);
                                // Set node-specific error
                                context_clone
                                    .set(format!("{}.error", node.id), json!(e.to_string()))
                                    .unwrap();
                                // Set global $error variable for convenient access in error handlers
                                context_clone
                                    .set("error".to_string(), json!({
                                        "node": node.id,
                                        "message": e.to_string()
                                    }))
                                    .unwrap();
                            }
                        }

                        // 显示上下文变化
                        if self_clone.debug_config.show_context {
                            if let Ok(ctx_val) = context_clone.get_as_value() {
                                info!("📋 [Debug] Context after [{}]: {}", node.id, serde_json::to_string_pretty(&ctx_val).unwrap_or_default());
                            }
                        }

                        completed_nodes_clone.lock().unwrap().insert(node_idx);

                        // Check if this node has a switch route
                        let switch_result: Option<String> = if let Some(switch_route) = workflow_clone.switch_routes.get(&node.id) {
                            // Evaluate the switch subject
                            let subject_value = if switch_route.subject.is_empty() {
                                // No subject means we use the node output
                                context_clone.resolve_path("output").ok().flatten()
                            } else {
                                let clean_subject = switch_route.subject.trim_start_matches('$');
                                context_clone.resolve_path(clean_subject).ok().flatten()
                            };

                            // Convert to string for comparison
                            let subject_str = match &subject_value {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Number(n)) => n.to_string(),
                                Some(Value::Bool(b)) => b.to_string(),
                                Some(v) => v.to_string(),
                                None => String::new(),
                            };

                            info!("  🔀 Switch on '{}' = '{}'", switch_route.subject, subject_str);
                            Some(subject_str)
                        } else {
                            None
                        };

                        let mut switch_matched = false;

                        for edge in workflow_clone.graph.edges(node_idx) {
                            let (edge_info, successor_idx) = (edge.weight(), edge.target());
                            let mut proceed = false;

                            if edge_info.is_error_path {
                                if !node_succeeded {
                                    proceed = true;
                                    info!(
                                        "  -> Taking 'on error' path to [{}]",
                                        workflow_clone.graph[successor_idx].id
                                    );
                                }
                            } else if node_succeeded {
                                // Handle switch case edges
                                if let Some(ref switch_value) = switch_result {
                                    if let Some(ref case_value) = edge_info.switch_case {
                                        // This is a switch case - compare values
                                        if case_value == switch_value && !switch_matched {
                                            proceed = true;
                                            switch_matched = true;
                                            info!(
                                                "  -> Switch case '{}' matched, taking path to [{}]",
                                                case_value,
                                                workflow_clone.graph[successor_idx].id
                                            );
                                        }
                                    } else if edge_info.switch_case.is_none() && workflow_clone.switch_routes.contains_key(&node.id) {
                                        // This is the default case - only take if no other case matched
                                        // We'll handle this after checking all cases
                                    } else if edge_info.condition.is_some() {
                                        // Regular conditional edge
                                        if let Some(condition) = &edge_info.condition {
                                            if self_clone
                                                .evaluate_condition_async(condition, &context_clone)
                                                .await
                                                .unwrap_or(false)
                                            {
                                                proceed = true;
                                                info!(
                                                    "  -> Condition TRUE, taking path to [{}]",
                                                    workflow_clone.graph[successor_idx].id
                                                );
                                            }
                                        }
                                    } else {
                                        // Unconditional edge (shouldn't happen with switch)
                                        proceed = true;
                                        info!(
                                            "  -> Taking unconditional path to [{}]",
                                            workflow_clone.graph[successor_idx].id
                                        );
                                    }
                                } else if let Some(condition) = &edge_info.condition {
                                    if self_clone
                                        .evaluate_condition_async(condition, &context_clone)
                                        .await
                                        .unwrap_or(false)
                                    {
                                        proceed = true;
                                        info!(
                                            "  -> Condition TRUE, taking path to [{}]",
                                            workflow_clone.graph[successor_idx].id
                                        );
                                    } else {
                                        info!(
                                            "  -> Condition FALSE, skipping path to [{}]",
                                            workflow_clone.graph[successor_idx].id
                                        );
                                    }
                                } else {
                                    proceed = true;
                                    info!(
                                        "  -> Taking unconditional path to [{}]",
                                        workflow_clone.graph[successor_idx].id
                                    );
                                }
                            }
                            if proceed {
                                let mut degrees_guard = in_degrees_clone.lock().unwrap();
                                if let Some(degree) = degrees_guard.get_mut(&successor_idx) {
                                    *degree -= 1;
                                    if *degree == 0 {
                                        info!(
                                            "Node [{}] is now ready to run.",
                                            workflow_clone.graph[successor_idx].id
                                        );
                                        ready_queue_clone.lock().unwrap().push_back(successor_idx);
                                    }
                                }
                            }
                        }

                        // Handle default switch case if no case matched
                        if switch_result.is_some() && !switch_matched {
                            for edge in workflow_clone.graph.edges(node_idx) {
                                let (edge_info, successor_idx) = (edge.weight(), edge.target());
                                if edge_info.switch_case.is_none() && !edge_info.is_error_path && edge_info.condition.is_none() {
                                    info!(
                                        "  -> Switch default, taking path to [{}]",
                                        workflow_clone.graph[successor_idx].id
                                    );
                                    let mut degrees_guard = in_degrees_clone.lock().unwrap();
                                    if let Some(degree) = degrees_guard.get_mut(&successor_idx) {
                                        *degree -= 1;
                                        if *degree == 0 {
                                            info!(
                                                "Node [{}] is now ready to run.",
                                                workflow_clone.graph[successor_idx].id
                                            );
                                            ready_queue_clone.lock().unwrap().push_back(successor_idx);
                                        }
                                    }
                                    break; // Only take one default
                                }
                            }
                        }
                    }));
                }
                join_all(tasks).await;
            }
            Ok(())
        })
    }
}
