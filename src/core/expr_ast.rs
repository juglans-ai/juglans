//! Juglans Expression Language AST
//!
//! Represents parsed expressions with Python-like semantics.
//! Operates directly on `serde_json::Value` — no intermediate type system.

/// Part of an f-string: either literal text or an interpolated expression.
#[derive(Debug, Clone)]
pub enum FStringPart {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Expr {
    // Literals
    Number(f64),
    String(String),
    Bool(bool),
    None,
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    /// F-string: `f"Hello {name}, count={count + 1}"`
    FString(Vec<FStringPart>),

    // References
    /// Variable reference: `$ctx.field.nested` — resolved at eval time via context
    Variable(String),
    /// Bare identifier: `name` — looked up in scope (template variables, loop vars)
    Identifier(String),

    // Operations
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },

    // Access
    /// Dot access: `expr.field`
    DotAccess {
        object: Box<Expr>,
        field: String,
    },
    /// Bracket access: `expr[index]`
    BracketAccess {
        object: Box<Expr>,
        index: Box<Expr>,
    },

    // Calls
    /// Function call: `len(x)`, `round(x, 2)`
    FuncCall {
        name: String,
        args: Vec<Expr>,
    },
    /// Method call: `x.upper()`, `arr.append(item)`
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// Pipe filter: `value | upper`, `value | round(2)` — desugars to FuncCall
    Pipe {
        value: Box<Expr>,
        filter: String,
        args: Vec<Expr>,
    },

    /// Lambda expression: `x => x + 1` or `(x, y) => x + y`
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    // Logical
    And,
    Or,
    // Membership
    In,
    NotIn,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}
