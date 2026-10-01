//! Syntax tree.

use crate::lexer::Pos;

#[derive(Debug, Clone)]
pub enum TypeExpr {
    Int,
    Tryte,
    Trit,
    Void,
    Named(String),
    Ptr(Box<TypeExpr>),
    Array(Box<Expr>, Box<TypeExpr>),
    Fn(Vec<TypeExpr>, Box<TypeExpr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Shl,
    Shr,
    Min,
    Max,
    Cmp3,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Deref,
    Addr,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Char(i64),
    Str(String),
    Null,
    Name(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `a = b` (op None) or `a op= b`.
    Assign(Option<BinOp>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Field(Box<Expr>, String),
    Cast(Box<Expr>, TypeExpr),
    Sizeof(TypeExpr),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `{ a, b, c }` (initialisers only).
    List(Vec<Expr>),
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub init: Option<Expr>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Var(VarDecl),
    Expr(Expr),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    Loop(Box<Stmt>),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Sign { e: Expr, neg: Vec<Stmt>, zero: Vec<Stmt>, pos: Vec<Stmt> },
    Switch { e: Expr, cases: Vec<(Vec<Expr>, Vec<Stmt>)>, default: Option<Vec<Stmt>> },
    Break,
    Continue,
    Return(Option<Expr>),
    Asm(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub interrupt: bool,
    /// `None` for `extern fn`.
    pub body: Option<Vec<Stmt>>,
    pub pos: Pos,
}

#[derive(Debug, Clone)]
pub enum Attr {
    Align(Expr),
    At(Expr),
}

#[derive(Debug, Clone)]
pub enum Item {
    Import(String, Pos),
    Var { attrs: Vec<Attr>, decl: VarDecl, external: bool },
    Const(VarDecl),
    Struct { name: String, fields: Vec<Param>, pos: Pos },
    Fn(FnDecl),
}
