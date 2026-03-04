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
use crate::registry::cache::find_entry_in_dir;
use crate::registry::package::{is_registry_import, parse_registry_import};

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
        base.join(rest).to_string_lossy().replace('\\', "/")
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

/// 解析 lib imports — 加载库文件，提取 function defs 并以命名空间前缀注册到父工作流
///
/// 与 flow imports 的区别：libs 只提取 functions，不合并 graph 节点/边。
///
/// 命名空间三级优先级（高 → 低）：
/// 1. 对象形式的显式命名（parser 阶段确定，不在 lib_auto_namespaces 中）
/// 2. 库文件内的 slug 字段（仅列表形式，即在 lib_auto_namespaces 中时）
/// 3. 文件名 stem（列表形式的默认值，parser 阶段已存为 key）
pub fn resolve_lib_imports(
    workflow: &mut WorkflowGraph,
    base_dir: &Path,
    import_stack: &mut Vec<PathBuf>,
    at_base: Option<&Path>,
) -> Result<()> {
    if workflow.lib_imports.is_empty() {
        return Ok(());
    }

    let imports: Vec<(String, String)> = workflow.lib_imports.clone().into_iter().collect();
    let auto_namespaces = workflow.lib_auto_namespaces.clone();

    for (parser_namespace, rel_path) in imports {
        // 【新增】Registry 包检测 — 非本地路径视为 registry 包
        if is_registry_import(&rel_path) {
            let (pkg_name, version_req) = parse_registry_import(&rel_path)?;

            // 在项目根目录及向上查找 jg_modules/
            let jg_modules_path = find_jg_modules_dir(base_dir).map(|d| d.join(&pkg_name));

            let entry_path = if let Some(ref pkg_dir) = jg_modules_path {
                if pkg_dir.exists() {
                    // 已安装 → 读取 entry 文件
                    find_entry_in_dir(pkg_dir)?
                } else {
                    // 未安装 → 尝试自动安装
                    auto_install_package(&pkg_name, version_req.as_deref(), base_dir)?
                }
            } else {
                // 无 jg_modules → 尝试自动安装
                auto_install_package(&pkg_name, version_req.as_deref(), base_dir)?
            };

            // 解析库文件（与本地 lib 相同逻辑）
            let canonical = entry_path.canonicalize().with_context(|| {
                format!(
                    "Lib import error: Cannot resolve registry package '{}' entry at {:?}",
                    pkg_name, entry_path
                )
            })?;

            if import_stack.contains(&canonical) {
                return Err(anyhow!(
                    "Circular lib import detected: '{}' ({:?})\nImport chain: {:?}",
                    parser_namespace,
                    canonical,
                    import_stack
                ));
            }
            import_stack.push(canonical.clone());

            let content = std::fs::read_to_string(&canonical)
                .with_context(|| format!("Lib import error: Cannot read '{:?}'", canonical))?;
            let mut lib_graph = GraphParser::parse_lib(&content)
                .with_context(|| format!("Lib import error: Failed to parse '{:?}'", canonical))?;

            let lib_base_dir = canonical.parent().unwrap_or(Path::new("."));
            resolve_lib_imports(&mut lib_graph, lib_base_dir, import_stack, at_base)?;

            // Registry 包命名空间优先级：
            // 1. 对象形式显式命名（parser_namespace 不在 auto_namespaces 中）
            // 2. 库文件的 slug 字段
            // 3. 包名（而非文件 stem）
            let namespace = if !auto_namespaces.contains(&parser_namespace) {
                parser_namespace.clone()
            } else if !lib_graph.slug.is_empty() {
                lib_graph.slug.clone()
            } else {
                pkg_name.clone()
            };

            for (func_name, func_def) in lib_graph.functions {
                let namespaced = format!("{}.{}", namespace, func_name);
                workflow.functions.insert(namespaced, func_def);
            }
            for (class_name, class_def) in lib_graph.classes {
                let namespaced = format!("{}.{}", namespace, class_name);
                workflow.classes.insert(namespaced, class_def);
            }

            import_stack.pop();
            continue;
        }

        // 本地文件路径解析（现有逻辑）
        let expanded = expand_at_prefix(&rel_path, at_base);
        let abs_path = if Path::new(&expanded).is_absolute() {
            PathBuf::from(&expanded)
        } else {
            base_dir.join(&expanded)
        };
        let canonical = abs_path.canonicalize().with_context(|| {
            format!(
                "Lib import error: Cannot resolve path '{}' (base: {:?})",
                rel_path, base_dir
            )
        })?;

        // 2. 循环导入检测
        if import_stack.contains(&canonical) {
            return Err(anyhow!(
                "Circular lib import detected: '{}' ({:?})\nImport chain: {:?}",
                parser_namespace,
                canonical,
                import_stack
            ));
        }
        import_stack.push(canonical.clone());

        // 3. 解析库文件
        let content = std::fs::read_to_string(&canonical)
            .with_context(|| format!("Lib import error: Cannot read '{:?}'", canonical))?;
        let mut lib_graph = GraphParser::parse_lib(&content)
            .with_context(|| format!("Lib import error: Failed to parse '{:?}'", canonical))?;

        // 4. 递归解析库自身的 lib imports
        let lib_base_dir = canonical.parent().unwrap_or(Path::new("."));
        resolve_lib_imports(&mut lib_graph, lib_base_dir, import_stack, at_base)?;

        // 5. 确定最终命名空间（三级优先级）
        let namespace = if !auto_namespaces.contains(&parser_namespace) {
            // 对象形式显式命名 — 最高优先级
            parser_namespace.clone()
        } else if !lib_graph.slug.is_empty() {
            // 列表形式 + 库文件有 slug — 中优先级
            lib_graph.slug.clone()
        } else {
            // 列表形式 + 无 slug — 用文件名 stem（最低优先级）
            parser_namespace.clone()
        };

        // 6. 提取 function defs，加命名空间前缀注册到父工作流
        for (func_name, func_def) in lib_graph.functions {
            let namespaced = format!("{}.{}", namespace, func_name);
            workflow.functions.insert(namespaced, func_def);
        }
        for (class_name, class_def) in lib_graph.classes {
            let namespaced = format!("{}.{}", namespace, class_name);
            workflow.classes.insert(namespaced, class_def);
        }

        import_stack.pop();
    }

    Ok(())
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
        NodeType::Foreach {
            item,
            list,
            body,
            parallel,
        } => {
            let prefixed_list = prefix_variables(list, prefix, child_node_ids);
            let prefixed_body = prefix_subgraph_body(body, prefix, child_node_ids);
            NodeType::Foreach {
                item: item.clone(),
                list: prefixed_list,
                body: Box::new(prefixed_body),
                parallel: *parallel,
            }
        }
        NodeType::Literal(val) => NodeType::Literal(val.clone()),
        NodeType::_ExternalCall {
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
            NodeType::_ExternalCall {
                call_path: call_path.clone(),
                args: prefixed_args,
                kwargs: prefixed_kwargs,
            }
        }
        NodeType::NewInstance { class_name, args } => {
            let prefixed_args: std::collections::HashMap<String, String> = args
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::NewInstance {
                class_name: class_name.clone(),
                args: prefixed_args,
            }
        }
        NodeType::MethodCall {
            instance_path,
            method_name,
            args,
        } => {
            let prefixed_args: std::collections::HashMap<String, String> = args
                .iter()
                .map(|(k, v)| (k.clone(), prefix_variables(v, prefix, child_node_ids)))
                .collect();
            NodeType::MethodCall {
                instance_path: instance_path.clone(),
                method_name: method_name.clone(),
                args: prefixed_args,
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

/// Find the jg_modules directory by searching from base_dir upward
fn find_jg_modules_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("jg_modules");
        if candidate.is_dir() {
            return Some(candidate);
        }
        // Also check if jgpackage.toml exists here (project root)
        if dir.join("jgpackage.toml").exists() {
            return Some(candidate); // Return even if doesn't exist yet — installer will create it
        }
        if !dir.pop() {
            break;
        }
    }
    // Fallback: create jg_modules in the start directory
    Some(start.join("jg_modules"))
}

/// Auto-install a registry package using the registry client.
/// This bridges from sync resolver code into async installer via tokio runtime.
fn auto_install_package(
    pkg_name: &str,
    version_req: Option<&str>,
    project_dir: &Path,
) -> Result<PathBuf> {
    tracing::info!("Auto-installing registry package '{}' ...", pkg_name);

    // Load registry URL from config, or use default
    let registry_url = crate::services::config::JuglansConfig::load()
        .ok()
        .and_then(|c| c.registry.map(|r| r.url))
        .unwrap_or_else(|| "https://jgr.juglans.ai".to_string());

    let installer = crate::registry::installer::PackageInstaller::with_defaults(&registry_url)
        .with_context(|| "Failed to create package installer")?;

    // Bridge into async: we're called from sync code, but the caller runs inside tokio
    let handle = tokio::runtime::Handle::try_current().with_context(|| {
        format!(
            "Cannot auto-install package '{}': no async runtime available. \
             Run 'juglans add {}' first, or ensure the workflow is run with 'juglans'.",
            pkg_name, pkg_name
        )
    })?;

    let name = pkg_name.to_string();
    let ver = version_req.map(|s| s.to_string());
    let proj = project_dir.to_path_buf();

    let installed = handle
        .block_on(async move { installer.install(&name, ver.as_deref(), &proj).await })
        .with_context(|| format!("Failed to auto-install package '{}'", pkg_name))?;

    Ok(installed.entry_path)
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

    #[test]
    fn test_resolve_lib_imports_explicit_namespace() {
        use std::io::Write;

        // 创建临时 lib 文件
        let dir = std::env::temp_dir().join("juglans_test_lib_explicit");
        let _ = std::fs::create_dir_all(&dir);
        let lib_path = dir.join("sqlite.jg");
        let mut f = std::fs::File::create(&lib_path).unwrap();
        writeln!(
            f,
            r#"
slug: "sqlite3"
name: "SQLite Lib"
[read(table)]: bash(command="sqlite3 db.sqlite 'SELECT * FROM " + $table + "'")
[write(table, data)]: bash(command="echo " + $data)
"#
        )
        .unwrap();

        // 主工作流
        let main_content = format!(
            r#"
name: "Main"
libs: {{ db: "{}" }}
entry: [step1]
[step1]: db.read(table="users")
"#,
            lib_path.to_string_lossy()
        );

        let mut graph = GraphParser::parse(&main_content).unwrap();

        // 验证 parser 存入了 lib_imports（显式命名空间）
        assert_eq!(
            graph.lib_imports.get("db").unwrap(),
            lib_path.to_str().unwrap()
        );
        assert!(!graph.lib_auto_namespaces.contains("db"));

        // 解析 lib imports
        let mut import_stack = vec![];
        resolve_lib_imports(&mut graph, &dir, &mut import_stack, None).unwrap();

        // 显式命名空间 "db"（忽略文件内 slug "sqlite3"）
        assert!(
            graph.functions.contains_key("db.read"),
            "functions: {:?}",
            graph.functions.keys().collect::<Vec<_>>()
        );
        assert!(graph.functions.contains_key("db.write"));
        assert!(!graph.functions.contains_key("sqlite3.read")); // 不应使用 slug
    }

    #[test]
    fn test_resolve_lib_imports_auto_namespace_with_slug() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("juglans_test_lib_slug");
        let _ = std::fs::create_dir_all(&dir);
        let lib_path = dir.join("my_sqlite_lib.jg");
        let mut f = std::fs::File::create(&lib_path).unwrap();
        writeln!(
            f,
            r#"
slug: "sqlite"
name: "SQLite Lib"
[query(sql)]: bash(command="sqlite3 db.sqlite '" + $sql + "'")
"#
        )
        .unwrap();

        // 列表形式 — stem = "my_sqlite_lib"，但文件有 slug = "sqlite"
        let main_content = format!(
            r#"
name: "Main"
libs: ["{}"]
entry: [step1]
[step1]: sqlite.query(sql="SELECT 1")
"#,
            lib_path.to_string_lossy()
        );

        let mut graph = GraphParser::parse(&main_content).unwrap();

        // 验证 parser 存入 stem 作为占位
        assert!(graph.lib_imports.contains_key("my_sqlite_lib"));
        assert!(graph.lib_auto_namespaces.contains("my_sqlite_lib"));

        // 解析 lib imports
        let mut import_stack = vec![];
        resolve_lib_imports(&mut graph, &dir, &mut import_stack, None).unwrap();

        // 列表形式 + 文件有 slug → 使用 slug "sqlite"（中优先级）
        assert!(
            graph.functions.contains_key("sqlite.query"),
            "functions: {:?}",
            graph.functions.keys().collect::<Vec<_>>()
        );
        assert!(!graph.functions.contains_key("my_sqlite_lib.query"));
    }

    #[test]
    fn test_resolve_lib_imports_auto_namespace_no_slug() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("juglans_test_lib_stem");
        let _ = std::fs::create_dir_all(&dir);
        let lib_path = dir.join("utils.jg");
        let mut f = std::fs::File::create(&lib_path).unwrap();
        writeln!(
            f,
            r#"
name: "Utils"
[helper(x)]: bash(command="echo " + $x)
"#
        )
        .unwrap();

        let main_content = format!(
            r#"
name: "Main"
libs: ["{}"]
entry: [step1]
[step1]: utils.helper(x="test")
"#,
            lib_path.to_string_lossy()
        );

        let mut graph = GraphParser::parse(&main_content).unwrap();

        let mut import_stack = vec![];
        resolve_lib_imports(&mut graph, &dir, &mut import_stack, None).unwrap();

        // 列表形式 + 无 slug → 使用文件名 stem "utils"
        assert!(
            graph.functions.contains_key("utils.helper"),
            "functions: {:?}",
            graph.functions.keys().collect::<Vec<_>>()
        );
    }
}
