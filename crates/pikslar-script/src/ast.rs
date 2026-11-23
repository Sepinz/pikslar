//! The compiled form of a script: a tree whose variables are already resolved to slots.

use crate::builtins::Builtin;
use crate::error::Pos;
use crate::value::Value;

/// A literal that is not a string. Programs hold no shared values, so they can be sent to
/// the thread that runs them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Scalar {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
}

impl Scalar {
    #[inline]
    pub fn value(self) -> Value {
        match self {
            Scalar::Nil => Value::Nil,
            Scalar::Bool(b) => Value::Bool(b),
            Scalar::Int(n) => Value::Int(n),
            Scalar::Float(x) => Value::Float(x),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug)]
pub(crate) enum Expr {
    Lit(Scalar, Pos),
    /// A string literal, by its index in [`Program::texts`].
    Text(u32, Pos),
    Local(u16, Pos),
    List(Vec<Expr>, Pos),
    Unary(UnOp, Box<Expr>, Pos),
    Binary(BinOp, Box<(Expr, Expr)>, Pos),
    And(Box<(Expr, Expr)>, Pos),
    Or(Box<(Expr, Expr)>, Pos),
    Index(Box<(Expr, Expr)>, Pos),
    Call(Box<Call>),
}

impl Expr {
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Lit(_, p)
            | Expr::Text(_, p)
            | Expr::Local(_, p)
            | Expr::List(_, p)
            | Expr::Unary(_, _, p)
            | Expr::Binary(_, _, p)
            | Expr::And(_, p)
            | Expr::Or(_, p)
            | Expr::Index(_, p) => *p,
            Expr::Call(c) => c.pos,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// A function the script defines, by its index in [`Program::functions`].
    User(u32),
    Host(u16),
    Builtin(Builtin),
}

#[derive(Debug)]
pub(crate) struct Call {
    pub target: Target,
    pub args: Vec<Expr>,
    pub pos: Pos,
}

#[derive(Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
    /// Fuel burnt each time the statement runs: one, plus one for each expression node in it
    /// (not counting nested blocks, whose statements burn their own).
    pub cost: u32,
}

#[derive(Debug)]
pub(crate) enum StmtKind {
    Set(u16, Expr),
    SetIndex(u16, Box<(Expr, Expr)>),
    Expr(Expr),
    If(Box<If>),
    While(Box<While>),
    ForRange(Box<ForRange>),
    ForEach(Box<ForEach>),
    Break,
    Continue,
    Return(Option<Expr>),
}

#[derive(Debug)]
pub(crate) struct If {
    pub cond: Expr,
    pub then: Vec<Stmt>,
    pub other: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct While {
    pub cond: Expr,
    pub body: Vec<Stmt>,
    /// Fuel for each test of the condition.
    pub test_cost: u32,
}

#[derive(Debug)]
pub(crate) struct ForRange {
    pub var: u16,
    pub from: Expr,
    pub to: Expr,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct ForEach {
    pub var: u16,
    pub list: Expr,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub(crate) struct Function {
    pub arity: u16,
    pub slots: u16,
    pub body: Vec<Stmt>,
}

/// A compiled script, ready to run any number of times.
#[derive(Debug)]
pub struct Program {
    /// Index 0 is the top level; the rest are the script's functions.
    pub(crate) functions: Vec<Function>,
    /// String literals. Each run makes its own value of one the first time it is used.
    pub(crate) texts: Vec<Box<str>>,
    /// Bytes charged for the program itself, held against the memory cap of every run.
    pub(crate) charge: usize,
}

impl Program {
    /// Bytes the compiled program counts against the memory cap.
    pub fn memory(&self) -> usize {
        self.charge
    }
}
