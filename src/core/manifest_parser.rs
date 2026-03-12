// src/core/manifest_parser.rs
//
// .jgflow Manifest parser — independent of .jg workflow parser
// Only handles metadata declarations, outputs Manifest struct

use anyhow::{anyhow, Result};
use std::collections::HashMap;

use crate::core::graph::Manifest;
use crate::core::jwl_lexer::Lexer;
use crate::core::jwl_token::{Span, Token, TokenKind};

pub struct ManifestParser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> ManifestParser<'a> {
    pub fn parse(content: &str) -> Result<Manifest> {
        let tokens = Lexer::new(content)
            .tokenize()
            .map_err(|e| anyhow!("Manifest Syntax Error:\n{}", e))?;
        let mut parser = ManifestParser {
            tokens: &tokens,
            pos: 0,
        };
        parser.parse_all()
    }

    fn parse_all(&mut self) -> Result<Manifest> {
        let mut m = Manifest::default();
        self.skip_newlines();

        while !self.at_eof() {
            self.skip_newlines();
            if self.at_eof() {
                break;
            }
            self.parse_field(&mut m)?;
        }
        Ok(m)
    }

    fn parse_field(&mut self, m: &mut Manifest) -> Result<()> {
        let key = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        self.skip_newlines();

        match key.as_str() {
            "slug" => m.slug = self.parse_string_value()?,
            "name" => m.name = self.parse_string_value()?,
            "version" => m.version = self.parse_string_value()?,
            "source" => m.source = self.parse_string_value()?,
            "author" => m.author = self.parse_string_value()?,
            "description" => m.description = self.parse_string_value()?,
            "is_public" => m.is_public = Some(self.parse_bool_value()?),
            "schedule" => m.schedule = Some(self.parse_string_value()?),
            "entry" => {
                let list = self.parse_string_list()?;
                m.entry_node = list.into_iter().next().unwrap_or_default();
            }
            "exit" => m.exit_nodes = self.parse_string_list()?,
            "flows" => self.parse_map_into(&mut m.flow_imports)?,
            "libs" => {
                if matches!(self.peek_kind(), TokenKind::LBrace) {
                    self.parse_map_into(&mut m.lib_imports)?;
                } else {
                    let list = self.parse_string_list()?;
                    for path in &list {
                        let stem = std::path::Path::new(path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(path)
                            .to_string();
                        m.lib_imports.insert(stem.clone(), path.clone());
                        m.lib_auto_namespaces.insert(stem);
                    }
                    m.libs.extend(list);
                }
            }
            "prompts" => m.prompt_patterns = self.parse_string_list()?,
            "agents" => m.agent_patterns = self.parse_string_list()?,
            "tools" => m.tool_patterns = self.parse_string_list()?,
            "python" => m.python_imports = self.parse_string_list()?,
            _ => {
                // Unknown key — skip value token
                self.skip_value();
            }
        }
        Ok(())
    }

    // ==================== Helpers ====================

    fn parse_string_value(&mut self) -> Result<String> {
        let tok = self.peek().clone();
        match &tok.kind {
            TokenKind::String(s) => {
                let s = strip_quotes(s);
                self.advance();
                Ok(s)
            }
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            _ => Err(self.error_at(tok.span, "Expected string or identifier value")),
        }
    }

    fn parse_bool_value(&mut self) -> Result<bool> {
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
                let val = strip_quotes(s) == "true";
                self.advance();
                Ok(val)
            }
            _ => {
                let tok = self.peek().clone();
                Err(self.error_at(tok.span, "Expected boolean value"))
            }
        }
    }

    fn parse_string_list(&mut self) -> Result<Vec<String>> {
        if !matches!(self.peek_kind(), TokenKind::LBracket) {
            let val = self.parse_string_value()?;
            return Ok(vec![val]);
        }
        self.expect(&TokenKind::LBracket)?;
        let mut items = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RBracket | TokenKind::Eof) {
            items.push(self.parse_string_value()?);
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(items)
    }

    fn parse_map_into(&mut self, map: &mut HashMap<String, String>) -> Result<()> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::RBrace | TokenKind::Eof) {
            let key = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            self.skip_newlines();
            let val = self.parse_string_value()?;
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

    /// Skip a value token (for unknown keys)
    fn skip_value(&mut self) {
        match self.peek_kind() {
            TokenKind::LBracket => {
                self.advance();
                let mut depth = 1;
                while depth > 0 && !self.at_eof() {
                    match self.peek_kind() {
                        TokenKind::LBracket => depth += 1,
                        TokenKind::RBracket => depth -= 1,
                        _ => {}
                    }
                    self.advance();
                }
            }
            TokenKind::LBrace => {
                self.advance();
                let mut depth = 1;
                while depth > 0 && !self.at_eof() {
                    match self.peek_kind() {
                        TokenKind::LBrace => depth += 1,
                        TokenKind::RBrace => depth -= 1,
                        _ => {}
                    }
                    self.advance();
                }
            }
            _ => {
                self.advance();
            }
        }
    }

    // ==================== Token navigation ====================

    fn peek(&self) -> &Token {
        static EOF_TOKEN: std::sync::LazyLock<Token> = std::sync::LazyLock::new(|| Token {
            kind: TokenKind::Eof,
            span: Span::default(),
        });
        self.tokens.get(self.pos).unwrap_or(&EOF_TOKEN)
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn advance(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline) {
            self.advance();
        }
    }

    fn expect(&mut self, expected: &TokenKind) -> Result<()> {
        let tok = self.peek().clone();
        if std::mem::discriminant(&tok.kind) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error_at(
                tok.span,
                &format!(
                    "Expected {}, got {}",
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
            // Allow keywords as identifiers in metadata context
            _ if is_keyword_as_ident(&tok.kind) => {
                let s = keyword_to_str(&tok.kind).to_string();
                self.advance();
                Ok(s)
            }
            _ => Err(self.error_at(
                tok.span,
                &format!("Expected identifier, got {}", tok.kind.describe()),
            )),
        }
    }

    fn error_at(&self, span: Span, msg: &str) -> anyhow::Error {
        anyhow!(
            "Manifest Syntax Error at line {}, col {}: {}",
            span.line,
            span.col,
            msg
        )
    }
}

fn strip_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn is_keyword_as_ident(kind: &TokenKind) -> bool {
    matches!(
        kind,
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
    )
}

fn keyword_to_str(kind: &TokenKind) -> &'static str {
    match kind {
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
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_manifest() {
        let m = ManifestParser::parse(
            r#"
            slug: "my-flow"
            name: "My Flow"
            version: "1.0.0"
            source: "./main.jg"
            entry: [start]
        "#,
        )
        .unwrap();
        assert_eq!(m.slug, "my-flow");
        assert_eq!(m.name, "My Flow");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.source, "./main.jg");
        assert_eq!(m.entry_node, "start");
    }

    #[test]
    fn test_manifest_with_imports() {
        let m = ManifestParser::parse(
            r#"
            name: "Test"
            source: "./main.jg"
            libs: ["./utils.jg"]
            prompts: ["./prompts/*.jgprompt"]
            agents: ["./agents/*.jgagent"]
        "#,
        )
        .unwrap();
        assert!(m.lib_imports.contains_key("utils"));
        assert_eq!(m.prompt_patterns, vec!["./prompts/*.jgprompt"]);
        assert_eq!(m.agent_patterns, vec!["./agents/*.jgagent"]);
    }

    #[test]
    fn test_manifest_with_schedule() {
        let m = ManifestParser::parse(
            r#"
            name: "Cron Job"
            source: "./cron.jg"
            schedule: "0 9 * * *"
            is_public: true
        "#,
        )
        .unwrap();
        assert_eq!(m.schedule, Some("0 9 * * *".to_string()));
        assert_eq!(m.is_public, Some(true));
    }
}
