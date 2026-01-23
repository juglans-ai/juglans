// src/core/renderer.rs
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use rhai::{Engine, Scope, Dynamic, ImmutableString};
use crate::core::prompt_parser::TemplateNode;

pub struct JwlRenderer {
    engine: Engine,
}

impl JwlRenderer {
    pub fn new() -> Self {
        let mut engine = Engine::new();
        engine.set_max_operations(100_000);

        // 注册内置过滤器
        engine.register_fn("round", |val: f64, digits: i64| {
            let p = 10f64.powi(digits as i32);
            (val * p).round() / p
        });

        engine.register_fn("truncate", |s: ImmutableString, len: i64| {
            if s.len() as i64 <= len {
                s.to_string()
            } else {
                let end = s.chars().take(len as usize).collect::<String>();
                format!("{}...", end)
            }
        });

        Self { engine }
    }

    /// 预处理表达式：
    /// 1. 将 Jinja 风格的 "val | filter" 转为 Rhai 的 "val.filter()"
    /// 2. 将保留关键字 "loop." 替换为 "_loop." 以绕过 Rhai 解析限制
    fn preprocess_expression(&self, expr: &str) -> String {
        // 替换 loop. 为 _loop.
        let mut processed = expr.replace("loop.", "_loop.");

        if processed.contains('|') {
            let parts: Vec<&str> = processed.split('|').map(|s| s.trim()).collect();
            let mut base = parts[0].to_string();
            for filter in &parts[1..] {
                if filter.contains('(') {
                    base = format!("{}.{}", base, filter);
                } else {
                    base = format!("{}.{}()", base, filter);
                }
            }
            processed = base;
        }
        processed
    }

    pub fn render(&self, ast: &[TemplateNode], context: &Value) -> Result<String> {
        let mut scope = Scope::new();
        
        let dynamic_ctx = rhai::serde::to_dynamic(context.clone())?;
        if let Some(map) = dynamic_ctx.try_cast::<rhai::Map>() {
            for (k, v) in map {
                scope.push(k, v);
            }
        }

        self.render_nodes(ast, &mut scope)
    }

    fn render_nodes(&self, nodes: &[TemplateNode], scope: &mut Scope) -> Result<String> {
        let mut output = String::new();
        for node in nodes {
            match node {
                TemplateNode::Text(t) => output.push_str(t),
                TemplateNode::Interpolation(expr) => {
                    let processed = self.preprocess_expression(expr);
                    let result = self.engine.eval_with_scope::<Dynamic>(scope, &processed)
                        .map_err(|e| anyhow!("Interpolation error in '{}': {}", expr, e))?;
                    output.push_str(&result.to_string());
                }
                TemplateNode::If { condition, then_branch, else_branch } => {
                    let processed = self.preprocess_expression(condition);
                    let cond_res = self.engine.eval_with_scope::<bool>(scope, &processed)
                        .map_err(|e| anyhow!("Condition error in '{}': {}", condition, e))?;
                    
                    if cond_res {
                        output.push_str(&self.render_nodes(then_branch, scope)?);
                    } else if let Some(eb) = else_branch {
                        output.push_str(&self.render_nodes(eb, scope)?);
                    }
                }
                TemplateNode::For { var_name, iterable_expr, body, else_branch } => {
                    let processed = self.preprocess_expression(iterable_expr);
                    let list = self.engine.eval_with_scope::<Dynamic>(scope, &processed)
                        .map_err(|e| anyhow!("For loop error in '{}': {}", iterable_expr, e))?;
                    
                    let mut ran_loop = false;
                    if let Some(array) = list.try_cast::<Vec<Dynamic>>() {
                        let total = array.len();
                        if total > 0 {
                            ran_loop = true;
                            for (idx, item) in array.into_iter().enumerate() {
                                // 构造 loop 上下文对象
                                let mut loop_map = rhai::Map::new();
                                loop_map.insert("index0".into(), (idx as i64).into());
                                loop_map.insert("index".into(), ((idx + 1) as i64).into());
                                loop_map.insert("first".into(), (idx == 0).into());
                                loop_map.insert("last".into(), (idx == total - 1).into());

                                // 注入变量 (注意使用 _loop 躲避关键字)
                                scope.push(var_name.clone(), item);
                                scope.push("_loop", loop_map);

                                output.push_str(&self.render_nodes(body, scope)?);

                                // 清理当前循环的作用域变量
                                scope.rewind(scope.len() - 2); 
                            }
                        }
                    }

                    if !ran_loop {
                        if let Some(eb) = else_branch {
                            output.push_str(&self.render_nodes(eb, scope)?);
                        }
                    }
                }
            }
        }
        Ok(output)
    }
}