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

use super::expr_ast::{BinOp, Expr, UnaryOp};

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
        self.eval_expr(&ast, resolver)
    }

    /// Evaluate and return as bool using Python truthiness
    pub fn eval_as_bool<F>(&self, expr_str: &str, resolver: &F) -> Result<bool>
    where
        F: Fn(&str) -> Option<Value>,
    {
        let val = self.eval(expr_str, resolver)?;
        Ok(is_truthy(&val))
    }

    /// Evaluate and return as a Vec<Value> for iteration
    pub fn eval_as_array<F>(&self, expr_str: &str, resolver: &F) -> Result<Vec<Value>>
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
        Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
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
            .map(|p| self.parse_expression(p))
            .collect()
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
}

// ============================================================
// Evaluator: AST → Value
// ============================================================

impl ExprEvaluator {
    fn eval_expr<F>(&self, expr: &Expr, resolver: &F) -> Result<Value>
    where
        F: Fn(&str) -> Option<Value>,
    {
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
                let arg_vals: Result<Vec<Value>> =
                    args.iter().map(|a| self.eval_expr(a, resolver)).collect();
                call_builtin(name, &arg_vals?)
            }

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                // Desugar: obj.method(args) → method(obj, args)
                let obj_val = self.eval_expr(object, resolver)?;
                let mut all_args = vec![obj_val];
                for a in args {
                    all_args.push(self.eval_expr(a, resolver)?);
                }
                call_builtin(method, &all_args)
            }

            Expr::Pipe {
                value,
                filter,
                args,
            } => {
                // Desugar: value | filter(args) → filter(value, args)
                let val = self.eval_expr(value, resolver)?;
                let mut all_args = vec![val];
                for a in args {
                    all_args.push(self.eval_expr(a, resolver)?);
                }
                call_builtin(filter, &all_args)
            }
        }
    }

    fn eval_binary_op<F>(
        &self,
        left_expr: &Expr,
        op: BinOp,
        right_expr: &Expr,
        resolver: &F,
    ) -> Result<Value>
    where
        F: Fn(&str) -> Option<Value>,
    {
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
            Ok(Value::String(
                serde_json::to_string(&args[0]).unwrap_or_default(),
            ))
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

        _ => Err(anyhow!("Unknown function: {}()", name)),
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
mod tests {
    use super::*;
    use serde_json::json;

    fn make_resolver(ctx: Value) -> impl Fn(&str) -> Option<Value> {
        move |path: &str| {
            // Strip ctx. prefix (like executor does for WorkflowContext)
            let clean = if path.starts_with("ctx.") {
                &path[4..]
            } else {
                path
            };
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
        evaluator.eval_as_bool(expr, &resolver).unwrap()
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
}
