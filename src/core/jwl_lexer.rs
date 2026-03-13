// src/core/jwl_lexer.rs
// Single-pass character-by-character tokenizer for JWL workflow syntax.

use crate::core::jwl_token::{Span, Token, TokenKind};

pub struct Lexer<'a> {
    source: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

#[derive(Debug)]
pub struct LexError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lexer error at line {}, col {}: {}",
            self.line, self.col, self.message
        )
    }
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn peek_byte_at(&self, offset: usize) -> Option<u8> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance_byte(&mut self) -> Option<u8> {
        let b = self.source.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn make_span(&self, start: usize, start_line: u32, start_col: u32) -> Span {
        Span {
            start,
            end: self.pos,
            line: start_line,
            col: start_col,
        }
    }

    fn err(&self, msg: impl Into<String>) -> LexError {
        LexError {
            message: msg.into(),
            line: self.line,
            col: self.col,
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek_byte() {
                Some(b' ') | Some(b'\t') | Some(b'\r') => {
                    self.advance_byte();
                }
                Some(b'#') => {
                    // Skip comment to end of line
                    while let Some(b) = self.peek_byte() {
                        if b == b'\n' {
                            break;
                        }
                        self.advance_byte();
                    }
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments();

        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        let Some(b) = self.peek_byte() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: self.make_span(start, start_line, start_col),
            });
        };

        let kind = match b {
            b'\n' => {
                self.advance_byte();
                TokenKind::Newline
            }
            b'[' => {
                self.advance_byte();
                TokenKind::LBracket
            }
            b']' => {
                self.advance_byte();
                TokenKind::RBracket
            }
            b'(' => {
                self.advance_byte();
                TokenKind::LParen
            }
            b')' => {
                self.advance_byte();
                TokenKind::RParen
            }
            b'{' => {
                self.advance_byte();
                TokenKind::LBrace
            }
            b'}' => {
                self.advance_byte();
                TokenKind::RBrace
            }
            b':' => {
                self.advance_byte();
                TokenKind::Colon
            }
            b',' => {
                self.advance_byte();
                TokenKind::Comma
            }
            b';' => {
                self.advance_byte();
                TokenKind::Semicolon
            }
            b'.' => {
                self.advance_byte();
                TokenKind::Dot
            }
            b'-' => {
                if self.peek_byte_at(1) == Some(b'>') {
                    self.advance_byte();
                    self.advance_byte();
                    TokenKind::Arrow
                } else if matches!(self.peek_byte_at(1), Some(b'0'..=b'9')) {
                    self.lex_number()?
                } else {
                    // Bare minus — treat as part of expression content
                    self.advance_byte();
                    TokenKind::Ident("-".to_string())
                }
            }
            b'=' => {
                self.advance_byte();
                if self.peek_byte() == Some(b'=') {
                    self.advance_byte();
                    TokenKind::EqEq
                } else {
                    TokenKind::Eq
                }
            }
            b'!' => {
                self.advance_byte();
                if self.peek_byte() == Some(b'=') {
                    self.advance_byte();
                    TokenKind::NotEq
                } else {
                    TokenKind::Ident("!".to_string())
                }
            }
            b'>' => {
                self.advance_byte();
                if self.peek_byte() == Some(b'=') {
                    self.advance_byte();
                    TokenKind::GtEq
                } else {
                    TokenKind::Gt
                }
            }
            b'<' => {
                self.advance_byte();
                if self.peek_byte() == Some(b'=') {
                    self.advance_byte();
                    TokenKind::LtEq
                } else {
                    TokenKind::Lt
                }
            }
            b'"' => self.lex_string()?,
            b'\'' => self.lex_single_quoted_string()?,
            b'$' => {
                return Err(self.err(
                    "The '$' prefix is no longer supported. Use bare identifiers instead: \
                     '$ctx.x' → 'x', '$input.x' → 'input.x', '$output' → 'output'"
                        .to_string(),
                ));
            }
            b'0'..=b'9' => self.lex_number()?,
            b'@' => {
                self.advance_byte();
                TokenKind::At
            }
            b'+' | b'*' | b'/' | b'%' | b'|' | b'&' => {
                self.advance_byte();
                TokenKind::Ident(String::from(b as char))
            }
            _ if is_ident_start(b) => self.lex_identifier_or_keyword()?,
            _ => {
                return Err(self.err(format!("Unexpected character: '{}'", b as char)));
            }
        };

        Ok(Token {
            kind,
            span: self.make_span(start, start_line, start_col),
        })
    }

    fn lex_string(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        // Consume opening "
        self.advance_byte();

        // Check for triple-quoted """
        if self.peek_byte() == Some(b'"') && self.peek_byte_at(1) == Some(b'"') {
            self.advance_byte(); // second "
            self.advance_byte(); // third "

            // Check for f-string prefix: was the char before start an 'f'?
            // (handled below after we capture the content)

            // Read until closing """
            loop {
                match self.peek_byte() {
                    None => return Err(self.err("Unterminated triple-quoted string")),
                    Some(b'"')
                        if self.peek_byte_at(1) == Some(b'"')
                            && self.peek_byte_at(2) == Some(b'"') =>
                    {
                        self.advance_byte();
                        self.advance_byte();
                        self.advance_byte();
                        break;
                    }
                    _ => {
                        self.advance_byte();
                    }
                }
            }
        } else {
            // Regular double-quoted string
            loop {
                match self.peek_byte() {
                    None | Some(b'\n') => {
                        return Err(self.err("Unterminated string"));
                    }
                    Some(b'\\') => {
                        self.advance_byte(); // backslash
                        self.advance_byte(); // escaped char
                    }
                    Some(b'"') => {
                        self.advance_byte();
                        break;
                    }
                    _ => {
                        self.advance_byte();
                    }
                }
            }
        }

        let raw = std::str::from_utf8(&self.source[start..self.pos])
            .unwrap_or("")
            .to_string();
        Ok(TokenKind::String(raw))
    }

    fn lex_single_quoted_string(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        self.advance_byte(); // opening '
        loop {
            match self.peek_byte() {
                None | Some(b'\n') => return Err(self.err("Unterminated single-quoted string")),
                Some(b'\\') => {
                    self.advance_byte();
                    self.advance_byte();
                }
                Some(b'\'') => {
                    self.advance_byte();
                    break;
                }
                _ => {
                    self.advance_byte();
                }
            }
        }
        let raw = std::str::from_utf8(&self.source[start..self.pos])
            .unwrap_or("")
            .to_string();
        Ok(TokenKind::String(raw))
    }

    // lex_variable removed — $ prefix is no longer supported

    fn lex_number(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        // Optional leading minus
        if self.peek_byte() == Some(b'-') {
            self.advance_byte();
        }
        while let Some(b'0'..=b'9') = self.peek_byte() {
            self.advance_byte();
        }
        // Optional decimal
        if self.peek_byte() == Some(b'.') && matches!(self.peek_byte_at(1), Some(b'0'..=b'9')) {
            self.advance_byte(); // .
            while let Some(b'0'..=b'9') = self.peek_byte() {
                self.advance_byte();
            }
        }
        let raw = std::str::from_utf8(&self.source[start..self.pos])
            .unwrap_or("0")
            .to_string();
        Ok(TokenKind::Number(raw))
    }

    fn lex_identifier_or_keyword(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance_byte();
            } else {
                break;
            }
        }
        let word = std::str::from_utf8(&self.source[start..self.pos]).unwrap_or("");

        // Check for f-string: f"..." or f"""..."""
        if word == "f" && self.peek_byte() == Some(b'"') {
            return self.lex_fstring();
        }

        let kind = match word {
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            "if" => TokenKind::If,
            "on" => TokenKind::On,
            "error" => TokenKind::Error,
            "switch" => TokenKind::Switch,
            "default" => TokenKind::Default,
            "foreach" => TokenKind::Foreach,
            "parallel" => TokenKind::Parallel,
            "in" => TokenKind::In,
            "while" => TokenKind::While,
            "assert" => TokenKind::Assert,
            "ok" => TokenKind::Ok,
            "err" => TokenKind::Err,
            "return" => TokenKind::Return,
            "new" => TokenKind::New,
            "yield" => TokenKind::Yield,
            _ => TokenKind::Ident(word.to_string()),
        };
        Ok(kind)
    }

    fn lex_fstring(&mut self) -> Result<TokenKind, LexError> {
        // We already consumed "f", now at the opening "
        let start = self.pos - 1; // include the 'f'
        self.advance_byte(); // first "

        if self.peek_byte() == Some(b'"') && self.peek_byte_at(1) == Some(b'"') {
            // f"""..."""
            self.advance_byte();
            self.advance_byte();
            loop {
                match self.peek_byte() {
                    None => return Err(self.err("Unterminated f-string")),
                    Some(b'"')
                        if self.peek_byte_at(1) == Some(b'"')
                            && self.peek_byte_at(2) == Some(b'"') =>
                    {
                        self.advance_byte();
                        self.advance_byte();
                        self.advance_byte();
                        break;
                    }
                    _ => {
                        self.advance_byte();
                    }
                }
            }
        } else {
            // f"..."
            loop {
                match self.peek_byte() {
                    None | Some(b'\n') => return Err(self.err("Unterminated f-string")),
                    Some(b'\\') => {
                        self.advance_byte();
                        self.advance_byte();
                    }
                    Some(b'"') => {
                        self.advance_byte();
                        break;
                    }
                    _ => {
                        self.advance_byte();
                    }
                }
            }
        }

        let raw = std::str::from_utf8(&self.source[start..self.pos])
            .unwrap_or("")
            .to_string();
        Ok(TokenKind::String(raw))
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Vec<TokenKind> {
        Lexer::new(s)
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Newline))
            .collect()
    }

    #[test]
    fn test_simple_tokens() {
        let tokens = lex("[start]: notify(message=\"hello\")");
        assert_eq!(tokens[0], TokenKind::LBracket);
        assert_eq!(tokens[1], TokenKind::Ident("start".into()));
        assert_eq!(tokens[2], TokenKind::RBracket);
        assert_eq!(tokens[3], TokenKind::Colon);
        assert_eq!(tokens[4], TokenKind::Ident("notify".into()));
        assert_eq!(tokens[5], TokenKind::LParen);
        assert_eq!(tokens[6], TokenKind::Ident("message".into()));
        assert_eq!(tokens[7], TokenKind::Eq);
        assert!(matches!(tokens[8], TokenKind::String(_)));
        assert_eq!(tokens[9], TokenKind::RParen);
        assert_eq!(tokens[10], TokenKind::Eof);
    }

    #[test]
    fn test_arrow() {
        let tokens = lex("[a] -> [b]");
        // [, a, ], ->, [, b, ], Eof
        assert_eq!(tokens[3], TokenKind::Arrow);
    }

    #[test]
    fn test_comparison_ops() {
        let tokens = lex("== != >= <=");
        assert_eq!(tokens[0], TokenKind::EqEq);
        assert_eq!(tokens[1], TokenKind::NotEq);
        assert_eq!(tokens[2], TokenKind::GtEq);
        assert_eq!(tokens[3], TokenKind::LtEq);
    }

    #[test]
    fn test_dollar_prefix_rejected() {
        let result = Lexer::new("$ctx.articles").tokenize();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no longer supported"), "Error: {}", err);
    }

    #[test]
    fn test_triple_quoted_string() {
        let tokens = lex(r#""""hello "world""""#);
        assert!(matches!(tokens[0], TokenKind::String(_)));
    }

    #[test]
    fn test_fstring() {
        let tokens = lex(r#"f"hello {name}""#);
        assert!(matches!(tokens[0], TokenKind::String(_)));
        if let TokenKind::String(s) = &tokens[0] {
            assert!(s.starts_with("f\""));
        }
    }

    #[test]
    fn test_negative_number() {
        let tokens = lex("-42");
        assert_eq!(tokens[0], TokenKind::Number("-42".into()));
    }

    #[test]
    fn test_keywords() {
        let tokens = lex("if on error switch default foreach parallel in while assert new");
        assert_eq!(tokens[0], TokenKind::If);
        assert_eq!(tokens[1], TokenKind::On);
        assert_eq!(tokens[2], TokenKind::Error);
        assert_eq!(tokens[3], TokenKind::Switch);
        assert_eq!(tokens[4], TokenKind::Default);
        assert_eq!(tokens[5], TokenKind::Foreach);
        assert_eq!(tokens[6], TokenKind::Parallel);
        assert_eq!(tokens[7], TokenKind::In);
        assert_eq!(tokens[8], TokenKind::While);
        assert_eq!(tokens[9], TokenKind::Assert);
        assert_eq!(tokens[10], TokenKind::New);
    }

    #[test]
    fn test_comment_skipped() {
        let tokens = lex("# this is a comment\n[start]");
        // Newline after comment, then [, start, ], Eof
        assert_eq!(tokens[0], TokenKind::LBracket);
    }

    #[test]
    fn test_scoped_identifier_dot() {
        let tokens = lex("pandas.read_csv");
        assert_eq!(tokens[0], TokenKind::Ident("pandas".into()));
        assert_eq!(tokens[1], TokenKind::Dot);
        assert_eq!(tokens[2], TokenKind::Ident("read_csv".into()));
    }

    #[test]
    fn test_number_with_decimal() {
        let tokens = lex("3.14");
        assert_eq!(tokens[0], TokenKind::Number("3.14".into()));
    }
}
