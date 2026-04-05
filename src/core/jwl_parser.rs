// src/core/jwl_parser.rs
// Hand-written recursive descent parser for JWL workflow syntax.
// Replaces pest-based jwl.pest parsing. Expressions are captured as raw strings
// and evaluated at runtime by expr_eval.rs (unchanged).

use crate::core::graph::{
    Action, ClassDef, ClassField, Edge, FunctionDef, Node, NodeType, SwitchCase, SwitchRoute,
    WorkflowGraph,
};
use crate::core::jwl_token::{is_meta_key, Span, Token, TokenKind};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;

pub struct JwlParser<'a> {
    tokens: &'a [Token],
    source: &'a str,
    pos: usize,
}

impl<'a> JwlParser<'a> {
    pub fn new(tokens: &'a [Token], source: &'a str) -> Self {
        Self {
            tokens,
            source,
            pos: 0,
        }
    }

    // ==================== Token utilities ====================

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline) {
            self.advance();
        }
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn expect(&mut self, expected: &TokenKind) -> Result<&Token> {
        let tok = self.peek();
        if std::mem::discriminant(&tok.kind) == std::mem::discriminant(expected) {
            Ok(self.advance())
        } else {
            Err(self.error_at(
                tok.span,
                format!(
                    "Expected {}, found {}",
                    expected.describe(),
                    tok.kind.describe()
                ),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            _ => Err(self.error_at(
                tok.span,
                format!("Expected identifier, found {}", tok.kind.describe()),
            )),
        }
    }

    /// Consume an identifier, but also accept keywords (since many identifiers share
    /// spelling with keywords, e.g. "error" as a node name, "default" as a field name).
    fn expect_ident_or_keyword(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            // Accept keywords as identifiers in contexts where they are valid names
            TokenKind::If
            | TokenKind::On
            | TokenKind::Error
            | TokenKind::Switch
            | TokenKind::Default
            | TokenKind::Ok
            | TokenKind::Err
            | TokenKind::Return
            | TokenKind::Foreach
            | TokenKind::Parallel
            | TokenKind::In
            | TokenKind::While
            | TokenKind::Assert
            | TokenKind::New => {
                let s = format!("{:?}", tok.kind).to_lowercase();
                // Keywords describe themselves in lowercase
                let name = match &tok.kind {
                    TokenKind::If => "if",
                    TokenKind::On => "on",
                    TokenKind::Error => "error",
                    TokenKind::Switch => "switch",
                    TokenKind::Default => "default",
                    TokenKind::Ok => "ok",
                    TokenKind::Err => "err",
                    TokenKind::Return => "return",
                    TokenKind::Foreach => "foreach",
                    TokenKind::Parallel => "parallel",
                    TokenKind::In => "in",
                    TokenKind::While => "while",
                    TokenKind::Assert => "assert",
                    TokenKind::New => "new",
                    _ => &s,
                };
                let name = name.to_string();
                self.advance();
                Ok(name)
            }
            _ => Err(self.error_at(
                tok.span,
                format!("Expected identifier, found {}", tok.kind.describe()),
            )),
        }
    }

    fn error_at(&self, span: Span, msg: String) -> anyhow::Error {
        let line_text = self.get_source_line(span.line);
        anyhow!(
            "error at line {}, col {}: {}\n  |\n{} | {}\n",
            span.line,
            span.col,
            msg,
            span.line,
            line_text
        )
    }

    fn get_source_line(&self, line_num: u32) -> &str {
        self.source
            .lines()
            .nth((line_num.saturating_sub(1)) as usize)
            .unwrap_or("")
    }

    // ==================== Expression capture ====================

    /// Capture raw source text as an expression, tracking balanced brackets.
    /// Stops when hitting an unbalanced terminator token at depth 0.
    /// `stop_at_comma` — stop at `,` at depth 0
    /// `stop_at_newline` — stop at newline at depth 0
    fn capture_expression(
        &mut self,
        stop_at_comma: bool,
        stop_at_newline: bool,
        stop_at_semicolon: bool,
    ) -> Result<String> {
        let start_pos = self.peek().span.start;
        let mut depth_paren: i32 = 0;
        let mut depth_bracket: i32 = 0;
        let mut depth_brace: i32 = 0;

        loop {
            let kind = self.peek_kind().clone();
            let at_depth_zero = depth_paren == 0 && depth_bracket == 0 && depth_brace == 0;

            match &kind {
                TokenKind::Eof => break,
                TokenKind::Newline if stop_at_newline && at_depth_zero => break,
                TokenKind::Comma if stop_at_comma && at_depth_zero => break,
                TokenKind::Semicolon if stop_at_semicolon && at_depth_zero => break,
                TokenKind::RParen if at_depth_zero => break,
                TokenKind::RBracket if at_depth_zero => break,
                TokenKind::RBrace if at_depth_zero => break,
                TokenKind::LParen => {
                    depth_paren += 1;
                    self.advance();
                }
                TokenKind::RParen => {
                    depth_paren -= 1;
                    self.advance();
                }
                TokenKind::LBracket => {
                    depth_bracket += 1;
                    self.advance();
                }
                TokenKind::RBracket => {
                    depth_bracket -= 1;
                    self.advance();
                }
                TokenKind::LBrace => {
                    depth_brace += 1;
                    self.advance();
                }
                TokenKind::RBrace => {
                    depth_brace -= 1;
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }

        let end_pos = self.peek().span.start;
        let captured = &self.source[start_pos..end_pos];
        Ok(captured.trim().to_string())
    }

    /// Capture expression for parameter values: stops at `,` or `)` at depth 0.
    /// Also detects missing commas by checking for bare `=` at depth 0.
    fn capture_param_value(&mut self) -> Result<String> {
        let start_pos = self.peek().span.start;
        let mut depth_paren: i32 = 0;
        let mut depth_bracket: i32 = 0;
        let mut depth_brace: i32 = 0;

        loop {
            let kind = self.peek_kind().clone();
            let at_depth_zero = depth_paren == 0 && depth_bracket == 0 && depth_brace == 0;

            match &kind {
                TokenKind::Eof => break,
                TokenKind::Comma if at_depth_zero => break,
                TokenKind::RParen if at_depth_zero => break,
                TokenKind::RBracket if at_depth_zero => break,
                TokenKind::RBrace if at_depth_zero => break,
                // Detect missing comma: bare `=` at depth 0 means `key=val key2=val2` without comma
                TokenKind::Eq if at_depth_zero => {
                    let tok = self.peek().clone();
                    return Err(
                        self.error_at(tok.span, "Missing comma between parameters".to_string())
                    );
                }
                TokenKind::LParen => {
                    depth_paren += 1;
                    self.advance();
                }
                TokenKind::RParen => {
                    depth_paren -= 1;
                    self.advance();
                }
                TokenKind::LBracket => {
                    depth_bracket += 1;
                    self.advance();
                }
                TokenKind::RBracket => {
                    depth_bracket -= 1;
                    self.advance();
                }
                TokenKind::LBrace => {
                    depth_brace += 1;
                    self.advance();
                }
                TokenKind::RBrace => {
                    depth_brace -= 1;
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }

        let end_pos = self.peek().span.start;
        let captured = &self.source[start_pos..end_pos];
        Ok(captured.trim().to_string())
    }

    /// Capture expression for assignment values: stops at `,` or newline or `;` or `}` at depth 0.
    fn capture_assign_value(&mut self) -> Result<String> {
        self.capture_expression(true, true, true)
    }

    // ==================== Top-level parsing ====================

    pub fn parse_workflow(&mut self) -> Result<WorkflowGraph> {
        let mut wf = WorkflowGraph::default();
        self.skip_newlines();

        while !self.at_eof() {
            self.skip_newlines();
            if self.at_eof() {
                break;
            }

            match self.peek_kind() {
                TokenKind::LBracket => {
                    // Could be node_def or edge_def — need lookahead
                    if self.is_edge_def() {
                        self.parse_edge_def(&mut wf)?;
                    } else {
                        self.parse_node_def(&mut wf)?;
                    }
                }
                TokenKind::At => {
                    self.parse_decorator_and_node(&mut wf)?;
                }
                TokenKind::Ident(s) if is_meta_key(s) => {
                    self.parse_metadata(&mut wf)?;
                }
                // entry/exit are also identifiers that happen to be meta keys
                TokenKind::Ident(_) => {
                    self.parse_metadata(&mut wf)?;
                }
                TokenKind::Impl => {
                    self.parse_impl_block(&mut wf)?;
                }
                TokenKind::Trait => {
                    self.parse_trait_def(&mut wf)?;
                }
                _ => {
                    let tok = self.peek().clone();
                    return Err(self.error_at(
                        tok.span,
                        format!("Unexpected token {}", tok.kind.describe()),
                    ));
                }
            }
        }

        // Merge external method definitions into their ClassDefs
        for (type_name, method_name, func_def) in wf.pending_methods.drain(..) {
            if let Some(class_def) = wf.classes.get_mut(&type_name) {
                Arc::make_mut(class_def)
                    .methods
                    .insert(method_name, func_def);
            }
        }

        Ok(wf)
    }

    /// Lookahead to determine if current `[` starts an edge definition.
    /// Edge: [ref] -> ... or [ref] if ... or [ref] on error ...
    /// Node: [id]: ...
    fn is_edge_def(&self) -> bool {
        // Scan forward past [ ... ] to see what follows
        let mut i = self.pos + 1; // skip [
                                  // Skip past the node ref contents (possibly dotted id)
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::RBracket => {
                    i += 1;
                    break;
                }
                TokenKind::Eof => return false,
                _ => i += 1,
            }
        }
        // Skip optional func_params: (...)
        if i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::LParen) {
            // This has func_params — it's a node_def, not an edge
            // But wait: `[node] if ...` starts with ident, not '('
            // Actually if next after ] is '(' it could be [name(params)]: ... which is node_def
            return false;
        }
        // Now check what follows ]
        if i >= self.tokens.len() {
            return false;
        }
        // Skip newlines between ] and the next significant token
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        match &self.tokens[i].kind {
            TokenKind::Arrow => true,  // [a] -> ...
            TokenKind::If => true,     // [a] if ... -> ...
            TokenKind::On => true,     // [a] on error -> ...
            TokenKind::Colon => false, // [a]: ... — node_def
            _ => false,
        }
    }

    // ==================== Metadata ====================

    fn parse_metadata(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        let key = self.expect_ident_or_keyword()?;
        self.expect(&TokenKind::Colon)?;
        self.skip_newlines();

        match key.as_str() {
            "flows" => {
                self.parse_meta_map_into(&mut wf.flow_imports)?;
            }
            "libs" => {
                if matches!(self.peek_kind(), TokenKind::LBrace) {
                    // Object form: libs: { db: "./libs/sqlite.jg" }
                    self.parse_meta_map_into(&mut wf.lib_imports)?;
                } else {
                    // List form: libs: ["./libs/sqlite.jg"]
                    let list = self.parse_meta_string_list()?;
                    for path in &list {
                        let stem = std::path::Path::new(path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(path)
                            .to_string();
                        wf.lib_imports.insert(stem.clone(), path.clone());
                        wf.lib_auto_namespaces.insert(stem);
                    }
                    wf.libs.extend(list);
                }
            }
            "prompts" => wf.prompt_patterns = self.parse_meta_string_list()?,
            "tools" => wf.tool_patterns = self.parse_meta_string_list()?,
            "python" => wf.python_imports = self.parse_meta_string_list()?,
            "agents" => {
                // Removed: agents are now defined inline as map nodes.
                // Silently skip for compatibility with existing files.
                let _ = self.parse_meta_string_list()?;
            }
            _ => {
                return Err(self.error_at(
                    self.tokens[self.pos.saturating_sub(2)].span,
                    format!(
                        "Unknown metadata key '{}'. Valid keys: libs, flows, prompts, tools, python. \
                         Note: name/version/entry/exit/description belong in .jgflow manifest, not .jg files.",
                        key
                    ),
                ));
            }
        }
        Ok(())
    }

    fn parse_meta_string_value(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::String(s) => {
                let s = strip_string_quotes(s);
                self.advance();
                Ok(s)
            }
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            _ => Err(self.error_at(tok.span, "Expected string or identifier value".into())),
        }
    }

    fn parse_meta_string_list(&mut self) -> Result<Vec<String>> {
        // Could be a single value or [list]
        if !matches!(self.peek_kind(), TokenKind::LBracket) {
            // Single value
            let val = self.parse_meta_string_value()?;
            return Ok(vec![val]);
        }
        self.expect(&TokenKind::LBracket)?;
        let mut items = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
            let val = self.parse_meta_string_value()?;
            items.push(val);
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(items)
    }

    fn parse_meta_map_into(&mut self, map: &mut HashMap<String, String>) -> Result<()> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let key = self.expect_ident_or_keyword()?;
            self.expect(&TokenKind::Colon)?;
            self.skip_newlines();
            let val = self.parse_meta_string_value()?;
            map.insert(key, val);
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(())
    }

    // ==================== Decorator ====================

    /// Parse `@dotted.path(args...) \n [node_id]: body` and expand at compile time.
    /// Generates a synthetic registration node `[_deco_N]: path(args..., handler="node_id")`.
    fn parse_decorator_and_node(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        use crate::core::graph::DecoratorApplication;

        self.expect(&TokenKind::At)?;

        // Parse dotted path: e.g. api.post or bare name: get
        let mut path = self.expect_ident_or_keyword()?;
        while matches!(self.peek_kind(), TokenKind::Dot) {
            self.advance(); // consume .
            let part = self.expect_ident_or_keyword()?;
            path = format!("{}.{}", path, part);
        }

        // Parse optional args: (expr, expr, ...)
        let mut deco_args: Vec<String> = Vec::new();
        if matches!(self.peek_kind(), TokenKind::LParen) {
            self.advance(); // consume (
            self.skip_newlines();
            while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
                let arg = self.capture_expression(true, false, true)?;
                deco_args.push(arg);
                self.skip_newlines();
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.advance();
                }
                self.skip_newlines();
            }
            self.expect(&TokenKind::RParen)?;
        }

        self.skip_newlines();

        // Next must be [node_def] or another @decorator
        if !matches!(self.peek_kind(), TokenKind::LBracket | TokenKind::At) {
            let tok = self.peek().clone();
            return Err(self.error_at(
                tok.span,
                "Expected [node_def] or @decorator after @decorator".to_string(),
            ));
        }

        // If next is another @decorator, parse it first (stacking)
        if matches!(self.peek_kind(), TokenKind::At) {
            // Record this decorator, then parse the next one which will parse the node
            // We need to peek ahead to find the target node ID
            let target_id = self.peek_decorated_target_id()?;

            wf.decorator_applications.push(DecoratorApplication {
                decorator_fn: path,
                args: deco_args,
                target_node_id: target_id,
            });

            return self.parse_decorator_and_node(wf);
        }

        // Peek the node_id from [node_id] before parsing
        let decorated_node_id = if self.pos + 1 < self.tokens.len() {
            match &self.tokens[self.pos + 1].kind {
                TokenKind::Ident(s) => s.clone(),
                _ => {
                    return Err(anyhow!("Expected identifier after '[' in decorated node"));
                }
            }
        } else {
            return Err(anyhow!("Unexpected end of file after @decorator"));
        };

        // Reject dotted paths — @instance.method is no longer supported
        if path.contains('.') {
            return Err(anyhow!(
                "@{} is not supported. Use @function_name(args) instead of @instance.method(args).",
                path
            ));
        }

        // Parse the decorated node normally
        self.parse_node_def(wf)?;

        // Record as DecoratorApplication for macro expand phase
        wf.decorator_applications.push(DecoratorApplication {
            decorator_fn: path,
            args: deco_args,
            target_node_id: decorated_node_id,
        });

        Ok(())
    }

    /// Peek ahead through stacked @decorators to find the ultimate target [node_id].
    fn peek_decorated_target_id(&self) -> Result<String> {
        let mut pos = self.pos;
        while pos < self.tokens.len() {
            match &self.tokens[pos].kind {
                TokenKind::At => {
                    pos += 1; // skip @
                              // Skip decorator path and args
                    while pos < self.tokens.len() {
                        match &self.tokens[pos].kind {
                            TokenKind::LBracket | TokenKind::At => break,
                            TokenKind::Newline => {
                                pos += 1;
                                continue;
                            }
                            _ => pos += 1,
                        }
                    }
                }
                TokenKind::LBracket => {
                    // Found the target node
                    if pos + 1 < self.tokens.len() {
                        if let TokenKind::Ident(s) = &self.tokens[pos + 1].kind {
                            return Ok(s.clone());
                        }
                    }
                    return Err(anyhow!("Expected identifier after '[' in decorated node"));
                }
                TokenKind::Newline => {
                    pos += 1;
                }
                _ => {
                    pos += 1;
                }
            }
        }
        Err(anyhow!("Could not find target node for stacked decorators"))
    }

    // ==================== Node definition ====================

    fn parse_node_def(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::LBracket)?;
        let node_id = self.expect_ident_or_keyword()?;

        // Dot after ident: check if it's an external method definition [Type.method(self)]
        if matches!(self.peek_kind(), TokenKind::Dot) && wf.classes.contains_key(&node_id) {
            return self.parse_ext_method_def(&node_id, wf);
        }

        // Check for optional func_params before ]
        let func_params = if matches!(self.peek_kind(), TokenKind::LParen) {
            Some(self.parse_func_params()?)
        } else {
            None
        };
        self.expect(&TokenKind::RBracket)?;
        self.expect(&TokenKind::Colon)?;
        self.skip_newlines();

        // If has func_params → function definition
        if let Some(params) = func_params {
            let func_def = self.parse_function_body_into_def(&node_id, params)?;
            wf.functions.insert(node_id, func_def);
            return Ok(());
        }

        // Determine node body type
        match self.peek_kind().clone() {
            TokenKind::LBrace => {
                // Could be: struct_body, func_body, json_object, or struct_init
                if self.is_struct_body() {
                    self.parse_struct_body_into_class(&node_id, wf)?;
                } else if self.is_json_object() {
                    // JSON object literal: { "key": value, ... }
                    let val = self.parse_json_object_literal()?;
                    self.add_node(wf, &node_id, NodeType::Literal(val))?;
                } else {
                    // Compound block (func_body) — expand inline into DAG
                    self.parse_compound_block(&node_id, wf)?;
                }
            }
            TokenKind::Return => {
                let nt = self.parse_return_err()?;
                self.add_node(wf, &node_id, nt)?;
            }
            TokenKind::While => {
                let nt = self.parse_while_def()?;
                self.add_node(wf, &node_id, nt)?;
            }
            TokenKind::Foreach => {
                let nt = self.parse_foreach_def()?;
                self.add_node(wf, &node_id, nt)?;
            }
            TokenKind::New => {
                let nt = self.parse_new_expr()?;
                self.add_node(wf, &node_id, nt)?;
            }
            TokenKind::Yield => {
                self.advance();
                let expr = self.capture_expression(false, true, true)?;
                self.add_node(wf, &node_id, NodeType::Yield(expr))?;
            }
            TokenKind::Ident(s) => {
                let s = s.clone();
                // Disambiguate: task_def (ident "(" ...), struct_init (Ident "{" ...),
                // or assignment_block (ident "=" ...)
                if self.lookahead_is_task_call() {
                    let nt = self.parse_task_def(&node_id)?;
                    self.add_node(wf, &node_id, nt)?;
                } else if self.is_struct_init(&s) {
                    let nt = self.parse_struct_init()?;
                    self.add_node(wf, &node_id, nt)?;
                } else {
                    // Assignment block: key = value, ...
                    let nt = self.parse_assignment_block()?;
                    self.add_node(wf, &node_id, nt)?;
                }
            }
            TokenKind::String(s) => {
                let raw = s.clone();
                self.advance();
                let val: serde_json::Value = serde_json::from_str(&raw)
                    .unwrap_or(serde_json::Value::String(strip_string_quotes(&raw)));
                self.add_node(wf, &node_id, NodeType::Literal(val))?;
            }
            TokenKind::Number(n) => {
                let raw = n.clone();
                self.advance();
                let val: serde_json::Value =
                    serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw));
                self.add_node(wf, &node_id, NodeType::Literal(val))?;
            }
            TokenKind::True | TokenKind::False => {
                let v = matches!(self.peek_kind(), TokenKind::True);
                self.advance();
                self.add_node(wf, &node_id, NodeType::Literal(serde_json::Value::Bool(v)))?;
            }
            TokenKind::Null => {
                self.advance();
                self.add_node(wf, &node_id, NodeType::Literal(serde_json::Value::Null))?;
            }
            _ => {
                let tok = self.peek().clone();
                return Err(self.error_at(
                    tok.span,
                    format!("Unexpected node body starting with {}", tok.kind.describe()),
                ));
            }
        }

        Ok(())
    }

    fn add_node(&self, wf: &mut WorkflowGraph, id: &str, node_type: NodeType) -> Result<()> {
        let node = Node {
            id: id.to_string(),
            node_type,
        };
        let idx = wf.graph.add_node(node);
        wf.node_map.insert(id.to_string(), idx);
        Ok(())
    }

    fn parse_func_params(&mut self) -> Result<Vec<String>> {
        self.expect(&TokenKind::LParen)?;
        let mut params = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
            let name = self.expect_ident()?;
            params.push(name);
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RParen)?;
        Ok(params)
    }

    /// Check if `{ ... }` is a struct body: peek for `ident : type_hint` pattern.
    fn is_struct_body(&self) -> bool {
        // Look past { to first content
        let mut i = self.pos + 1; // skip {
                                  // Skip newlines
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        if i >= self.tokens.len() {
            return false;
        }
        // Need: ident : ident pattern (field_name : type_hint)
        let is_ident = matches!(&self.tokens[i].kind, TokenKind::Ident(_));
        if !is_ident {
            return false;
        }
        i += 1;
        // Skip newlines
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        if i >= self.tokens.len() {
            return false;
        }
        matches!(self.tokens[i].kind, TokenKind::Colon)
    }

    /// Check if current `{` starts a JSON object literal (string keys: `{ "key": ... }`)
    fn is_json_object(&self) -> bool {
        let mut i = self.pos + 1; // skip {
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        if i >= self.tokens.len() {
            return false;
        }
        matches!(&self.tokens[i].kind, TokenKind::String(_))
    }

    /// Parse a JSON object literal: `{ "key": value, ... }`
    /// Consumes tokens from `{` through `}` and returns a serde_json::Value::Object.
    fn parse_json_object_literal(&mut self) -> Result<serde_json::Value> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut map = serde_json::Map::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            // Key must be a string
            let key = match self.peek_kind().clone() {
                TokenKind::String(s) => {
                    self.advance();
                    strip_string_quotes(&s)
                }
                _ => {
                    let tok = self.peek().clone();
                    return Err(self.error_at(
                        tok.span,
                        format!(
                            "Expected string key in JSON object, got {}",
                            tok.kind.describe()
                        ),
                    ));
                }
            };

            self.expect(&TokenKind::Colon)?;

            // Value: string, number, true, false, null, nested object, array
            let value = self.parse_json_value()?;
            map.insert(key, value);

            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(serde_json::Value::Object(map))
    }

    /// Parse a single JSON value (string, number, bool, null, object, array).
    fn parse_json_value(&mut self) -> Result<serde_json::Value> {
        match self.peek_kind().clone() {
            TokenKind::String(s) => {
                self.advance();
                Ok(serde_json::Value::String(strip_string_quotes(&s)))
            }
            TokenKind::Number(n) => {
                self.advance();
                Ok(serde_json::from_str(&n).unwrap_or(serde_json::Value::String(n)))
            }
            TokenKind::True => {
                self.advance();
                Ok(serde_json::Value::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(serde_json::Value::Bool(false))
            }
            TokenKind::Null => {
                self.advance();
                Ok(serde_json::Value::Null)
            }
            TokenKind::LBrace => self.parse_json_object_literal(),
            TokenKind::LBracket => {
                self.advance(); // [
                let mut arr = Vec::new();
                self.skip_newlines();
                while !matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
                    arr.push(self.parse_json_value()?);
                    self.skip_newlines();
                    if matches!(self.peek_kind(), TokenKind::Comma) {
                        self.advance();
                    }
                    self.skip_newlines();
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(serde_json::Value::Array(arr))
            }
            _ => {
                let tok = self.peek().clone();
                Err(self.error_at(
                    tok.span,
                    format!("Expected JSON value, got {}", tok.kind.describe()),
                ))
            }
        }
    }

    fn is_struct_init(&self, _first_ident: &str) -> bool {
        // Check if pattern is Ident[.Ident]* { ... } (struct init)
        // Skip dotted name parts, then check for {
        let mut i = self.pos + 1;
        // Skip .Ident chains (e.g., ai_tools.AI_TOOL)
        while i + 1 < self.tokens.len()
            && matches!(self.tokens[i].kind, TokenKind::Dot)
            && matches!(self.tokens[i + 1].kind, TokenKind::Ident(_))
        {
            i += 2;
        }
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::LBrace)
    }

    /// Lookahead: is the current identifier followed (possibly with dots) by `(`?
    fn lookahead_is_task_call(&self) -> bool {
        let mut i = self.pos + 1;
        // Skip dots and identifiers for scoped_identifier
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::Dot => i += 1,
                TokenKind::Ident(_) => i += 1,
                _ => break,
            }
        }
        i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::LParen)
    }

    // ==================== Task definition ====================

    fn parse_task_def(&mut self, context_id: &str) -> Result<NodeType> {
        let name = self.parse_scoped_identifier()?;
        if name == "set_context" {
            let span = self.peek().span;
            return Err(self.error_at(
                span,
                "set_context() is no longer supported. Use assignment syntax instead: key = value"
                    .into(),
            ));
        }
        let params = self.parse_param_pairs(context_id)?;
        Ok(NodeType::Task(Action { name, params }))
    }

    fn parse_scoped_identifier(&mut self) -> Result<String> {
        let mut name = self.expect_ident_or_keyword()?;
        while matches!(self.peek_kind(), TokenKind::Dot) {
            self.advance(); // .
            let part = self.expect_ident_or_keyword()?;
            name = format!("{}.{}", name, part);
        }
        Ok(name)
    }

    fn parse_param_pairs(&mut self, context_id: &str) -> Result<HashMap<String, String>> {
        self.expect(&TokenKind::LParen)?;
        let mut params = HashMap::new();
        let mut positional_index: usize = 0;
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
            // Check if this is a named parameter (ident =) or a positional argument
            let is_named = self.is_named_param();

            if is_named {
                // Named: key=value
                let key = self.expect_ident_or_keyword()?;
                self.expect(&TokenKind::Eq)?;
                self.skip_newlines();
                let value = self.capture_param_value()?;
                if params.contains_key(&key) {
                    return Err(anyhow!(
                        "Duplicate parameter '{}' in node [{}]",
                        key,
                        context_id
                    ));
                }
                params.insert(key, value);
            } else {
                // Positional: capture as arg0, arg1, ...
                let value = self.capture_param_value()?;
                let key = format!("arg{}", positional_index);
                params.insert(key, value);
                positional_index += 1;
            }

            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RParen)?;
        Ok(params)
    }

    /// Check if the current position is a named parameter (ident followed by =).
    fn is_named_param(&self) -> bool {
        if self.pos + 1 >= self.tokens.len() {
            return false;
        }
        matches!(
            (&self.tokens[self.pos].kind, &self.tokens[self.pos + 1].kind),
            (TokenKind::Ident(_), TokenKind::Eq)
        )
    }

    // ==================== Compound block (func_body) ====================

    /// Parse { step; step; ... } and expand inline into the main DAG.
    fn parse_compound_block(&mut self, root_id: &str, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut step_index = 0;
        let mut last_idx: Option<petgraph::graph::NodeIndex> = None;

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let step_id = if step_index == 0 {
                root_id.to_string()
            } else {
                format!("{}.__{}", root_id, step_index)
            };

            let node_type = self.parse_func_step(&step_id)?;
            let node = Node {
                id: step_id.clone(),
                node_type,
            };
            let idx = wf.graph.add_node(node);
            wf.node_map.insert(step_id, idx);

            if let Some(prev_idx) = last_idx {
                wf.graph.add_edge(prev_idx, idx, Edge::default());
            }

            last_idx = Some(idx);
            step_index += 1;

            // Optional semicolon between steps
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Semicolon) {
                self.advance();
            }
            self.skip_newlines();
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(())
    }

    /// Parse a single step inside a compound block.
    fn parse_func_step(&mut self, context_id: &str) -> Result<NodeType> {
        match self.peek_kind().clone() {
            TokenKind::Assert => {
                self.advance(); // consume 'assert'
                let expr = self.capture_expression(false, true, true)?;
                Ok(NodeType::Assert(expr))
            }
            TokenKind::Return => self.parse_return_err(),
            TokenKind::Yield => {
                self.advance();
                let expr = self.capture_expression(false, true, true)?;
                Ok(NodeType::Yield(expr))
            }
            TokenKind::Ident(_) => {
                // Could be: assign_call (ident = task(...)), task_def (ident(...)),
                // or assignment_block (ident = value)
                if self.is_assign_call() {
                    self.parse_assign_call(context_id)
                } else if self.lookahead_is_task_call() {
                    self.parse_task_def(context_id)
                } else {
                    // assignment_block
                    self.parse_assignment_block()
                }
            }
            _ => {
                let tok = self.peek().clone();
                Err(self.error_at(
                    tok.span,
                    format!(
                        "Unexpected token in compound block: {}",
                        tok.kind.describe()
                    ),
                ))
            }
        }
    }

    /// Check if current position is `ident = scoped_ident(key=val, ...)` pattern (assign_call).
    /// Must have keyword args (param_pairs), not positional args.
    fn is_assign_call(&self) -> bool {
        let mut i = self.pos;
        // First: ident
        if !matches!(&self.tokens[i].kind, TokenKind::Ident(_)) {
            return false;
        }
        i += 1;
        // Then: =
        if i >= self.tokens.len() || !matches!(self.tokens[i].kind, TokenKind::Eq) {
            return false;
        }
        i += 1;
        // Skip newlines
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        // Then: ident (possibly scoped with dots)
        if i >= self.tokens.len() || !matches!(&self.tokens[i].kind, TokenKind::Ident(_)) {
            return false;
        }
        i += 1;
        // Skip dots + idents for scoped identifier
        while i + 1 < self.tokens.len()
            && matches!(self.tokens[i].kind, TokenKind::Dot)
            && matches!(&self.tokens[i + 1].kind, TokenKind::Ident(_))
        {
            i += 2;
        }
        // Then: (
        if i >= self.tokens.len() || !matches!(self.tokens[i].kind, TokenKind::LParen) {
            return false;
        }
        i += 1;
        // Skip newlines
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        // Check first arg is keyword style: ident =
        // If first thing after ( is ) → empty args, still valid assign_call
        if i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::RParen) {
            return true;
        }
        // Must see ident = pattern (keyword arg)
        if i >= self.tokens.len() || !matches!(&self.tokens[i].kind, TokenKind::Ident(_)) {
            return false;
        }
        i += 1;
        // Skip newlines
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Eq)
    }

    fn parse_assign_call(&mut self, context_id: &str) -> Result<NodeType> {
        let var_name = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        self.skip_newlines();
        let name = self.parse_scoped_identifier()?;
        let params = self.parse_param_pairs(context_id)?;
        Ok(NodeType::AssignCall {
            var: var_name,
            action: Action { name, params },
        })
    }

    // ==================== Function body (for FunctionDef) ====================

    fn parse_function_body_into_def(
        &mut self,
        name: &str,
        params: Vec<String>,
    ) -> Result<FunctionDef> {
        let mut body = WorkflowGraph::default();

        if matches!(self.peek_kind(), TokenKind::LBrace) {
            self.expect(&TokenKind::LBrace)?;
            self.skip_newlines();

            if matches!(self.peek_kind(), TokenKind::LBracket) {
                // DAG mode: { [name]: action(); [a] -> [b]; ... }
                // Supports named nodes, conditional edges, switch — full workflow syntax.
                while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                    self.skip_newlines();
                    if matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                        break;
                    }
                    match self.peek_kind() {
                        TokenKind::LBracket => {
                            if self.is_edge_def() {
                                self.parse_edge_def(&mut body)?;
                            } else {
                                self.parse_node_def(&mut body)?;
                            }
                        }
                        TokenKind::Ident(_) => {
                            // Bare assignment: var = expr (auto-named node)
                            let step_id = format!("__{}", body.node_map.len());
                            let node_type = self.parse_assignment_block()?;
                            let node = Node {
                                id: step_id.clone(),
                                node_type,
                            };
                            let idx = body.graph.add_node(node);
                            body.node_map.insert(step_id, idx);
                        }
                        _ => {
                            let tok = self.peek().clone();
                            return Err(self.error_at(
                                tok.span,
                                format!(
                                    "Unexpected token in function body: {}",
                                    tok.kind.describe()
                                ),
                            ));
                        }
                    }
                }
                self.expect(&TokenKind::RBrace)?;

                // Auto-detect entry node: first node with no incoming edges
                if body.entry_node.is_empty() {
                    use petgraph::Direction;
                    for idx in body.graph.node_indices() {
                        if body
                            .graph
                            .neighbors_directed(idx, Direction::Incoming)
                            .next()
                            .is_none()
                        {
                            body.entry_node = body.graph[idx].id.clone();
                            break;
                        }
                    }
                }
            } else {
                // Linear mode: { step; step; ... } (existing behavior)
                let mut step_index = 0;
                let mut last_idx: Option<petgraph::graph::NodeIndex> = None;

                while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                    let step_id = format!("__{}", step_index);
                    let context_id = format!("{}.__{}", name, step_index);
                    let node_type = self.parse_func_step(&context_id)?;
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

                    self.skip_newlines();
                    if matches!(self.peek_kind(), TokenKind::Semicolon) {
                        self.advance();
                    }
                    self.skip_newlines();
                }
                self.expect(&TokenKind::RBrace)?;
            }
        } else if self.lookahead_is_task_call() || matches!(self.peek_kind(), TokenKind::Ident(_)) {
            // Single-step
            let step_id = "__0".to_string();
            let node_type = if self.lookahead_is_task_call() {
                self.parse_task_def(name)?
            } else {
                self.parse_assignment_block()?
            };
            let node = Node {
                id: step_id.clone(),
                node_type,
            };
            let idx = body.graph.add_node(node);
            body.node_map.insert(step_id.clone(), idx);
            body.entry_node = "__0".to_string();
        } else {
            let tok = self.peek().clone();
            return Err(self.error_at(
                tok.span,
                format!(
                    "Function '{}' body must be a task call, assignment, or a {{...}} block",
                    name
                ),
            ));
        }

        Ok(FunctionDef {
            params,
            body: Arc::new(body),
            annotations: HashMap::new(),
        })
    }

    // ==================== Assignment block ====================

    fn parse_assignment_block(&mut self) -> Result<NodeType> {
        let mut params = HashMap::new();
        loop {
            let key = self.expect_ident_or_keyword()?;
            self.expect(&TokenKind::Eq)?;
            self.skip_newlines();
            let value = self.capture_assign_value()?;
            params.insert(key, value);

            // Check for comma or end
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
                // Check if next is still an assignment (ident = ...)
                if !matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(NodeType::Task(Action {
            name: "set_context".to_string(),
            params,
        }))
    }

    // ==================== Return err ====================

    fn parse_return_err(&mut self) -> Result<NodeType> {
        self.expect(&TokenKind::Return)?;
        self.expect(&TokenKind::Err)?;
        // Parse JSON-like object: { kind: "...", message: "..." }
        // Keys may be unquoted identifiers (DSL style) — build a proper Value
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut map = serde_json::Map::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let key = self.expect_ident_or_keyword()?;
            self.expect(&TokenKind::Colon)?;
            let tok = self.peek().clone();
            let value = match &tok.kind {
                TokenKind::String(s) => {
                    let v = strip_string_quotes(s);
                    self.advance();
                    serde_json::Value::String(v)
                }
                TokenKind::Number(n) => {
                    let v: serde_json::Value =
                        serde_json::from_str(n).unwrap_or(serde_json::Value::String(n.clone()));
                    self.advance();
                    v
                }
                TokenKind::True => {
                    self.advance();
                    serde_json::Value::Bool(true)
                }
                TokenKind::False => {
                    self.advance();
                    serde_json::Value::Bool(false)
                }
                TokenKind::Null => {
                    self.advance();
                    serde_json::Value::Null
                }
                _ => {
                    // Capture as raw expression string (for template rendering)
                    let expr = self.capture_expression(true, true, false)?;
                    serde_json::Value::String(expr)
                }
            };
            map.insert(key, value);
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(NodeType::ReturnErr(serde_json::Value::Object(map)))
    }

    // ==================== While / Foreach ====================

    fn parse_while_def(&mut self) -> Result<NodeType> {
        self.expect(&TokenKind::While)?;
        self.expect(&TokenKind::LParen)?;
        let condition = self.capture_expression(false, false, false)?;
        self.expect(&TokenKind::RParen)?;
        self.skip_newlines();
        let inner_graph = self.parse_block_body()?;
        Ok(NodeType::Loop {
            condition,
            body: Box::new(inner_graph),
        })
    }

    fn parse_foreach_def(&mut self) -> Result<NodeType> {
        self.expect(&TokenKind::Foreach)?;
        let parallel = if matches!(self.peek_kind(), TokenKind::Parallel) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(&TokenKind::LParen)?;

        // item in list
        let item = self.parse_var_or_ident()?;
        self.expect(&TokenKind::In)?;
        let list = self.parse_var_or_ident()?;

        self.expect(&TokenKind::RParen)?;
        self.skip_newlines();
        let inner_graph = self.parse_block_body()?;

        Ok(NodeType::Foreach {
            item,
            list,
            body: Box::new(inner_graph),
            parallel,
        })
    }

    fn parse_var_or_ident(&mut self) -> Result<String> {
        match self.peek_kind().clone() {
            TokenKind::Ident(s) => {
                let mut name = s.clone();
                self.advance();
                while matches!(self.peek_kind(), TokenKind::Dot) {
                    self.advance();
                    let part = self.expect_ident()?;
                    name = format!("{}.{}", name, part);
                }
                Ok(name)
            }
            _ => {
                let tok = self.peek().clone();
                Err(self.error_at(
                    tok.span,
                    format!("Expected identifier, found {}", tok.kind.describe()),
                ))
            }
        }
    }

    fn parse_block_body(&mut self) -> Result<WorkflowGraph> {
        self.expect(&TokenKind::LBrace)?;
        let mut inner = WorkflowGraph::default();
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            match self.peek_kind() {
                TokenKind::LBracket => {
                    if self.is_edge_def() {
                        self.parse_edge_def(&mut inner)?;
                    } else {
                        self.parse_node_def(&mut inner)?;
                    }
                }
                _ => {
                    let tok = self.peek().clone();
                    return Err(self.error_at(
                        tok.span,
                        format!(
                            "Expected node or edge definition in block, found {}",
                            tok.kind.describe()
                        ),
                    ));
                }
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(inner)
    }

    // ==================== New / Method call / Struct init ====================

    fn parse_new_expr(&mut self) -> Result<NodeType> {
        self.expect(&TokenKind::New)?;
        let mut class_name = self.expect_ident()?;
        while matches!(self.peek_kind(), TokenKind::Dot) {
            self.advance();
            let part = self.expect_ident()?;
            class_name = format!("{}.{}", class_name, part);
        }
        let args = self.parse_param_pairs(&class_name)?;
        Ok(NodeType::NewInstance { class_name, args })
    }

    // parse_method_call_node removed — $ prefix no longer supported.
    // Method calls on instances (instance.method()) are now handled at runtime
    // through the scoped task_def path (parse_task_def → NodeType::Task).

    fn parse_struct_init(&mut self) -> Result<NodeType> {
        let mut class_name = self.expect_ident()?;
        while matches!(self.peek_kind(), TokenKind::Dot) {
            self.advance();
            let part = self.expect_ident()?;
            class_name = format!("{}.{}", class_name, part);
        }
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut args = HashMap::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let key = self.expect_ident_or_keyword()?;
            self.expect(&TokenKind::Eq)?;
            self.skip_newlines();
            let value = self.capture_assign_value()?;
            args.insert(key, value);
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(NodeType::NewInstance { class_name, args })
    }

    // ==================== Struct body (as ClassDef) ====================

    fn parse_struct_body_into_class(&mut self, id: &str, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut fields = Vec::new();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let name = self.expect_ident_or_keyword()?;
            self.expect(&TokenKind::Colon)?;
            self.skip_newlines();
            let type_hint = self.expect_ident()?;
            let default = if matches!(self.peek_kind(), TokenKind::Eq) {
                self.advance();
                self.skip_newlines();
                Some(self.capture_assign_value()?)
            } else {
                None
            };
            fields.push(ClassField {
                name,
                type_hint: Some(type_hint),
                default,
            });
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;

        wf.classes.insert(
            id.to_string(),
            Arc::new(ClassDef::new(fields, HashMap::new())),
        );
        Ok(())
    }

    // ==================== External method definition ====================
    // [Type.method(self, params)]: body → pending_methods → merge into ClassDef

    fn parse_ext_method_def(&mut self, type_name: &str, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::Dot)?;
        let method_name = self.expect_ident_or_keyword()?;

        let mut params = if matches!(self.peek_kind(), TokenKind::LParen) {
            self.parse_func_params()?
        } else {
            Vec::new()
        };

        self.expect(&TokenKind::RBracket)?;
        self.expect(&TokenKind::Colon)?;
        self.skip_newlines();

        // Filter out `self` from params
        if params.first().map(|s| s.as_str()) == Some("self") {
            params.remove(0);
        }

        let full_name = format!("{}.{}", type_name, method_name);
        let func_def = self.parse_function_body_into_def(&full_name, params)?;
        wf.pending_methods
            .push((type_name.to_string(), method_name, func_def));
        Ok(())
    }

    // ==================== impl block ====================
    // impl Type { [method(self)]: body; ... }
    // impl Trait for Type { [method(self)]: body; ... }

    fn parse_impl_block(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::Impl)?;
        let first_name = self.expect_ident()?;
        self.skip_newlines();

        // Check for `impl Trait for Type` form
        let type_name = if matches!(self.peek_kind(), TokenKind::For) {
            self.advance(); // consume 'for'
            self.skip_newlines();
            let target = self.expect_ident()?;

            // Copy default methods from the trait into the target class
            if let Some(trait_class) = wf.classes.get(&first_name) {
                let trait_class = trait_class.clone();
                for (method_name, func_def) in &trait_class.methods {
                    wf.pending_methods.push((
                        target.clone(),
                        method_name.clone(),
                        func_def.clone(),
                    ));
                }
            }

            target
        } else {
            first_name
        };

        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            // Expect [method(params)]: body
            self.expect(&TokenKind::LBracket)?;
            let method_name = self.expect_ident_or_keyword()?;
            let mut params = if matches!(self.peek_kind(), TokenKind::LParen) {
                self.parse_func_params()?
            } else {
                Vec::new()
            };
            self.expect(&TokenKind::RBracket)?;
            self.expect(&TokenKind::Colon)?;
            self.skip_newlines();

            // Filter out `self` from params
            if params.first().map(|s| s.as_str()) == Some("self") {
                params.remove(0);
            }

            let full_name = format!("{}.{}", type_name, method_name);
            let func_def = self.parse_function_body_into_def(&full_name, params)?;
            wf.pending_methods
                .push((type_name.clone(), method_name, func_def));
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(())
    }

    // ==================== trait definition ====================
    // trait Name { [method(self)]: default_body; [required(self)]: }

    fn parse_trait_def(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::Trait)?;
        let trait_name = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut methods: HashMap<String, FunctionDef> = HashMap::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
                break;
            }
            // [method(self)]: body_or_empty
            self.expect(&TokenKind::LBracket)?;
            let method_name = self.expect_ident_or_keyword()?;
            let mut params = if matches!(self.peek_kind(), TokenKind::LParen) {
                self.parse_func_params()?
            } else {
                Vec::new()
            };
            self.expect(&TokenKind::RBracket)?;
            self.expect(&TokenKind::Colon)?;

            // Filter out `self` from params
            if params.first().map(|s| s.as_str()) == Some("self") {
                params.remove(0);
            }

            // Check if this is a required method (no body) or has a default body
            self.skip_newlines();
            if matches!(
                self.peek_kind(),
                TokenKind::LBracket | TokenKind::RBrace | TokenKind::Eof
            ) {
                // Required method stub — no body, skip
                // We don't store it; the trait ClassDef only holds default methods
            } else {
                // Has a default body
                let full_name = format!("{}.{}", trait_name, method_name);
                let func_def = self.parse_function_body_into_def(&full_name, params)?;
                methods.insert(method_name, func_def);
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;

        // Store trait as a ClassDef with no fields but with default methods
        wf.classes
            .insert(trait_name, Arc::new(ClassDef::new(Vec::new(), methods)));
        Ok(())
    }

    // ==================== Edge definitions ====================

    fn parse_edge_def(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        // Parse source node ref
        let from_id = self.parse_node_ref()?;

        match self.peek_kind().clone() {
            TokenKind::Arrow => {
                self.advance(); // ->
                self.skip_newlines();
                // Could be: switch, or chain/simple target
                if matches!(self.peek_kind(), TokenKind::Switch) {
                    self.parse_switch_edge_body(&from_id, wf)?;
                } else {
                    // Chain or simple edge: [a] -> [b] -> [c]
                    let mut last_id = self.parse_node_ref()?;
                    commit_edge_to_graph(wf, &from_id, &last_id, Edge::default())?;

                    // Continue chain
                    while matches!(self.peek_kind(), TokenKind::Arrow) {
                        self.advance();
                        self.skip_newlines();
                        let next_id = self.parse_node_ref()?;
                        commit_edge_to_graph(wf, &last_id, &next_id, Edge::default())?;
                        last_id = next_id;
                    }
                }
            }
            TokenKind::If => {
                // [a] if condition -> [b]
                self.advance(); // if
                let condition = self.capture_edge_condition()?;
                self.expect(&TokenKind::Arrow)?;
                self.skip_newlines();
                let to_id = self.parse_node_ref()?;
                let edge = Edge {
                    condition: Some(condition),
                    is_error_path: false,
                    switch_case: None,
                };
                commit_edge_to_graph(wf, &from_id, &to_id, edge)?;
            }
            TokenKind::On => {
                // [a] on error -> [b]
                self.advance(); // on
                self.expect(&TokenKind::Error)?;
                self.expect(&TokenKind::Arrow)?;
                self.skip_newlines();
                let to_id = self.parse_node_ref()?;
                let edge = Edge {
                    condition: None,
                    is_error_path: true,
                    switch_case: None,
                };
                commit_edge_to_graph(wf, &from_id, &to_id, edge)?;
            }
            _ => {
                let tok = self.peek().clone();
                return Err(self.error_at(
                    tok.span,
                    format!(
                        "Expected '->', 'if', or 'on' after node ref, found {}",
                        tok.kind.describe()
                    ),
                ));
            }
        }

        Ok(())
    }

    fn parse_node_ref(&mut self) -> Result<String> {
        self.expect(&TokenKind::LBracket)?;
        let mut name = String::new();
        loop {
            match self.peek_kind().clone() {
                TokenKind::RBracket => break,
                TokenKind::Ident(s) => {
                    name.push_str(&s);
                    self.advance();
                }
                TokenKind::Dot => {
                    name.push('.');
                    self.advance();
                }
                // Allow keywords as part of node names (e.g. [ok], [error])
                TokenKind::If
                | TokenKind::On
                | TokenKind::Error
                | TokenKind::Switch
                | TokenKind::Default
                | TokenKind::Ok
                | TokenKind::Err
                | TokenKind::Return
                | TokenKind::Foreach
                | TokenKind::Parallel
                | TokenKind::In
                | TokenKind::While
                | TokenKind::Assert
                | TokenKind::New => {
                    let kw = match self.peek_kind() {
                        TokenKind::If => "if",
                        TokenKind::On => "on",
                        TokenKind::Error => "error",
                        TokenKind::Switch => "switch",
                        TokenKind::Default => "default",
                        TokenKind::Ok => "ok",
                        TokenKind::Err => "err",
                        TokenKind::Return => "return",
                        TokenKind::Foreach => "foreach",
                        TokenKind::Parallel => "parallel",
                        TokenKind::In => "in",
                        TokenKind::While => "while",
                        TokenKind::Assert => "assert",
                        TokenKind::New => "new",
                        _ => unreachable!(),
                    };
                    name.push_str(kw);
                    self.advance();
                }
                _ => {
                    let tok = self.peek().clone();
                    return Err(self.error_at(
                        tok.span,
                        format!(
                            "Expected identifier or '.' in node reference, found {}",
                            tok.kind.describe()
                        ),
                    ));
                }
            }
        }
        if name.is_empty() {
            let tok = self.peek().clone();
            return Err(self.error_at(tok.span, "Empty node reference".to_string()));
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(name)
    }

    fn capture_edge_condition(&mut self) -> Result<String> {
        // Capture everything until ->
        let start_pos = self.peek().span.start;
        while !matches!(self.peek_kind(), TokenKind::Arrow | TokenKind::Eof) {
            self.advance();
        }
        let end_pos = self.peek().span.start;
        let captured = &self.source[start_pos..end_pos];
        Ok(captured.trim().to_string())
    }

    fn parse_switch_edge_body(&mut self, from_id: &str, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::Switch)?;

        // Optional subject expression before {
        let subject = if !matches!(self.peek_kind(), TokenKind::LBrace) {
            self.capture_switch_subject()?
        } else {
            String::new()
        };

        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut cases = Vec::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                break;
            }

            // Case value or default / ok / err
            let mut is_ok = false;
            let mut is_err = false;
            let mut err_kind: Option<String> = None;

            let case_value = if matches!(self.peek_kind(), TokenKind::Default) {
                self.advance();
                None
            } else if matches!(self.peek_kind(), TokenKind::Ok) {
                self.advance();
                is_ok = true;
                Some("__ok__".to_string())
            } else if matches!(self.peek_kind(), TokenKind::Err) {
                self.advance();
                is_err = true;
                // Optional err kind: err "timeout"
                if let TokenKind::String(s) = self.peek_kind().clone() {
                    let kind = strip_string_quotes(&s);
                    err_kind = Some(kind.clone());
                    self.advance();
                    Some(format!("__err_{}__", kind))
                } else {
                    Some("__err__".to_string())
                }
            } else {
                let tok = self.peek().clone();
                match &tok.kind {
                    TokenKind::String(s) => {
                        let v = strip_string_quotes(s);
                        self.advance();
                        Some(v)
                    }
                    TokenKind::Number(n) => {
                        let v = n.clone();
                        self.advance();
                        Some(v)
                    }
                    TokenKind::True => {
                        self.advance();
                        Some("true".to_string())
                    }
                    TokenKind::False => {
                        self.advance();
                        Some("false".to_string())
                    }
                    TokenKind::Ident(s) => {
                        let s = s.clone();
                        self.advance();
                        Some(s)
                    }
                    _ => {
                        return Err(self.error_at(
                            tok.span,
                            format!("Expected case value, found {}", tok.kind.describe()),
                        ));
                    }
                }
            };

            self.expect(&TokenKind::Colon)?;
            self.skip_newlines();

            let target_id = self.parse_node_ref()?;

            cases.push(SwitchCase {
                value: case_value.clone(),
                target: target_id.clone(),
                is_ok,
                is_err,
                err_kind,
            });

            let edge = Edge {
                condition: None,
                is_error_path: is_err,
                switch_case: case_value,
            };
            commit_edge_to_graph(wf, from_id, &target_id, edge)?;

            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }

        self.expect(&TokenKind::RBrace)?;

        wf.switch_routes
            .insert(from_id.to_string(), SwitchRoute { subject, cases });

        Ok(())
    }

    fn capture_switch_subject(&mut self) -> Result<String> {
        let start_pos = self.peek().span.start;
        let mut depth = 0;
        while !self.at_eof() {
            match self.peek_kind() {
                TokenKind::LBrace if depth == 0 => break,
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth -= 1;
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        let end_pos = self.peek().span.start;
        Ok(self.source[start_pos..end_pos].trim().to_string())
    }
}

// ==================== Edge commit (shared with parser.rs) ====================

fn commit_edge_to_graph(wf: &mut WorkflowGraph, f_id: &str, t_id: &str, e: Edge) -> Result<()> {
    // Wildcard edges → deferred to resolver phase for expansion
    if f_id.contains('*') || t_id.contains('*') {
        wf.pending_wildcard_edges
            .push((f_id.to_string(), t_id.to_string(), e));
        return Ok(());
    }

    let f_ns = f_id.contains('.');
    let t_ns = t_id.contains('.');

    if f_ns || t_ns {
        wf.pending_edges
            .push((f_id.to_string(), t_id.to_string(), e));
        return Ok(());
    }

    let f_idx = *wf.node_map.get(f_id).ok_or_else(|| {
        anyhow!(
            "Graph Error: Attempted to link from undefined node '{}'.",
            f_id
        )
    })?;
    let t_idx = *wf.node_map.get(t_id).ok_or_else(|| {
        anyhow!(
            "Graph Error: Attempted to link to undefined node '{}'.",
            t_id
        )
    })?;

    wf.graph.add_edge(f_idx, t_idx, e);
    Ok(())
}

// ==================== Helpers ====================

/// Strip surrounding quotes from a string token's raw text.
fn strip_string_quotes(s: &str) -> String {
    if s.starts_with("\"\"\"") && s.ends_with("\"\"\"") && s.len() >= 6 {
        s[3..s.len() - 3].to_string()
    } else if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::jwl_lexer::Lexer;

    fn parse(content: &str) -> Result<WorkflowGraph> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("{}", e))?;
        let mut parser = JwlParser::new(&tokens, content);
        let mut wf = parser.parse_workflow()?;

        // Auto-set entry node like GraphParser::parse does
        if wf.entry_node.is_empty() {
            if let Some(first_idx) = wf.graph.node_indices().next() {
                wf.entry_node = wf.graph[first_idx].id.clone();
            }
        }

        Ok(wf)
    }

    #[test]
    fn test_simple_node() {
        let wf = parse(
            r#"
            [start]: notify(message="hello")
        "#,
        )
        .unwrap();
        assert!(wf.node_map.contains_key("start"));
        assert_eq!(wf.entry_node, "start");
    }

    #[test]
    fn test_chain_edge() {
        let wf = parse(
            r#"
            [a]: notify(message="a")
            [b]: notify(message="b")
            [a] -> [b]
        "#,
        )
        .unwrap();
        assert_eq!(wf.graph.edge_count(), 1);
    }

    #[test]
    fn test_switch_edge() {
        let wf = parse(
            r#"
            [start]: notify(message="start")
            [case_a]: notify(message="A")
            [case_b]: notify(message="B")
            [fallback]: notify(message="default")
            [start] -> switch type {
                "a": [case_a]
                "b": [case_b]
                default: [fallback]
            }
        "#,
        )
        .unwrap();
        assert!(wf.switch_routes.contains_key("start"));
        let sr = wf.switch_routes.get("start").unwrap();
        assert_eq!(sr.subject.trim(), "type");
        assert_eq!(sr.cases.len(), 3);
    }

    #[test]
    fn test_compound_block() {
        let wf = parse(
            r#"
            [run]: {
                notify(message="step1")
                notify(message="step2")
            }
        "#,
        )
        .unwrap();
        assert!(wf.node_map.contains_key("run"));
        assert!(wf.node_map.contains_key("run.__1"));
        assert_eq!(wf.graph.edge_count(), 1);
    }

    #[test]
    fn test_function_def() {
        let wf = parse(
            r#"
            [greet(name)]: bash(command="echo " + name)
            [step1]: greet(name="world")
        "#,
        )
        .unwrap();
        assert!(wf.functions.contains_key("greet"));
        assert!(!wf.node_map.contains_key("greet"));
        assert!(wf.node_map.contains_key("step1"));
    }

    #[test]
    fn test_multi_step_function() {
        let wf = parse(
            r#"
            [build(dir)]: {
                bash(command="cd " + dir + " && make")
                bash(command="cd " + dir + " && make test")
            }
            [step1]: build(dir="/app")
        "#,
        )
        .unwrap();
        let func = wf.functions.get("build").unwrap();
        assert_eq!(func.params, vec!["dir"]);
        assert_eq!(func.body.node_map.len(), 2);
        assert_eq!(func.body.graph.edge_count(), 1);
    }

    #[test]
    fn test_missing_comma() {
        let result = parse(
            r#"
            [start]: notify(message="hello" status="ok")
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_param() {
        let result = parse(
            r#"
            [start]: notify(message="first", message="second")
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_assignment_block() {
        let wf = parse(
            r#"
            [init]: count = 0, name = "Alice"
            [next]: notify(message="done")
            [init] -> [next]
        "#,
        )
        .unwrap();
        let node = &wf.graph[*wf.node_map.get("init").unwrap()];
        if let NodeType::Task(action) = &node.node_type {
            assert_eq!(action.name, "set_context");
            assert_eq!(action.params.get("count").unwrap(), "0");
            assert_eq!(action.params.get("name").unwrap(), "\"Alice\"");
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_nested_function_calls() {
        let wf = parse(r#"
            [start]: chat(message=p(slug="test", user_message=input.message, articles=map(data, x => {"title": x.title})))
        "#).unwrap();
        let node = &wf.graph[*wf.node_map.get("start").unwrap()];
        if let NodeType::Task(action) = &node.node_type {
            let msg = action.params.get("message").unwrap();
            assert!(msg.contains("p("));
            assert!(msg.contains("map("));
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_multiline_params() {
        let wf = parse(
            r#"
            [start]: chat(
                agent="helper",
                message=input.query
            )
        "#,
        )
        .unwrap();
        assert!(wf.node_map.contains_key("start"));
    }

    #[test]
    fn test_triple_quoted_string() {
        let wf = parse(
            r#"
            [run]: bash(command="""echo "hello world" && echo '{"key":"value"}'""")
        "#,
        )
        .unwrap();
        let node = &wf.graph[*wf.node_map.get("run").unwrap()];
        if let NodeType::Task(action) = &node.node_type {
            let cmd = action.params.get("command").unwrap();
            assert!(cmd.contains(r#"echo "hello world""#));
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_foreach() {
        let wf = parse(
            r#"
            [loop]: foreach(item in input.items) {
                [step]: notify(message="ok")
            }
        "#,
        )
        .unwrap();
        let node = &wf.graph[*wf.node_map.get("loop").unwrap()];
        if let NodeType::Foreach { item, list, .. } = &node.node_type {
            assert_eq!(item, "item");
            assert_eq!(list, "input.items");
        } else {
            panic!("Expected Foreach node");
        }
    }

    #[test]
    fn test_condition_edge() {
        let wf = parse(
            r#"
            [start]: notify(message="test")
            [a]: notify(message="a")
            [b]: notify(message="b")
            [start] if output.category == "technical" -> [a]
            [start] -> [b]
        "#,
        )
        .unwrap();
        assert_eq!(wf.graph.edge_count(), 2);
    }

    #[test]
    fn test_on_error_edge() {
        let wf = parse(
            r#"
            [start]: notify(message="test")
            [fallback]: notify(message="error")
            [start] on error -> [fallback]
        "#,
        )
        .unwrap();
        assert_eq!(wf.graph.edge_count(), 1);
    }

    #[test]
    fn test_namespaced_edge() {
        let wf = parse(
            r#"
            [start]: notify(message="start")
            [start] -> [trading.extract]
        "#,
        )
        .unwrap();
        assert_eq!(wf.pending_edges.len(), 1);
        assert_eq!(wf.pending_edges[0].1, "trading.extract");
    }

    #[test]
    fn test_python_imports() {
        let wf = parse(
            r#"
            python: ["pandas", "sklearn.ensemble", "./utils.py"]
            [load]: pandas.read_csv(path="data.csv")
        "#,
        )
        .unwrap();
        assert_eq!(wf.python_imports.len(), 3);
        assert!(wf.python_imports.contains(&"pandas".to_string()));
    }

    #[test]
    fn test_flows_metadata() {
        let wf = parse(
            r#"
            flows: { trading: "./trading.jg", events: "./events.jg" }
            [start]: notify(message="start")
        "#,
        )
        .unwrap();
        assert_eq!(wf.flow_imports.get("trading").unwrap(), "./trading.jg");
    }

    #[test]
    fn test_scoped_task_call() {
        let wf = parse(
            r#"
            python: ["pandas"]
            [load]: pandas.read_csv(path="data.csv", encoding="utf-8")
        "#,
        )
        .unwrap();
        let node = &wf.graph[*wf.node_map.get("load").unwrap()];
        if let NodeType::Task(action) = &node.node_type {
            assert_eq!(action.name, "pandas.read_csv");
            assert_eq!(action.params.get("path"), Some(&"\"data.csv\"".to_string()));
        } else {
            panic!("Expected Task node");
        }
    }

    #[test]
    fn test_assign_call_in_block() {
        let wf = parse(
            r#"
            [test]: {
                data = vector_search(space="articles", query="test", limit=3, model="qwen")
                print(message="done")
            }
        "#,
        )
        .unwrap();
        assert!(wf.node_map.contains_key("test"));
        assert!(wf.node_map.contains_key("test.__1"));
        let node = &wf.graph[*wf.node_map.get("test").unwrap()];
        if let NodeType::AssignCall { var, action } = &node.node_type {
            assert_eq!(var, "data");
            assert_eq!(action.name, "vector_search");
        } else {
            panic!("Expected AssignCall, got {:?}", node.node_type);
        }
    }

    // ==================== External method definition ====================

    #[test]
    fn test_ext_method_basic() {
        let wf = parse(
            r#"
            [User]: {
                name: str
                age: int
            }
            [User.greet(self, prefix)]: {
                notify(message=prefix + " " + self.name)
            }
        "#,
        )
        .unwrap();
        let class = wf.classes.get("User").unwrap();
        assert!(class.methods.contains_key("greet"));
        // self should be filtered out
        assert_eq!(class.methods["greet"].params, vec!["prefix"]);
    }

    #[test]
    fn test_ext_method_no_params() {
        let wf = parse(
            r#"
            [Point]: {
                x: int
                y: int
            }
            [Point.describe(self)]: notify(message="point")
        "#,
        )
        .unwrap();
        let class = wf.classes.get("Point").unwrap();
        assert!(class.methods.contains_key("describe"));
        assert!(class.methods["describe"].params.is_empty());
    }

    #[test]
    fn test_ext_method_multiple() {
        let wf = parse(
            r#"
            [Item]: { name: str }
            [Item.save(self)]: notify(message="saving")
            [Item.delete(self)]: notify(message="deleting")
        "#,
        )
        .unwrap();
        let class = wf.classes.get("Item").unwrap();
        assert_eq!(class.methods.len(), 2);
        assert!(class.methods.contains_key("save"));
        assert!(class.methods.contains_key("delete"));
    }
}
