// src/core/resolver.rs
//
// Flow Import Resolver — 编译时图合并
//
// 解析 WorkflowGraph 中的 flow_imports 声明，加载子工作流文件，
// 将子工作流的节点和边以命名空间前缀合并到父图中，最后解析 pending_edges。

use anyhow::{anyhow, Context, Result};
use regex::{Captures, Regex};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use petgraph::visit::EdgeRef;

use crate::core::graph::{Action, Node, NodeType, SwitchCase, SwitchRoute, WorkflowGraph};
use crate::core::parser::GraphParser;

lazy_static::lazy_static! {
    /// 匹配变量引用：$identifier.path.segments
    static ref VAR_REF_RE: Regex = Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_]*)(\.[a-zA-Z0-9_.]+)?").unwrap();
}

/// 展开 "@/" 前缀为 base_path（project_root + config.paths.base）
/// at_base = None 时功能禁用，原样返回
pub fn expand_at_prefix(pattern: &str, at_base: Option<&Path>) -> String {
    let Some(base) = at_base else {
        return pattern.to_string();
    };
    if let Some(rest) = pattern.strip_prefix("@/") {
        base.join(rest).to_string_lossy().to_string()
    } else {
        pattern.to_string()
    }
}

/// 批量展开 "@/" 前缀
pub fn expand_at_prefixes(patterns: &[String], at_base: Option<&Path>) -> Vec<String> {
    patterns
        .iter()
        .map(|p| expand_at_prefix(p, at_base))
        .collect()
}

/// 解析 flow imports 并合并子图到父工作流
///
/// - `workflow`: 父工作流（会被修改）
/// - `base_dir`: 父工作流文件所在目录（用于解析相对路径）
/// - `import_stack`: 已导入文件的绝对路径栈（用于检测循环导入）
/// - `at_base`: @ 路径别名的基准目录（None = 禁用）
pub fn resolve_flow_imports(
    workflow: &mut WorkflowGraph,
    base_dir: &Path,
    import_stack: &mut Vec<PathBuf>,
    at_base: Option<&Path>,
) -> Result<()> {
    if workflow.flow_imports.is_empty() {
        // 即使没有 flow_imports，也需要解析 pending_edges（可能有误写的命名空间引用）
        commit_pending_edges(workflow)?;
        return Ok(());
    }

    // Clone imports 避免 borrow 冲突
    let imports: Vec<(String, String)> = workflow.flow_imports.clone().into_iter().collect();

    for (alias, rel_path) in imports {
        // 1. 展开 @/ 前缀并解析绝对路径
        let expanded_rel = expand_at_prefix(&rel_path, at_base);
        let abs_path = if Path::new(&expanded_rel).is_absolute() {
            PathBuf::from(&expanded_rel)
        } else {
            base_dir.join(&expanded_rel)
        };
        let canonical = abs_path.canonicalize().with_context(|| {
            format!(
                "Flow import error: Cannot resolve path '{}' (base: {:?})",
                rel_path, base_dir
            )
        })?;

        // 2. 循环导入检测
        if import_stack.contains(&canonical) {
            return Err(anyhow!(
                "Circular flow import detected: '{}' ({:?})\nImport chain: {:?}",
                alias,
                canonical,
                import_stack
            ));
        }
        import_stack.push(canonical.clone());

        // 3. 加载并解析子工作流
        let content = std::fs::read_to_string(&canonical)
            .with_context(|| format!("Flow import error: Cannot read '{:?}'", canonical))?;
        let mut child_graph = GraphParser::parse(&content)
            .with_context(|| format!("Flow import error: Failed to parse '{:?}'", canonical))?;

        // 4. 递归解析子工作流自身的 flow imports
        let child_base_dir = canonical.parent().unwrap_or(Path::new("."));
        resolve_flow_imports(&mut child_graph, child_base_dir, import_stack, at_base)?;

        // 5. 合并子图到父图
        merge_subgraph(workflow, &child_graph, &alias, child_base_dir, at_base)?;

        import_stack.pop();
    }

    // 6. 所有子图合并完毕，解析 pending_edges
    commit_pending_edges(workflow)?;

    Ok(())
}

/// 将子工作流的节点、边、switch 路由合并到父图中
fn merge_subgraph(
    parent: &mut WorkflowGraph,
    child: &WorkflowGraph,
    prefix: &str,
    child_base_dir: &Path,
    at_base: Option<&Path>,
) -> Result<()> {
    // 收集子工作流的所有节点 ID（用于变量命名空间转换）
    let child_node_ids: HashSet<String> = child
        .graph
        .node_indices()
        .map(|idx| child.graph[idx].id.clone())
        .collect();

    // --- 1. 合并节点 ---
    for idx in child.graph.node_indices() {
        let child_node = &child.graph[idx];
        let prefixed_id = format!("{}.{}", prefix, child_node.id);

        // 克隆 node_type 并做变量命名空间转换
        let prefixed_node_type = prefix_node_type(&child_node.node_type, prefix, &child_node_ids);

        let new_node = Node {
            id: prefixed_id.clone(),
            node_type: prefixed_node_type,
        };

        let new_idx = parent.graph.add_node(new_node);
        parent.node_map.insert(prefixed_id, new_idx);
    }

    // --- 2. 合并边 ---
    for edge_ref in child.graph.edge_references() {
        let from_id = format!("{}.{}", prefix, child.graph[edge_ref.source()].id);
        let to_id = format!("{}.{}", prefix, child.graph[edge_ref.target()].id);
        let mut edge = edge_ref.weight().clone();

        // 条件表达式中的变量也需要转换
        if let Some(ref cond) = edge.condition {
            edge.condition = Some(prefix_variables(cond, prefix, &child_node_ids));
        }

        // 此时两个节点都已添加到 parent，可以直接 commit
        let f_idx = *parent.node_map.get(&from_id).ok_or_else(|| {
            anyhow!(
                "Merge error: source node '{}' not found after merge",
                from_id
            )
        })?;
        let t_idx = *parent
            .node_map
            .get(&to_id)
            .ok_or_else(|| anyhow!("Merge error: target node '{}' not found after merge", to_id))?;
        parent.graph.add_edge(f_idx, t_idx, edge);
    }

    // --- 3. 合并 switch 路由 ---
    for (key, route) in &child.switch_routes {
        let prefixed_key = format!("{}.{}", prefix, key);
        let prefixed_route = SwitchRoute {
            subject: prefix_variables(&route.subject, prefix, &child_node_ids),
            cases: route
                .cases
                .iter()
                .map(|c| SwitchCase {
                    value: c.value.clone(),
                    target: format!("{}.{}", prefix, c.target),
                })
                .collect(),
        };
        parent.switch_routes.insert(prefixed_key, prefixed_route);
    }

    // --- 4. 合并子工作流的 pending_edges（加前缀后转移到父图） ---
    for (f_id, t_id, mut edge) in child.pending_edges.clone() {
        let prefixed_f = format!("{}.{}", prefix, f_id);
        let prefixed_t = format!("{}.{}", prefix, t_id);
        if let Some(ref cond) = edge.condition {
            edge.condition = Some(prefix_variables(cond, prefix, &child_node_ids));
        }
        parent.pending_edges.push((prefixed_f, prefixed_t, edge));
    }

    // --- 5. 合并资源模式（先展开 @/ 别名，非绝对路径调整为基于子工作流目录） ---
    for pattern in &child.prompt_patterns {
        let expanded = expand_at_prefix(pattern, at_base);
        if Path::new(&expanded).is_absolute() {
            parent.prompt_patterns.push(expanded);
        } else {
            parent
                .prompt_patterns
                .push(child_base_dir.join(&expanded).to_string_lossy().to_string());
        }
    }
    for pattern in &child.agent_patterns {
        let expanded = expand_at_prefix(pattern, at_base);
        if Path::new(&expanded).is_absolute() {
            parent.agent_patterns.push(expanded);
        } else {
            parent
                .agent_patterns
                .push(child_base_dir.join(&expanded).to_string_lossy().to_string());
        }
    }
    for pattern in &child.tool_patterns {
        let expanded = expand_at_prefix(pattern, at_base);
        if Path::new(&expanded).is_absolute() {
            parent.tool_patterns.push(expanded);
        } else {
            parent
                .tool_patterns
                .push(child_base_dir.join(&expanded).to_string_lossy().to_string());
        }
    }
    for import in &child.python_imports {
        if !parent.python_imports.contains(import) {
            parent.python_imports.push(import.clone());
        }
    }

    Ok(())
}

/// 解析并提交所有 pending_edges（flow 合并完成后调用）
fn commit_pending_edges(workflow: &mut WorkflowGraph) -> Result<()> {
    let pending = std::mem::take(&mut workflow.pending_edges);

    for (f_id, t_id, edge) in pending {
        let f_idx = *workflow.node_map.get(&f_id).ok_or_else(|| {
            anyhow!(
                "Graph Error: Pending edge references undefined node '{}'. \
                 Did you declare it in 'flows:' and define it in the imported workflow?",
                f_id
            )
        })?;
        let t_idx = *workflow.node_map.get(&t_id).ok_or_else(|| {
            anyhow!(
                "Graph Error: Pending edge references undefined node '{}'. \
                 Did you declare it in 'flows:' and define it in the imported workflow?",
                t_id
            )
        })?;
        workflow.graph.add_edge(f_idx, t_idx, edge);
    }

    Ok(())
}

// =============================================================================
// 变量命名空间转换
// =============================================================================

/// 对字符串中的变量引用加命名空间前缀
///
/// 规则：只有第一段匹配子工作流内部节点 ID 的变量才加前缀
/// - $verify.output       → $prefix.verify.output   (verify 是子流节点)
/// - $ctx.some_var        → $ctx.some_var            (ctx 不是节点，不变)
/// - $input.message       → $input.message           (不变)
/// - $output              → $output                  (不变)
fn prefix_variables(text: &str, prefix: &str, child_node_ids: &HashSet<String>) -> String {
    VAR_REF_RE
        .replace_all(text, |caps: &Captures| {
            let first_segment = &caps[1]; // 变量的第一段（如 verify, ctx, input）
            if child_node_ids.contains(first_segment) {
                // 是子流节点 → 加前缀
                let rest = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                format!("${}.{}{}", prefix, first_segment, rest)
            } else {
                // 不是节点（ctx, input, output 等）→ 保持不变
                caps[0].to_string()
            }
        })
        .to_string()
}

/// 对 NodeType 内部的变量引用做命名空间转换
fn prefix_node_type(
    node_type: &NodeType,
    prefix: &str,
    child_node_ids: &HashSet<String>,
) -> NodeType {
    match node_type {
        NodeType::Task(action) => {
            let prefixed_params: std::collections::HashMap<String, String> = action
                .params
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::Task(Action {
                name: action.name.clone(),
                params: prefixed_params,
            })
        }
        NodeType::Loop { condition, body } => {
            let prefixed_cond = prefix_variables(condition, prefix, child_node_ids);
            // 递归处理循环体内的节点
            let prefixed_body = prefix_subgraph_body(body, prefix, child_node_ids);
            NodeType::Loop {
                condition: prefixed_cond,
                body: Box::new(prefixed_body),
            }
        }
        NodeType::Foreach { item, list, body } => {
            let prefixed_list = prefix_variables(list, prefix, child_node_ids);
            let prefixed_body = prefix_subgraph_body(body, prefix, child_node_ids);
            NodeType::Foreach {
                item: item.clone(),
                list: prefixed_list,
                body: Box::new(prefixed_body),
            }
        }
        NodeType::Literal(val) => NodeType::Literal(val.clone()),
        NodeType::ExternalCall {
            call_path,
            args,
            kwargs,
        } => {
            let prefixed_args: Vec<String> = args
                .iter()
                .map(|a| prefix_variables(a, prefix, child_node_ids))
                .collect();
            let prefixed_kwargs: std::collections::HashMap<String, String> = kwargs
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::ExternalCall {
                call_path: call_path.clone(),
                args: prefixed_args,
                kwargs: prefixed_kwargs,
            }
        }
    }
}

/// 对嵌套工作流体（loop/foreach body）做变量转换
fn prefix_subgraph_body(
    body: &WorkflowGraph,
    prefix: &str,
    child_node_ids: &HashSet<String>,
) -> WorkflowGraph {
    let mut new_body = body.clone();
    // 转换 body 内部节点的变量引用
    for idx in new_body.graph.node_indices() {
        let node = &new_body.graph[idx];
        let new_type = prefix_node_type(&node.node_type, prefix, child_node_ids);
        new_body.graph[idx].node_type = new_type;
    }
    // 转换 body 内部边的条件表达式
    for edge_idx in new_body.graph.edge_indices() {
        let edge = &new_body.graph[edge_idx];
        if let Some(ref cond) = edge.condition {
            let new_cond = prefix_variables(cond, prefix, child_node_ids);
            new_body.graph[edge_idx].condition = Some(new_cond);
        }
    }
    new_body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_variables_basic() {
        let mut node_ids = HashSet::new();
        node_ids.insert("verify".to_string());
        node_ids.insert("extract".to_string());

        // 子流节点引用 → 加前缀
        assert_eq!(
            prefix_variables("$verify.output", "auth", &node_ids),
            "$auth.verify.output"
        );
        assert_eq!(
            prefix_variables("$extract.output.intent", "auth", &node_ids),
            "$auth.extract.output.intent"
        );

        // 全局变量 → 不变
        assert_eq!(
            prefix_variables("$ctx.some_var", "auth", &node_ids),
            "$ctx.some_var"
        );
        assert_eq!(
            prefix_variables("$input.message", "auth", &node_ids),
            "$input.message"
        );
        assert_eq!(prefix_variables("$output", "auth", &node_ids), "$output");
    }

    #[test]
    fn test_prefix_variables_mixed() {
        let mut node_ids = HashSet::new();
        node_ids.insert("classify".to_string());

        let input = r#"$classify.output.intent == "trade" && $ctx.ready"#;
        let result = prefix_variables(input, "trading", &node_ids);
        assert_eq!(
            result,
            r#"$trading.classify.output.intent == "trade" && $ctx.ready"#
        );
    }

    #[test]
    fn test_expand_at_prefix() {
        let base = Path::new("/project/src");

        // @/ 开头 → 展开为 base + 剩余部分
        assert_eq!(
            expand_at_prefix("@/prompts/foo.jgprompt", Some(base)),
            "/project/src/prompts/foo.jgprompt"
        );

        // 非 @/ 开头 → 原样返回
        assert_eq!(expand_at_prefix("./local/file", Some(base)), "./local/file");
        assert_eq!(
            expand_at_prefix("relative/path", Some(base)),
            "relative/path"
        );

        // 只有 @ 没有 / → 原样返回
        assert_eq!(expand_at_prefix("@noslash", Some(base)), "@noslash");

        // at_base = None → 功能禁用，原样返回
        assert_eq!(
            expand_at_prefix("@/prompts/foo.jgprompt", None),
            "@/prompts/foo.jgprompt"
        );
    }

    #[test]
    fn test_expand_at_prefixes_batch() {
        let base = Path::new("/project");
        let patterns = vec![
            "@/prompts/*.jgprompt".to_string(),
            "./local/file.jgprompt".to_string(),
            "@/agents/my-agent.jgagent".to_string(),
        ];
        let result = expand_at_prefixes(&patterns, Some(base));
        assert_eq!(result[0], "/project/prompts/*.jgprompt");
        assert_eq!(result[1], "./local/file.jgprompt");
        assert_eq!(result[2], "/project/agents/my-agent.jgagent");
    }

    #[test]
    fn test_prefix_variables_no_match() {
        let node_ids = HashSet::new(); // 空集合

        assert_eq!(
            prefix_variables("$output + $ctx.x", "ns", &node_ids),
            "$output + $ctx.x"
        );
    }
}
