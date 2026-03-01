// src/core/renderer.rs
use crate::core::expr_eval::{is_truthy, ExprEvaluator};
use crate::core::prompt_parser::TemplateNode;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct JwlRenderer {
    eval: ExprEvaluator,
}

impl Default for JwlRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl JwlRenderer {
    pub fn new() -> Self {
        Self {
            eval: ExprEvaluator::new(),
        }
    }

    pub fn render(&self, ast: &[TemplateNode], context: &Value) -> Result<String> {
        // Flatten context object into a HashMap for scope lookup
        let mut scope = HashMap::new();
        if let Some(obj) = context.as_object() {
            for (k, v) in obj {
                scope.insert(k.clone(), v.clone());
            }
        }
        self.render_nodes(ast, &mut scope)
    }

    fn render_nodes(
        &self,
        nodes: &[TemplateNode],
        scope: &mut HashMap<String, Value>,
    ) -> Result<String> {
        let mut output = String::new();
        for node in nodes {
            match node {
                TemplateNode::Text(t) => output.push_str(t),

                TemplateNode::Interpolation(expr) => {
                    let result = {
                        let resolver = make_resolver(scope);
                        self.eval
                            .eval(expr.trim(), &resolver)
                            .map_err(|e| anyhow!("Interpolation error in '{}': {}", expr, e))?
                    };
                    // Convert to display string
                    match &result {
                        Value::String(s) => output.push_str(s),
                        Value::Null => {} // null renders as empty
                        Value::Bool(b) => output.push_str(&b.to_string()),
                        Value::Number(n) => output.push_str(&n.to_string()),
                        _ => output.push_str(&serde_json::to_string(&result).unwrap_or_default()),
                    }
                }

                TemplateNode::If {
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch,
                } => {
                    let cond_res = {
                        let resolver = make_resolver(scope);
                        self.eval
                            .eval(condition.trim(), &resolver)
                            .map_err(|e| anyhow!("Condition error in '{}': {}", condition, e))?
                    };

                    if is_truthy(&cond_res) {
                        output.push_str(&self.render_nodes(then_branch, scope)?);
                    } else {
                        let mut matched = false;
                        for (elif_cond, elif_body) in elif_branches {
                            let elif_res = {
                                let elif_resolver = make_resolver(scope);
                                self.eval
                                    .eval(elif_cond.trim(), &elif_resolver)
                                    .map_err(|e| {
                                        anyhow!("Elif condition error in '{}': {}", elif_cond, e)
                                    })?
                            };
                            if is_truthy(&elif_res) {
                                output.push_str(&self.render_nodes(elif_body, scope)?);
                                matched = true;
                                break;
                            }
                        }
                        if !matched {
                            if let Some(eb) = else_branch {
                                output.push_str(&self.render_nodes(eb, scope)?);
                            }
                        }
                    }
                }

                TemplateNode::For {
                    var_name,
                    iterable_expr,
                    body,
                    else_branch,
                } => {
                    let list = {
                        let resolver = make_resolver(scope);
                        self.eval
                            .eval(iterable_expr.trim(), &resolver)
                            .map_err(|e| anyhow!("For loop error in '{}': {}", iterable_expr, e))?
                    };

                    let mut ran_loop = false;
                    if let Value::Array(array) = list {
                        let total = array.len();
                        if total > 0 {
                            ran_loop = true;
                            for (idx, item) in array.into_iter().enumerate() {
                                // Inject loop variable and loop metadata into scope
                                scope.insert(var_name.clone(), item);
                                scope.insert(
                                    "loop".to_string(),
                                    json!({
                                        "index0": idx,
                                        "index": idx + 1,
                                        "first": idx == 0,
                                        "last": idx == total - 1,
                                    }),
                                );

                                output.push_str(&self.render_nodes(body, scope)?);

                                // Clean up loop variables
                                scope.remove(var_name);
                                scope.remove("loop");
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

/// Create a resolver from a scope HashMap.
/// The resolver looks up bare identifiers directly in scope,
/// and handles dot-path access into nested values.
fn make_resolver<'a>(scope: &'a HashMap<String, Value>) -> impl Fn(&str) -> Option<Value> + 'a {
    move |path: &str| {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        // Look up the root variable in scope
        let root = scope.get(parts[0])?;

        // Navigate into nested fields
        let mut current = root.clone();
        for part in &parts[1..] {
            current = match current {
                Value::Object(ref map) => map.get(*part)?.clone(),
                _ => return None,
            };
        }
        Some(current)
    }
}
