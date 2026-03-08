/// Juglans Expression Language — Parser + Tree-Walking Evaluator
///
/// Replaces Rhai with a custom expression engine that:
/// - Uses Python-like semantics (truthiness, None handling, `and`/`or`/`not`/`in`)
/// - Operates directly on `serde_json::Value` (no intermediate type system)
/// - Supports pipe/filter syntax natively (`value | upper | truncate(10)`)
/// - Provides clear error messages with expression context
use anyhow::{anyhow, Result};
use pest::Parser;
use pest_derive::Parser;
use serde_json::{json, Value};

use super::expr_ast::{BinOp, Expr, FStringPart, UnaryOp};

// ============================================================
// Pest Grammar
// ============================================================

#[derive(Parser)]
#[grammar = "core/expr.pest"]
struct ExprParser;

// ============================================================
// Evaluator
// ============================================================

pub struct ExprEvaluator;

impl Default for ExprEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl ExprEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate an expression string with a variable resolver.
    /// The resolver maps variable paths (e.g. "input.field") to JSON values.
    pub fn eval<F>(&self, expr_str: &str, resolver: &F) -> Result<Value>
    where
        F: Fn(&str) -> Option<Value>,
    {
        let trimmed = expr_str.trim();
        if trimmed.is_empty() {
            return Ok(Value::Null);
        }

        let pairs = ExprParser::parse(Rule::expression, trimmed)
            .map_err(|e| anyhow!("Expression parse error: {}", e))?;

        let expr_pair = pairs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Empty parse result for '{}'", trimmed))?;

        let ast = self.parse_expression(expr_pair)?;
        self.eval_expr(&ast, resolver as &dyn Fn(&str) -> Option<Value>)
    }

    /// Parse an expression string into an AST node (used internally for f-string interpolation)
    fn parse(&self, expr_str: &str) -> Result<Expr> {
        let trimmed = expr_str.trim();
        let pairs = ExprParser::parse(Rule::expression, trimmed)
            .map_err(|e| anyhow!("F-string expression parse error: {}", e))?;
        let expr_pair = pairs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Empty parse result for f-string expr '{}'", trimmed))?;
        self.parse_expression(expr_pair)
    }

    /// Evaluate and return as bool using Python truthiness
    pub fn _eval_as_bool<F>(&self, expr_str: &str, resolver: &F) -> Result<bool>
    where
        F: Fn(&str) -> Option<Value>,
    {
        let val = self.eval(expr_str, resolver)?;
        Ok(is_truthy(&val))
    }

    /// Evaluate and return as a Vec<Value> for iteration
    pub fn _eval_as_array<F>(&self, expr_str: &str, resolver: &F) -> Result<Vec<Value>>
    where
        F: Fn(&str) -> Option<Value>,
    {
        let val = self.eval(expr_str, resolver)?;
        match val {
            Value::Array(arr) => Ok(arr),
            Value::Null => Ok(vec![]),
            other => Err(anyhow!(
                "Expected array for iteration, got {}",
                type_name(&other)
            )),
        }
    }
}

// ============================================================
// Python-like Truthiness
// ============================================================

pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "None",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.as_f64().map_or("0".to_string(), |f| {
                    if f == f.floor() && f.abs() < 1e15 {
                        format!("{:.0}", f)
                    } else {
                        f.to_string()
                    }
                })
            }
        }
        Value::Array(_) | Value::Object(_) => serde_json::to_string(val).unwrap_or_default(),
    }
}

/// Create a JSON number, preserving integer type when possible
fn json_number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn value_to_f64(val: &Value) -> Result<f64> {
    match val {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| anyhow!("Cannot convert number to f64")),
        Value::String(s) => s
            .parse::<f64>()
            .map_err(|_| anyhow!("Cannot convert '{}' to number", s)),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        _ => Err(anyhow!("Cannot convert {} to number", type_name(val))),
    }
}

// ============================================================
// Parser: Pest Pairs → AST
// ============================================================

impl ExprEvaluator {
    fn parse_expression(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        match pair.as_rule() {
            Rule::expression | Rule::expr => {
                let inner = pair.into_inner().next().unwrap();
                self.parse_expression(inner)
            }
            Rule::coalesce_expr => self.parse_coalesce_expr(pair),
            Rule::pipe_expr => self.parse_pipe_expr(pair),
            Rule::or_expr => self.parse_binary_left(pair, |op| match op.as_str() {
                "or" | "||" => Some(BinOp::Or),
                _ => Option::None,
            }),
            Rule::and_expr => self.parse_binary_left(pair, |op| match op.as_str() {
                "and" | "&&" => Some(BinOp::And),
                _ => Option::None,
            }),
            Rule::in_expr => self.parse_in_expr(pair),
            Rule::cmp_expr => self.parse_binary_left(pair, |op| match op.as_str() {
                "==" => Some(BinOp::Eq),
                "!=" => Some(BinOp::Ne),
                ">" => Some(BinOp::Gt),
                "<" => Some(BinOp::Lt),
                ">=" => Some(BinOp::Ge),
                "<=" => Some(BinOp::Le),
                _ => Option::None,
            }),
            Rule::add_expr => self.parse_binary_left(pair, |op| match op.as_str() {
                "+" => Some(BinOp::Add),
                "-" => Some(BinOp::Sub),
                _ => Option::None,
            }),
            Rule::mul_expr => self.parse_binary_left(pair, |op| match op.as_str() {
                "*" => Some(BinOp::Mul),
                "/" => Some(BinOp::Div),
                "%" => Some(BinOp::Mod),
                _ => Option::None,
            }),
            Rule::unary_expr => self.parse_unary_expr(pair),
            Rule::postfix_expr => self.parse_postfix_expr(pair),
            Rule::atom => {
                let inner = pair.into_inner().next().unwrap();
                self.parse_expression(inner)
            }
            Rule::paren_expr => {
                let inner = pair.into_inner().next().unwrap();
                self.parse_expression(inner)
            }
            Rule::func_call => self.parse_func_call(pair),
            Rule::number => self.parse_number(pair),
            Rule::string => self.parse_string(pair),
            Rule::fstring => self.parse_fstring(pair),
            Rule::bool_lit => {
                let inner = pair.into_inner().next().unwrap();
                match inner.as_rule() {
                    Rule::true_lit => Ok(Expr::Bool(true)),
                    Rule::false_lit => Ok(Expr::Bool(false)),
                    _ => unreachable!(),
                }
            }
            Rule::none_lit => Ok(Expr::None),
            Rule::variable => {
                let var_str = pair.as_str().to_string();
                Ok(Expr::Variable(var_str))
            }
            Rule::identifier => Ok(Expr::Identifier(pair.as_str().to_string())),
            Rule::array_lit => {
                let items: Result<Vec<Expr>> = pair
                    .into_inner()
                    .map(|p| self.parse_expression(p))
                    .collect();
                Ok(Expr::Array(items?))
            }
            Rule::object_lit => {
                let pairs: Result<Vec<(String, Expr)>> = pair
                    .into_inner()
                    .map(|p| {
                        let mut inner = p.into_inner();
                        let key_pair = inner.next().unwrap();
                        let key = match key_pair.as_rule() {
                            Rule::string => self.extract_string_value(key_pair),
                            Rule::identifier => key_pair.as_str().to_string(),
                            _ => key_pair.as_str().to_string(),
                        };
                        let val = self.parse_expression(inner.next().unwrap())?;
                        Ok((key, val))
                    })
                    .collect();
                Ok(Expr::Object(pairs?))
            }
            _ => Err(anyhow!(
                "Unexpected rule {:?}: '{}'",
                pair.as_rule(),
                pair.as_str()
            )),
        }
    }

    fn parse_coalesce_expr(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let mut expr = self.parse_expression(inner.next().unwrap())?;

        while inner.peek().is_some() {
            let op = inner.next().unwrap();
            if op.as_rule() == Rule::coalesce_op {
                let right = self.parse_expression(inner.next().unwrap())?;
                expr = Expr::Coalesce {
                    left: Box::new(expr),
                    right: Box::new(right),
                };
            }
        }
        Ok(expr)
    }

    fn parse_pipe_expr(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let mut expr = self.parse_expression(inner.next().unwrap())?;

        for filter_pair in inner {
            if filter_pair.as_rule() == Rule::pipe_filter {
                let mut filter_inner = filter_pair.into_inner();
                let name = filter_inner.next().unwrap().as_str().to_string();
                let args = if let Some(args_pair) = filter_inner.next() {
                    self.parse_call_args(args_pair)?
                } else {
                    vec![]
                };
                expr = Expr::Pipe {
                    value: Box::new(expr),
                    filter: name,
                    args,
                };
            }
        }
        Ok(expr)
    }

    fn parse_binary_left<F>(&self, pair: pest::iterators::Pair<Rule>, op_map: F) -> Result<Expr>
    where
        F: Fn(&pest::iterators::Pair<Rule>) -> Option<BinOp>,
    {
        let mut inner = pair.into_inner();
        let mut left = self.parse_expression(inner.next().unwrap())?;

        while let Some(op_pair) = inner.next() {
            if let Some(op) = op_map(&op_pair) {
                let right = self.parse_expression(inner.next().unwrap())?;
                left = Expr::BinaryOp {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                };
            }
        }
        Ok(left)
    }

    fn parse_in_expr(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let left = self.parse_expression(inner.next().unwrap())?;

        if let Some(op_pair) = inner.next() {
            let op = if op_pair.as_str().contains("not") {
                BinOp::NotIn
            } else {
                BinOp::In
            };
            let right = self.parse_expression(inner.next().unwrap())?;
            Ok(Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        } else {
            Ok(left)
        }
    }

    fn parse_unary_expr(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let first = inner.next().unwrap();

        match first.as_rule() {
            Rule::not_op => {
                let operand = self.parse_expression(inner.next().unwrap())?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                })
            }
            Rule::neg_op => {
                let operand = self.parse_expression(inner.next().unwrap())?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                })
            }
            _ => self.parse_expression(first),
        }
    }

    fn parse_postfix_expr(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let mut expr = self.parse_expression(inner.next().unwrap())?;

        for postfix in inner {
            match postfix.as_rule() {
                Rule::method_call => {
                    let mut mc_inner = postfix.into_inner();
                    let method = mc_inner.next().unwrap().as_str().to_string();
                    let args = if let Some(args_pair) = mc_inner.next() {
                        self.parse_call_args(args_pair)?
                    } else {
                        vec![]
                    };
                    expr = Expr::MethodCall {
                        object: Box::new(expr),
                        method,
                        args,
                    };
                }
                Rule::dot_access => {
                    let field = postfix.into_inner().next().unwrap().as_str().to_string();
                    expr = Expr::DotAccess {
                        object: Box::new(expr),
                        field,
                    };
                }
                Rule::bracket_access => {
                    let index = self.parse_expression(postfix.into_inner().next().unwrap())?;
                    expr = Expr::BracketAccess {
                        object: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                _ => {}
            }
        }
        Ok(expr)
    }

    fn parse_func_call(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let name = inner.next().unwrap().as_str().to_string();
        let args = if let Some(args_pair) = inner.next() {
            self.parse_call_args(args_pair)?
        } else {
            vec![]
        };
        Ok(Expr::FuncCall { name, args })
    }

    fn parse_call_args(&self, pair: pest::iterators::Pair<Rule>) -> Result<Vec<Expr>> {
        pair.into_inner()
            .map(|p| match p.as_rule() {
                Rule::call_arg => {
                    let inner = p.into_inner().next().unwrap();
                    match inner.as_rule() {
                        Rule::lambda_expr => self.parse_lambda(inner),
                        _ => self.parse_expression(inner),
                    }
                }
                _ => self.parse_expression(p),
            })
            .collect()
    }

    fn parse_lambda(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let mut inner = pair.into_inner();
        let params_pair = inner.next().unwrap();
        let params = self.parse_lambda_params(params_pair)?;
        let body = self.parse_expression(inner.next().unwrap())?;
        Ok(Expr::Lambda {
            params,
            body: Box::new(body),
        })
    }

    fn parse_lambda_params(&self, pair: pest::iterators::Pair<Rule>) -> Result<Vec<String>> {
        let inner = pair.into_inner().next().unwrap();
        match inner.as_rule() {
            Rule::lambda_params_single => Ok(vec![inner.as_str().to_string()]),
            Rule::lambda_params_multi => {
                Ok(inner.into_inner().map(|p| p.as_str().to_string()).collect())
            }
            _ => Err(anyhow!(
                "Unexpected lambda params rule: {:?}",
                inner.as_rule()
            )),
        }
    }

    fn parse_number(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let inner = pair.into_inner().next().unwrap();
        let s = inner.as_str();
        let n: f64 = s.parse().map_err(|_| anyhow!("Invalid number: '{}'", s))?;
        Ok(Expr::Number(n))
    }

    fn parse_string(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let s = self.extract_string_value(pair.into_inner().next().unwrap());
        Ok(Expr::String(s))
    }

    fn extract_string_value(&self, pair: pest::iterators::Pair<Rule>) -> String {
        let raw = pair.as_str();
        // Triple-quoted: raw string, no escape processing
        if raw.starts_with("\"\"\"") {
            return raw[3..raw.len() - 3].to_string();
        }
        // Strip surrounding quotes
        let inner = &raw[1..raw.len() - 1];
        // Process escape sequences
        let mut result = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    match next {
                        'n' => result.push('\n'),
                        'r' => result.push('\r'),
                        't' => result.push('\t'),
                        '\\' => result.push('\\'),
                        '"' => result.push('"'),
                        '\'' => result.push('\''),
                        other => {
                            result.push('\\');
                            result.push(other);
                        }
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    fn parse_fstring(&self, pair: pest::iterators::Pair<Rule>) -> Result<Expr> {
        let inner_pair = pair.into_inner().next();
        let is_triple = inner_pair
            .as_ref()
            .map(|p| p.as_rule() == Rule::fstring_triple_body)
            .unwrap_or(false);
        let body = inner_pair.map(|p| p.as_str()).unwrap_or("");

        let mut parts = Vec::new();
        let mut chars = body.chars().peekable();
        let mut text_buf = String::new();

        while let Some(c) = chars.next() {
            match c {
                '\\' if !is_triple => {
                    if let Some(&next) = chars.peek() {
                        chars.next();
                        match next {
                            'n' => text_buf.push('\n'),
                            'r' => text_buf.push('\r'),
                            't' => text_buf.push('\t'),
                            '\\' => text_buf.push('\\'),
                            '"' => text_buf.push('"'),
                            other => {
                                text_buf.push('\\');
                                text_buf.push(other);
                            }
                        }
                    }
                }
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    text_buf.push('{');
                }
                '{' => {
                    // Flush text buffer
                    if !text_buf.is_empty() {
                        parts.push(FStringPart::Text(std::mem::take(&mut text_buf)));
                    }
                    // Extract expression until matching '}'
                    let mut depth = 1;
                    let mut expr_str = String::new();
                    for ch in chars.by_ref() {
                        match ch {
                            '{' => {
                                depth += 1;
                                expr_str.push(ch);
                            }
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                expr_str.push(ch);
                            }
                            _ => expr_str.push(ch),
                        }
                    }
                    // Parse expression string
                    let expr = self.parse(&expr_str)?;
                    parts.push(FStringPart::Expr(expr));
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    text_buf.push('}');
                }
                _ => text_buf.push(c),
            }
        }

        // Flush remaining text
        if !text_buf.is_empty() {
            parts.push(FStringPart::Text(text_buf));
        }

        Ok(Expr::FString(parts))
    }
}

// ============================================================
// Evaluator: AST → Value
// ============================================================

impl ExprEvaluator {
    fn eval_expr(&self, expr: &Expr, resolver: &dyn Fn(&str) -> Option<Value>) -> Result<Value> {
        match expr {
            Expr::Number(n) => {
                // Preserve integer type when the f64 is a whole number
                if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    Ok(json!(*n as i64))
                } else {
                    Ok(json!(*n))
                }
            }
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::FString(parts) => {
                let mut buf = String::new();
                for part in parts {
                    match part {
                        FStringPart::Text(t) => buf.push_str(t),
                        FStringPart::Expr(e) => {
                            let val = self.eval_expr(e, resolver)?;
                            match &val {
                                Value::String(s) => buf.push_str(s),
                                Value::Null => buf.push_str("None"),
                                _ => buf.push_str(&val.to_string()),
                            }
                        }
                    }
                }
                Ok(Value::String(buf))
            }
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::None => Ok(Value::Null),

            Expr::Array(items) => {
                let vals: Result<Vec<Value>> =
                    items.iter().map(|e| self.eval_expr(e, resolver)).collect();
                Ok(Value::Array(vals?))
            }
            Expr::Object(pairs) => {
                let mut map = serde_json::Map::new();
                for (k, v) in pairs {
                    map.insert(k.clone(), self.eval_expr(v, resolver)?);
                }
                Ok(Value::Object(map))
            }

            Expr::Variable(var) => {
                // Strip leading $ and resolve via context
                let path = var.trim_start_matches('$');
                Ok(resolver(path).unwrap_or(Value::Null))
            }

            Expr::Identifier(name) => {
                // Bare identifiers are looked up in resolver scope
                Ok(resolver(name).unwrap_or(Value::Null))
            }

            Expr::BinaryOp { left, op, right } => self.eval_binary_op(left, *op, right, resolver),

            Expr::UnaryOp { op, operand } => {
                let val = self.eval_expr(operand, resolver)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!is_truthy(&val))),
                    UnaryOp::Neg => {
                        let n = value_to_f64(&val)?;
                        Ok(json_number(-n))
                    }
                }
            }

            Expr::DotAccess { object, field } => {
                let obj_val = self.eval_expr(object, resolver)?;
                Ok(access_field(&obj_val, field))
            }

            Expr::BracketAccess { object, index } => {
                let obj_val = self.eval_expr(object, resolver)?;
                let idx_val = self.eval_expr(index, resolver)?;
                Ok(access_bracket(&obj_val, &idx_val))
            }

            Expr::FuncCall { name, args } => {
                if args.iter().any(|a| matches!(a, Expr::Lambda { .. })) {
                    self.eval_higher_order_call(name, args, resolver)
                } else {
                    let arg_vals: Result<Vec<Value>> =
                        args.iter().map(|a| self.eval_expr(a, resolver)).collect();
                    call_builtin(name, &arg_vals?)
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                if args.iter().any(|a| matches!(a, Expr::Lambda { .. })) {
                    // Desugar: obj.method(lambda) → method(obj, lambda)
                    let mut all_args = vec![object.as_ref().clone()];
                    all_args.extend(args.iter().cloned());
                    self.eval_higher_order_call(method, &all_args, resolver)
                } else {
                    let obj_val = self.eval_expr(object, resolver)?;
                    let mut all_args = vec![obj_val];
                    for a in args {
                        all_args.push(self.eval_expr(a, resolver)?);
                    }
                    call_builtin(method, &all_args)
                }
            }

            Expr::Pipe {
                value,
                filter,
                args,
            } => {
                if args.iter().any(|a| matches!(a, Expr::Lambda { .. })) {
                    // Desugar: value | filter(lambda) → filter(value, lambda)
                    let mut all_args = vec![value.as_ref().clone()];
                    all_args.extend(args.iter().cloned());
                    self.eval_higher_order_call(filter, &all_args, resolver)
                } else {
                    let val = self.eval_expr(value, resolver)?;
                    let mut all_args = vec![val];
                    for a in args {
                        all_args.push(self.eval_expr(a, resolver)?);
                    }
                    call_builtin(filter, &all_args)
                }
            }

            Expr::Coalesce { left, right } => {
                match self.eval_expr(left, resolver) {
                    Ok(val) => {
                        // null → fallback; { err: ... } → fallback; anything else → keep
                        if val.is_null() {
                            self.eval_expr(right, resolver)
                        } else if val.is_object()
                            && val.as_object().unwrap().contains_key("err")
                        {
                            self.eval_expr(right, resolver)
                        } else {
                            Ok(val)
                        }
                    }
                    Err(_) => self.eval_expr(right, resolver),
                }
            }

            Expr::Lambda { .. } => {
                Err(anyhow!("Lambda expressions can only be used as arguments to higher-order functions (map, filter, reduce, etc.)"))
            }
        }
    }

    fn eval_binary_op(
        &self,
        left_expr: &Expr,
        op: BinOp,
        right_expr: &Expr,
        resolver: &dyn Fn(&str) -> Option<Value>,
    ) -> Result<Value> {
        // Short-circuit for and/or
        match op {
            BinOp::And => {
                let left = self.eval_expr(left_expr, resolver)?;
                if !is_truthy(&left) {
                    return Ok(left);
                }
                return self.eval_expr(right_expr, resolver);
            }
            BinOp::Or => {
                let left = self.eval_expr(left_expr, resolver)?;
                if is_truthy(&left) {
                    return Ok(left);
                }
                return self.eval_expr(right_expr, resolver);
            }
            _ => {}
        }

        let left = self.eval_expr(left_expr, resolver)?;
        let right = self.eval_expr(right_expr, resolver)?;

        match op {
            BinOp::Eq => Ok(Value::Bool(values_equal(&left, &right))),
            BinOp::Ne => Ok(Value::Bool(!values_equal(&left, &right))),

            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => eval_comparison(&left, op, &right),

            BinOp::Add => eval_add(&left, &right),
            BinOp::Sub => {
                let l = value_to_f64(&left)?;
                let r = value_to_f64(&right)?;
                Ok(json_number(l - r))
            }
            BinOp::Mul => {
                let l = value_to_f64(&left)?;
                let r = value_to_f64(&right)?;
                Ok(json_number(l * r))
            }
            BinOp::Div => {
                let l = value_to_f64(&left)?;
                let r = value_to_f64(&right)?;
                if r == 0.0 {
                    return Err(anyhow!("Division by zero"));
                }
                Ok(json_number(l / r))
            }
            BinOp::Mod => {
                let l = value_to_f64(&left)?;
                let r = value_to_f64(&right)?;
                if r == 0.0 {
                    return Err(anyhow!("Modulo by zero"));
                }
                Ok(json_number(l % r))
            }

            BinOp::In => Ok(Value::Bool(value_in(&left, &right))),
            BinOp::NotIn => Ok(Value::Bool(!value_in(&left, &right))),

            BinOp::And | BinOp::Or => unreachable!(), // handled above
        }
    }

    // ============================================================
    // Higher-Order Functions (lambda support)
    // ============================================================

    /// Dispatch higher-order function calls that contain lambda arguments.
    fn eval_higher_order_call(
        &self,
        name: &str,
        args: &[Expr],
        resolver: &dyn Fn(&str) -> Option<Value>,
    ) -> Result<Value> {
        match name {
            "map" => {
                if args.len() != 2 {
                    return Err(anyhow!("map() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        let results: Result<Vec<Value>> = arr
                            .iter()
                            .map(|item| {
                                self.apply_lambda(
                                    params,
                                    body,
                                    std::slice::from_ref(item),
                                    resolver,
                                )
                            })
                            .collect();
                        Ok(Value::Array(results?))
                    }
                    Value::Object(obj) => {
                        let results: Result<Vec<Value>> = obj
                            .iter()
                            .map(|(k, v)| {
                                let entry = json!([k, v]);
                                self.apply_lambda(params, body, &[entry], resolver)
                            })
                            .collect();
                        Ok(Value::Array(results?))
                    }
                    _ => Err(anyhow!(
                        "map() expects list or dict, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "filter" => {
                if args.len() != 2 {
                    return Err(anyhow!("filter() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        let mut results = Vec::new();
                        for item in arr {
                            let test = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            if is_truthy(&test) {
                                results.push(item.clone());
                            }
                        }
                        Ok(Value::Array(results))
                    }
                    _ => Err(anyhow!(
                        "filter() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "reduce" => {
                if args.len() != 3 {
                    return Err(anyhow!(
                        "reduce() expects 3 arguments (list, lambda, initial), got {}",
                        args.len()
                    ));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                if params.len() != 2 {
                    return Err(anyhow!(
                        "reduce() lambda must have 2 parameters (acc, item), got {}",
                        params.len()
                    ));
                }
                let initial = self.eval_expr(&args[2], resolver)?;
                match &collection {
                    Value::Array(arr) => {
                        let mut acc = initial;
                        for item in arr {
                            acc =
                                self.apply_lambda(params, body, &[acc, item.clone()], resolver)?;
                        }
                        Ok(acc)
                    }
                    _ => Err(anyhow!(
                        "reduce() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "sort_by" => {
                if args.len() != 2 {
                    return Err(anyhow!("sort_by() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        // Compute keys for each element
                        let mut keyed: Vec<(Value, Value)> = Vec::new();
                        for item in arr {
                            let key = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            keyed.push((key, item.clone()));
                        }
                        keyed.sort_by(|(a, _), (b, _)| match (a.as_f64(), b.as_f64()) {
                            (Some(fa), Some(fb)) => {
                                fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
                            }
                            _ => value_to_string(a).cmp(&value_to_string(b)),
                        });
                        Ok(Value::Array(keyed.into_iter().map(|(_, v)| v).collect()))
                    }
                    _ => Err(anyhow!(
                        "sort_by() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "find_by" => {
                if args.len() != 2 {
                    return Err(anyhow!("find_by() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        for item in arr {
                            let test = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            if is_truthy(&test) {
                                return Ok(item.clone());
                            }
                        }
                        Ok(Value::Null)
                    }
                    _ => Err(anyhow!(
                        "find_by() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "group_by" => {
                if args.len() != 2 {
                    return Err(anyhow!(
                        "group_by() expects 2 arguments, got {}",
                        args.len()
                    ));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        let mut groups: serde_json::Map<String, Value> = serde_json::Map::new();
                        for item in arr {
                            let key = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            let key_str = value_to_string(&key);
                            let group = groups.entry(key_str).or_insert_with(|| json!([]));
                            group.as_array_mut().unwrap().push(item.clone());
                        }
                        Ok(Value::Object(groups))
                    }
                    _ => Err(anyhow!(
                        "group_by() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "flat_map" => {
                if args.len() != 2 {
                    return Err(anyhow!(
                        "flat_map() expects 2 arguments, got {}",
                        args.len()
                    ));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        let mut results = Vec::new();
                        for item in arr {
                            let mapped = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            match mapped {
                                Value::Array(inner) => results.extend(inner),
                                other => results.push(other),
                            }
                        }
                        Ok(Value::Array(results))
                    }
                    _ => Err(anyhow!(
                        "flat_map() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "count_by" => {
                if args.len() != 2 {
                    return Err(anyhow!(
                        "count_by() expects 2 arguments, got {}",
                        args.len()
                    ));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        let mut counts: serde_json::Map<String, Value> = serde_json::Map::new();
                        for item in arr {
                            let key = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            let key_str = value_to_string(&key);
                            let count = counts.entry(key_str).or_insert_with(|| json!(0));
                            *count = json!(count.as_i64().unwrap_or(0) + 1);
                        }
                        Ok(Value::Object(counts))
                    }
                    _ => Err(anyhow!(
                        "count_by() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "min_by" => {
                if args.len() != 2 {
                    return Err(anyhow!("min_by() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) if arr.is_empty() => Ok(Value::Null),
                    Value::Array(arr) => {
                        let mut best = &arr[0];
                        let mut best_key =
                            self.apply_lambda(params, body, &[arr[0].clone()], resolver)?;
                        for item in &arr[1..] {
                            let key = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            let is_less = match (key.as_f64(), best_key.as_f64()) {
                                (Some(a), Some(b)) => a < b,
                                _ => value_to_string(&key) < value_to_string(&best_key),
                            };
                            if is_less {
                                best = item;
                                best_key = key;
                            }
                        }
                        Ok(best.clone())
                    }
                    _ => Err(anyhow!(
                        "min_by() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "max_by" => {
                if args.len() != 2 {
                    return Err(anyhow!("max_by() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) if arr.is_empty() => Ok(Value::Null),
                    Value::Array(arr) => {
                        let mut best = &arr[0];
                        let mut best_key =
                            self.apply_lambda(params, body, &[arr[0].clone()], resolver)?;
                        for item in &arr[1..] {
                            let key = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            let is_greater = match (key.as_f64(), best_key.as_f64()) {
                                (Some(a), Some(b)) => a > b,
                                _ => value_to_string(&key) > value_to_string(&best_key),
                            };
                            if is_greater {
                                best = item;
                                best_key = key;
                            }
                        }
                        Ok(best.clone())
                    }
                    _ => Err(anyhow!(
                        "max_by() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "every" => {
                if args.len() != 2 {
                    return Err(anyhow!("every() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        for item in arr {
                            let test = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            if !is_truthy(&test) {
                                return Ok(json!(false));
                            }
                        }
                        Ok(json!(true))
                    }
                    _ => Err(anyhow!(
                        "every() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            "some" => {
                if args.len() != 2 {
                    return Err(anyhow!("some() expects 2 arguments, got {}", args.len()));
                }
                let collection = self.eval_expr(&args[0], resolver)?;
                let (params, body) = self.expect_lambda(&args[1])?;
                match &collection {
                    Value::Array(arr) => {
                        for item in arr {
                            let test = self.apply_lambda(
                                params,
                                body,
                                std::slice::from_ref(item),
                                resolver,
                            )?;
                            if is_truthy(&test) {
                                return Ok(json!(true));
                            }
                        }
                        Ok(json!(false))
                    }
                    _ => Err(anyhow!(
                        "some() expects list, got {}",
                        type_name(&collection)
                    )),
                }
            }

            _ => Err(anyhow!("{}() does not accept lambda arguments", name)),
        }
    }

    /// Extract lambda parameters and body from an Expr::Lambda
    fn expect_lambda<'a>(&self, expr: &'a Expr) -> Result<(&'a [String], &'a Expr)> {
        match expr {
            Expr::Lambda { params, body } => Ok((params.as_slice(), body.as_ref())),
            _ => Err(anyhow!("Expected lambda expression")),
        }
    }

    /// Evaluate a lambda body with bound parameters.
    /// Creates a scoped resolver that binds lambda params, then falls through to outer scope.
    fn apply_lambda(
        &self,
        params: &[String],
        body: &Expr,
        values: &[Value],
        outer: &dyn Fn(&str) -> Option<Value>,
    ) -> Result<Value> {
        if params.len() != values.len() {
            return Err(anyhow!(
                "Lambda expects {} parameter(s), got {}",
                params.len(),
                values.len()
            ));
        }

        let lambda_resolver = |path: &str| -> Option<Value> {
            // Check lambda parameter bindings first
            for (name, val) in params.iter().zip(values.iter()) {
                if path == name {
                    return Some(val.clone());
                }
                // Support dot access on lambda params: x.field
                if let Some(rest) = path
                    .strip_prefix(name.as_str())
                    .and_then(|s| s.strip_prefix('.'))
                {
                    let mut current = val.clone();
                    for part in rest.split('.') {
                        current = match &current {
                            Value::Object(map) => map.get(part).cloned().unwrap_or(Value::Null),
                            Value::Array(arr) => {
                                if let Ok(idx) = part.parse::<usize>() {
                                    arr.get(idx).cloned().unwrap_or(Value::Null)
                                } else {
                                    Value::Null
                                }
                            }
                            _ => Value::Null,
                        };
                    }
                    return Some(current);
                }
            }
            // Fall through to outer scope (captures $ctx, $input, etc.)
            outer(path)
        };

        self.eval_expr(body, &lambda_resolver)
    }
}

// ============================================================
// Comparison Helpers
// ============================================================

fn values_equal(a: &Value, b: &Value) -> bool {
    // Direct serde_json equality — type-safe, no Rhai unit confusion
    a == b
}

fn eval_comparison(left: &Value, op: BinOp, right: &Value) -> Result<Value> {
    // Try numeric comparison first
    if let (Ok(l), Ok(r)) = (value_to_f64(left), value_to_f64(right)) {
        // But only if at least one side is actually a number (not string-to-number coercion)
        if left.is_number() || right.is_number() {
            let result = match op {
                BinOp::Lt => l < r,
                BinOp::Gt => l > r,
                BinOp::Le => l <= r,
                BinOp::Ge => l >= r,
                _ => unreachable!(),
            };
            return Ok(Value::Bool(result));
        }
    }

    // String comparison
    if let (Value::String(l), Value::String(r)) = (left, right) {
        let result = match op {
            BinOp::Lt => l < r,
            BinOp::Gt => l > r,
            BinOp::Le => l <= r,
            BinOp::Ge => l >= r,
            _ => unreachable!(),
        };
        return Ok(Value::Bool(result));
    }

    // Null comparisons always false for ordering
    if left.is_null() || right.is_null() {
        return Ok(Value::Bool(false));
    }

    Err(anyhow!(
        "Cannot compare {} with {}",
        type_name(left),
        type_name(right)
    ))
}

fn eval_add(left: &Value, right: &Value) -> Result<Value> {
    // String concatenation: if either side is a string, concatenate
    match (left, right) {
        (Value::String(l), _) => Ok(Value::String(format!("{}{}", l, value_to_string(right)))),
        (_, Value::String(r)) => Ok(Value::String(format!("{}{}", value_to_string(left), r))),
        // Array concatenation
        (Value::Array(l), Value::Array(r)) => {
            let mut result = l.clone();
            result.extend(r.clone());
            Ok(Value::Array(result))
        }
        // Numeric addition
        _ => {
            let l = value_to_f64(left)?;
            let r = value_to_f64(right)?;
            Ok(json_number(l + r))
        }
    }
}

fn value_in(needle: &Value, haystack: &Value) -> bool {
    match haystack {
        Value::Array(arr) => arr.iter().any(|item| values_equal(needle, item)),
        Value::String(s) => {
            if let Value::String(n) = needle {
                s.contains(n.as_str())
            } else {
                false
            }
        }
        Value::Object(obj) => {
            if let Value::String(key) = needle {
                obj.contains_key(key)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn access_field(obj: &Value, field: &str) -> Value {
    match obj {
        Value::Object(map) => map.get(field).cloned().unwrap_or(Value::Null),
        Value::Array(arr) => {
            // Support array.length
            if field == "length" {
                json!(arr.len())
            } else {
                Value::Null
            }
        }
        Value::String(s) => {
            if field == "length" {
                json!(s.len())
            } else if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                // 字符串是 JSON 编码的对象/数组，先解析再导航
                access_field(&parsed, field)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

fn access_bracket(obj: &Value, index: &Value) -> Value {
    match obj {
        Value::Array(arr) => {
            if let Some(i) = index.as_i64().or_else(|| index.as_f64().map(|f| f as i64)) {
                let idx = if i < 0 {
                    (arr.len() as i64 + i) as usize
                } else {
                    i as usize
                };
                arr.get(idx).cloned().unwrap_or(Value::Null)
            } else if let Some(s) = index.as_str() {
                // Try parsing as integer
                if let Ok(i) = s.parse::<usize>() {
                    arr.get(i).cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                }
            } else {
                Value::Null
            }
        }
        Value::Object(map) => {
            if let Some(key) = index.as_str() {
                map.get(key).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

// ============================================================
// Built-in Functions
// ============================================================

fn call_builtin(name: &str, args: &[Value]) -> Result<Value> {
    match name {
        "len" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::String(s) => Ok(json!(s.chars().count())),
                Value::Array(a) => Ok(json!(a.len())),
                Value::Object(o) => Ok(json!(o.len())),
                _ => Err(anyhow!(
                    "len() expects str, list, or dict, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "str" => {
            require_args(name, args, 1)?;
            Ok(Value::String(value_to_string(&args[0])))
        }

        "int" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Number(n) => Ok(json!(n
                    .as_i64()
                    .unwrap_or(n.as_f64().unwrap_or(0.0) as i64))),
                Value::String(s) => {
                    let i: i64 = s
                        .parse()
                        .map_err(|_| anyhow!("Cannot convert '{}' to int", s))?;
                    Ok(json!(i))
                }
                Value::Bool(b) => Ok(json!(if *b { 1 } else { 0 })),
                Value::Null => Ok(json!(0)),
                _ => Err(anyhow!("int() cannot convert {}", type_name(&args[0]))),
            }
        }

        "float" => {
            require_args(name, args, 1)?;
            let f = value_to_f64(&args[0])?;
            Ok(json!(f))
        }

        "bool" => {
            require_args(name, args, 1)?;
            Ok(Value::Bool(is_truthy(&args[0])))
        }

        "type" => {
            require_args(name, args, 1)?;
            Ok(Value::String(type_name(&args[0]).to_string()))
        }

        "abs" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])?;
            Ok(json!(n.abs()))
        }

        "min" => {
            require_min_args(name, args, 2)?;
            let mut result = value_to_f64(&args[0])?;
            for a in &args[1..] {
                let v = value_to_f64(a)?;
                if v < result {
                    result = v;
                }
            }
            Ok(json!(result))
        }

        "max" => {
            require_min_args(name, args, 2)?;
            let mut result = value_to_f64(&args[0])?;
            for a in &args[1..] {
                let v = value_to_f64(a)?;
                if v > result {
                    result = v;
                }
            }
            Ok(json!(result))
        }

        "round" => {
            require_min_args(name, args, 1)?;
            let val = value_to_f64(&args[0])?;
            let digits = if args.len() > 1 {
                value_to_f64(&args[1])? as i32
            } else {
                0
            };
            let p = 10f64.powi(digits);
            Ok(json!((val * p).round() / p))
        }

        "truncate" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let len = value_to_f64(&args[1])? as usize;
            if s.chars().count() <= len {
                Ok(Value::String(s))
            } else {
                let truncated: String = s.chars().take(len).collect();
                Ok(Value::String(format!("{}...", truncated)))
            }
        }

        "upper" => {
            require_args(name, args, 1)?;
            Ok(Value::String(value_to_string(&args[0]).to_uppercase()))
        }

        "lower" => {
            require_args(name, args, 1)?;
            Ok(Value::String(value_to_string(&args[0]).to_lowercase()))
        }

        "default" => {
            require_args(name, args, 2)?;
            if args[0].is_null() || (args[0].is_string() && args[0].as_str().unwrap().is_empty()) {
                Ok(args[1].clone())
            } else {
                Ok(args[0].clone())
            }
        }

        "json" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::String(s) => {
                    // 字符串输入 → 解析 JSON（decode 方向）
                    Ok(serde_json::from_str(s).unwrap_or_else(|_| args[0].clone()))
                }
                other => {
                    // 非字符串 → 序列化为 JSON 字符串（encode 方向）
                    Ok(Value::String(
                        serde_json::to_string(other).unwrap_or_default(),
                    ))
                }
            }
        }

        "keys" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Object(obj) => Ok(Value::Array(obj.keys().map(|k| json!(k)).collect())),
                _ => Err(anyhow!("keys() expects dict, got {}", type_name(&args[0]))),
            }
        }

        "values" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Object(obj) => Ok(Value::Array(obj.values().cloned().collect())),
                _ => Err(anyhow!(
                    "values() expects dict, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "contains" => {
            require_args(name, args, 2)?;
            Ok(Value::Bool(value_in(&args[1], &args[0])))
        }

        "append" => {
            require_args(name, args, 2)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut result = arr.clone();
                    result.push(args[1].clone());
                    Ok(Value::Array(result))
                }
                _ => Err(anyhow!(
                    "append() expects list as first argument, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "join" => {
            require_args(name, args, 2)?;
            match &args[0] {
                Value::Array(arr) => {
                    let sep = value_to_string(&args[1]);
                    let parts: Vec<String> = arr.iter().map(value_to_string).collect();
                    Ok(Value::String(parts.join(&sep)))
                }
                _ => Err(anyhow!("join() expects list, got {}", type_name(&args[0]))),
            }
        }

        "split" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let sep = value_to_string(&args[1]);
            let parts: Vec<Value> = s.split(&sep).map(|p| json!(p)).collect();
            Ok(Value::Array(parts))
        }

        "replace" => {
            require_args(name, args, 3)?;
            let s = value_to_string(&args[0]);
            let old = value_to_string(&args[1]);
            let new = value_to_string(&args[2]);
            Ok(Value::String(s.replace(&old, &new)))
        }

        "startswith" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let prefix = value_to_string(&args[1]);
            Ok(Value::Bool(s.starts_with(&prefix)))
        }

        "endswith" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let suffix = value_to_string(&args[1]);
            Ok(Value::Bool(s.ends_with(&suffix)))
        }

        "range" => {
            if args.is_empty() || args.len() > 3 {
                return Err(anyhow!("range() expects 1-3 arguments, got {}", args.len()));
            }
            let (start, end, step) = match args.len() {
                1 => (0i64, value_to_f64(&args[0])? as i64, 1i64),
                2 => (
                    value_to_f64(&args[0])? as i64,
                    value_to_f64(&args[1])? as i64,
                    1i64,
                ),
                3 => (
                    value_to_f64(&args[0])? as i64,
                    value_to_f64(&args[1])? as i64,
                    value_to_f64(&args[2])? as i64,
                ),
                _ => unreachable!(),
            };
            if step == 0 {
                return Err(anyhow!("range() step cannot be zero"));
            }
            let mut result = vec![];
            let mut i = start;
            if step > 0 {
                while i < end {
                    result.push(json!(i));
                    i += step;
                }
            } else {
                while i > end {
                    result.push(json!(i));
                    i += step;
                }
            }
            Ok(Value::Array(result))
        }

        "strip" | "trim" => {
            require_args(name, args, 1)?;
            Ok(Value::String(value_to_string(&args[0]).trim().to_string()))
        }

        // ============================================================
        // String operations (extended)
        // ============================================================
        "find" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let sub = value_to_string(&args[1]);
            match s.find(&sub) {
                Some(byte_pos) => {
                    // Convert byte position to char position
                    let char_pos = s[..byte_pos].chars().count();
                    Ok(json!(char_pos as i64))
                }
                None => Ok(json!(-1)),
            }
        }

        "slice" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(anyhow!("slice() expects 2-3 arguments, got {}", args.len()));
            }
            let start = value_to_f64(&args[1])? as i64;
            let end = if args.len() == 3 {
                Some(value_to_f64(&args[2])? as i64)
            } else {
                None
            };
            match &args[0] {
                Value::String(s) => {
                    let len = s.chars().count() as i64;
                    let s_start = normalize_index(start, len) as usize;
                    let s_end = end.map_or(len as usize, |e| normalize_index(e, len) as usize);
                    let sliced: String = s
                        .chars()
                        .skip(s_start)
                        .take(s_end.saturating_sub(s_start))
                        .collect();
                    Ok(Value::String(sliced))
                }
                Value::Array(arr) => {
                    let len = arr.len() as i64;
                    let s_start = normalize_index(start, len) as usize;
                    let s_end = end.map_or(len as usize, |e| normalize_index(e, len) as usize);
                    let sliced: Vec<Value> = arr
                        .iter()
                        .skip(s_start)
                        .take(s_end.saturating_sub(s_start))
                        .cloned()
                        .collect();
                    Ok(Value::Array(sliced))
                }
                _ => Err(anyhow!(
                    "slice() expects str or list, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "count" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let sub = value_to_string(&args[1]);
            if sub.is_empty() {
                return Err(anyhow!("count() substring cannot be empty"));
            }
            Ok(json!(s.matches(&sub).count()))
        }

        "capitalize" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let cap = if s.is_empty() {
                s
            } else {
                let mut chars = s.chars();
                let first: String = chars.next().unwrap().to_uppercase().collect();
                format!("{}{}", first, chars.as_str().to_lowercase())
            };
            Ok(Value::String(cap))
        }

        "title" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let titled: String = s
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        Some(c) => {
                            let first: String = c.to_uppercase().collect();
                            format!("{}{}", first, chars.as_str().to_lowercase())
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            Ok(Value::String(titled))
        }

        "lpad" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(anyhow!("lpad() expects 2-3 arguments, got {}", args.len()));
            }
            let s = value_to_string(&args[0]);
            let width = value_to_f64(&args[1])? as usize;
            let pad_char = if args.len() == 3 {
                let p = value_to_string(&args[2]);
                p.chars().next().unwrap_or(' ')
            } else {
                ' '
            };
            let char_count = s.chars().count();
            if char_count >= width {
                Ok(Value::String(s))
            } else {
                let padding: String = std::iter::repeat(pad_char)
                    .take(width - char_count)
                    .collect();
                Ok(Value::String(format!("{}{}", padding, s)))
            }
        }

        "rpad" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(anyhow!("rpad() expects 2-3 arguments, got {}", args.len()));
            }
            let s = value_to_string(&args[0]);
            let width = value_to_f64(&args[1])? as usize;
            let pad_char = if args.len() == 3 {
                let p = value_to_string(&args[2]);
                p.chars().next().unwrap_or(' ')
            } else {
                ' '
            };
            let char_count = s.chars().count();
            if char_count >= width {
                Ok(Value::String(s))
            } else {
                let padding: String = std::iter::repeat(pad_char)
                    .take(width - char_count)
                    .collect();
                Ok(Value::String(format!("{}{}", s, padding)))
            }
        }

        "repeat" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let n = value_to_f64(&args[1])? as usize;
            Ok(Value::String(s.repeat(n)))
        }

        // ============================================================
        // Collection operations
        // ============================================================
        "sort" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut sorted = arr.clone();
                    sorted.sort_by(|a, b| {
                        // Numbers first, then strings, then by JSON repr
                        match (a.as_f64(), b.as_f64()) {
                            (Some(fa), Some(fb)) => {
                                fa.partial_cmp(&fb).unwrap_or(std::cmp::Ordering::Equal)
                            }
                            _ => value_to_string(a).cmp(&value_to_string(b)),
                        }
                    });
                    Ok(Value::Array(sorted))
                }
                _ => Err(anyhow!("sort() expects list, got {}", type_name(&args[0]))),
            }
        }

        "reverse" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut rev = arr.clone();
                    rev.reverse();
                    Ok(Value::Array(rev))
                }
                Value::String(s) => Ok(Value::String(s.chars().rev().collect())),
                _ => Err(anyhow!(
                    "reverse() expects list or str, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "unique" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut seen = Vec::new();
                    let mut result = Vec::new();
                    for item in arr {
                        let key = serde_json::to_string(item).unwrap_or_default();
                        if !seen.contains(&key) {
                            seen.push(key);
                            result.push(item.clone());
                        }
                    }
                    Ok(Value::Array(result))
                }
                _ => Err(anyhow!(
                    "unique() expects list, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "flatten" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut result = Vec::new();
                    for item in arr {
                        match item {
                            Value::Array(inner) => result.extend(inner.clone()),
                            other => result.push(other.clone()),
                        }
                    }
                    Ok(Value::Array(result))
                }
                _ => Err(anyhow!(
                    "flatten() expects list, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "sum" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut total = 0.0f64;
                    for item in arr {
                        total += value_to_f64(item)?;
                    }
                    Ok(json_number(total))
                }
                _ => Err(anyhow!("sum() expects list, got {}", type_name(&args[0]))),
            }
        }

        "zip" => {
            require_args(name, args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Array(a), Value::Array(b)) => {
                    let pairs: Vec<Value> =
                        a.iter().zip(b.iter()).map(|(x, y)| json!([x, y])).collect();
                    Ok(Value::Array(pairs))
                }
                _ => Err(anyhow!("zip() expects two lists")),
            }
        }

        "enumerate" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let pairs: Vec<Value> =
                        arr.iter().enumerate().map(|(i, v)| json!([i, v])).collect();
                    Ok(Value::Array(pairs))
                }
                _ => Err(anyhow!(
                    "enumerate() expects list, got {}",
                    type_name(&args[0])
                )),
            }
        }

        "first" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => Ok(arr.first().cloned().unwrap_or(Value::Null)),
                _ => Err(anyhow!("first() expects list, got {}", type_name(&args[0]))),
            }
        }

        "last" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => Ok(arr.last().cloned().unwrap_or(Value::Null)),
                _ => Err(anyhow!("last() expects list, got {}", type_name(&args[0]))),
            }
        }

        "chunk" => {
            require_args(name, args, 2)?;
            match &args[0] {
                Value::Array(arr) => {
                    let size = value_to_f64(&args[1])? as usize;
                    if size == 0 {
                        return Err(anyhow!("chunk() size must be > 0"));
                    }
                    let chunks: Vec<Value> =
                        arr.chunks(size).map(|c| Value::Array(c.to_vec())).collect();
                    Ok(Value::Array(chunks))
                }
                _ => Err(anyhow!("chunk() expects list, got {}", type_name(&args[0]))),
            }
        }

        // ============================================================
        // Math operations (extended)
        // ============================================================
        "floor" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])?;
            Ok(json!(n.floor() as i64))
        }

        "ceil" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])?;
            Ok(json!(n.ceil() as i64))
        }

        "pow" => {
            require_args(name, args, 2)?;
            let base = value_to_f64(&args[0])?;
            let exp = value_to_f64(&args[1])?;
            Ok(json_number(base.powf(exp)))
        }

        "sqrt" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])?;
            if n < 0.0 {
                return Err(anyhow!("sqrt() of negative number"));
            }
            Ok(json_number(n.sqrt()))
        }

        "log" => {
            if args.is_empty() || args.len() > 2 {
                return Err(anyhow!("log() expects 1-2 arguments, got {}", args.len()));
            }
            let n = value_to_f64(&args[0])?;
            if n <= 0.0 {
                return Err(anyhow!("log() of non-positive number"));
            }
            let result = if args.len() == 2 {
                let base = value_to_f64(&args[1])?;
                n.log(base)
            } else {
                n.ln()
            };
            Ok(json_number(result))
        }

        "random" => {
            require_args(name, args, 0)?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                use rand::Rng;
                let mut rng = rand::rng();
                Ok(json!(rng.random::<f64>()))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(0.5)) // Fallback for WASM
            }
        }

        "randint" => {
            require_args(name, args, 2)?;
            let min_val = value_to_f64(&args[0])? as i64;
            let max_val = value_to_f64(&args[1])? as i64;
            if min_val > max_val {
                return Err(anyhow!("randint() min must be <= max"));
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                use rand::Rng;
                let mut rng = rand::rng();
                Ok(json!(rng.random_range(min_val..=max_val)))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(min_val))
            }
        }

        "clamp" => {
            require_args(name, args, 3)?;
            let n = value_to_f64(&args[0])?;
            let min_val = value_to_f64(&args[1])?;
            let max_val = value_to_f64(&args[2])?;
            Ok(json_number(n.max(min_val).min(max_val)))
        }

        // ============================================================
        // Data / JSON operations
        // ============================================================
        "from_json" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let parsed: Value =
                serde_json::from_str(&s).map_err(|e| anyhow!("from_json() parse error: {}", e))?;
            Ok(parsed)
        }

        "merge" => {
            require_args(name, args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Object(a), Value::Object(b)) => {
                    let mut merged = a.clone();
                    for (k, v) in b {
                        merged.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Object(merged))
                }
                _ => Err(anyhow!("merge() expects two dicts")),
            }
        }

        "pick" => {
            require_args(name, args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Object(obj), Value::Array(keys)) => {
                    let mut result = serde_json::Map::new();
                    for key in keys {
                        if let Value::String(k) = key {
                            if let Some(v) = obj.get(k) {
                                result.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    Ok(Value::Object(result))
                }
                _ => Err(anyhow!("pick() expects (dict, list_of_keys)")),
            }
        }

        "omit" => {
            require_args(name, args, 2)?;
            match (&args[0], &args[1]) {
                (Value::Object(obj), Value::Array(keys)) => {
                    let exclude: Vec<String> = keys
                        .iter()
                        .filter_map(|k| k.as_str().map(|s| s.to_string()))
                        .collect();
                    let mut result = obj.clone();
                    for k in &exclude {
                        result.remove(k);
                    }
                    Ok(Value::Object(result))
                }
                _ => Err(anyhow!("omit() expects (dict, list_of_keys)")),
            }
        }

        "has" => {
            require_args(name, args, 2)?;
            match &args[0] {
                Value::Object(obj) => {
                    let key = value_to_string(&args[1]);
                    Ok(json!(obj.contains_key(&key)))
                }
                Value::Array(arr) => Ok(json!(arr.contains(&args[1]))),
                _ => Err(anyhow!("has() expects dict or list as first argument")),
            }
        }

        "get" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(anyhow!("get() expects 2-3 arguments, got {}", args.len()));
            }
            let default = if args.len() == 3 {
                args[2].clone()
            } else {
                Value::Null
            };
            match &args[0] {
                Value::Object(obj) => {
                    let key = value_to_string(&args[1]);
                    Ok(obj.get(&key).cloned().unwrap_or(default))
                }
                Value::Array(arr) => {
                    let idx = value_to_f64(&args[1])? as i64;
                    let len = arr.len() as i64;
                    let normalized = if idx < 0 { idx + len } else { idx };
                    if normalized >= 0 && (normalized as usize) < arr.len() {
                        Ok(arr[normalized as usize].clone())
                    } else {
                        Ok(default)
                    }
                }
                _ => Err(anyhow!("get() expects dict or list as first argument")),
            }
        }

        "items" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Object(obj) => {
                    let pairs: Vec<Value> = obj.iter().map(|(k, v)| json!([k, v])).collect();
                    Ok(Value::Array(pairs))
                }
                _ => Err(anyhow!("items() expects dict, got {}", type_name(&args[0]))),
            }
        }

        "from_entries" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => {
                    let mut map = serde_json::Map::new();
                    for item in arr {
                        match item {
                            Value::Array(pair) if pair.len() == 2 => {
                                let key = value_to_string(&pair[0]);
                                map.insert(key, pair[1].clone());
                            }
                            _ => {
                                return Err(anyhow!(
                                    "from_entries() expects list of [key, value] pairs"
                                ))
                            }
                        }
                    }
                    Ok(Value::Object(map))
                }
                _ => Err(anyhow!(
                    "from_entries() expects list, got {}",
                    type_name(&args[0])
                )),
            }
        }

        // ============================================================
        // Date/Time operations
        // ============================================================
        "now" => {
            require_args(name, args, 0)?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let now = chrono::Utc::now();
                Ok(json!(now.to_rfc3339()))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!("1970-01-01T00:00:00+00:00"))
            }
        }

        "timestamp" => {
            require_args(name, args, 0)?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let ts = chrono::Utc::now().timestamp();
                Ok(json!(ts))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(0))
            }
        }

        "timestamp_ms" => {
            require_args(name, args, 0)?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let ts = chrono::Utc::now().timestamp_millis();
                Ok(json!(ts))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(0))
            }
        }

        "format_date" => {
            require_args(name, args, 2)?;
            let iso_str = value_to_string(&args[0]);
            let fmt = value_to_string(&args[1]);
            let dt = chrono::DateTime::parse_from_rfc3339(&iso_str)
                .map_err(|e| anyhow!("format_date() invalid ISO 8601: {}", e))?;
            Ok(json!(dt.format(&fmt).to_string()))
        }

        "parse_date" => {
            require_args(name, args, 2)?;
            let date_str = value_to_string(&args[0]);
            let fmt = value_to_string(&args[1]);
            let naive = chrono::NaiveDateTime::parse_from_str(&date_str, &fmt)
                .map_err(|e| anyhow!("parse_date() parse error: {}", e))?;
            let dt = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc);
            Ok(json!(dt.to_rfc3339()))
        }

        // ============================================================
        // Encoding operations
        // ============================================================
        "base64_encode" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            #[cfg(not(target_arch = "wasm32"))]
            {
                use base64::Engine;
                Ok(json!(
                    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
                ))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(s)) // Fallback
            }
        }

        "base64_decode" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            #[cfg(not(target_arch = "wasm32"))]
            {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(s.as_bytes())
                    .map_err(|e| anyhow!("base64_decode() error: {}", e))?;
                let decoded = String::from_utf8(bytes)
                    .map_err(|e| anyhow!("base64_decode() not valid UTF-8: {}", e))?;
                Ok(json!(decoded))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(s))
            }
        }

        "url_encode" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            #[cfg(not(target_arch = "wasm32"))]
            {
                Ok(json!(urlencoding::encode(&s).into_owned()))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(s))
            }
        }

        "url_decode" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            #[cfg(not(target_arch = "wasm32"))]
            {
                let decoded =
                    urlencoding::decode(&s).map_err(|e| anyhow!("url_decode() error: {}", e))?;
                Ok(json!(decoded.into_owned()))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(s))
            }
        }

        "md5" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            #[cfg(not(target_arch = "wasm32"))]
            {
                use md5::Digest;
                let mut hasher = md5::Md5::new();
                hasher.update(s.as_bytes());
                let result = hasher.finalize();
                Ok(json!(format!("{:x}", result)))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(""))
            }
        }

        "sha256" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            #[cfg(not(target_arch = "wasm32"))]
            {
                use sha2::Digest;
                let mut hasher = sha2::Sha256::new();
                hasher.update(s.as_bytes());
                let result = hasher.finalize();
                Ok(json!(format!("{:x}", result)))
            }
            #[cfg(target_arch = "wasm32")]
            {
                Ok(json!(""))
            }
        }

        // ============================================================
        // Type checking
        // ============================================================
        "is_null" => {
            require_args(name, args, 1)?;
            Ok(json!(args[0].is_null()))
        }

        "is_string" => {
            require_args(name, args, 1)?;
            Ok(json!(args[0].is_string()))
        }

        "is_number" => {
            require_args(name, args, 1)?;
            Ok(json!(args[0].is_number()))
        }

        "is_bool" => {
            require_args(name, args, 1)?;
            Ok(json!(args[0].is_boolean()))
        }

        "is_array" => {
            require_args(name, args, 1)?;
            Ok(json!(args[0].is_array()))
        }

        "is_object" => {
            require_args(name, args, 1)?;
            Ok(json!(args[0].is_object()))
        }

        // ============================================================
        // Path operations
        // ============================================================
        "basename" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let path = std::path::Path::new(&s);
            Ok(json!(path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")))
        }

        "dirname" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let path = std::path::Path::new(&s);
            Ok(json!(path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()))
        }

        "extname" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let path = std::path::Path::new(&s);
            match path.extension().and_then(|e| e.to_str()) {
                Some(ext) => Ok(json!(format!(".{}", ext))),
                None => Ok(json!("")),
            }
        }

        "join_path" => {
            require_min_args(name, args, 2)?;
            let mut path = std::path::PathBuf::from(value_to_string(&args[0]));
            for arg in &args[1..] {
                path = path.join(value_to_string(arg));
            }
            Ok(json!(path.to_string_lossy().replace('\\', "/")))
        }

        // ============================================================
        // Phase 2: Practical built-ins (no lambda needed)
        // ============================================================
        "all" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => Ok(json!(arr.iter().all(is_truthy))),
                _ => Err(anyhow!("all() expects list, got {}", type_name(&args[0]))),
            }
        }

        "any" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Array(arr) => Ok(json!(arr.iter().any(is_truthy))),
                _ => Err(anyhow!("any() expects list, got {}", type_name(&args[0]))),
            }
        }

        "chr" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])? as u32;
            let c = char::from_u32(n).ok_or_else(|| anyhow!("chr() invalid code point: {}", n))?;
            Ok(json!(c.to_string()))
        }

        "ord" => {
            require_args(name, args, 1)?;
            let s = value_to_string(&args[0]);
            let c = s
                .chars()
                .next()
                .ok_or_else(|| anyhow!("ord() expects non-empty string"))?;
            Ok(json!(c as u32))
        }

        "hex" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])? as i64;
            Ok(json!(format!("0x{:x}", n)))
        }

        "bin" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])? as i64;
            Ok(json!(format!("0b{:b}", n)))
        }

        "oct" => {
            require_args(name, args, 1)?;
            let n = value_to_f64(&args[0])? as i64;
            Ok(json!(format!("0o{:o}", n)))
        }

        "regex_match" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let pat = value_to_string(&args[1]);
            let re = regex::Regex::new(&pat)
                .map_err(|e| anyhow!("regex_match() invalid pattern: {}", e))?;
            Ok(json!(re.is_match(&s)))
        }

        "regex_find" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let pat = value_to_string(&args[1]);
            let re = regex::Regex::new(&pat)
                .map_err(|e| anyhow!("regex_find() invalid pattern: {}", e))?;
            match re.find(&s) {
                Some(m) => Ok(json!(m.as_str())),
                None => Ok(Value::Null),
            }
        }

        "regex_find_all" => {
            require_args(name, args, 2)?;
            let s = value_to_string(&args[0]);
            let pat = value_to_string(&args[1]);
            let re = regex::Regex::new(&pat)
                .map_err(|e| anyhow!("regex_find_all() invalid pattern: {}", e))?;
            let matches: Vec<Value> = re.find_iter(&s).map(|m| json!(m.as_str())).collect();
            Ok(Value::Array(matches))
        }

        "regex_replace" => {
            require_args(name, args, 3)?;
            let s = value_to_string(&args[0]);
            let pat = value_to_string(&args[1]);
            let rep = value_to_string(&args[2]);
            let re = regex::Regex::new(&pat)
                .map_err(|e| anyhow!("regex_replace() invalid pattern: {}", e))?;
            Ok(json!(re.replace_all(&s, rep.as_str()).into_owned()))
        }

        "uuid" => {
            require_args(name, args, 0)?;
            Ok(json!(uuid::Uuid::new_v4().to_string()))
        }

        "env" => {
            if args.is_empty() || args.len() > 2 {
                return Err(anyhow!("env() expects 1-2 arguments, got {}", args.len()));
            }
            let name_str = value_to_string(&args[0]);
            match std::env::var(&name_str) {
                Ok(val) => Ok(json!(val)),
                Err(_) => {
                    if args.len() == 2 {
                        Ok(args[1].clone())
                    } else {
                        Ok(Value::Null)
                    }
                }
            }
        }

        "format" => {
            require_min_args(name, args, 1)?;
            let template = value_to_string(&args[0]);
            let format_args = &args[1..];
            let mut result = String::new();
            let mut arg_idx = 0;
            let mut chars = template.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '{' {
                    if chars.peek() == Some(&'}') {
                        chars.next(); // consume '}'
                        if arg_idx < format_args.len() {
                            result.push_str(&value_to_string(&format_args[arg_idx]));
                            arg_idx += 1;
                        } else {
                            result.push_str("{}");
                        }
                    } else if chars.peek() == Some(&'{') {
                        chars.next(); // escape {{
                        result.push('{');
                    } else {
                        result.push(c);
                    }
                } else if c == '}' && chars.peek() == Some(&'}') {
                    chars.next(); // escape }}
                    result.push('}');
                } else {
                    result.push(c);
                }
            }
            Ok(json!(result))
        }

        "json_pretty" => {
            require_args(name, args, 1)?;
            let pretty = serde_json::to_string_pretty(&args[0]).unwrap_or_default();
            Ok(json!(pretty))
        }

        "review" => {
            if args.is_empty() {
                return Err(anyhow!("review() requires at least 1 argument"));
            }
            let content = &args[0];
            let prompt = args.get(1).and_then(|v| v.as_str()).unwrap_or("Pass?");

            println!("\n  ┌─ Review");
            let display =
                serde_json::to_string_pretty(content).unwrap_or_else(|_| format!("{:?}", content));
            for line in display.lines() {
                println!("  │ {}", line);
            }
            print!("  └─ {} (y/n): ", prompt);
            std::io::Write::flush(&mut std::io::stdout())?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            match input.trim().to_lowercase().as_str() {
                "y" | "yes" | "" => Ok(json!(true)),
                _ => Ok(json!(false)),
            }
        }

        // ── Result inspection (Rust-style) ──
        "is_err" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Object(obj) => Ok(Value::Bool(obj.contains_key("err"))),
                Value::Null => Ok(Value::Bool(true)),
                _ => Ok(Value::Bool(false)),
            }
        }
        "is_ok" => {
            require_args(name, args, 1)?;
            match &args[0] {
                Value::Object(obj) => Ok(Value::Bool(!obj.contains_key("err"))),
                Value::Null => Ok(Value::Bool(false)),
                _ => Ok(Value::Bool(true)),
            }
        }
        "unwrap_or" => {
            require_args(name, args, 2)?;
            match &args[0] {
                Value::Null => Ok(args[1].clone()),
                Value::Object(obj) if obj.contains_key("err") => Ok(args[1].clone()),
                _ => Ok(args[0].clone()),
            }
        }

        _ => Err(anyhow!("Unknown function: {}()", name)),
    }
}

/// Normalize a negative index (Python-style): -1 → len-1, etc.
fn normalize_index(idx: i64, len: i64) -> i64 {
    if idx < 0 {
        (len + idx).max(0)
    } else {
        idx.min(len)
    }
}

fn require_args(name: &str, args: &[Value], expected: usize) -> Result<()> {
    if args.len() != expected {
        Err(anyhow!(
            "{}() expects {} argument(s), got {}",
            name,
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn require_min_args(name: &str, args: &[Value], min: usize) -> Result<()> {
    if args.len() < min {
        Err(anyhow!(
            "{}() expects at least {} argument(s), got {}",
            name,
            min,
            args.len()
        ))
    } else {
        Ok(())
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_resolver(ctx: Value) -> impl Fn(&str) -> Option<Value> {
        move |path: &str| {
            // Strip ctx. prefix (like executor does for WorkflowContext)
            let clean = path.strip_prefix("ctx.").unwrap_or(path);
            let parts: Vec<&str> = clean.split('.').collect();
            let pointer = format!("/{}", parts.join("/"));
            ctx.pointer(&pointer).cloned()
        }
    }

    fn eval(expr: &str) -> Value {
        let evaluator = ExprEvaluator::new();
        let resolver = make_resolver(json!({}));
        evaluator.eval(expr, &resolver).unwrap()
    }

    fn eval_with(expr: &str, ctx: Value) -> Value {
        let evaluator = ExprEvaluator::new();
        let resolver = make_resolver(ctx);
        evaluator.eval(expr, &resolver).unwrap()
    }

    fn eval_bool(expr: &str, ctx: Value) -> bool {
        let evaluator = ExprEvaluator::new();
        let resolver = make_resolver(ctx);
        evaluator._eval_as_bool(expr, &resolver).unwrap()
    }

    // ---- Truthiness ----

    #[test]
    fn test_truthiness() {
        assert!(!is_truthy(&Value::Null));
        assert!(!is_truthy(&json!("")));
        assert!(!is_truthy(&json!(0)));
        assert!(!is_truthy(&json!(0.0)));
        assert!(!is_truthy(&json!(false)));
        assert!(!is_truthy(&json!([])));
        assert!(!is_truthy(&json!({})));

        assert!(is_truthy(&json!("hello")));
        assert!(is_truthy(&json!(1)));
        assert!(is_truthy(&json!(-1)));
        assert!(is_truthy(&json!(true)));
        assert!(is_truthy(&json!([1])));
        assert!(is_truthy(&json!({"a": 1})));
    }

    // ---- Literals ----

    #[test]
    fn test_number_literals() {
        assert_eq!(eval("42"), json!(42));
        assert_eq!(eval("-3"), json!(-3));
        assert_eq!(eval("3.14"), json!(3.14));
    }

    #[test]
    fn test_string_literals() {
        assert_eq!(eval("\"hello\""), json!("hello"));
        assert_eq!(eval("'world'"), json!("world"));
        assert_eq!(eval("\"he said \\\"hi\\\"\""), json!("he said \"hi\""));
    }

    #[test]
    fn test_bool_none_literals() {
        assert_eq!(eval("true"), json!(true));
        assert_eq!(eval("True"), json!(true));
        assert_eq!(eval("false"), json!(false));
        assert_eq!(eval("False"), json!(false));
        assert_eq!(eval("None"), Value::Null);
        assert_eq!(eval("null"), Value::Null);
    }

    #[test]
    fn test_array_literal() {
        assert_eq!(eval("[1, 2, 3]"), json!([1, 2, 3]));
        assert_eq!(eval("[]"), json!([]));
    }

    #[test]
    fn test_object_literal() {
        assert_eq!(eval("{\"a\": 1, \"b\": 2}"), json!({"a": 1, "b": 2}));
    }

    // ---- Equality (the original Rhai bug) ----

    #[test]
    fn test_null_equality() {
        // null == null → true
        assert_eq!(eval("null == null"), json!(true));
        assert_eq!(eval("None == None"), json!(true));

        // null != "" → true (different types, unlike Rhai unit weirdness)
        assert_eq!(eval("null != \"\""), json!(true));

        // null == "" → false
        assert_eq!(eval("null == \"\""), json!(false));
    }

    #[test]
    fn test_missing_variable_is_null() {
        let ctx = json!({"message": "hello"});
        // Non-existent variable → null
        assert_eq!(eval_with("$event_type", ctx.clone()), Value::Null);
        // null is falsy
        assert!(!eval_bool("$event_type", ctx.clone()));
    }

    #[test]
    fn test_original_bug_scenario() {
        // The bug: input has no event_type field
        let ctx = json!({
            "input": {"message": "Buy 0.1 BTC at market price"}
        });
        // $input.event_type should be null (not Rhai unit)
        assert_eq!(eval_with("$input.event_type", ctx.clone()), Value::Null);
        // null is falsy — so in a condition context, this is false
        assert!(!eval_bool("$input.event_type", ctx.clone()));
        // $input.event_type == "" → false (null != empty string)
        assert!(!eval_bool("$input.event_type == \"\"", ctx.clone()));
        // $input.event_type != "" → true
        assert!(eval_bool("$input.event_type != \"\"", ctx.clone()));
        // But: not $input.event_type → true (null is falsy)
        assert!(eval_bool("not $input.event_type", ctx.clone()));
    }

    // ---- Comparisons ----

    #[test]
    fn test_comparisons() {
        assert_eq!(eval("1 == 1"), json!(true));
        assert_eq!(eval("1 != 2"), json!(true));
        assert_eq!(eval("1 < 2"), json!(true));
        assert_eq!(eval("2 > 1"), json!(true));
        assert_eq!(eval("1 <= 1"), json!(true));
        assert_eq!(eval("1 >= 1"), json!(true));
    }

    #[test]
    fn test_string_comparisons() {
        assert_eq!(eval("\"abc\" == \"abc\""), json!(true));
        assert_eq!(eval("\"abc\" != \"def\""), json!(true));
        assert_eq!(eval("\"abc\" < \"def\""), json!(true));
    }

    // ---- Arithmetic ----

    #[test]
    fn test_arithmetic() {
        assert_eq!(eval("2 + 3"), json!(5));
        assert_eq!(eval("10 - 4"), json!(6));
        assert_eq!(eval("3 * 4"), json!(12));
        assert_eq!(eval("10 / 3"), json!(10.0 / 3.0));
        assert_eq!(eval("10 % 3"), json!(1));
    }

    // ---- String Concatenation ----

    #[test]
    fn test_string_concat() {
        assert_eq!(eval("\"hello\" + \" \" + \"world\""), json!("hello world"));
        // Auto-coerce number to string
        assert_eq!(eval("\"count: \" + 42"), json!("count: 42"));
        // Auto-coerce bool to string
        assert_eq!(eval("\"flag: \" + true"), json!("flag: true"));
    }

    #[test]
    fn test_string_concat_with_variable() {
        let ctx = json!({"input": {"query": "test"}});
        assert_eq!(
            eval_with("\"message: \" + $input.query", ctx),
            json!("message: test")
        );
    }

    // ---- Logical ----

    #[test]
    fn test_and_or_not() {
        assert_eq!(eval("true and true"), json!(true));
        assert_eq!(eval("true and false"), json!(false));
        assert_eq!(eval("false or true"), json!(true));
        assert_eq!(eval("false or false"), json!(false));
        assert_eq!(eval("not true"), json!(false));
        assert_eq!(eval("not false"), json!(true));
        assert_eq!(eval("not null"), json!(true));
        assert_eq!(eval("not \"\""), json!(true));
    }

    #[test]
    fn test_short_circuit() {
        // and returns first falsy or last value
        assert_eq!(eval("\"hello\" and \"world\""), json!("world"));
        assert_eq!(eval("\"\" and \"world\""), json!(""));
        // or returns first truthy or last value
        assert_eq!(eval("\"\" or \"fallback\""), json!("fallback"));
        assert_eq!(eval("\"hello\" or \"world\""), json!("hello"));
    }

    #[test]
    fn test_symbol_operators() {
        // && || ! should also work
        assert_eq!(eval("true && true"), json!(true));
        assert_eq!(eval("false || true"), json!(true));
        assert_eq!(eval("!true"), json!(false));
    }

    // ---- In / Not In ----

    #[test]
    fn test_in_operator() {
        assert_eq!(eval("\"a\" in \"abc\""), json!(true));
        assert_eq!(eval("\"d\" in \"abc\""), json!(false));
        assert_eq!(eval("1 in [1, 2, 3]"), json!(true));
        assert_eq!(eval("4 in [1, 2, 3]"), json!(false));
    }

    #[test]
    fn test_not_in_operator() {
        assert_eq!(eval("\"d\" not in \"abc\""), json!(true));
        assert_eq!(eval("\"a\" not in \"abc\""), json!(false));
    }

    #[test]
    fn test_in_object_keys() {
        let ctx = json!({"data": {"name": "Alice", "age": 30}});
        assert!(eval_bool("\"name\" in $data", ctx.clone()));
        assert!(!eval_bool("\"email\" in $data", ctx));
    }

    // ---- Variables ----

    #[test]
    fn test_variable_resolution() {
        // ctx. prefix is stripped by resolver, so $ctx.intent → /intent
        let ctx = json!({
            "intent": "greeting",
            "input": {"query": "hello"},
            "output": {"category": "technical"}
        });
        assert_eq!(eval_with("$ctx.intent", ctx.clone()), json!("greeting"));
        assert_eq!(eval_with("$input.query", ctx.clone()), json!("hello"));
        assert_eq!(
            eval_with("$output.category", ctx.clone()),
            json!("technical")
        );
    }

    #[test]
    fn test_condition_with_variable() {
        let ctx = json!({"intent": "greeting"});
        assert!(eval_bool("$ctx.intent == \"greeting\"", ctx.clone()));
        assert!(!eval_bool("$ctx.intent == \"question\"", ctx));
    }

    // ---- Functions ----

    #[test]
    fn test_len() {
        assert_eq!(eval("len(\"hello\")"), json!(5));
        assert_eq!(eval("len([1, 2, 3])"), json!(3));
        assert_eq!(eval("len({\"a\": 1})"), json!(1));
    }

    #[test]
    fn test_str_int_float() {
        assert_eq!(eval("str(42)"), json!("42"));
        assert_eq!(eval("int(\"42\")"), json!(42));
        assert_eq!(eval("float(\"3.14\")"), json!(3.14));
    }

    #[test]
    fn test_round() {
        assert_eq!(eval("round(3.14159, 2)"), json!(3.14));
        assert_eq!(eval("round(3.5)"), json!(4.0));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(eval("truncate(\"hello world\", 5)"), json!("hello..."));
        assert_eq!(eval("truncate(\"hi\", 5)"), json!("hi"));
    }

    #[test]
    fn test_upper_lower() {
        assert_eq!(eval("upper(\"hello\")"), json!("HELLO"));
        assert_eq!(eval("lower(\"HELLO\")"), json!("hello"));
    }

    #[test]
    fn test_default() {
        assert_eq!(eval("default(null, \"fallback\")"), json!("fallback"));
        assert_eq!(eval("default(\"\", \"fallback\")"), json!("fallback"));
        assert_eq!(eval("default(\"value\", \"fallback\")"), json!("value"));
    }

    #[test]
    fn test_json_fn() {
        assert_eq!(eval("json([1, 2, 3])"), json!("[1,2,3]"));
    }

    #[test]
    fn test_keys_values() {
        let ctx = json!({"data": {"a": 1, "b": 2}});
        let keys = eval_with("keys($data)", ctx.clone());
        assert!(keys.as_array().unwrap().contains(&json!("a")));
        assert!(keys.as_array().unwrap().contains(&json!("b")));
    }

    #[test]
    fn test_join_split() {
        assert_eq!(eval("join([\"a\", \"b\", \"c\"], \",\")"), json!("a,b,c"));
        assert_eq!(eval("split(\"a,b,c\", \",\")"), json!(["a", "b", "c"]));
    }

    #[test]
    fn test_replace() {
        assert_eq!(
            eval("replace(\"hello world\", \"world\", \"rust\")"),
            json!("hello rust")
        );
    }

    #[test]
    fn test_startswith_endswith() {
        assert_eq!(eval("startswith(\"hello\", \"hel\")"), json!(true));
        assert_eq!(eval("endswith(\"hello\", \"llo\")"), json!(true));
    }

    #[test]
    fn test_range() {
        assert_eq!(eval("range(5)"), json!([0, 1, 2, 3, 4]));
        assert_eq!(eval("range(2, 5)"), json!([2, 3, 4]));
    }

    #[test]
    fn test_append() {
        assert_eq!(eval("append([1, 2], 3)"), json!([1, 2, 3]));
    }

    // ---- Pipe / Filter Syntax ----

    #[test]
    fn test_pipe_no_args() {
        assert_eq!(eval("\"hello\" | upper"), json!("HELLO"));
    }

    #[test]
    fn test_pipe_with_args() {
        assert_eq!(eval("3.14159 | round(2)"), json!(3.14));
    }

    #[test]
    fn test_pipe_chain() {
        assert_eq!(
            eval("\"hello world\" | upper | truncate(5)"),
            json!("HELLO...")
        );
    }

    // ---- Method Call Syntax ----

    #[test]
    fn test_method_call() {
        assert_eq!(eval("\"hello\".upper()"), json!("HELLO"));
        assert_eq!(eval("3.14159.round(2)"), json!(3.14));
    }

    // ---- Access ----

    #[test]
    fn test_dot_access() {
        let ctx = json!({"data": {"nested": {"value": 42}}});
        assert_eq!(eval_with("$data.nested.value", ctx), json!(42));
    }

    #[test]
    fn test_bracket_access() {
        let ctx = json!({"items": ["a", "b", "c"]});
        assert_eq!(eval_with("$items[0]", ctx.clone()), json!("a"));
        assert_eq!(eval_with("$items[2]", ctx.clone()), json!("c"));
        // Negative indexing
        assert_eq!(eval_with("$items[-1]", ctx), json!("c"));
    }

    // ---- Complex Expressions ----

    #[test]
    fn test_complex_condition() {
        let ctx = json!({"intent": "greeting", "confidence": 0.9});
        assert!(eval_bool(
            "$ctx.intent == \"greeting\" and $ctx.confidence > 0.8",
            ctx
        ));
    }

    #[test]
    fn test_precedence() {
        // * before +
        assert_eq!(eval("2 + 3 * 4"), json!(14));
        // () overrides
        assert_eq!(eval("(2 + 3) * 4"), json!(20));
    }

    #[test]
    fn test_nested_function() {
        assert_eq!(eval("len(split(\"a,b,c\", \",\"))"), json!(3));
    }

    // ---- Bare Identifiers (for template scope) ----

    #[test]
    fn test_bare_identifier_in_scope() {
        let ctx = json!({"name": "Alice", "items": [1, 2, 3]});
        assert_eq!(eval_with("name", ctx.clone()), json!("Alice"));
        assert!(eval_bool("items", ctx)); // truthy (non-empty array)
    }

    // ---- Extended String Operations ----

    #[test]
    fn test_string_extended() {
        // find
        assert_eq!(eval("find(\"hello world\", \"world\")"), json!(6));
        assert_eq!(eval("find(\"hello\", \"xyz\")"), json!(-1));
        assert_eq!(eval("find(\"abcabc\", \"bc\")"), json!(1));

        // slice
        assert_eq!(eval("slice(\"hello\", 1, 4)"), json!("ell"));
        assert_eq!(eval("slice(\"hello\", 2)"), json!("llo"));
        assert_eq!(eval("slice(\"hello\", -3)"), json!("llo"));
        assert_eq!(eval("slice([1,2,3,4,5], 1, 3)"), json!([2, 3]));

        // count
        assert_eq!(eval("count(\"abcabc\", \"ab\")"), json!(2));
        assert_eq!(eval("count(\"hello\", \"x\")"), json!(0));

        // capitalize
        assert_eq!(eval("capitalize(\"hello world\")"), json!("Hello world"));
        assert_eq!(eval("capitalize(\"HELLO\")"), json!("Hello"));

        // title
        assert_eq!(eval("title(\"hello world\")"), json!("Hello World"));
        assert_eq!(
            eval("title(\"the quick brown fox\")"),
            json!("The Quick Brown Fox")
        );

        // lpad / rpad
        assert_eq!(eval("lpad(\"42\", 5, \"0\")"), json!("00042"));
        assert_eq!(eval("lpad(\"hi\", 5)"), json!("   hi"));
        assert_eq!(eval("rpad(\"hi\", 5)"), json!("hi   "));
        assert_eq!(eval("rpad(\"42\", 5, \".\")"), json!("42..."));

        // repeat
        assert_eq!(eval("repeat(\"ab\", 3)"), json!("ababab"));
        assert_eq!(eval("repeat(\"-\", 5)"), json!("-----"));
    }

    // ---- Extended Collection Operations ----

    #[test]
    fn test_collection_extended() {
        // sort
        assert_eq!(eval("sort([3, 1, 2])"), json!([1, 2, 3]));
        assert_eq!(eval("sort([\"c\", \"a\", \"b\"])"), json!(["a", "b", "c"]));

        // reverse
        assert_eq!(eval("reverse([1, 2, 3])"), json!([3, 2, 1]));
        assert_eq!(eval("reverse(\"hello\")"), json!("olleh"));

        // unique
        assert_eq!(eval("unique([1, 2, 2, 3, 1])"), json!([1, 2, 3]));

        // flatten
        assert_eq!(eval("flatten([[1, 2], [3, 4], 5])"), json!([1, 2, 3, 4, 5]));

        // sum
        assert_eq!(eval("sum([1, 2, 3, 4])"), json!(10));

        // zip
        assert_eq!(
            eval("zip([1, 2, 3], [\"a\", \"b\", \"c\"])"),
            json!([[1, "a"], [2, "b"], [3, "c"]])
        );

        // enumerate
        assert_eq!(
            eval("enumerate([\"a\", \"b\", \"c\"])"),
            json!([[0, "a"], [1, "b"], [2, "c"]])
        );

        // first / last
        assert_eq!(eval("first([10, 20, 30])"), json!(10));
        assert_eq!(eval("last([10, 20, 30])"), json!(30));
        assert_eq!(eval("first([])"), Value::Null);
        assert_eq!(eval("last([])"), Value::Null);

        // chunk
        assert_eq!(
            eval("chunk([1, 2, 3, 4, 5], 2)"),
            json!([[1, 2], [3, 4], [5]])
        );
    }

    // ---- Extended Math Operations ----

    #[test]
    fn test_math_extended() {
        // floor / ceil
        assert_eq!(eval("floor(3.7)"), json!(3));
        assert_eq!(eval("floor(-2.3)"), json!(-3));
        assert_eq!(eval("ceil(3.2)"), json!(4));
        assert_eq!(eval("ceil(-2.7)"), json!(-2));

        // pow
        assert_eq!(eval("pow(2, 10)"), json!(1024));
        assert_eq!(eval("pow(3, 2)"), json!(9));

        // sqrt
        assert_eq!(eval("sqrt(16)"), json!(4));
        assert_eq!(eval("sqrt(2)"), json!(std::f64::consts::SQRT_2));

        // log
        let ln_e = eval("log(2.718281828459045)");
        assert!((ln_e.as_f64().unwrap() - 1.0).abs() < 0.001);
        let log10 = eval("log(100, 10)");
        assert!((log10.as_f64().unwrap() - 2.0).abs() < 0.001);

        // clamp
        assert_eq!(eval("clamp(5, 0, 10)"), json!(5));
        assert_eq!(eval("clamp(-5, 0, 10)"), json!(0));
        assert_eq!(eval("clamp(15, 0, 10)"), json!(10));
    }

    // ---- Data / JSON Operations ----

    #[test]
    fn test_data_operations() {
        // from_json
        assert_eq!(eval("from_json(\"{\\\"a\\\": 1}\")"), json!({"a": 1}));
        assert_eq!(eval("from_json(\"[1,2,3]\")"), json!([1, 2, 3]));

        // merge
        let ctx = json!({"a": {"x": 1}, "b": {"y": 2, "x": 99}});
        assert_eq!(eval_with("merge($a, $b)", ctx), json!({"x": 99, "y": 2}));

        // pick
        let ctx = json!({"d": {"a": 1, "b": 2, "c": 3}});
        assert_eq!(
            eval_with("pick($d, [\"a\", \"c\"])", ctx),
            json!({"a": 1, "c": 3})
        );

        // omit
        let ctx = json!({"d": {"a": 1, "b": 2, "c": 3}});
        assert_eq!(eval_with("omit($d, [\"b\"])", ctx), json!({"a": 1, "c": 3}));

        // has
        let ctx = json!({"d": {"name": "alice"}, "arr": [1, 2, 3]});
        assert_eq!(eval_with("has($d, \"name\")", ctx.clone()), json!(true));
        assert_eq!(eval_with("has($d, \"age\")", ctx.clone()), json!(false));
        assert_eq!(eval_with("has($arr, 2)", ctx.clone()), json!(true));
        assert_eq!(eval_with("has($arr, 5)", ctx.clone()), json!(false));

        // get
        let ctx = json!({"d": {"a": 1}});
        assert_eq!(eval_with("get($d, \"a\")", ctx.clone()), json!(1));
        assert_eq!(
            eval_with("get($d, \"missing\", \"default\")", ctx),
            json!("default")
        );

        // items
        let ctx = json!({"d": {"x": 1, "y": 2}});
        let items = eval_with("items($d)", ctx);
        let arr = items.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        // from_entries
        assert_eq!(
            eval("from_entries([[\"a\", 1], [\"b\", 2]])"),
            json!({"a": 1, "b": 2})
        );
    }

    // ---- Date/Time Operations ----

    #[test]
    fn test_datetime() {
        // now() returns an ISO 8601 string
        let result = eval("now()");
        assert!(result.as_str().unwrap().contains("T"));

        // timestamp() returns a positive number
        let ts = eval("timestamp()");
        assert!(ts.as_i64().unwrap() > 1_700_000_000);

        // timestamp_ms()
        let ts_ms = eval("timestamp_ms()");
        assert!(ts_ms.as_i64().unwrap() > 1_700_000_000_000);

        // format_date
        assert_eq!(
            eval("format_date(\"2024-01-15T10:30:00+00:00\", \"%Y-%m-%d\")"),
            json!("2024-01-15")
        );

        // parse_date
        let parsed = eval("parse_date(\"2024-01-15 10:30:00\", \"%Y-%m-%d %H:%M:%S\")");
        assert!(parsed.as_str().unwrap().starts_with("2024-01-15"));
    }

    // ---- Encoding Operations ----

    #[test]
    fn test_encoding() {
        // base64
        assert_eq!(eval("base64_encode(\"hello\")"), json!("aGVsbG8="));
        assert_eq!(eval("base64_decode(\"aGVsbG8=\")"), json!("hello"));

        // url encode/decode
        assert_eq!(eval("url_encode(\"hello world\")"), json!("hello%20world"));
        assert_eq!(eval("url_decode(\"hello%20world\")"), json!("hello world"));

        // md5
        assert_eq!(
            eval("md5(\"hello\")"),
            json!("5d41402abc4b2a76b9719d911017c592")
        );

        // sha256
        assert_eq!(
            eval("sha256(\"hello\")"),
            json!("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    // ---- Type Checking ----

    #[test]
    fn test_type_checks() {
        assert_eq!(eval("is_null(null)"), json!(true));
        assert_eq!(eval("is_null(1)"), json!(false));

        assert_eq!(eval("is_string(\"hello\")"), json!(true));
        assert_eq!(eval("is_string(42)"), json!(false));

        assert_eq!(eval("is_number(42)"), json!(true));
        assert_eq!(eval("is_number(3.14)"), json!(true));
        assert_eq!(eval("is_number(\"42\")"), json!(false));

        assert_eq!(eval("is_bool(true)"), json!(true));
        assert_eq!(eval("is_bool(1)"), json!(false));

        assert_eq!(eval("is_array([1, 2])"), json!(true));
        assert_eq!(eval("is_array(\"[1,2]\")"), json!(false));

        assert_eq!(eval("is_object({\"a\": 1})"), json!(true));
        assert_eq!(eval("is_object([1])"), json!(false));
    }

    // ---- Path Operations ----

    #[test]
    fn test_path_operations() {
        // basename
        assert_eq!(
            eval("basename(\"/usr/local/bin/juglans\")"),
            json!("juglans")
        );
        assert_eq!(eval("basename(\"file.txt\")"), json!("file.txt"));

        // dirname
        assert_eq!(
            eval("dirname(\"/usr/local/bin/juglans\")"),
            json!("/usr/local/bin")
        );

        // extname
        assert_eq!(eval("extname(\"file.txt\")"), json!(".txt"));
        assert_eq!(eval("extname(\"archive.tar.gz\")"), json!(".gz"));
        assert_eq!(eval("extname(\"noext\")"), json!(""));

        // join_path
        assert_eq!(
            eval("join_path(\"/usr\", \"local\", \"bin\")"),
            json!("/usr/local/bin")
        );
    }

    // ---- Phase 2: Practical Built-ins ----

    #[test]
    fn test_all_any() {
        assert_eq!(eval("all([true, true, true])"), json!(true));
        assert_eq!(eval("all([true, false, true])"), json!(false));
        assert_eq!(eval("all([1, 2, 3])"), json!(true));
        assert_eq!(eval("all([1, 0, 3])"), json!(false));
        assert_eq!(eval("all([])"), json!(true)); // vacuous truth

        assert_eq!(eval("any([false, false, true])"), json!(true));
        assert_eq!(eval("any([false, false])"), json!(false));
        assert_eq!(eval("any([0, \"\", null])"), json!(false));
        assert_eq!(eval("any([0, 1])"), json!(true));
        assert_eq!(eval("any([])"), json!(false));
    }

    #[test]
    fn test_chr_ord() {
        assert_eq!(eval("chr(65)"), json!("A"));
        assert_eq!(eval("chr(97)"), json!("a"));
        assert_eq!(eval("chr(20320)"), json!("你")); // Unicode
        assert_eq!(eval("ord(\"A\")"), json!(65));
        assert_eq!(eval("ord(\"a\")"), json!(97));
        assert_eq!(eval("ord(\"你\")"), json!(20320));
    }

    #[test]
    fn test_hex_bin_oct() {
        assert_eq!(eval("hex(255)"), json!("0xff"));
        assert_eq!(eval("hex(16)"), json!("0x10"));
        assert_eq!(eval("bin(10)"), json!("0b1010"));
        assert_eq!(eval("bin(255)"), json!("0b11111111"));
        assert_eq!(eval("oct(8)"), json!("0o10"));
        assert_eq!(eval("oct(63)"), json!("0o77"));
    }

    #[test]
    fn test_regex() {
        // regex_match
        assert_eq!(
            eval("regex_match(\"hello123\", \"[a-z]+\\\\d+\")"),
            json!(true)
        );
        assert_eq!(eval("regex_match(\"hello\", \"\\\\d+\")"), json!(false));

        // regex_find
        assert_eq!(
            eval("regex_find(\"price: $42.50\", \"\\\\d+\\\\.\\\\d+\")"),
            json!("42.50")
        );
        assert_eq!(eval("regex_find(\"no numbers\", \"\\\\d+\")"), Value::Null);

        // regex_find_all
        assert_eq!(
            eval("regex_find_all(\"a1 b2 c3\", \"[a-z]\\\\d\")"),
            json!(["a1", "b2", "c3"])
        );

        // regex_replace
        assert_eq!(
            eval("regex_replace(\"hello world\", \"\\\\s+\", \"-\")"),
            json!("hello-world")
        );
    }

    #[test]
    fn test_uuid() {
        let result = eval("uuid()");
        let s = result.as_str().unwrap();
        assert_eq!(s.len(), 36); // UUID format: 8-4-4-4-12
        assert!(s.contains('-'));
    }

    #[test]
    fn test_env_fn() {
        // PATH should always exist
        let result = eval("env(\"PATH\")");
        assert!(result.is_string());
        assert!(!result.as_str().unwrap().is_empty());

        // Non-existent var returns null
        assert_eq!(eval("env(\"JUGLANS_TEST_NONEXISTENT_VAR\")"), Value::Null);

        // Non-existent var with default
        assert_eq!(
            eval("env(\"JUGLANS_TEST_NONEXISTENT_VAR\", \"fallback\")"),
            json!("fallback")
        );
    }

    #[test]
    fn test_format_fn() {
        assert_eq!(
            eval("format(\"hello {}\", \"world\")"),
            json!("hello world")
        );
        assert_eq!(
            eval("format(\"{} + {} = {}\", 1, 2, 3)"),
            json!("1 + 2 = 3")
        );
        assert_eq!(eval("format(\"no args\")"), json!("no args"));
        assert_eq!(eval("format(\"escape {{}}\")"), json!("escape {}"));
    }

    #[test]
    fn test_json_pretty() {
        let result = eval("json_pretty({\"a\": 1})");
        let s = result.as_str().unwrap();
        assert!(s.contains('\n')); // Pretty-printed has newlines
        assert!(s.contains("\"a\""));
    }

    // ---- Lambda & Higher-Order Functions ----

    #[test]
    fn test_lambda_map() {
        assert_eq!(eval("map([1, 2, 3], x => x * 2)"), json!([2, 4, 6]));
        assert_eq!(
            eval("map([\"a\", \"b\"], s => upper(s))"),
            json!(["A", "B"])
        );
        // Map over empty list
        assert_eq!(eval("map([], x => x + 1)"), json!([]));
    }

    #[test]
    fn test_lambda_filter() {
        assert_eq!(eval("filter([1, 2, 3, 4, 5], x => x > 3)"), json!([4, 5]));
        assert_eq!(
            eval("filter([\"hello\", \"\", \"world\"], s => s)"),
            json!(["hello", "world"])
        );
    }

    #[test]
    fn test_lambda_reduce() {
        assert_eq!(
            eval("reduce([1, 2, 3, 4], (acc, x) => acc + x, 0)"),
            json!(10)
        );
        assert_eq!(
            eval("reduce([\"a\", \"b\", \"c\"], (acc, x) => acc + x, \"\")"),
            json!("abc")
        );
    }

    #[test]
    fn test_lambda_sort_by() {
        let ctx = json!({"items": [
            {"name": "Charlie", "age": 30},
            {"name": "Alice", "age": 25},
            {"name": "Bob", "age": 35}
        ]});
        let result = eval_with("sort_by($items, x => x.age)", ctx);
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["name"], json!("Alice"));
        assert_eq!(arr[1]["name"], json!("Charlie"));
        assert_eq!(arr[2]["name"], json!("Bob"));
    }

    #[test]
    fn test_lambda_find_by() {
        let ctx = json!({"items": [
            {"name": "Alice", "age": 25},
            {"name": "Bob", "age": 35}
        ]});
        assert_eq!(
            eval_with("find_by($items, x => x.age > 30)", ctx.clone()),
            json!({"name": "Bob", "age": 35})
        );
        assert_eq!(
            eval_with("find_by($items, x => x.age > 100)", ctx),
            Value::Null
        );
    }

    #[test]
    fn test_lambda_group_by() {
        let ctx = json!({"items": [
            {"type": "fruit", "name": "apple"},
            {"type": "veggie", "name": "carrot"},
            {"type": "fruit", "name": "banana"}
        ]});
        let result = eval_with("group_by($items, x => x.type)", ctx);
        let obj = result.as_object().unwrap();
        assert_eq!(obj["fruit"].as_array().unwrap().len(), 2);
        assert_eq!(obj["veggie"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_lambda_flat_map() {
        assert_eq!(
            eval("flat_map([[1, 2], [3, 4]], x => x)"),
            json!([1, 2, 3, 4])
        );
        assert_eq!(
            eval("flat_map([1, 2, 3], x => [x, x * 10])"),
            json!([1, 10, 2, 20, 3, 30])
        );
    }

    #[test]
    fn test_lambda_count_by() {
        let result = eval("count_by([\"a\", \"b\", \"a\", \"c\", \"b\", \"a\"], x => x)");
        let obj = result.as_object().unwrap();
        assert_eq!(obj["a"], json!(3));
        assert_eq!(obj["b"], json!(2));
        assert_eq!(obj["c"], json!(1));
    }

    #[test]
    fn test_lambda_min_max_by() {
        let ctx = json!({"items": [
            {"name": "Alice", "score": 85},
            {"name": "Bob", "score": 92},
            {"name": "Charlie", "score": 78}
        ]});
        assert_eq!(
            eval_with("min_by($items, x => x.score)", ctx.clone()),
            json!({"name": "Charlie", "score": 78})
        );
        assert_eq!(
            eval_with("max_by($items, x => x.score)", ctx),
            json!({"name": "Bob", "score": 92})
        );
        // Empty list
        assert_eq!(eval("min_by([], x => x)"), Value::Null);
        assert_eq!(eval("max_by([], x => x)"), Value::Null);
    }

    #[test]
    fn test_lambda_every_some() {
        assert_eq!(eval("every([2, 4, 6], x => x > 0)"), json!(true));
        assert_eq!(eval("every([2, -1, 6], x => x > 0)"), json!(false));
        assert_eq!(eval("every([], x => x > 0)"), json!(true)); // vacuous truth

        assert_eq!(eval("some([0, 0, 1], x => x > 0)"), json!(true));
        assert_eq!(eval("some([0, 0, 0], x => x > 0)"), json!(false));
        assert_eq!(eval("some([], x => x > 0)"), json!(false));
    }

    #[test]
    fn test_lambda_capture_outer_scope() {
        let ctx = json!({"offset": 10, "items": [1, 2, 3]});
        assert_eq!(
            eval_with("map($items, x => x + $offset)", ctx),
            json!([11, 12, 13])
        );
    }

    #[test]
    fn test_lambda_nested() {
        // Nested lambdas: map inside map
        assert_eq!(
            eval("map([[1, 2], [3, 4]], row => map(row, x => x * 10))"),
            json!([[10, 20], [30, 40]])
        );
    }

    #[test]
    fn test_lambda_method_call() {
        // Method call syntax: [1,2,3].map(x => x * 2)
        assert_eq!(eval("[1, 2, 3].map(x => x * 2)"), json!([2, 4, 6]));
        assert_eq!(eval("[1, 2, 3, 4, 5].filter(x => x > 3)"), json!([4, 5]));
    }

    #[test]
    fn test_lambda_pipe() {
        // Pipe syntax: [1,2,3] | map(x => x * 2)
        assert_eq!(eval("[1, 2, 3] | map(x => x * 2)"), json!([2, 4, 6]));
        // Chained pipes with lambdas
        assert_eq!(
            eval("[1, 2, 3, 4, 5] | filter(x => x > 2) | map(x => x * 10)"),
            json!([30, 40, 50])
        );
    }

    #[test]
    fn test_lambda_multi_param() {
        assert_eq!(
            eval("reduce([1, 2, 3, 4], (acc, x) => acc + x, 0)"),
            json!(10)
        );
        // Multi-param lambda for zip + map
        assert_eq!(
            eval("map(zip([1, 2, 3], [10, 20, 30]), pair => first(pair) + last(pair))"),
            json!([11, 22, 33])
        );
    }

    // ---- F-String Interpolation ----

    #[test]
    fn test_fstring_basic() {
        let ctx = json!({"name": "Alice", "age": 30});
        assert_eq!(
            eval_with(r#"f"Hello {$ctx.name}""#, ctx),
            json!("Hello Alice")
        );
    }

    #[test]
    fn test_fstring_expression() {
        let ctx = json!({"count": 5});
        assert_eq!(
            eval_with(r#"f"Count: {$ctx.count + 1}""#, ctx),
            json!("Count: 6")
        );
    }

    #[test]
    fn test_fstring_escaped_braces() {
        assert_eq!(eval(r#"f"Escaped {{braces}}""#), json!("Escaped {braces}"));
    }

    #[test]
    fn test_fstring_plain_text() {
        assert_eq!(eval(r#"f"No interpolation""#), json!("No interpolation"));
    }

    #[test]
    fn test_fstring_empty() {
        assert_eq!(eval(r#"f"""#), json!(""));
    }

    #[test]
    fn test_fstring_multiple_interps() {
        let ctx = json!({"a": "X", "b": "Y"});
        assert_eq!(
            eval_with(r#"f"{$ctx.a} and {$ctx.b}""#, ctx),
            json!("X and Y")
        );
    }

    #[test]
    fn test_fstring_null_value() {
        assert_eq!(eval(r#"f"val={none}""#), json!("val=None"));
    }

    #[test]
    fn test_fstring_number_value() {
        assert_eq!(eval(r#"f"pi={3.14}""#), json!("pi=3.14"));
    }

    #[test]
    fn test_fstring_nested_func() {
        // Inside f-strings, use single quotes for string literals
        assert_eq!(eval(r#"f"upper={upper('hello')}""#), json!("upper=HELLO"));
    }

    // ---- Triple-quoted strings ----

    #[test]
    fn test_triple_quoted_basic() {
        assert_eq!(eval(r#""""hello world""""#), json!("hello world"));
    }

    #[test]
    fn test_triple_quoted_with_double_quotes() {
        assert_eq!(
            eval(r#""""he said "hello" and left""""#),
            json!(r#"he said "hello" and left"#)
        );
    }

    #[test]
    fn test_triple_quoted_with_json() {
        assert_eq!(
            eval(r#""""{"key":"value","nested":{"id":1}}""""#),
            json!(r#"{"key":"value","nested":{"id":1}}"#)
        );
    }

    #[test]
    fn test_triple_quoted_multiline() {
        let input = "\"\"\"line1\nline2\nline3\"\"\"";
        assert_eq!(eval(input), json!("line1\nline2\nline3"));
    }

    #[test]
    fn test_triple_quoted_with_single_quotes() {
        assert_eq!(
            eval(r#""""it's a "test" string""""#),
            json!(r#"it's a "test" string"#)
        );
    }

    #[test]
    fn test_triple_quoted_empty() {
        assert_eq!(eval(r#""""""""#), json!(""));
    }

    #[test]
    fn test_triple_quoted_with_backslash() {
        // Backslash NOT processed as escape (raw string)
        assert_eq!(eval(r#""""path\to\file""""#), json!(r"path\to\file"));
    }

    #[test]
    fn test_triple_quoted_curl_command() {
        let input = r#""""curl -sf -X POST http://localhost:8001/api/publish -H 'Content-Type: application/json' -d '{"channel":"test","data":{"id":1}}'""""#;
        let result = eval(input);
        assert!(result.as_str().unwrap().contains(r#"{"channel":"test"#));
    }

    #[test]
    fn test_triple_quoted_concat() {
        assert_eq!(
            eval(r#""""hello """ + """ world""""#),
            json!("hello  world")
        );
    }

    // ---- Triple-quoted f-strings ----

    #[test]
    fn test_fstring_triple_basic() {
        assert_eq!(
            eval_with(r#"f"""Hello {$ctx.name}!""""#, json!({"name": "Alice"})),
            json!("Hello Alice!")
        );
    }

    #[test]
    fn test_fstring_triple_with_quotes() {
        assert_eq!(
            eval_with(
                r#"f"""curl -H "Authorization: Bearer {$ctx.key}" https://api.com""""#,
                json!({"key": "sk-123"})
            ),
            json!(r#"curl -H "Authorization: Bearer sk-123" https://api.com"#)
        );
    }

    #[test]
    fn test_fstring_triple_json_template() {
        let result = eval_with(
            r#"f"""{{"channel":"{$ctx.channel}","data":{{"content":"{$ctx.content}"}}}}""""#,
            json!({"channel": "chat:dm:abc", "content": "hello"}),
        );
        let s = result.as_str().unwrap();
        assert!(s.contains(r#""channel":"chat:dm:abc""#));
        assert!(s.contains(r#""content":"hello""#));
    }

    #[test]
    fn test_fstring_triple_multiline() {
        let input = "f\"\"\"Hello\n{$ctx.name}\nBye\"\"\"";
        assert_eq!(
            eval_with(input, json!({"name": "World"})),
            json!("Hello\nWorld\nBye")
        );
    }

    #[test]
    fn test_fstring_triple_escaped_braces() {
        assert_eq!(
            eval(r#"f"""literal {{braces}}""""#),
            json!("literal {braces}")
        );
    }
}
