// src/core/expr_parser.rs
//
// Hand-written expression parser replacing expr.pest.
// Lexer + Pratt precedence-climbing parser → Expr AST.

use crate::core::expr_ast::{BinOp, Expr, FStringPart, UnaryOp};
use anyhow::{anyhow, Result};

// ============================================================
// Token Types
// ============================================================

#[derive(Debug, Clone, PartialEq)]
enum Tk {
    // Literals
    Integer(i64),
    Float(f64),
    Str(String),
    FStringBody(String),
    FStringTriple(String),
    True,
    False,
    Null,
    // Identifiers
    Ident(String),
    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Dot,
    Comma,
    Colon,
    Pipe,
    Bang,
    FatArrow,
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    QQ, // ??
    // Keywords (merged with symbol equivalents)
    And, // and, &&
    Or,  // or, ||
    Not,
    In,
    // End
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Tk,
    pos: usize,
}

// ============================================================
// Lexer
// ============================================================

struct Lexer<'a> {
    src: &'a [u8],
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            src: input.as_bytes(),
            input,
            pos: 0,
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.src.len() {
                tokens.push(Token {
                    kind: Tk::Eof,
                    pos: self.pos,
                });
                break;
            }
            tokens.push(self.next_token()?);
        }
        Ok(tokens)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.src.len() {
            match self.src[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    /// Advance one UTF-8 character and push it to the buffer.
    fn push_utf8_char(&mut self, buf: &mut String) {
        let ch = self.input[self.pos..].chars().next().unwrap();
        buf.push(ch);
        self.pos += ch.len_utf8();
    }

    fn next_token(&mut self) -> Result<Token> {
        let start = self.pos;
        let b = self.src[self.pos];

        match b {
            // F-string
            b'f' if self.peek_at(1) == Some(b'"') => self.lex_fstring(start),
            // String literals
            b'"' => self.lex_double_string(start),
            b'\'' => self.lex_single_string(start),
            // $ prefix is no longer supported
            b'$' => Err(anyhow!(
                "The '$' prefix is no longer supported. Use bare identifiers instead: \
                     '$ctx.x' → 'x', '$input.x' → 'input.x', '$output' → 'output'"
            ))?,
            // Number
            b'0'..=b'9' => self.lex_number(start),
            // Identifier / keyword
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident(start),
            // Delimiters
            b'(' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::LParen,
                    pos: start,
                })
            }
            b')' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::RParen,
                    pos: start,
                })
            }
            b'[' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::LBracket,
                    pos: start,
                })
            }
            b']' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::RBracket,
                    pos: start,
                })
            }
            b'{' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::LBrace,
                    pos: start,
                })
            }
            b'}' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::RBrace,
                    pos: start,
                })
            }
            // Operators
            b'+' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Plus,
                    pos: start,
                })
            }
            b'-' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Minus,
                    pos: start,
                })
            }
            b'*' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Star,
                    pos: start,
                })
            }
            b'/' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Slash,
                    pos: start,
                })
            }
            b'%' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Percent,
                    pos: start,
                })
            }
            b'.' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Dot,
                    pos: start,
                })
            }
            b',' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Comma,
                    pos: start,
                })
            }
            b':' => {
                self.pos += 1;
                Ok(Token {
                    kind: Tk::Colon,
                    pos: start,
                })
            }
            b'|' => {
                if self.peek_at(1) == Some(b'|') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::Or,
                        pos: start,
                    })
                } else {
                    self.pos += 1;
                    Ok(Token {
                        kind: Tk::Pipe,
                        pos: start,
                    })
                }
            }
            b'&' if self.peek_at(1) == Some(b'&') => {
                self.pos += 2;
                Ok(Token {
                    kind: Tk::And,
                    pos: start,
                })
            }
            b'!' => {
                if self.peek_at(1) == Some(b'=') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::NotEq,
                        pos: start,
                    })
                } else {
                    self.pos += 1;
                    Ok(Token {
                        kind: Tk::Bang,
                        pos: start,
                    })
                }
            }
            b'=' => {
                if self.peek_at(1) == Some(b'=') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::EqEq,
                        pos: start,
                    })
                } else if self.peek_at(1) == Some(b'>') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::FatArrow,
                        pos: start,
                    })
                } else {
                    Err(anyhow!(
                        "Unexpected '=' at position {} (did you mean '=='?)",
                        start
                    ))
                }
            }
            b'<' => {
                if self.peek_at(1) == Some(b'=') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::LtEq,
                        pos: start,
                    })
                } else {
                    self.pos += 1;
                    Ok(Token {
                        kind: Tk::Lt,
                        pos: start,
                    })
                }
            }
            b'>' => {
                if self.peek_at(1) == Some(b'=') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::GtEq,
                        pos: start,
                    })
                } else {
                    self.pos += 1;
                    Ok(Token {
                        kind: Tk::Gt,
                        pos: start,
                    })
                }
            }
            b'?' => {
                if self.peek_at(1) == Some(b'?') {
                    self.pos += 2;
                    Ok(Token {
                        kind: Tk::QQ,
                        pos: start,
                    })
                } else {
                    Err(anyhow!("Unexpected '?' at position {}", start))
                }
            }
            _ => Err(anyhow!(
                "Unexpected character '{}' at position {}",
                b as char,
                start
            )),
        }
    }

    fn lex_number(&mut self, start: usize) -> Result<Token> {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.src.len() && self.src[self.pos] == b'.' {
            // Check it's followed by a digit (not a dot-access like `3.field`)
            if self.pos + 1 < self.src.len() && self.src[self.pos + 1].is_ascii_digit() {
                self.pos += 1; // consume '.'
                while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
                let n: f64 = s.parse().map_err(|_| anyhow!("Invalid float: '{}'", s))?;
                return Ok(Token {
                    kind: Tk::Float(n),
                    pos: start,
                });
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let n: i64 = s.parse().map_err(|_| anyhow!("Invalid integer: '{}'", s))?;
        Ok(Token {
            kind: Tk::Integer(n),
            pos: start,
        })
    }

    fn lex_ident(&mut self, start: usize) -> Result<Token> {
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_alphanumeric() || self.src[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let word = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = match word {
            "and" => Tk::And,
            "or" => Tk::Or,
            "not" => Tk::Not,
            "in" => Tk::In,
            "true" | "True" => Tk::True,
            "false" | "False" => Tk::False,
            "null" | "none" | "None" => Tk::Null,
            _ => Tk::Ident(word.to_string()),
        };
        Ok(Token { kind, pos: start })
    }

    // lex_variable removed — $ prefix is no longer supported

    fn lex_double_string(&mut self, start: usize) -> Result<Token> {
        self.pos += 1; // skip first "
                       // Check for triple-quoted
        if self.peek() == Some(b'"') && self.peek_at(1) == Some(b'"') {
            self.pos += 2; // skip remaining ""
            return self.lex_triple_string(start);
        }
        let mut buf = String::new();
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(Token {
                    kind: Tk::Str(buf),
                    pos: start,
                });
            }
            if b == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 1;
                buf.push(decode_escape(self.src[self.pos]));
                self.pos += 1;
            } else {
                self.push_utf8_char(&mut buf);
            }
        }
        Err(anyhow!(
            "Unterminated string starting at position {}",
            start
        ))
    }

    fn lex_triple_string(&mut self, start: usize) -> Result<Token> {
        // Already consumed the opening """
        let body_start = self.pos;
        loop {
            if self.pos + 2 >= self.src.len() {
                return Err(anyhow!(
                    "Unterminated triple-quoted string starting at position {}",
                    start
                ));
            }
            if self.src[self.pos] == b'"'
                && self.src[self.pos + 1] == b'"'
                && self.src[self.pos + 2] == b'"'
            {
                let body = std::str::from_utf8(&self.src[body_start..self.pos])
                    .unwrap()
                    .to_string();
                self.pos += 3;
                return Ok(Token {
                    kind: Tk::Str(body),
                    pos: start,
                });
            }
            self.pos += 1;
        }
    }

    fn lex_single_string(&mut self, start: usize) -> Result<Token> {
        self.pos += 1; // skip '
        let mut buf = String::new();
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b'\'' {
                self.pos += 1;
                return Ok(Token {
                    kind: Tk::Str(buf),
                    pos: start,
                });
            }
            if b == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 1;
                buf.push(decode_escape(self.src[self.pos]));
                self.pos += 1;
            } else {
                self.push_utf8_char(&mut buf);
            }
        }
        Err(anyhow!(
            "Unterminated string starting at position {}",
            start
        ))
    }

    fn lex_fstring(&mut self, start: usize) -> Result<Token> {
        self.pos += 1; // skip 'f'
        self.pos += 1; // skip first '"'
                       // Check for triple-quoted f-string
        if self.peek() == Some(b'"') && self.peek_at(1) == Some(b'"') {
            self.pos += 2;
            return self.lex_fstring_triple(start);
        }
        // Regular f-string: collect raw body until closing "
        let body_start = self.pos;
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b'"' {
                let body = std::str::from_utf8(&self.src[body_start..self.pos])
                    .unwrap()
                    .to_string();
                self.pos += 1;
                return Ok(Token {
                    kind: Tk::FStringBody(body),
                    pos: start,
                });
            }
            if b == b'\\' && self.pos + 1 < self.src.len() {
                self.pos += 2; // skip escaped char
            } else {
                self.pos += 1;
            }
        }
        Err(anyhow!(
            "Unterminated f-string starting at position {}",
            start
        ))
    }

    fn lex_fstring_triple(&mut self, start: usize) -> Result<Token> {
        let body_start = self.pos;
        loop {
            if self.pos + 2 >= self.src.len() {
                return Err(anyhow!(
                    "Unterminated triple-quoted f-string starting at position {}",
                    start
                ));
            }
            if self.src[self.pos] == b'"'
                && self.src[self.pos + 1] == b'"'
                && self.src[self.pos + 2] == b'"'
            {
                let body = std::str::from_utf8(&self.src[body_start..self.pos])
                    .unwrap()
                    .to_string();
                self.pos += 3;
                return Ok(Token {
                    kind: Tk::FStringTriple(body),
                    pos: start,
                });
            }
            self.pos += 1;
        }
    }
}

fn decode_escape(b: u8) -> char {
    match b {
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        b'\\' => '\\',
        b'"' => '"',
        b'\'' => '\'',
        other => other as char,
    }
}

// ============================================================
// Parser (Pratt precedence climbing)
// ============================================================

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    source: &'a str,
}

/// Public entry point: parse an expression string into an AST.
pub fn parse_expr(input: &str) -> Result<Expr> {
    let tokens = Lexer::new(input).tokenize()?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        source: input,
    };
    let expr = parser.expr()?;
    if !matches!(parser.peek(), Tk::Eof) {
        return Err(anyhow!(
            "Expression parse error: unexpected trailing content at position {}",
            parser.tokens[parser.pos].pos
        ));
    }
    Ok(expr)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Tk {
        &self.tokens[self.pos.min(self.tokens.len() - 1)].kind
    }

    fn peek_at(&self, offset: usize) -> &Tk {
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

    fn expect(&mut self, expected: &Tk) -> Result<()> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            self.advance();
            Ok(())
        } else {
            Err(anyhow!(
                "Expression parse error: expected {:?}, got {:?} at position {}",
                expected,
                self.peek(),
                self.tokens[self.pos.min(self.tokens.len() - 1)].pos
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek().clone() {
            Tk::Ident(s) => {
                self.advance();
                Ok(s)
            }
            _ => Err(anyhow!(
                "Expression parse error: expected identifier, got {:?}",
                self.peek()
            )),
        }
    }

    // ==================== Top-level ====================

    fn expr(&mut self) -> Result<Expr> {
        self.expr_bp(0)
    }

    // ==================== Pratt core ====================

    fn expr_bp(&mut self, min_bp: u8) -> Result<Expr> {
        let mut lhs = self.prefix()?;

        loop {
            // Postfix: .field, [idx], .method(args) — highest binding power
            match self.peek() {
                Tk::Dot => {
                    if 19 < min_bp {
                        break;
                    }
                    self.advance(); // consume .
                    let field = self.expect_ident()?;
                    // Check for method call: .field(...)
                    if matches!(self.peek(), Tk::LParen) {
                        self.advance(); // consume (
                        let args = self.parse_call_args()?;
                        self.expect(&Tk::RParen)?;
                        lhs = Expr::MethodCall {
                            object: Box::new(lhs),
                            method: field,
                            args,
                        };
                    } else {
                        lhs = Expr::DotAccess {
                            object: Box::new(lhs),
                            field,
                        };
                    }
                    continue;
                }
                Tk::LBracket => {
                    if 19 < min_bp {
                        break;
                    }
                    self.advance(); // consume [
                    let index = self.expr()?;
                    self.expect(&Tk::RBracket)?;
                    lhs = Expr::BracketAccess {
                        object: Box::new(lhs),
                        index: Box::new(index),
                    };
                    continue;
                }
                _ => {}
            }

            // Infix operators
            if let Some((l_bp, r_bp, op_kind)) = self.infix_bp() {
                if l_bp < min_bp {
                    break;
                }
                match op_kind {
                    InfixOp::Binary(op) => {
                        self.advance(); // consume operator
                        let rhs = self.expr_bp(r_bp)?;
                        lhs = Expr::BinaryOp {
                            left: Box::new(lhs),
                            op,
                            right: Box::new(rhs),
                        };
                    }
                    InfixOp::NotIn => {
                        self.advance(); // consume 'not'
                        self.advance(); // consume 'in'
                        let rhs = self.expr_bp(r_bp)?;
                        lhs = Expr::BinaryOp {
                            left: Box::new(lhs),
                            op: BinOp::NotIn,
                            right: Box::new(rhs),
                        };
                    }
                    InfixOp::Pipe => {
                        self.advance(); // consume |
                        let filter_name = self.expect_ident()?;
                        let args = if matches!(self.peek(), Tk::LParen) {
                            self.advance();
                            let a = self.parse_call_args()?;
                            self.expect(&Tk::RParen)?;
                            a
                        } else {
                            vec![]
                        };
                        lhs = Expr::Pipe {
                            value: Box::new(lhs),
                            filter: filter_name,
                            args,
                        };
                    }
                    InfixOp::Coalesce => {
                        self.advance(); // consume ??
                        let rhs = self.expr_bp(r_bp)?;
                        lhs = Expr::Coalesce {
                            left: Box::new(lhs),
                            right: Box::new(rhs),
                        };
                    }
                }
                continue;
            }

            break;
        }

        Ok(lhs)
    }

    /// Parse prefix / atom
    fn prefix(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            // Unary prefix operators
            Tk::Not | Tk::Bang => {
                self.advance();
                let operand = self.expr_bp(17)?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                })
            }
            Tk::Minus => {
                self.advance();
                let operand = self.expr_bp(17)?;
                // Constant fold negative numbers
                if let Expr::Number(n) = operand {
                    Ok(Expr::Number(-n))
                } else {
                    Ok(Expr::UnaryOp {
                        op: UnaryOp::Neg,
                        operand: Box::new(operand),
                    })
                }
            }
            // Parenthesized expr (or possible lambda multi-param detection handled in call_arg)
            Tk::LParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(&Tk::RParen)?;
                Ok(inner)
            }
            // Array literal
            Tk::LBracket => self.parse_array(),
            // Object literal
            Tk::LBrace => self.parse_object(),
            // Number
            Tk::Integer(n) => {
                self.advance();
                Ok(Expr::Number(n as f64))
            }
            Tk::Float(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            // String
            Tk::Str(s) => {
                self.advance();
                Ok(Expr::String(s))
            }
            // F-string
            Tk::FStringBody(body) => {
                self.advance();
                self.parse_fstring(&body, false)
            }
            Tk::FStringTriple(body) => {
                self.advance();
                self.parse_fstring(&body, true)
            }
            // Boolean
            Tk::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Tk::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            // Null
            Tk::Null => {
                self.advance();
                Ok(Expr::None)
            }
            // Identifier or function call
            Tk::Ident(name) => {
                self.advance();
                if matches!(self.peek(), Tk::LParen) {
                    self.advance(); // consume (
                    let args = self.parse_call_args()?;
                    self.expect(&Tk::RParen)?;
                    Ok(Expr::FuncCall { name, args })
                } else {
                    Ok(Expr::Identifier(name))
                }
            }
            other => Err(anyhow!(
                "Expression parse error in '{}': unexpected token {:?} at position {}",
                self.source,
                other,
                self.tokens[self.pos.min(self.tokens.len() - 1)].pos
            )),
        }
    }

    // ==================== Infix binding power ====================

    fn infix_bp(&self) -> Option<(u8, u8, InfixOp)> {
        match self.peek() {
            Tk::QQ => Some((1, 2, InfixOp::Coalesce)),
            Tk::Pipe => Some((3, 4, InfixOp::Pipe)),
            Tk::Or => Some((5, 6, InfixOp::Binary(BinOp::Or))),
            Tk::And => Some((7, 8, InfixOp::Binary(BinOp::And))),
            // `not in` as a two-token infix operator
            Tk::Not if matches!(self.peek_at(1), Tk::In) => Some((9, 10, InfixOp::NotIn)),
            Tk::In => Some((9, 10, InfixOp::Binary(BinOp::In))),
            Tk::EqEq => Some((11, 12, InfixOp::Binary(BinOp::Eq))),
            Tk::NotEq => Some((11, 12, InfixOp::Binary(BinOp::Ne))),
            Tk::Lt => Some((11, 12, InfixOp::Binary(BinOp::Lt))),
            Tk::Gt => Some((11, 12, InfixOp::Binary(BinOp::Gt))),
            Tk::LtEq => Some((11, 12, InfixOp::Binary(BinOp::Le))),
            Tk::GtEq => Some((11, 12, InfixOp::Binary(BinOp::Ge))),
            Tk::Plus => Some((13, 14, InfixOp::Binary(BinOp::Add))),
            Tk::Minus => Some((13, 14, InfixOp::Binary(BinOp::Sub))),
            Tk::Star => Some((15, 16, InfixOp::Binary(BinOp::Mul))),
            Tk::Slash => Some((15, 16, InfixOp::Binary(BinOp::Div))),
            Tk::Percent => Some((15, 16, InfixOp::Binary(BinOp::Mod))),
            _ => None,
        }
    }

    // ==================== Call arguments (handles lambdas) ====================

    fn parse_call_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        if matches!(self.peek(), Tk::RParen | Tk::Eof) {
            return Ok(args);
        }
        args.push(self.parse_call_arg()?);
        while matches!(self.peek(), Tk::Comma) {
            self.advance(); // consume ,
            if matches!(self.peek(), Tk::RParen) {
                break; // trailing comma
            }
            args.push(self.parse_call_arg()?);
        }
        Ok(args)
    }

    fn parse_call_arg(&mut self) -> Result<Expr> {
        // Lambda detection: `ident => expr` or `(ident, ...) => expr`
        if let Tk::Ident(name) = self.peek().clone() {
            if matches!(self.peek_at(1), Tk::FatArrow) {
                self.advance(); // consume ident
                self.advance(); // consume =>
                let body = self.expr()?;
                return Ok(Expr::Lambda {
                    params: vec![name],
                    body: Box::new(body),
                });
            }
        }
        if matches!(self.peek(), Tk::LParen) {
            // Check if this is (ident, ident, ...) => expr
            if let Some(params) = self.try_lambda_params() {
                self.advance(); // consume =>
                let body = self.expr()?;
                return Ok(Expr::Lambda {
                    params,
                    body: Box::new(body),
                });
            }
        }
        self.expr()
    }

    /// Try to parse `(ident, ident, ...) =>` as lambda params.
    /// On success: consumes tokens up to and including `)`, returns params (caller consumes `=>`).
    /// On failure: restores position, returns None.
    fn try_lambda_params(&mut self) -> Option<Vec<String>> {
        let save = self.pos;
        self.advance(); // consume LParen
        let mut params = Vec::new();
        loop {
            match self.peek().clone() {
                Tk::Ident(name) => {
                    params.push(name);
                    self.advance();
                }
                _ => {
                    self.pos = save;
                    return None;
                }
            }
            match self.peek() {
                Tk::Comma => {
                    self.advance();
                    continue;
                }
                Tk::RParen => {
                    self.advance();
                    break;
                }
                _ => {
                    self.pos = save;
                    return None;
                }
            }
        }
        // Check for => after )
        if matches!(self.peek(), Tk::FatArrow) {
            Some(params)
        } else {
            self.pos = save;
            None
        }
    }

    // ==================== Compound literals ====================

    fn parse_array(&mut self) -> Result<Expr> {
        self.advance(); // consume [
        let mut items = Vec::new();
        if !matches!(self.peek(), Tk::RBracket) {
            items.push(self.expr()?);
            while matches!(self.peek(), Tk::Comma) {
                self.advance();
                if matches!(self.peek(), Tk::RBracket) {
                    break; // trailing comma
                }
                items.push(self.expr()?);
            }
        }
        self.expect(&Tk::RBracket)?;
        Ok(Expr::Array(items))
    }

    fn parse_object(&mut self) -> Result<Expr> {
        self.advance(); // consume {
        let mut pairs = Vec::new();
        if !matches!(self.peek(), Tk::RBrace) {
            pairs.push(self.parse_object_pair()?);
            while matches!(self.peek(), Tk::Comma) {
                self.advance();
                if matches!(self.peek(), Tk::RBrace) {
                    break;
                }
                pairs.push(self.parse_object_pair()?);
            }
        }
        self.expect(&Tk::RBrace)?;
        Ok(Expr::Object(pairs))
    }

    fn parse_object_pair(&mut self) -> Result<(String, Expr)> {
        let key = match self.peek().clone() {
            Tk::Str(s) => {
                self.advance();
                s
            }
            Tk::Ident(s) => {
                self.advance();
                s
            }
            _ => {
                return Err(anyhow!(
                    "Expression parse error: expected string or identifier key in object, got {:?}",
                    self.peek()
                ))
            }
        };
        self.expect(&Tk::Colon)?;
        let val = self.expr()?;
        Ok((key, val))
    }

    // ==================== F-string ====================

    fn parse_fstring(&self, body: &str, is_triple: bool) -> Result<Expr> {
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
                    if !text_buf.is_empty() {
                        parts.push(FStringPart::Text(std::mem::take(&mut text_buf)));
                    }
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
                    let expr = parse_expr(expr_str.trim())?;
                    parts.push(FStringPart::Expr(expr));
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    text_buf.push('}');
                }
                _ => text_buf.push(c),
            }
        }

        if !text_buf.is_empty() {
            parts.push(FStringPart::Text(text_buf));
        }

        Ok(Expr::FString(parts))
    }
}

enum InfixOp {
    Binary(BinOp),
    NotIn,
    Pipe,
    Coalesce,
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn p(input: &str) -> Expr {
        parse_expr(input).unwrap_or_else(|e| panic!("Failed to parse '{}': {}", input, e))
    }

    #[test]
    fn test_number_literals() {
        assert!(matches!(p("42"), Expr::Number(n) if n == 42.0));
        assert!(matches!(p("3.14"), Expr::Number(n) if (n - 3.14).abs() < 1e-10));
        assert!(matches!(p("-5"), Expr::Number(n) if n == -5.0));
        assert!(matches!(p("0"), Expr::Number(n) if n == 0.0));
    }

    #[test]
    fn test_string_literals() {
        assert!(matches!(p(r#""hello""#), Expr::String(ref s) if s == "hello"));
        assert!(matches!(p("'world'"), Expr::String(ref s) if s == "world"));
        assert!(matches!(p(r#""a\"b""#), Expr::String(ref s) if s == "a\"b"));
        assert!(matches!(p(r#""""triple""""#), Expr::String(ref s) if s == "triple"));
    }

    #[test]
    fn test_bool_null() {
        assert!(matches!(p("true"), Expr::Bool(true)));
        assert!(matches!(p("True"), Expr::Bool(true)));
        assert!(matches!(p("false"), Expr::Bool(false)));
        assert!(matches!(p("False"), Expr::Bool(false)));
        assert!(matches!(p("null"), Expr::None));
        assert!(matches!(p("none"), Expr::None));
        assert!(matches!(p("None"), Expr::None));
    }

    #[test]
    fn test_dollar_prefix_rejected() {
        assert!(parse_expr("$ctx.field").is_err());
        assert!(parse_expr("$input").is_err());
    }

    #[test]
    fn test_bare_identifiers() {
        assert!(matches!(p("field"), Expr::Identifier(ref s) if s == "field"));
        assert!(matches!(p("input"), Expr::Identifier(ref s) if s == "input"));
    }

    #[test]
    fn test_identifier() {
        assert!(matches!(p("foo"), Expr::Identifier(ref s) if s == "foo"));
    }

    #[test]
    fn test_func_call() {
        let e = p("len(x)");
        assert!(
            matches!(e, Expr::FuncCall { ref name, ref args } if name == "len" && args.len() == 1)
        );
    }

    #[test]
    fn test_binary_precedence() {
        // 2 + 3 * 4 → Add(2, Mul(3, 4))
        let e = p("2 + 3 * 4");
        match e {
            Expr::BinaryOp { op: BinOp::Add, .. } => {}
            _ => panic!("Expected Add at top level, got {:?}", e),
        }
    }

    #[test]
    fn test_parens() {
        // (2 + 3) * 4 → Mul(Add(2, 3), 4)
        let e = p("(2 + 3) * 4");
        match e {
            Expr::BinaryOp { op: BinOp::Mul, .. } => {}
            _ => panic!("Expected Mul at top level, got {:?}", e),
        }
    }

    #[test]
    fn test_dot_access_and_method_call() {
        let e = p("x.upper()");
        assert!(matches!(e, Expr::MethodCall { ref method, .. } if method == "upper"));

        // Bare identifier with dot access chain
        let e = p("items.length");
        assert!(matches!(e, Expr::DotAccess { .. }));
    }

    #[test]
    fn test_bracket_access() {
        let e = p("arr[0]");
        assert!(matches!(e, Expr::BracketAccess { .. }));
    }

    #[test]
    fn test_pipe() {
        let e = p("value | upper");
        assert!(matches!(e, Expr::Pipe { ref filter, .. } if filter == "upper"));
    }

    #[test]
    fn test_coalesce() {
        let e = p("a ?? b");
        assert!(matches!(e, Expr::Coalesce { .. }));
    }

    #[test]
    fn test_not_in() {
        let e = p("x not in list");
        match e {
            Expr::BinaryOp {
                op: BinOp::NotIn, ..
            } => {}
            _ => panic!("Expected NotIn, got {:?}", e),
        }
    }

    #[test]
    fn test_array_object() {
        let e = p("[1, 2, 3]");
        assert!(matches!(e, Expr::Array(ref items) if items.len() == 3));

        let e = p(r#"{"key": "val"}"#);
        assert!(matches!(e, Expr::Object(ref pairs) if pairs.len() == 1));
    }

    #[test]
    fn test_lambda_in_call() {
        let e = p("map(items, x => x + 1)");
        match e {
            Expr::FuncCall { ref args, .. } => {
                assert!(matches!(args[1], Expr::Lambda { .. }));
            }
            _ => panic!("Expected FuncCall"),
        }
    }

    #[test]
    fn test_fstring() {
        let e = p(r#"f"hello {name}""#);
        assert!(matches!(e, Expr::FString(ref parts) if parts.len() == 2));
    }

    #[test]
    fn test_unary_not() {
        let e = p("not true");
        assert!(matches!(
            e,
            Expr::UnaryOp {
                op: UnaryOp::Not,
                ..
            }
        ));
    }

    #[test]
    fn test_logical_ops() {
        let e = p("a and b or c");
        // or is lower precedence → Or(And(a, b), c)
        assert!(matches!(e, Expr::BinaryOp { op: BinOp::Or, .. }));
    }
}
