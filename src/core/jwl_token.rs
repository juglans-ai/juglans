// src/core/jwl_token.rs
// Token types for the JWL hand-written recursive descent parser.

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Delimiters
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,

    // Punctuation
    Colon,
    Comma,
    Semicolon,
    Eq,
    Arrow, // ->
    Dot,

    // Comparison operators
    EqEq,  // ==
    NotEq, // !=
    GtEq,  // >=
    LtEq,  // <=
    Gt,    // >
    Lt,    // <

    // Literals
    String(String), // raw content including quotes
    Number(String),
    True,
    False,
    Null,

    // Identifiers
    Ident(String),

    // Keywords
    If,
    On,
    Error,
    Switch,
    Default,
    Ok,
    Err,
    Return,
    Foreach,
    Parallel,
    In,
    While,
    Assert,
    New,
    Yield,
    At, // @

    // Special
    Newline,
    Eof,
}

impl TokenKind {
    /// Human-readable name for error messages
    pub fn describe(&self) -> &'static str {
        match self {
            Self::LBracket => "'['",
            Self::RBracket => "']'",
            Self::LParen => "'('",
            Self::RParen => "')'",
            Self::LBrace => "'{'",
            Self::RBrace => "'}'",
            Self::Colon => "':'",
            Self::Comma => "','",
            Self::Semicolon => "';'",
            Self::Eq => "'='",
            Self::Arrow => "'->'",
            Self::Dot => "'.'",
            Self::EqEq => "'=='",
            Self::NotEq => "'!='",
            Self::GtEq => "'>='",
            Self::LtEq => "'<='",
            Self::Gt => "'>'",
            Self::Lt => "'<'",
            Self::String(_) => "string",
            Self::Number(_) => "number",
            Self::True | Self::False => "boolean",
            Self::Null => "null",
            Self::Ident(_) => "identifier",
            Self::If => "'if'",
            Self::On => "'on'",
            Self::Error => "'error'",
            Self::Switch => "'switch'",
            Self::Default => "'default'",
            Self::Foreach => "'foreach'",
            Self::Parallel => "'parallel'",
            Self::In => "'in'",
            Self::While => "'while'",
            Self::Assert => "'assert'",
            Self::Ok => "'ok'",
            Self::Err => "'err'",
            Self::Return => "'return'",
            Self::New => "'new'",
            Self::Yield => "'yield'",
            Self::At => "'@'",
            Self::Newline => "newline",
            Self::Eof => "end of file",
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            line: 1,
            col: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Known metadata keys (import-only — non-import meta lives in .jgflow manifest)
pub const META_KEYS: &[&str] = &["libs", "flows", "prompts", "agents", "tools", "python"];

pub fn is_meta_key(s: &str) -> bool {
    META_KEYS.contains(&s)
}
