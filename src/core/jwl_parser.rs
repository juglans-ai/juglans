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

    fn peek_ahead(&self, offset: usize) -> &TokenKind {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].kind
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

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind)
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
            | TokenKind::New
            | TokenKind::Class => {
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
                    TokenKind::Class => "class",
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
                TokenKind::Class => {
                    self.parse_class_def(&mut wf)?;
                }
                TokenKind::Ident(s) if is_meta_key(s) => {
                    self.parse_metadata(&mut wf)?;
                }
                // entry/exit are also identifiers that happen to be meta keys
                TokenKind::Ident(_) => {
                    self.parse_metadata(&mut wf)?;
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

        Ok(wf)
    }

    pub fn parse_manifest(&mut self) -> Result<WorkflowGraph> {
        let mut wf = WorkflowGraph::default();
        self.skip_newlines();
        while !self.at_eof() {
            self.skip_newlines();
            if self.at_eof() {
                break;
            }
            self.parse_metadata(&mut wf)?;
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
            "slug" => wf.slug = self.parse_meta_string_value()?,
            "name" => wf.name = self.parse_meta_string_value()?,
            "version" => wf.version = self.parse_meta_string_value()?,
            "source" => wf.source = self.parse_meta_string_value()?,
            "author" => wf.author = self.parse_meta_string_value()?,
            "description" => wf.description = self.parse_meta_string_value()?,
            "is_public" => {
                wf.is_public = Some(self.parse_meta_bool_value()?);
            }
            "schedule" => {
                wf.schedule = Some(self.parse_meta_string_value()?);
            }
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
            "entry" => {
                let list = self.parse_meta_string_list()?;
                wf.entry_node = list.into_iter().next().unwrap_or_default();
            }
            "exit" => {
                wf.exit_nodes = self.parse_meta_string_list()?;
            }
            "prompts" => wf.prompt_patterns = self.parse_meta_string_list()?,
            "agents" => wf.agent_patterns = self.parse_meta_string_list()?,
            "tools" => wf.tool_patterns = self.parse_meta_string_list()?,
            "python" => wf.python_imports = self.parse_meta_string_list()?,
            _ => {
                // Unknown meta key — skip value
                self.capture_expression(false, true, false)?;
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

    fn parse_meta_bool_value(&mut self) -> Result<bool> {
        match self.peek_kind() {
            TokenKind::True => {
                self.advance();
                Ok(true)
            }
            TokenKind::False => {
                self.advance();
                Ok(false)
            }
            TokenKind::String(s) => {
                let val = strip_string_quotes(s) == "true";
                self.advance();
                Ok(val)
            }
            _ => {
                let tok = self.peek().clone();
                Err(self.error_at(tok.span, "Expected boolean value".into()))
            }
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

    // ==================== Node definition ====================

    fn parse_node_def(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::LBracket)?;
        let node_id = self.expect_ident_or_keyword()?;
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
            TokenKind::Variable(_) => {
                let nt = self.parse_method_call_node()?;
                self.add_node(wf, &node_id, nt)?;
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

    fn is_struct_init(&self, _first_ident: &str) -> bool {
        // Check if pattern is Ident { ... } (uppercase first letter convention)
        // Look ahead: current is Ident, next non-newline is {
        let mut i = self.pos + 1;
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
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
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
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RParen)?;
        Ok(params)
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
            // Multi-step: { step; step; ... }
            self.expect(&TokenKind::LBrace)?;
            self.skip_newlines();
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
            body: Box::new(body),
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
                TokenKind::Variable(v) => {
                    let v = v.clone();
                    self.advance();
                    serde_json::Value::String(v)
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
            TokenKind::Variable(v) => {
                self.advance();
                Ok(v.trim_start_matches('$').to_string())
            }
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
                    format!(
                        "Expected variable or identifier, found {}",
                        tok.kind.describe()
                    ),
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
        let class_name = self.expect_ident()?;
        let args = self.parse_param_pairs(&class_name)?;
        Ok(NodeType::NewInstance { class_name, args })
    }

    fn parse_method_call_node(&mut self) -> Result<NodeType> {
        let tok = self.peek().clone();
        let var_ref = match &tok.kind {
            TokenKind::Variable(v) => v.clone(),
            _ => return Err(self.error_at(tok.span, "Expected variable reference".into())),
        };
        self.advance();

        // variable_ref includes the $, and method is already consumed via dots
        // Format: $instance.path.method — but the lexer captured the full $x.y.z as one token
        // We need to split off the last segment as the method name
        let clean = var_ref.trim_start_matches('$');
        let (instance_path, method_name) = clean.rsplit_once('.').ok_or_else(|| {
            self.error_at(
                tok.span,
                format!(
                    "Invalid method call '{}': expected $instance.method",
                    var_ref
                ),
            )
        })?;

        let args = self.parse_param_pairs(&var_ref)?;
        Ok(NodeType::MethodCall {
            instance_path: instance_path.to_string(),
            method_name: method_name.to_string(),
            args,
        })
    }

    fn parse_struct_init(&mut self) -> Result<NodeType> {
        let class_name = self.expect_ident()?;
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
            ClassDef {
                fields,
                methods: HashMap::new(),
            },
        );
        Ok(())
    }

    // ==================== Class definition ====================

    fn parse_class_def(&mut self, wf: &mut WorkflowGraph) -> Result<()> {
        self.expect(&TokenKind::Class)?;
        let class_name = self.expect_ident()?;

        // Optional constructor params: Counter(count=0)
        let fields = if matches!(self.peek_kind(), TokenKind::LParen) {
            self.parse_class_field_params()?
        } else {
            Vec::new()
        };

        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut body_fields = Vec::new();
        let mut methods = HashMap::new();

        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::RBrace) {
                break;
            }
            if matches!(self.peek_kind(), TokenKind::LBracket) {
                // Method: [method_name(params)]: body
                self.expect(&TokenKind::LBracket)?;
                let method_name = self.expect_ident_or_keyword()?;
                let params = if matches!(self.peek_kind(), TokenKind::LParen) {
                    self.parse_func_params()?
                } else {
                    Vec::new()
                };
                self.expect(&TokenKind::RBracket)?;
                self.expect(&TokenKind::Colon)?;
                self.skip_newlines();

                let full_name = format!("{}.{}", class_name, method_name);
                let func_def = self.parse_function_body_into_def(&full_name, params)?;
                methods.insert(method_name, func_def);
            } else {
                // Field declaration: name: type = default
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
                body_fields.push(ClassField {
                    name,
                    type_hint: Some(type_hint),
                    default,
                });
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;

        let mut all_fields = fields;
        all_fields.extend(body_fields);

        wf.classes.insert(
            class_name,
            ClassDef {
                fields: all_fields,
                methods,
            },
        );
        Ok(())
    }

    fn parse_class_field_params(&mut self) -> Result<Vec<ClassField>> {
        self.expect(&TokenKind::LParen)?;
        let mut fields = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RParen | TokenKind::Eof) {
            let name = self.expect_ident_or_keyword()?;
            let default = if matches!(self.peek_kind(), TokenKind::Eq) {
                self.advance();
                self.skip_newlines();
                Some(self.capture_param_value()?)
            } else {
                None
            };
            fields.push(ClassField {
                name,
                type_hint: None,
                default,
            });
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RParen)?;
        Ok(fields)
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
                    let to_id = self.parse_node_ref()?;
                    commit_edge_to_graph(wf, &from_id, &to_id, Edge::default())?;

                    // Continue chain
                    while matches!(self.peek_kind(), TokenKind::Arrow) {
                        self.advance();
                        self.skip_newlines();
                        let next_id = self.parse_node_ref()?;
                        commit_edge_to_graph(wf, &to_id, &next_id, Edge::default())?;
                        // Note: for chains longer than 2, we need to track the last id properly
                        // For now the pest parser also has this limitation in how it tracks last_id
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
        let mut name = self.expect_ident_or_keyword()?;
        while matches!(self.peek_kind(), TokenKind::Dot) {
            self.advance();
            let part = self.expect_ident_or_keyword()?;
            name = format!("{}.{}", name, part);
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
                    TokenKind::Variable(v) => {
                        let v = v.clone();
                        self.advance();
                        Some(v)
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
    } else if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
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
            name: "Test"
            entry: [start]
            [start]: notify(message="hello")
        "#,
        )
        .unwrap();
        assert!(wf.node_map.contains_key("start"));
        assert_eq!(wf.name, "Test");
    }

    #[test]
    fn test_chain_edge() {
        let wf = parse(
            r#"
            entry: [a]
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
        "#,
        )
        .unwrap();
        assert!(wf.switch_routes.contains_key("start"));
        let sr = wf.switch_routes.get("start").unwrap();
        assert_eq!(sr.subject.trim(), "$type");
        assert_eq!(sr.cases.len(), 3);
    }

    #[test]
    fn test_compound_block() {
        let wf = parse(
            r#"
            entry: [run]
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
            name: "Test"
            entry: [step1]
            [greet(name)]: bash(command="echo " + $name)
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
            name: "Test"
            entry: [step1]
            [build(dir)]: {
                bash(command="cd " + $dir + " && make")
                bash(command="cd " + $dir + " && make test")
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
            name: "Test"
            entry: [start]
            [start]: notify(message="hello" status="ok")
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_param() {
        let result = parse(
            r#"
            name: "Test"
            entry: [start]
            [start]: notify(message="first", message="second")
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_assignment_block() {
        let wf = parse(
            r#"
            name: "Assignment Test"
            entry: [init]
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
    fn test_class_definition() {
        let wf = parse(
            r#"
            name: "Class Test"
            entry: [c]
            class Counter(count=0) {
                [increment(n)]: count = $self.count + $n
                [reset]: count = 0
            }
            [c]: new Counter(count=10)
            [r]: $c.increment(n=5)
            [c] -> [r]
        "#,
        )
        .unwrap();
        assert!(wf.classes.contains_key("Counter"));
        let class = wf.classes.get("Counter").unwrap();
        assert_eq!(class.fields.len(), 1);
        assert!(class.methods.contains_key("increment"));
        assert!(class.methods.contains_key("reset"));
    }

    #[test]
    fn test_nested_function_calls() {
        let wf = parse(r#"
            entry: [start]
            [start]: chat(message=p(slug="test", user_message=$input.message, articles=map($data, x => {"title": x.title})))
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
            entry: [start]
            [start]: chat(
                agent="helper",
                message=$input.query
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
            name: "Foreach"
            entry: [loop]
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
            entry: [start]
            [start]: notify(message="test")
            [a]: notify(message="a")
            [b]: notify(message="b")
            [start] if $output.category == "technical" -> [a]
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
            entry: [start]
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
            entry: [start]
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
            name: "Python Workflow"
            python: ["pandas", "sklearn.ensemble", "./utils.py"]
            entry: [load]
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
            entry: [start]
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
            name: "Scoped Call Test"
            python: ["pandas"]
            entry: [load]
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
            entry: [test]
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
}
