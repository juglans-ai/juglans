// src/wasm/executor.rs — WASM DAG executor
//
// Simplified version of core/executor.rs for browser execution.
// All tool calls are delegated to a single JS callback via bridge::call_tool_handler.
// No tokio, no builtins, no MCP, no Python — pure DAG scheduling + expression eval.

use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::core::context::WorkflowContext;
use crate::core::expr_eval::{self, ExprEvaluator};
use crate::core::graph::{self, NodeType, WorkflowGraph};
use crate::core::instance_arena::{MethodScope, TypedSlot};
use crate::core::parser::GraphParser;

use super::bridge;

lazy_static! {
    static ref FUNC_CALL_RE: Regex =
        Regex::new(r"(?s)^([a-zA-Z0-9_.]+)\((.*)\)(\.[a-zA-Z0-9_]+)?$").unwrap();
}

/// Context for evaluating outgoing edges from a completed node
struct EdgeEvalContext<'a> {
    executor: &'a WasmExecutor,
    node_idx: NodeIndex,
    node_succeeded: bool,
    switch_result: &'a Option<String>,
    workflow: &'a WorkflowGraph,
    context: &'a WorkflowContext,
    in_degrees: &'a Mutex<HashMap<NodeIndex, usize>>,
    ready_queue: &'a Mutex<VecDeque<NodeIndex>>,
}

pub struct WasmExecutor {
    expr_eval: ExprEvaluator,
    tool_handler: js_sys::Function,
    /// Known tool names (for disambiguating tools vs. expression functions in params)
    tool_names: HashSet<String>,
    max_loop_iterations: usize,
}

impl WasmExecutor {
    pub fn new(tool_handler: js_sys::Function, tool_names: HashSet<String>) -> Self {
        Self {
            expr_eval: ExprEvaluator::new(),
            tool_handler,
            tool_names,
            max_loop_iterations: 100,
        }
    }

    // ================================================================
    // Tool execution — single bridge to JS
    // ================================================================

    async fn execute_tool(
        &self,
        name: &str,
        params: &HashMap<String, String>,
    ) -> Result<Option<Value>> {
        let mut args = serde_json::Map::new();
        for (k, v) in params {
            let val = serde_json::from_str(v).unwrap_or(Value::String(v.clone()));
            args.insert(k.clone(), val);
        }
        let result =
            bridge::call_tool_handler(&self.tool_handler, name, &Value::Object(args)).await?;
        Ok(Some(result))
    }

    // ================================================================
    // Parameter resolution (expr eval + nested tool calls)
    // ================================================================

    fn process_parameter<'a>(
        &'a self,
        param_str: &'a str,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        Box::pin(async move {
            let clean_param = param_str.trim();

            // Check for nested tool calls: tool_name(args).field
            if let Some(caps) = FUNC_CALL_RE.captures(clean_param) {
                let tool_name = &caps[1];
                let is_tool = self.tool_names.contains(tool_name);

                if is_tool {
                    let args_str = &caps[2];
                    let field_access = caps.get(3).map(|m| m.as_str().trim_start_matches('.'));

                    let raw_args = GraphParser::parse_arguments_str(args_str);
                    let mut resolved_args = HashMap::new();

                    for (k, v) in raw_args {
                        let resolved_val = self.process_parameter(&v, context).await?;
                        let val_str = match resolved_val {
                            Value::String(s) => s,
                            Value::Null => "".to_string(),
                            other => other.to_string(),
                        };
                        resolved_args.insert(k, val_str);
                    }

                    let result_val = self.execute_tool(tool_name, &resolved_args).await?;

                    if let Some(field) = field_access {
                        if let Some(obj) = result_val.as_ref().and_then(|v| v.as_object()) {
                            let field_val = obj.get(field).cloned().unwrap_or(Value::Null);
                            return Ok(field_val);
                        }
                    }
                    return Ok(result_val.unwrap_or(Value::Null));
                }
            }

            // Quoted string literal — return directly
            if clean_param.starts_with('"') && clean_param.ends_with('"') {
                let inner = &clean_param[1..clean_param.len() - 1];
                if !inner.contains('"') {
                    return Ok(Value::String(inner.to_string()));
                }
            }

            // Method scope fast path (for class method bodies)
            let method_result = context
                .with_method_scope(|scope| {
                    let ast = self.expr_eval.parse_cached(clean_param).ok()?;
                    let optimized = expr_eval::ExprEvaluator::optimize_for_method(
                        &ast,
                        &scope.class_def,
                        &scope.method_params,
                    );
                    self.expr_eval
                        .eval_method_expr(&optimized, &scope.field_cache, &scope.param_values)
                        .ok()
                })
                .flatten();
            if let Some(slot) = method_result {
                return Ok(slot.to_value());
            }

            // General path: TypedSlot fast path + Value fallback
            let typed_resolver = |path: &str| -> Option<TypedSlot> {
                let clean = path.strip_prefix("ctx.").unwrap_or(path);
                if let Some(slot) = context.resolve_path_typed(clean) {
                    return Some(slot);
                }
                let resolved = context.resolve_path(clean).ok().flatten();
                resolved.map(TypedSlot::from_value)
            };

            match self.expr_eval.eval_typed(clean_param, &typed_resolver) {
                Ok(slot) => Ok(slot.to_value()),
                Err(_) => Ok(json!(clean_param)),
            }
        })
    }

    async fn evaluate_condition(&self, script: &str, context: &WorkflowContext) -> Result<bool> {
        let val = self.process_parameter(script, context).await?;
        Ok(expr_eval::is_truthy(&val))
    }

    // ================================================================
    // Node execution
    // ================================================================

    fn run_single_node<'a>(
        &'a self,
        node_idx: NodeIndex,
        workflow: &'a WorkflowGraph,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>>> + 'a>> {
        Box::pin(async move {
            let node = &workflow.graph[node_idx];

            match &node.node_type {
                NodeType::Literal(val) => Ok(Some(val.clone())),

                NodeType::Task(action) => {
                    // Class instantiation
                    if workflow.classes.contains_key(&action.name) {
                        let class_def = workflow.classes.get(&action.name).unwrap();
                        let mut fields_vec = Vec::with_capacity(class_def.fields.len());
                        for field in &class_def.fields {
                            let value = if let Some(arg_expr) = action.params.get(&field.name) {
                                self.process_parameter(arg_expr, context).await?
                            } else if let Some(default_expr) = &field.default {
                                self.process_parameter(default_expr, context).await?
                            } else {
                                Value::Null
                            };
                            fields_vec.push(value);
                        }
                        let class_def_arc = Arc::clone(class_def);
                        let id = context.alloc_instance(
                            node.id.clone(),
                            action.name.clone(),
                            class_def_arc,
                            fields_vec,
                        )?;
                        return Ok(Some(json!({"__arena_ref__": id.0})));
                    }

                    // Function node call
                    if workflow.functions.contains_key(&action.name) {
                        let mut args = HashMap::new();
                        for (key, val_template) in &action.params {
                            let val = self.process_parameter(val_template, context).await?;
                            args.insert(key.clone(), val);
                        }
                        context.emit_node_start(
                            &node.id,
                            &action.name,
                            &self.params_display(&args),
                        );
                        let result = self
                            .execute_function(action.name.clone(), args, workflow, context)
                            .await;
                        context.emit_node_complete(&node.id, &action.name, &result);
                        return result;
                    }

                    // Regular tool call — resolve params then call JS bridge
                    let mut rendered_params = HashMap::new();
                    for (key, val_template) in &action.params {
                        let processed_val = self.process_parameter(val_template, context).await?;
                        let val_str = match processed_val {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        rendered_params.insert(key.clone(), val_str);
                    }
                    context.emit_node_start(&node.id, &action.name, &rendered_params);
                    let result = self.execute_tool(&action.name, &rendered_params).await;
                    context.emit_node_complete(&node.id, &action.name, &result);
                    result
                }

                NodeType::Assert(expr_str) => {
                    context.emit_node_start(&node.id, "assert", &HashMap::new());
                    let resolver = |path: &str| -> Option<Value> {
                        let clean = path.strip_prefix("ctx.").unwrap_or(path);
                        match clean {
                            "_tools_called" => {
                                let tools: Vec<Value> = context
                                    .trace_entries()
                                    .iter()
                                    .map(|e| json!(e.tool.clone()))
                                    .collect();
                                Some(json!(tools))
                            }
                            "_trace" => {
                                let entries: Vec<Value> = context
                                    .trace_entries()
                                    .iter()
                                    .map(|e| {
                                        json!({
                                            "tool": e.tool,
                                            "params": e.params,
                                            "status": format!("{:?}", e.status),
                                        })
                                    })
                                    .collect();
                                Some(json!(entries))
                            }
                            _ => context.resolve_path(clean).ok().flatten(),
                        }
                    };
                    let result = match self.expr_eval.eval(expr_str, &resolver) {
                        Ok(val) => {
                            if expr_eval::is_truthy(&val) {
                                Ok(Some(json!({"assert": true, "expr": expr_str})))
                            } else {
                                Err(anyhow!("Assertion failed: `{}` → {:?}", expr_str, val))
                            }
                        }
                        Err(e) => Err(anyhow!("Assertion error: `{}` → {}", expr_str, e)),
                    };
                    context.emit_node_complete(&node.id, "assert", &result);
                    result
                }

                NodeType::ReturnErr(err_obj) => {
                    context.emit_node_start(&node.id, "return_err", &HashMap::new());
                    let rendered = if let Value::Object(map) = err_obj {
                        let mut rendered_map = serde_json::Map::new();
                        for (k, v) in map {
                            if let Value::String(s) = v {
                                if s.contains('$') || s.contains('{') {
                                    let resolved = self.process_parameter(s, context).await.ok();
                                    rendered_map.insert(
                                        k.clone(),
                                        resolved.unwrap_or_else(|| Value::String(s.clone())),
                                    );
                                } else {
                                    rendered_map.insert(k.clone(), Value::String(s.clone()));
                                }
                            } else {
                                rendered_map.insert(k.clone(), v.clone());
                            }
                        }
                        Value::Object(rendered_map)
                    } else {
                        err_obj.clone()
                    };
                    let kind = rendered
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("custom");
                    let message = rendered
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("error");
                    let result: Result<Option<Value>> = Err(anyhow!("[{}] {}", kind, message));
                    context.emit_node_complete(&node.id, "return_err", &result);
                    result
                }

                NodeType::AssignCall { var, action } => {
                    let mut rendered_params = HashMap::new();
                    for (key, val_template) in &action.params {
                        let processed_val = self.process_parameter(val_template, context).await?;
                        let val_str = match processed_val {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        rendered_params.insert(key.clone(), val_str);
                    }
                    context.emit_node_start(&node.id, &action.name, &rendered_params);
                    let result = self.execute_tool(&action.name, &rendered_params).await;
                    if let Ok(Some(ref val)) = result {
                        context.set(var.clone(), val.clone())?;
                    }
                    context.emit_node_complete(&node.id, &action.name, &result);
                    result
                }

                NodeType::Foreach {
                    item,
                    list,
                    body,
                    parallel,
                } => {
                    let clean_path = list.strip_prefix("ctx.").unwrap_or(list);
                    let list_val = context
                        .resolve_path(clean_path)?
                        .ok_or_else(|| anyhow!("Foreach list '{}' not found", clean_path))?;
                    let array = list_val
                        .as_array()
                        .ok_or_else(|| anyhow!("Variable '{}' is not an array", list))?;

                    if *parallel {
                        // WASM parallel foreach: sequential execution (single-threaded)
                        // TODO: consider join_all for I/O concurrency
                        let mut outputs = vec![];
                        for (i, val) in array.iter().enumerate() {
                            let ctx_clone = context.fork();
                            ctx_clone.set(item.clone(), val.clone())?;
                            ctx_clone.set("_index".to_string(), json!(i))?;
                            self.execute_graph(body, &ctx_clone).await?;
                            let output = ctx_clone.resolve_path("output")?.unwrap_or(Value::Null);
                            outputs.push(output);
                        }
                        let collected = json!(outputs);
                        context.set("output".to_string(), collected.clone())?;
                        Ok(Some(collected))
                    } else {
                        for (i, val) in array.iter().enumerate() {
                            context.set(item.clone(), val.clone())?;
                            if let Err(e) = self.execute_graph(body, context).await {
                                return Err(anyhow!(
                                    "Error inside foreach body at index {}: {}",
                                    i,
                                    e
                                ));
                            }
                        }
                        Ok(None)
                    }
                }

                NodeType::Loop { condition, body } => {
                    let mut loop_count = 0;
                    loop {
                        if loop_count > self.max_loop_iterations {
                            return Err(anyhow!(
                                "Loop limit exceeded (max: {})",
                                self.max_loop_iterations
                            ));
                        }
                        if !self.evaluate_condition(condition, context).await? {
                            break;
                        }
                        if let Err(e) = self.execute_graph(body, context).await {
                            return Err(anyhow!("Error inside loop body: {}", e));
                        }
                        loop_count += 1;
                    }
                    Ok(None)
                }

                NodeType::_ExternalCall {
                    call_path,
                    args,
                    kwargs,
                } => {
                    // In WASM, external calls go through JS bridge as regular tool calls
                    let mut resolved_kwargs = HashMap::new();
                    for (k, v) in kwargs {
                        let resolved = self.process_parameter(v, context).await?;
                        let val_str = match resolved {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        resolved_kwargs.insert(k.clone(), val_str);
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let resolved = self.process_parameter(arg, context).await?;
                        let val_str = match resolved {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        resolved_kwargs.insert(format!("__arg{}", i), val_str);
                    }
                    self.execute_tool(call_path, &resolved_kwargs).await
                }

                NodeType::NewInstance { class_name, args } => {
                    let class_def = workflow
                        .classes
                        .get(class_name)
                        .ok_or_else(|| anyhow!("Class '{}' not found", class_name))?;

                    let mut fields_vec = Vec::with_capacity(class_def.fields.len());
                    for field in &class_def.fields {
                        let value = if let Some(arg_expr) = args.get(&field.name) {
                            self.process_parameter(arg_expr, context).await?
                        } else if let Some(default_expr) = &field.default {
                            self.process_parameter(default_expr, context).await?
                        } else {
                            return Err(anyhow!(
                                "Class '{}' field '{}' has no default and was not provided",
                                class_name,
                                field.name
                            ));
                        };
                        fields_vec.push(value);
                    }

                    let class_def_arc = Arc::clone(class_def);
                    let id = context.alloc_instance(
                        node.id.clone(),
                        class_name.clone(),
                        class_def_arc,
                        fields_vec,
                    )?;
                    Ok(Some(json!({"__arena_ref__": id.0})))
                }

                NodeType::MethodCall {
                    instance_path,
                    method_name,
                    args,
                } => {
                    let instance_id = context.lookup_instance(instance_path)?;
                    let class_def = context.arena().class_def(instance_id).ok_or_else(|| {
                        anyhow!("Instance '{}' class_def not found", instance_path)
                    })?;
                    let class_name = context.arena().class_name(instance_id).unwrap_or_default();

                    let method_def = class_def.methods.get(method_name).ok_or_else(|| {
                        anyhow!("Class '{}' has no method '{}'", class_name, method_name)
                    })?;

                    let field_cache = context
                        .arena()
                        .snapshot_fields(instance_id)
                        .unwrap_or_default();
                    let scope = MethodScope {
                        instance_id,
                        class_def: Arc::clone(&class_def),
                        instance_path: instance_path.to_string(),
                        dirty: HashMap::new(),
                        field_cache,
                        method_params: method_def.params.clone(),
                        param_values: Vec::new(),
                    };
                    context.push_method_scope(scope)?;

                    let mut param_slots = Vec::with_capacity(method_def.params.len());
                    for param_name in &method_def.params {
                        if let Some(arg_expr) = args.get(param_name) {
                            let val = self.process_parameter(arg_expr, context).await?;
                            param_slots.push(TypedSlot::from_value(val.clone()));
                            context.set(param_name.clone(), val)?;
                        } else {
                            param_slots.push(TypedSlot::Null);
                        }
                    }
                    context.set_method_param_values(param_slots)?;

                    let body_arc = Arc::clone(&method_def.body);
                    self.execute_graph(&body_arc, context).await?;

                    if let Some(scope) = context.pop_method_scope()? {
                        context.flush_dirty_to_arena(&scope);
                    }

                    Ok(Some(json!({"__arena_ref__": instance_id.0})))
                }
            }
        })
    }

    // ================================================================
    // Function execution
    // ================================================================

    fn execute_function<'a>(
        &'a self,
        func_name: String,
        args: HashMap<String, Value>,
        workflow: &'a WorkflowGraph,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Value>>> + 'a>> {
        Box::pin(async move {
            let func_def = workflow
                .functions
                .get(&func_name)
                .ok_or_else(|| anyhow!("Function '{}' not found", func_name))?;

            for param_name in &func_def.params {
                if let Some(val) = args.get(param_name) {
                    context.set(param_name.clone(), val.clone())?;
                }
            }

            let body_arc = Arc::clone(&func_def.body);
            self.execute_graph(&body_arc, context).await?;

            Ok(context.resolve_path("output")?.or(Some(Value::Null)))
        })
    }

    // ================================================================
    // DAG scheduler — main execution loop
    // ================================================================

    pub fn execute_graph<'a>(
        &'a self,
        workflow: &'a WorkflowGraph,
        context: &'a WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move {
            context.set_current_workflow(Arc::new(workflow.clone()));

            if !workflow.classes.is_empty() {
                context.set_class_registry(&workflow.classes);
                self.expr_eval
                    .set_class_registry(Arc::new(workflow.classes.clone()));
            }

            // Exclude test_* nodes
            let test_node_count = workflow
                .graph
                .node_indices()
                .filter(|&idx| graph::is_test_node_id(&workflow.graph[idx].id))
                .count();
            let total_nodes = workflow.graph.node_count() - test_node_count;
            if total_nodes == 0 {
                return Ok(());
            }

            // Compute in-degrees
            let in_degrees: Mutex<HashMap<NodeIndex, usize>> = Mutex::new(
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
            );

            let completed_nodes: Mutex<HashSet<NodeIndex>> = Mutex::new(HashSet::new());

            // Seed ready queue with in-degree 0 (non-test) nodes
            let ready_queue: Mutex<VecDeque<NodeIndex>> = Mutex::new({
                let guard = in_degrees.lock().unwrap();
                guard
                    .iter()
                    .filter(|(idx, &degree)| {
                        degree == 0 && !graph::is_test_node_id(&workflow.graph[**idx].id)
                    })
                    .map(|(&idx, _)| idx)
                    .collect()
            });

            while completed_nodes.lock().unwrap().len() < total_nodes {
                let current_batch: Vec<NodeIndex> = ready_queue.lock().unwrap().drain(..).collect();

                if current_batch.is_empty() {
                    let completed = completed_nodes.lock().unwrap().len();
                    if completed < total_nodes {
                        Self::cleanup_unreachable_nodes(
                            workflow,
                            &in_degrees,
                            &ready_queue,
                            &completed_nodes,
                        );
                        if ready_queue.lock().unwrap().is_empty() {
                            break;
                        }
                        continue;
                    }
                    break;
                }

                // Process batch sequentially (WASM is single-threaded)
                for node_idx in current_batch {
                    let node = &workflow.graph[node_idx];
                    let node_result = self.run_single_node(node_idx, workflow, context).await;
                    let node_succeeded = node_result.is_ok();

                    if !matches!(node.node_type, NodeType::NewInstance { .. }) {
                        Self::store_node_result(&node.id, &node_result, context);
                    }

                    completed_nodes.lock().unwrap().insert(node_idx);

                    let switch_result = Self::resolve_switch_subject(&node.id, workflow, context);

                    self.evaluate_outgoing_edges(EdgeEvalContext {
                        executor: self,
                        node_idx,
                        node_succeeded,
                        switch_result: &switch_result,
                        workflow,
                        context,
                        in_degrees: &in_degrees,
                        ready_queue: &ready_queue,
                    })
                    .await;
                }
            }

            // Check for unhandled errors
            if let Ok(Some(error_val)) = context.resolve_path("error") {
                if !error_val.is_null() {
                    let node = error_val
                        .get("node")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let msg = error_val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    return Err(anyhow!("Node [{}] failed: {}", node, msg));
                }
            }

            Ok(())
        })
    }

    // ================================================================
    // Pure computation helpers (same logic as native executor)
    // ================================================================

    fn store_node_result(node_id: &str, result: &Result<Option<Value>>, context: &WorkflowContext) {
        match result {
            Ok(Some(output)) => {
                context
                    .set(format!("{}.output", node_id), output.clone())
                    .unwrap();
                context.set("output".to_string(), output.clone()).unwrap();
            }
            Ok(None) => {
                context
                    .set(format!("{}.output", node_id), Value::Null)
                    .unwrap();
                context.set("output".to_string(), Value::Null).unwrap();
            }
            Err(e) => {
                let kind = classify_error(e);
                let err_value = json!({
                    "err": {
                        "kind": kind,
                        "message": e.to_string(),
                        "node": node_id,
                    }
                });
                context
                    .set(format!("{}.output", node_id), err_value.clone())
                    .unwrap();
                context.set("output".to_string(), err_value).unwrap();
                context
                    .set(format!("{}.error", node_id), json!(e.to_string()))
                    .unwrap();
                context
                    .set(
                        "error".to_string(),
                        json!({
                            "node": node_id,
                            "message": e.to_string()
                        }),
                    )
                    .unwrap();
            }
        }
    }

    fn resolve_switch_subject(
        node_id: &str,
        workflow: &WorkflowGraph,
        context: &WorkflowContext,
    ) -> Option<String> {
        let switch_route = workflow.switch_routes.get(node_id)?;

        let subject_value = if switch_route.subject.is_empty() {
            context.resolve_path("output").ok().flatten()
        } else {
            let clean_subject = switch_route.subject.trim_start_matches('$');
            let clean_subject = clean_subject.strip_prefix("ctx.").unwrap_or(clean_subject);
            context.resolve_path(clean_subject).ok().flatten()
        };

        let subject_str = match &subject_value {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Null) => String::new(),
            Some(v) => v.to_string(),
            None => String::new(),
        };

        Some(subject_str)
    }

    fn activate_successor(
        successor_idx: NodeIndex,
        in_degrees: &Mutex<HashMap<NodeIndex, usize>>,
        ready_queue: &Mutex<VecDeque<NodeIndex>>,
    ) {
        let mut degrees_guard = in_degrees.lock().unwrap();
        if let Some(degree) = degrees_guard.get_mut(&successor_idx) {
            *degree -= 1;
            if *degree == 0 {
                ready_queue.lock().unwrap().push_back(successor_idx);
            }
        }
    }

    fn cleanup_unreachable_nodes(
        workflow: &WorkflowGraph,
        in_degrees: &Mutex<HashMap<NodeIndex, usize>>,
        ready_queue: &Mutex<VecDeque<NodeIndex>>,
        completed_nodes: &Mutex<HashSet<NodeIndex>>,
    ) {
        let unreachable_nodes: Mutex<HashSet<NodeIndex>> = Mutex::new(HashSet::new());
        let completed = completed_nodes.lock().unwrap().clone();
        let degrees = in_degrees.lock().unwrap();

        let unreachable: Vec<NodeIndex> = degrees
            .iter()
            .filter(|(idx, &degree)| {
                if degree == 0 || completed.contains(idx) {
                    return false;
                }
                if graph::is_test_node_id(&workflow.graph[**idx].id) {
                    return false;
                }
                workflow
                    .graph
                    .edges_directed(**idx, Direction::Incoming)
                    .all(|e| completed.contains(&e.source()))
            })
            .map(|(idx, _)| *idx)
            .collect();

        drop(degrees);
        drop(completed);

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

    fn mark_unreachable_recursive(
        node_idx: NodeIndex,
        workflow: &WorkflowGraph,
        in_degrees: &Mutex<HashMap<NodeIndex, usize>>,
        ready_queue: &Mutex<VecDeque<NodeIndex>>,
        completed_nodes: &Mutex<HashSet<NodeIndex>>,
        unreachable_nodes: &Mutex<HashSet<NodeIndex>>,
    ) {
        if completed_nodes.lock().unwrap().contains(&node_idx) {
            return;
        }

        completed_nodes.lock().unwrap().insert(node_idx);
        unreachable_nodes.lock().unwrap().insert(node_idx);

        for edge in workflow.graph.edges(node_idx) {
            let successor_idx = edge.target();

            let mut degrees = in_degrees.lock().unwrap();
            if let Some(degree) = degrees.get_mut(&successor_idx) {
                *degree -= 1;
                let new_degree = *degree;
                drop(degrees);

                if new_degree == 0 {
                    let unreachable = unreachable_nodes.lock().unwrap();
                    let all_preds_unreachable = workflow
                        .graph
                        .edges_directed(successor_idx, Direction::Incoming)
                        .all(|e| unreachable.contains(&e.source()));
                    drop(unreachable);

                    if all_preds_unreachable {
                        Self::mark_unreachable_recursive(
                            successor_idx,
                            workflow,
                            in_degrees,
                            ready_queue,
                            completed_nodes,
                            unreachable_nodes,
                        );
                    } else {
                        ready_queue.lock().unwrap().push_back(successor_idx);
                    }
                }
            }
        }
    }

    // ================================================================
    // Edge evaluation
    // ================================================================

    async fn evaluate_outgoing_edges(&self, edge_ctx: EdgeEvalContext<'_>) {
        let EdgeEvalContext {
            executor,
            node_idx,
            node_succeeded,
            switch_result,
            workflow,
            context,
            in_degrees,
            ready_queue,
        } = edge_ctx;
        let node_id = &workflow.graph[node_idx].id;
        let mut switch_matched = false;

        // Check for Result switch route (ok/err cases)
        let is_result_switch = workflow
            .switch_routes
            .get(node_id)
            .map(|r| r.cases.iter().any(|c| c.is_ok || c.is_err))
            .unwrap_or(false);

        let error_kind = if !node_succeeded {
            context.resolve_path("output").ok().flatten().and_then(|v| {
                v.get("err")
                    .and_then(|e| e.get("kind"))
                    .and_then(|k| k.as_str())
                    .map(|s| s.to_string())
            })
        } else {
            None
        };

        // Result switch: two-pass matching
        if is_result_switch {
            // Pass 1: ok + specific err "kind"
            for edge in workflow.graph.edges(node_idx) {
                let (edge_info, successor_idx) = (edge.weight(), edge.target());
                if let Some(ref case_value) = edge_info.switch_case {
                    if case_value == "__ok__" && node_succeeded && !switch_matched {
                        switch_matched = true;
                        Self::activate_successor(successor_idx, in_degrees, ready_queue);
                    } else if case_value.starts_with("__err_")
                        && case_value.ends_with("__")
                        && case_value != "__err__"
                        && !node_succeeded
                        && !switch_matched
                    {
                        let case_kind = &case_value[6..case_value.len() - 2];
                        if error_kind.as_deref() == Some(case_kind) {
                            switch_matched = true;
                            Self::activate_successor(successor_idx, in_degrees, ready_queue);
                        }
                    }
                }
            }
            // Pass 2: catch-all err
            if !switch_matched && !node_succeeded {
                for edge in workflow.graph.edges(node_idx) {
                    let (edge_info, successor_idx) = (edge.weight(), edge.target());
                    if let Some(ref case_value) = edge_info.switch_case {
                        if case_value == "__err__" && !switch_matched {
                            switch_matched = true;
                            Self::activate_successor(successor_idx, in_degrees, ready_queue);
                        }
                    }
                }
            }
            if switch_matched && !node_succeeded {
                let _ = context.set("error".to_string(), Value::Null);
            }
        }

        // General edge evaluation
        for edge in workflow.graph.edges(node_idx) {
            let (edge_info, successor_idx) = (edge.weight(), edge.target());
            let mut proceed = false;

            if is_result_switch {
                // Already handled above
            } else if edge_info.is_error_path {
                if !node_succeeded {
                    proceed = true;
                }
            } else if node_succeeded {
                if let Some(ref switch_value) = switch_result {
                    if let Some(ref case_value) = edge_info.switch_case {
                        if case_value == switch_value && !switch_matched {
                            proceed = true;
                            switch_matched = true;
                        }
                    } else if edge_info.switch_case.is_none()
                        && workflow.switch_routes.contains_key(node_id)
                    {
                        // Default case — handled below
                    } else if let Some(condition) = &edge_info.condition {
                        if executor
                            .evaluate_condition(condition, context)
                            .await
                            .unwrap_or(false)
                        {
                            proceed = true;
                        }
                    } else {
                        proceed = true;
                    }
                } else if let Some(condition) = &edge_info.condition {
                    if executor
                        .evaluate_condition(condition, context)
                        .await
                        .unwrap_or(false)
                    {
                        proceed = true;
                    }
                } else {
                    proceed = true;
                }
            }

            if proceed {
                Self::activate_successor(successor_idx, in_degrees, ready_queue);
            }
        }

        // Handle default switch case
        let has_switch = switch_result.is_some() || is_result_switch;
        if has_switch && !switch_matched {
            for edge in workflow.graph.edges(node_idx) {
                let (edge_info, successor_idx) = (edge.weight(), edge.target());
                if edge_info.switch_case.is_none()
                    && !edge_info.is_error_path
                    && edge_info.condition.is_none()
                {
                    Self::activate_successor(successor_idx, in_degrees, ready_queue);
                    break;
                }
            }
        }
    }

    // ================================================================
    // Utility
    // ================================================================

    fn params_display(&self, args: &HashMap<String, Value>) -> HashMap<String, String> {
        args.iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                )
            })
            .collect()
    }
}

/// Classify an anyhow error into a kind string for structured error output.
fn classify_error(e: &anyhow::Error) -> String {
    let msg = e.to_string();

    if msg.starts_with('[') {
        if let Some(end) = msg.find(']') {
            return msg[1..end].to_string();
        }
    }

    let lower = msg.to_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") {
        "timeout".to_string()
    } else if lower.contains("connection refused") || lower.contains("network") {
        "network".to_string()
    } else if lower.contains("not found") {
        "not_found".to_string()
    } else if lower.contains("parse") || lower.contains("invalid json") {
        "parse".to_string()
    } else if lower.contains("assertion") || lower.contains("assert") {
        "assertion".to_string()
    } else {
        "runtime".to_string()
    }
}
