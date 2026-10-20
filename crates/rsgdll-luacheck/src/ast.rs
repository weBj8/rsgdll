//! GLua AST with byte spans.

use crate::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Nil,
    True,
    False,
    Vararg,
    Number(f64),
    Str(String),
    Capture(String, Span),
    Name(String, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    /// `a.b` (field name kept for checks).
    Field(Box<Expr>, String, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Method(Box<Expr>, String, Vec<Expr>, Span),
    Function(Box<FuncBody>),
    Table(Vec<(Option<Expr>, Expr)>),
    Bin(Box<Expr>, &'static str, Box<Expr>),
    Un(&'static str, Box<Expr>),
    Paren(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncBody {
    pub params: Vec<String>,
    pub vararg: bool,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stat {
    Expr(Expr),
    Assign(Vec<Expr>, Vec<Expr>),
    Local(Vec<String>, Vec<Expr>),
    LocalFunction(String, Box<FuncBody>),
    Function(Expr, Box<FuncBody>),
    Do(Block),
    While(Expr, Block),
    Repeat(Block, Expr),
    If(Vec<(Expr, Block)>, Option<Block>),
    NumFor(String, Expr, Expr, Option<Expr>, Block),
    GenFor(Vec<String>, Vec<Expr>, Block),
    Return(Vec<Expr>, Span),
    Break,
    Continue,
    Goto(String),
    Label(String),
}

pub type Block = Vec<Stat>;

impl Expr {
    pub fn span(&self) -> Option<Span> {
        match self {
            Expr::Capture(_, s)
            | Expr::Name(_, s)
            | Expr::Index(_, _, s)
            | Expr::Field(_, _, s)
            | Expr::Call(_, _, s)
            | Expr::Method(_, _, _, s) => Some(*s),
            Expr::Paren(e) | Expr::Un(_, e) => e.span(),
            Expr::Bin(a, _, b) => a.span().zip(b.span()).map(|(a, b)| Span {
                start: a.start,
                end: b.end,
            }),
            _ => None,
        }
    }

    /// Dotted path of a pure name/field chain (`net.WriteString`).
    pub fn path(&self) -> Option<String> {
        match self {
            Expr::Name(n, _) => Some(n.clone()),
            Expr::Field(b, f, _) => b.path().map(|p| format!("{p}.{f}")),
            _ => None,
        }
    }

    /// Whether this expression can produce multiple values (call/vararg in last position).
    pub fn is_multi(&self) -> bool {
        matches!(self, Expr::Call(..) | Expr::Method(..) | Expr::Vararg)
    }
}
