// src/core/parser.rs
use crate::core::graph::{
    Action, ClassDef, ClassField, Edge, FunctionDef, Node, NodeType, SwitchCase, SwitchRoute,
    WorkflowGraph,
};
use crate::core::jwl_lexer::Lexer;
use crate::core::jwl_parser::JwlParser;
use anyhow::{anyhow, Result};
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Parser)]
#[grammar = "core/jwl.pest"]
struct JwlGrammar;

fn use_pest_parser() -> bool {
    std::env::var("JUGLANS_PARSER").as_deref() == Ok("pest")
}

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
        if !use_pest_parser() {
            return Self::parse_rdp(content);
        }
        Self::parse_pest(content)
    }

    /// 解析 .jgflow 清单文件 — 只提取 metadata，不允许节点/边/类定义
    pub fn parse_manifest(content: &str) -> Result<WorkflowGraph> {
        if !use_pest_parser() {
            return Self::parse_manifest_rdp(content);
        }
        Self::parse_manifest_pest(content)
    }

    /// 解析库文件 — 允许只包含 function 定义，无需 entry 节点或常规节点
    pub fn parse_lib(content: &str) -> Result<WorkflowGraph> {
        if !use_pest_parser() {
            return Self::parse_lib_rdp(content);
        }
        Self::parse_lib_pest(content)
    }

    // ==================== RDP implementations ====================

    fn parse_rdp(content: &str) -> Result<WorkflowGraph> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("JWL Compilation Syntax Error:\n{}", e))?;
        let mut parser = JwlParser::new(&tokens, content);
        let mut wf = parser.parse_workflow()?;

        if wf.entry_node.is_empty() {
            if let Some(first_idx) = wf.graph.node_indices().next() {
                wf.entry_node = wf.graph[first_idx].id.clone();
            } else {
                return Err(anyhow!("Architecture Error: Workflow must define an 'entry' node or contain at least one valid node."));
            }
        }
        Ok(wf)
    }

    fn parse_manifest_rdp(content: &str) -> Result<WorkflowGraph> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("Manifest Syntax Error:\n{}", e))?;
        let mut parser = JwlParser::new(&tokens, content);
        parser.parse_manifest()
    }

    fn parse_lib_rdp(content: &str) -> Result<WorkflowGraph> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("JWL Compilation Syntax Error:\n{}", e))?;
        let mut parser = JwlParser::new(&tokens, content);
        let wf = parser.parse_workflow()?;

        if wf.entry_node.is_empty()
            && wf.graph.node_indices().next().is_none()
            && wf.functions.is_empty()
        {
            return Err(anyhow!(
                "Library Error: Library file must define at least one function node."
            ));
        }
        Ok(wf)
    }

    // ==================== Pest implementations (legacy) ====================

    fn parse_pest(content: &str) -> Result<WorkflowGraph> {
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

    fn parse_manifest_pest(content: &str) -> Result<WorkflowGraph> {
        let mut pairs = JwlGrammar::parse(Rule::manifest, content)
            .map_err(|e| anyhow!("Manifest Syntax Error:\n{}", e))?;

        let manifest_pair = pairs
            .next()
            .ok_or_else(|| anyhow!("Manifest Error: The input is empty."))?;

        let mut workflow_instance = WorkflowGraph::default();

        for inner in manifest_pair.into_inner() {
            if inner.as_rule() == Rule::metadata {
                Self::parse_metadata(inner, &mut workflow_instance)?;
            }
        }

        Ok(workflow_instance)
    }

    fn parse_lib_pest(content: &str) -> Result<WorkflowGraph> {
        let mut pairs = JwlGrammar::parse(Rule::workflow, content)
            .map_err(|e| anyhow!("JWL Compilation Syntax Error:\n{}", e))?;

        let workflow_pair = pairs
            .next()
            .ok_or_else(|| anyhow!("Compilation Error: The input workflow source is empty."))?;

        let mut workflow_instance = WorkflowGraph::default();

        if workflow_pair.as_rule() == Rule::workflow {
            Self::parse_block(workflow_pair, &mut workflow_instance)?;
        }

        if workflow_instance.entry_node.is_empty()
            && workflow_instance.graph.node_indices().next().is_none()
            && workflow_instance.functions.is_empty()
        {
            return Err(anyhow!(
                "Library Error: Library file must define at least one function node."
            ));
        }

        Ok(workflow_instance)
    }

    fn parse_block(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        for inner in pair.into_inner() {
            match inner.as_rule() {
                Rule::metadata => Self::parse_metadata(inner, workflow)?,
                Rule::class_def => Self::parse_class_def(inner, workflow)?,
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
            "source" => workflow.source = Self::parse_text_value_raw(val_node),
            "author" => workflow.author = Self::parse_text_value_raw(val_node),
            "description" => workflow.description = Self::parse_text_value_raw(val_node),
            "is_public" => {
                workflow.is_public = Some(Self::parse_text_value_raw(val_node) == "true");
            }
            "schedule" => {
                workflow.schedule = Some(Self::parse_text_value_raw(val_node));
            }
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
            "libs" => {
                if val_node.as_rule() == Rule::meta_val_map {
                    // 对象形式: libs: { db: "./libs/sqlite.jg" }
                    // 显式命名空间 → lib_imports
                    for pair in val_node.into_inner() {
                        if pair.as_rule() == Rule::meta_map_pair {
                            let mut it = pair.into_inner();
                            let namespace = it.next().unwrap().as_str().to_string();
                            let path = it.next().unwrap().as_str().trim_matches('"').to_string();
                            workflow.lib_imports.insert(namespace, path);
                        }
                    }
                } else {
                    // 列表形式: libs: ["./libs/sqlite.jg"]
                    // 自动命名空间（stem），resolver 可能用 slug 覆盖
                    let string_vec = Self::parse_string_list_helper(val_node)?;
                    for path in &string_vec {
                        let stem = std::path::Path::new(path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(path)
                            .to_string();
                        workflow.lib_imports.insert(stem.clone(), path.clone());
                        workflow.lib_auto_namespaces.insert(stem);
                    }
                    // 保留旧字段向后兼容（extend 以支持多个 libs: 声明合并）
                    workflow.libs.extend(string_vec);
                }
            }
            "entry" | "exit" | "prompts" | "agents" | "tools" | "python" => {
                let string_vec = Self::parse_string_list_helper(val_node)?;
                match key_str {
                    "entry" => {
                        workflow.entry_node = string_vec.first().cloned().unwrap_or_default()
                    }
                    "exit" => workflow.exit_nodes = string_vec,
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
            Rule::string => {
                let s = pair.as_str();
                if s.starts_with("\"\"\"") {
                    s[3..s.len() - 3].to_string()
                } else {
                    s.trim_matches('"').to_string()
                }
            }
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

    /// 解析 task_def 内部结构为 Action
    fn parse_task_action(task_pair: Pair<Rule>, context_id: &str) -> Result<Action> {
        let mut task_inner = task_pair.into_inner();
        let tool_name = task_inner
            .next()
            .ok_or_else(|| anyhow!("Task Error in [{}]: Missing tool name.", context_id))?
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
                    pk,
                    context_id
                ));
            }
            param_map.insert(pk, pv);
        }
        Ok(Action {
            name: tool_name,
            params: param_map,
        })
    }

    fn parse_node(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut inner_parts = pair.into_inner();

        // 第一个元素：identifier（节点名）
        let id_node = inner_parts
            .next()
            .ok_or_else(|| anyhow!("Node Syntax Error: Node is missing an ID."))?;
        let node_id_str = id_node.as_str().to_string();

        // 第二个元素：func_params 或 content
        let next = inner_parts.next().ok_or_else(|| {
            anyhow!(
                "Node Content Error: Node '{}' has no executable body.",
                node_id_str
            )
        })?;

        // 检查是否有函数参数
        let (func_params, content_node) = if next.as_rule() == Rule::func_params {
            let params: Vec<String> = next
                .into_inner()
                .filter(|p| p.as_rule() == Rule::identifier)
                .map(|p| p.as_str().to_string())
                .collect();
            let content = inner_parts.next().ok_or_else(|| {
                anyhow!(
                    "Node Content Error: Function '{}' has no body.",
                    node_id_str
                )
            })?;
            (Some(params), content)
        } else {
            (None, next)
        };

        // 如果有函数参数 → 存为 FunctionDef，不加入主 DAG
        if let Some(params) = func_params {
            return Self::parse_function_def(node_id_str, params, content_node, workflow);
        }

        // [name]: { func_body } without params — expand compound block inline into DAG
        // Parameterized functions [name(a,b)]: { ... } are already handled above.
        if content_node.as_rule() == Rule::func_body {
            return Self::expand_test_block(node_id_str, content_node, workflow);
        }

        // [name]: { struct_body } — 纯字段声明 → 存为 ClassDef（不进 DAG）
        if content_node.as_rule() == Rule::struct_body {
            let mut fields = Vec::new();
            for field_pair in content_node.into_inner() {
                if field_pair.as_rule() == Rule::struct_field {
                    let mut parts = field_pair.into_inner();
                    let name = parts.next().unwrap().as_str().to_string();
                    let type_hint = parts.next().unwrap().as_str().to_string();
                    let default = parts.next().map(|v| v.as_str().trim().to_string());
                    fields.push(ClassField {
                        name,
                        type_hint: Some(type_hint),
                        default,
                    });
                }
            }
            workflow.classes.insert(
                node_id_str,
                ClassDef {
                    fields,
                    methods: HashMap::new(),
                },
            );
            return Ok(());
        }

        // 常规节点（现有逻辑）
        let node_type_res = match content_node.as_rule() {
            Rule::new_expr => {
                let mut ne_inner = content_node.into_inner();
                let class_name = ne_inner.next().unwrap().as_str().to_string();
                let mut args = HashMap::new();
                for p_pair in ne_inner {
                    if p_pair.as_rule() == Rule::param_pair {
                        let mut p_it = p_pair.into_inner();
                        let pk = p_it.next().unwrap().as_str().to_string();
                        let pv = p_it.next().unwrap().as_str().trim().to_string();
                        args.insert(pk, pv);
                    }
                }
                NodeType::NewInstance { class_name, args }
            }
            Rule::method_call_node => {
                let mut mc_inner = content_node.into_inner();
                let var_ref = mc_inner.next().unwrap().as_str();
                // Strip "$" and split into instance_path + method_name
                let clean = var_ref.trim_start_matches('$');
                let (instance_path, method_name) = clean.rsplit_once('.').ok_or_else(|| {
                    anyhow!(
                        "Invalid method call '{}' in node [{}]: expected $instance.method",
                        var_ref,
                        node_id_str
                    )
                })?;
                let mut args = HashMap::new();
                for p_pair in mc_inner {
                    if p_pair.as_rule() == Rule::param_pair {
                        let mut p_it = p_pair.into_inner();
                        let pk = p_it.next().unwrap().as_str().to_string();
                        let pv = p_it.next().unwrap().as_str().trim().to_string();
                        args.insert(pk, pv);
                    }
                }
                NodeType::MethodCall {
                    instance_path: instance_path.to_string(),
                    method_name: method_name.to_string(),
                    args,
                }
            }
            Rule::struct_init => {
                // struct 花括号实例化：UserResponse { id = "u_001", name = "Alice" }
                let mut inner = content_node.into_inner();
                let struct_name = inner.next().unwrap().as_str().to_string();
                let mut args = HashMap::new();
                if let Some(fields_pair) = inner.next() {
                    // struct_init_fields → assignment*
                    for assignment in fields_pair.into_inner() {
                        if assignment.as_rule() == Rule::assignment {
                            let mut parts = assignment.into_inner();
                            let key = parts.next().unwrap().as_str().to_string();
                            let value = parts.next().unwrap().as_str().trim().to_string();
                            args.insert(key, value);
                        }
                    }
                }
                NodeType::NewInstance {
                    class_name: struct_name,
                    args,
                }
            }
            Rule::task_def => NodeType::Task(Self::parse_task_action(content_node, &node_id_str)?),
            Rule::assignment_block => {
                // Desugar: count = 0, name = input.user → set_context(count=0, name=input.user)
                let mut params = HashMap::new();
                for assignment in content_node.into_inner() {
                    let mut parts = assignment.into_inner();
                    let key = parts.next().unwrap().as_str().to_string();
                    let value = parts.next().unwrap().as_str().trim().to_string();
                    params.insert(key, value);
                }
                NodeType::Task(Action {
                    name: "set_context".to_string(),
                    params,
                })
            }
            Rule::return_err => {
                // return err { kind: "...", message: "..." }
                let json_pair = content_node.into_inner().next().unwrap();
                let raw = json_pair.as_str();
                let val: Value =
                    serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string()));
                NodeType::ReturnErr(val)
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
                // Check for optional "parallel" mode
                let mut parallel = false;
                let mut next = f_it.next().unwrap();
                if next.as_rule() == Rule::foreach_mode {
                    parallel = true;
                    next = f_it.next().unwrap();
                }
                let item_v = next.as_str().trim_start_matches('$').to_string();
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
                    parallel,
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

    /// 解析函数/方法体为 FunctionDef（共用逻辑）
    fn parse_function_body(
        name: &str,
        params: Vec<String>,
        content: Pair<Rule>,
    ) -> Result<FunctionDef> {
        let mut body = WorkflowGraph::default();

        match content.as_rule() {
            Rule::func_body => {
                // 多步函数：展开为 _0, _1, ... 串联子图
                let mut step_index = 0;
                let mut last_idx: Option<petgraph::graph::NodeIndex> = None;

                for step in content.into_inner() {
                    if step.as_rule() == Rule::func_step {
                        let inner_pair = step.into_inner().next().unwrap();
                        let step_id = format!("__{}", step_index);
                        let node_type = match inner_pair.as_rule() {
                            Rule::assert_stmt => {
                                let expr_str = inner_pair
                                    .into_inner()
                                    .next()
                                    .unwrap()
                                    .as_str()
                                    .trim()
                                    .to_string();
                                NodeType::Assert(expr_str)
                            }
                            Rule::return_err => {
                                let json_pair = inner_pair.into_inner().next().unwrap();
                                let raw = json_pair.as_str();
                                let val: Value = serde_json::from_str(raw)
                                    .unwrap_or(Value::String(raw.to_string()));
                                NodeType::ReturnErr(val)
                            }
                            Rule::assign_call => {
                                let mut parts = inner_pair.into_inner();
                                let var_name = parts.next().unwrap().as_str().to_string();
                                let task_pair = parts.next().unwrap();
                                let action = Self::parse_task_action(
                                    task_pair,
                                    &format!("{}.__{}", name, step_index),
                                )?;
                                NodeType::AssignCall {
                                    var: var_name,
                                    action,
                                }
                            }
                            Rule::assignment_block => {
                                let mut params = HashMap::new();
                                for assignment in inner_pair.into_inner() {
                                    let mut parts = assignment.into_inner();
                                    let key = parts.next().unwrap().as_str().to_string();
                                    let value = parts.next().unwrap().as_str().trim().to_string();
                                    params.insert(key, value);
                                }
                                NodeType::Task(Action {
                                    name: "set_context".to_string(),
                                    params,
                                })
                            }
                            _ => {
                                let action = Self::parse_task_action(
                                    inner_pair,
                                    &format!("{}.__{}", name, step_index),
                                )?;
                                NodeType::Task(action)
                            }
                        };

                        let node = Node {
                            id: step_id.clone(),
                            node_type,
                        };
                        let idx = body.graph.add_node(node);
                        body.node_map.insert(step_id.clone(), idx);

                        if let Some(prev_idx) = last_idx {
                            body.graph.add_edge(prev_idx, idx, Edge::default());
                        } else {
                            body.entry_node = step_id.clone();
                        }

                        last_idx = Some(idx);
                        step_index += 1;
                    }
                }
            }
            Rule::task_def => {
                // 单步函数：包装为单节点子图
                let step_id = "__0".to_string();
                let action = Self::parse_task_action(content, name)?;

                let node = Node {
                    id: step_id.clone(),
                    node_type: NodeType::Task(action),
                };
                let idx = body.graph.add_node(node);
                body.node_map.insert(step_id.clone(), idx);
                body.entry_node = step_id;
            }
            Rule::assignment_block => {
                // 单步赋值：解糖为 set_context
                let step_id = "__0".to_string();
                let mut params = HashMap::new();
                for assignment in content.into_inner() {
                    let mut parts = assignment.into_inner();
                    let key = parts.next().unwrap().as_str().to_string();
                    let value = parts.next().unwrap().as_str().trim().to_string();
                    params.insert(key, value);
                }
                let node = Node {
                    id: step_id.clone(),
                    node_type: NodeType::Task(Action {
                        name: "set_context".to_string(),
                        params,
                    }),
                };
                let idx = body.graph.add_node(node);
                body.node_map.insert(step_id.clone(), idx);
                body.entry_node = "__0".to_string();
            }
            _ => {
                return Err(anyhow!(
                    "Function '{}' body must be a task call, assignment, or a {{ ... }} block",
                    name
                ));
            }
        }

        Ok(FunctionDef {
            params,
            body: Box::new(body),
        })
    }

    /// 解析函数节点定义，存入 workflow.functions
    fn parse_function_def(
        name: String,
        params: Vec<String>,
        content: Pair<Rule>,
        workflow: &mut WorkflowGraph,
    ) -> Result<()> {
        let func_def = Self::parse_function_body(&name, params, content)?;
        workflow.functions.insert(name, func_def);
        Ok(())
    }

    /// Expand a test_ block into chained DAG nodes in the main graph.
    /// [test_foo]: { step1; step2; step3 }
    /// → nodes: test_foo (step1), test_foo.__1 (step2), test_foo.__2 (step3)
    /// → edges: test_foo → test_foo.__1 → test_foo.__2
    fn expand_test_block(
        root_id: String,
        content: Pair<Rule>,
        workflow: &mut WorkflowGraph,
    ) -> Result<()> {
        let mut step_index = 0;
        let mut last_idx: Option<petgraph::graph::NodeIndex> = None;

        for step in content.into_inner() {
            if step.as_rule() != Rule::func_step {
                continue;
            }
            let inner_pair = step.into_inner().next().unwrap();
            let step_id = if step_index == 0 {
                root_id.clone()
            } else {
                format!("{}.__{}", root_id, step_index)
            };

            let node_type = match inner_pair.as_rule() {
                Rule::assert_stmt => {
                    let expr_str = inner_pair
                        .into_inner()
                        .next()
                        .unwrap()
                        .as_str()
                        .trim()
                        .to_string();
                    NodeType::Assert(expr_str)
                }
                Rule::return_err => {
                    let json_pair = inner_pair.into_inner().next().unwrap();
                    let raw = json_pair.as_str();
                    let val: Value =
                        serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string()));
                    NodeType::ReturnErr(val)
                }
                Rule::assign_call => {
                    let mut parts = inner_pair.into_inner();
                    let var_name = parts.next().unwrap().as_str().to_string();
                    let task_pair = parts.next().unwrap();
                    let action = Self::parse_task_action(
                        task_pair,
                        &format!("{}.__{}", root_id, step_index),
                    )?;
                    NodeType::AssignCall {
                        var: var_name,
                        action,
                    }
                }
                Rule::assignment_block => {
                    let mut params = HashMap::new();
                    for assignment in inner_pair.into_inner() {
                        let mut parts = assignment.into_inner();
                        let key = parts.next().unwrap().as_str().to_string();
                        let value = parts.next().unwrap().as_str().trim().to_string();
                        params.insert(key, value);
                    }
                    NodeType::Task(Action {
                        name: "set_context".to_string(),
                        params,
                    })
                }
                _ => {
                    let action = Self::parse_task_action(
                        inner_pair,
                        &format!("{}.__{}", root_id, step_index),
                    )?;
                    NodeType::Task(action)
                }
            };

            let node = Node {
                id: step_id.clone(),
                node_type,
            };
            let idx = workflow.graph.add_node(node);
            workflow.node_map.insert(step_id, idx);

            if let Some(prev_idx) = last_idx {
                workflow.graph.add_edge(prev_idx, idx, Edge::default());
            }

            last_idx = Some(idx);
            step_index += 1;
        }

        Ok(())
    }

    /// 解析 class 定义：class ClassName(field1=default1, field2) { [method]: body }
    fn parse_class_def(pair: Pair<Rule>, workflow: &mut WorkflowGraph) -> Result<()> {
        let mut inner = pair.into_inner();

        let class_name = inner
            .next()
            .ok_or_else(|| anyhow!("Class definition missing name"))?
            .as_str()
            .to_string();

        let next = inner
            .next()
            .ok_or_else(|| anyhow!("Class '{}' has no body", class_name))?;

        // Optional: class_params
        let (fields, body_pair) = if next.as_rule() == Rule::class_params {
            let fields = Self::parse_class_fields(next)?;
            let body = inner
                .next()
                .ok_or_else(|| anyhow!("Class '{}' has no body", class_name))?;
            (fields, body)
        } else {
            (Vec::new(), next)
        };

        // Parse class body (field declarations + methods)
        let mut body_fields = Vec::new();
        let mut methods = HashMap::new();
        for method_pair in body_pair.into_inner() {
            if method_pair.as_rule() == Rule::class_field_decl {
                let mut parts = method_pair.into_inner();
                let name = parts.next().unwrap().as_str().to_string();
                let type_hint = parts.next().unwrap().as_str().to_string();
                let default = parts.next().map(|v| v.as_str().trim().to_string());
                body_fields.push(ClassField {
                    name,
                    type_hint: Some(type_hint),
                    default,
                });
            } else if method_pair.as_rule() == Rule::class_method {
                let mut m_inner = method_pair.into_inner();
                let method_name = m_inner.next().unwrap().as_str().to_string();

                let m_next = m_inner.next().ok_or_else(|| {
                    anyhow!(
                        "Method '{}' in class '{}' has no body",
                        method_name,
                        class_name
                    )
                })?;

                let (params, content) = if m_next.as_rule() == Rule::func_params {
                    let params: Vec<String> = m_next
                        .into_inner()
                        .filter(|p| p.as_rule() == Rule::identifier)
                        .map(|p| p.as_str().to_string())
                        .collect();
                    let content = m_inner.next().ok_or_else(|| {
                        anyhow!(
                            "Method '{}' in class '{}' has no body",
                            method_name,
                            class_name
                        )
                    })?;
                    (params, content)
                } else {
                    (Vec::new(), m_next)
                };

                let full_name = format!("{}.{}", class_name, method_name);
                let func_def = Self::parse_function_body(&full_name, params, content)?;
                methods.insert(method_name, func_def);
            }
        }

        // Merge body field declarations after constructor params
        let mut all_fields = fields;
        all_fields.extend(body_fields);

        workflow.classes.insert(
            class_name,
            ClassDef {
                fields: all_fields,
                methods,
            },
        );
        Ok(())
    }

    fn parse_class_fields(pair: Pair<Rule>) -> Result<Vec<ClassField>> {
        let mut fields = Vec::new();
        for field_pair in pair.into_inner() {
            if field_pair.as_rule() == Rule::class_field {
                let mut parts = field_pair.into_inner();
                let name = parts.next().unwrap().as_str().to_string();
                let default = parts.next().map(|v| v.as_str().trim().to_string());
                fields.push(ClassField {
                    name,
                    type_hint: None,
                    default,
                });
            }
        }
        Ok(fields)
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
        let from_node = it
            .next()
            .ok_or_else(|| anyhow!("Switch edge missing source node"))?;
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

                            let mut is_ok = false;
                            let mut is_err = false;
                            let mut err_kind: Option<String> = None;

                            let case_value = match case_value_or_default.as_rule() {
                                Rule::switch_default => None,
                                Rule::switch_ok => {
                                    is_ok = true;
                                    Some("__ok__".to_string())
                                }
                                Rule::switch_err => {
                                    is_err = true;
                                    // Check for optional err kind: err "timeout"
                                    let kind_str =
                                        case_value_or_default.into_inner().next().map(|s| {
                                            let raw = s.as_str().trim();
                                            if raw.starts_with('"') && raw.ends_with('"') {
                                                raw[1..raw.len() - 1].to_string()
                                            } else {
                                                raw.to_string()
                                            }
                                        });
                                    err_kind = kind_str.clone();
                                    Some(
                                        kind_str
                                            .map(|k| format!("__err_{}__", k))
                                            .unwrap_or_else(|| "__err__".to_string()),
                                    )
                                }
                                _ => {
                                    // It's a switch_case_value (string, number, boolean, or variable_ref)
                                    let val_str = case_value_or_default.as_str().trim();
                                    let clean_val =
                                        if val_str.starts_with('"') && val_str.ends_with('"') {
                                            val_str[1..val_str.len() - 1].to_string()
                                        } else {
                                            val_str.to_string()
                                        };
                                    Some(clean_val)
                                }
                            };

                            let target_node = case_it.next().ok_or_else(|| {
                                anyhow!(
                                    "Switch case missing target node for value: {:?}",
                                    case_value
                                )
                            })?;
                            let target_id = target_node
                                .into_inner()
                                .next()
                                .ok_or_else(|| anyhow!("Invalid target node in switch case"))?
                                .as_str()
                                .to_string();

                            cases.push(SwitchCase {
                                value: case_value.clone(),
                                target: target_id.clone(),
                                is_ok,
                                is_err,
                                err_kind,
                            });

                            // Create edge with switch_case marker
                            let edge = Edge {
                                condition: None,
                                is_error_path: is_err,
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
        workflow
            .switch_routes
            .insert(from_id.clone(), SwitchRoute { subject, cases });

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
            workflow
                .pending_edges
                .push((f_id.to_string(), t_id.to_string(), e_obj));
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
        assert!(graph
            .python_imports
            .contains(&"sklearn.ensemble".to_string()));
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
            assert_eq!(
                action.params.get("encoding"),
                Some(&"\"utf-8\"".to_string())
            );
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
        assert!(
            result.is_ok(),
            "Comma-separated params should parse: {:?}",
            result.err()
        );
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
        assert!(
            result.is_ok(),
            "Comparison operators should be valid: {:?}",
            result.err()
        );
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
        assert!(
            result.is_ok(),
            "Multiline params should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_single_step_function() {
        let content = r#"
name: "Test"
entry: [step1]
[greet(name)]: bash(command="echo Hello, " + $name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert!(graph.functions.contains_key("greet"));
        let func = graph.functions.get("greet").unwrap();
        assert_eq!(func.params, vec!["name"]);
        assert_eq!(func.body.node_map.len(), 1);
    }

    #[test]
    fn test_multi_step_function() {
        let content = r#"
name: "Test"
entry: [step1]
[build(dir)]: {
  bash(command="cd " + $dir + " && make")
  bash(command="cd " + $dir + " && make test")
}
[step1]: build(dir="/app")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert!(graph.functions.contains_key("build"));
        let func = graph.functions.get("build").unwrap();
        assert_eq!(func.params, vec!["dir"]);
        assert_eq!(func.body.node_map.len(), 2);
        // Verify sequential edge exists
        assert_eq!(func.body.graph.edge_count(), 1);
    }

    #[test]
    fn test_multi_step_function_with_semicolons() {
        let content = r#"
name: "Test"
entry: [step1]
[build(a, b)]: { bash(command=$a); bash(command=$b) }
[step1]: build(a="foo", b="bar")
"#;
        let graph = GraphParser::parse(content).unwrap();
        let func = graph.functions.get("build").unwrap();
        assert_eq!(func.params, vec!["a", "b"]);
        assert_eq!(func.body.node_map.len(), 2);
    }

    #[test]
    fn test_function_not_in_main_graph() {
        let content = r#"
name: "Test"
entry: [step1]
[greet(name)]: bash(command="echo " + $name)
[step1]: greet(name="world")
"#;
        let graph = GraphParser::parse(content).unwrap();
        // Function node should NOT be in main graph
        assert!(!graph.node_map.contains_key("greet"));
        // But the caller should be
        assert!(graph.node_map.contains_key("step1"));
    }

    #[test]
    fn test_no_params_backward_compat() {
        let content = r#"
name: "Test"
entry: [start]
[start]: bash(command="echo hello")
"#;
        let graph = GraphParser::parse(content).unwrap();
        assert!(graph.node_map.contains_key("start"));
        assert!(graph.functions.is_empty());
    }

    #[test]
    fn test_string_concat_expression() {
        let content = r#"
name: "Test"
entry: [start]
[start]: chat(agent="helper", message="[Expert] " + $input.query)
"#;
        let result = GraphParser::parse(content);
        assert!(
            result.is_ok(),
            "String concat should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_foreach_without_dollar() {
        let content = r#"
name: "Foreach no $"
entry: [loop]
[loop]: foreach(item in input.items) {
    [step]: notify(message="ok")
}
"#;
        let graph = GraphParser::parse(content).unwrap();
        let node = graph.node_map.get("loop").unwrap();
        let node_data = &graph.graph[*node];
        if let NodeType::Foreach { item, list, .. } = &node_data.node_type {
            assert_eq!(item, "item");
            assert_eq!(list, "input.items");
        } else {
            panic!("Expected Foreach node");
        }
    }

    #[test]
    fn test_assignment_block_parsing() {
        let content = r#"
name: "Assignment Test"
entry: [init]
[init]: count = 0, name = "Alice"
[next]: notify(message="done")
[init] -> [next]
"#;
        let graph = GraphParser::parse(content).unwrap();
        let node = graph.node_map.get("init").unwrap();
        let node_data = &graph.graph[*node];
        if let NodeType::Task(action) = &node_data.node_type {
            assert_eq!(action.name, "set_context");
            assert_eq!(action.params.get("count").unwrap(), "0");
            assert_eq!(action.params.get("name").unwrap(), "\"Alice\"");
        } else {
            panic!("Expected Task node, got {:?}", node_data.node_type);
        }
    }

    #[test]
    fn test_assignment_single() {
        let content = r#"
name: "Single Assign"
entry: [init]
[init]: result = $output.data
"#;
        let graph = GraphParser::parse(content).unwrap();
        let node = graph.node_map.get("init").unwrap();
        let node_data = &graph.graph[*node];
        if let NodeType::Task(action) = &node_data.node_type {
            assert_eq!(action.name, "set_context");
            assert_eq!(action.params.get("result").unwrap(), "$output.data");
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_class_definition_parsing() {
        let content = r#"
name: "Class Test"
entry: [c]

class Counter(count=0) {
  [increment(n)]: count = $self.count + $n
  [reset]: count = 0
}

[c]: new Counter(count=10)
[r]: $c.increment(n=5)
[c] -> [r]
"#;
        let graph = GraphParser::parse(content).unwrap();

        // Verify class was parsed
        assert!(graph.classes.contains_key("Counter"));
        let class_def = graph.classes.get("Counter").unwrap();
        assert_eq!(class_def.fields.len(), 1);
        assert_eq!(class_def.fields[0].name, "count");
        assert_eq!(class_def.fields[0].default, Some("0".to_string()));

        // Verify methods
        assert!(class_def.methods.contains_key("increment"));
        assert!(class_def.methods.contains_key("reset"));
        let increment = class_def.methods.get("increment").unwrap();
        assert_eq!(increment.params, vec!["n"]);

        // Verify NewInstance node
        let c_idx = graph.node_map.get("c").unwrap();
        let c_node = &graph.graph[*c_idx];
        if let NodeType::NewInstance { class_name, args } = &c_node.node_type {
            assert_eq!(class_name, "Counter");
            assert_eq!(args.get("count"), Some(&"10".to_string()));
        } else {
            panic!("Expected NewInstance node, got {:?}", c_node.node_type);
        }

        // Verify MethodCall node
        let r_idx = graph.node_map.get("r").unwrap();
        let r_node = &graph.graph[*r_idx];
        if let NodeType::MethodCall {
            instance_path,
            method_name,
            args,
        } = &r_node.node_type
        {
            assert_eq!(instance_path, "c");
            assert_eq!(method_name, "increment");
            assert_eq!(args.get("n"), Some(&"5".to_string()));
        } else {
            panic!("Expected MethodCall node, got {:?}", r_node.node_type);
        }
    }

    #[test]
    fn test_class_no_params() {
        let content = r#"
name: "Class No Params"
entry: [c]

class Logger {
  [log(msg)]: notify(message=$msg)
}

[c]: new Logger()
"#;
        let graph = GraphParser::parse(content).unwrap();

        assert!(graph.classes.contains_key("Logger"));
        let class_def = graph.classes.get("Logger").unwrap();
        assert!(class_def.fields.is_empty());
        assert!(class_def.methods.contains_key("log"));
    }

    #[test]
    fn test_class_multi_step_method() {
        let content = r#"
name: "Multi Step Method"
entry: [c]

class Processor(data="") {
  [process(x)]: {
    notify(message=$x)
    data = $x
  }
}

[c]: new Processor()
"#;
        let graph = GraphParser::parse(content).unwrap();

        let class_def = graph.classes.get("Processor").unwrap();
        let process = class_def.methods.get("process").unwrap();
        assert_eq!(process.params, vec!["x"]);
        // Multi-step body should have 2 nodes
        assert_eq!(process.body.node_map.len(), 2);
        assert_eq!(process.body.graph.edge_count(), 1);
    }

    // ---- Triple-quoted strings ----

    #[test]
    fn test_triple_quoted_in_task_param() {
        let input = r#"
            [run]: bash(command="""echo "hello world" && echo '{"key":"value"}'""")
        "#;
        let wf = GraphParser::parse(input).unwrap();
        let node = &wf.graph[*wf.node_map.get("run").unwrap()];
        if let NodeType::Task(action) = &node.node_type {
            assert_eq!(action.name, "bash");
            let cmd = action.params.get("command").unwrap();
            assert!(cmd.contains(r#"echo "hello world""#));
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_triple_quoted_multiline_param() {
        let input = "[run]: bash(command=\"\"\"line1\nline2\nline3\"\"\")";
        let wf = GraphParser::parse(input).unwrap();
        assert!(wf.node_map.contains_key("run"));
    }

    #[test]
    fn test_triple_quoted_with_regular_string() {
        let input = r#"
            [a]: bash(command="""echo "test" done""")
            [b]: bash(command="echo simple")
        "#;
        let wf = GraphParser::parse(input).unwrap();
        assert!(wf.node_map.contains_key("a"));
        assert!(wf.node_map.contains_key("b"));
    }

    #[test]
    fn test_triple_quoted_assignment() {
        let input = r#"
            [setup]: cmd = """curl -H "Auth: key" https://api.com"""
        "#;
        let wf = GraphParser::parse(input).unwrap();
        assert!(wf.node_map.contains_key("setup"));
    }
}
