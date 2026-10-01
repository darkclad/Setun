//! Recursive-descent parser.

use crate::ast::*;
use crate::lexer::{Pos, Tok, Token};
use crate::Error;

pub struct Parser {
    toks: Vec<Token>,
    i: usize,
}

type R<T> = Result<T, Error>;

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, i: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }

    fn pos(&self) -> Pos {
        self.toks[self.i].pos
    }

    fn next(&mut self) -> Tok {
        let t = self.toks[self.i].tok.clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }

    fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(Error { pos: self.pos(), msg: msg.into() })
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p) || matches!(self.peek(), Tok::Kw(k) if *k == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: &str) -> R<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(format!("expected `{p}`, found {}", describe(self.peek())))
        }
    }

    fn ident(&mut self) -> R<String> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            t => {
                self.i -= 1;
                self.err(format!("expected a name, found {}", describe(&t)))
            }
        }
    }

    // ---- items -------------------------------------------------------------

    pub fn program(&mut self) -> R<Vec<Item>> {
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            items.push(self.item()?);
        }
        Ok(items)
    }

    fn item(&mut self) -> R<Item> {
        let pos = self.pos();
        if self.eat("import") {
            let Tok::Str(path) = self.next() else { return self.err("expected a file name string") };
            self.expect(";")?;
            return Ok(Item::Import(path, pos));
        }
        let mut attrs = Vec::new();
        while self.eat("@") {
            let name = self.ident()?;
            self.expect("(")?;
            let e = self.expr()?;
            self.expect(")")?;
            attrs.push(match name.as_str() {
                "align" => Attr::Align(e),
                "at" => Attr::At(e),
                _ => return Err(Error { pos, msg: format!("unknown attribute `@{name}`") }),
            });
        }
        if self.is("var") {
            let decl = self.var_decl()?;
            return Ok(Item::Var { attrs, decl, external: false });
        }
        if !attrs.is_empty() {
            return self.err("attributes apply to `var` declarations");
        }
        if self.eat("const") {
            let name = self.ident()?;
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect("=")?;
            let init = Some(self.expr()?);
            self.expect(";")?;
            return Ok(Item::Const(VarDecl { name, ty, init, pos }));
        }
        if self.eat("struct") {
            let name = self.ident()?;
            self.expect("{")?;
            let mut fields = Vec::new();
            while !self.eat("}") {
                let f = self.ident()?;
                self.expect(":")?;
                let ty = self.ty()?;
                self.expect(";")?;
                fields.push(Param { name: f, ty });
            }
            return Ok(Item::Struct { name, fields, pos });
        }
        if self.eat("extern") {
            if self.is("var") {
                self.next();
                let name = self.ident()?;
                self.expect(":")?;
                let ty = self.ty()?;
                self.expect(";")?;
                return Ok(Item::Var { attrs, decl: VarDecl { name, ty, init: None, pos }, external: true });
            }
            self.expect("fn")?;
            let (name, params, ret) = self.fn_sig()?;
            self.expect(";")?;
            return Ok(Item::Fn(FnDecl { name, params, ret, interrupt: false, body: None, pos }));
        }
        let interrupt = self.eat("interrupt");
        if self.eat("fn") {
            let (name, params, ret) = self.fn_sig()?;
            let body = self.block()?;
            return Ok(Item::Fn(FnDecl { name, params, ret, interrupt, body: Some(body), pos }));
        }
        self.err(format!("expected a declaration, found {}", describe(self.peek())))
    }

    fn fn_sig(&mut self) -> R<(String, Vec<Param>, TypeExpr)> {
        let name = self.ident()?;
        self.expect("(")?;
        let mut params = Vec::new();
        if !self.eat(")") {
            loop {
                let p = self.ident()?;
                self.expect(":")?;
                params.push(Param { name: p, ty: self.ty()? });
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        let ret = if self.eat("->") { self.ty()? } else { TypeExpr::Void };
        Ok((name, params, ret))
    }

    fn var_decl(&mut self) -> R<VarDecl> {
        let pos = self.pos();
        self.expect("var")?;
        let name = self.ident()?;
        self.expect(":")?;
        let ty = self.ty()?;
        let init = if self.eat("=") { Some(self.init()?) } else { None };
        self.expect(";")?;
        Ok(VarDecl { name, ty, init, pos })
    }

    fn init(&mut self) -> R<Expr> {
        let pos = self.pos();
        if self.eat("{") {
            let mut v = Vec::new();
            if !self.eat("}") {
                loop {
                    v.push(self.init()?);
                    if self.eat("}") {
                        break;
                    }
                    self.expect(",")?;
                    if self.eat("}") {
                        break;
                    }
                }
            }
            return Ok(Expr { kind: ExprKind::List(v), pos });
        }
        self.expr()
    }

    fn ty(&mut self) -> R<TypeExpr> {
        let t = match self.next() {
            Tok::Kw("int") => TypeExpr::Int,
            Tok::Kw("tryte") | Tok::Kw("char") => TypeExpr::Tryte,
            Tok::Kw("trit") => TypeExpr::Trit,
            Tok::Kw("void") => TypeExpr::Void,
            Tok::Ident(s) => TypeExpr::Named(s),
            Tok::Punct("*") => TypeExpr::Ptr(Box::new(self.ty()?)),
            Tok::Punct("[") => {
                let n = self.expr()?;
                self.expect("]")?;
                TypeExpr::Array(Box::new(n), Box::new(self.ty()?))
            }
            Tok::Kw("fn") => {
                self.expect("(")?;
                let mut ps = Vec::new();
                if !self.eat(")") {
                    loop {
                        ps.push(self.ty()?);
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                let ret = if self.eat("->") { self.ty()? } else { TypeExpr::Void };
                TypeExpr::Fn(ps, Box::new(ret))
            }
            t => {
                self.i -= 1;
                return self.err(format!("expected a type, found {}", describe(&t)));
            }
        };
        Ok(t)
    }

    // ---- statements --------------------------------------------------------

    fn block(&mut self) -> R<Vec<Stmt>> {
        self.expect("{")?;
        let mut v = Vec::new();
        while !self.eat("}") {
            if *self.peek() == Tok::Eof {
                return self.err("unterminated block: expected `}`");
            }
            v.push(self.stmt()?);
        }
        Ok(v)
    }

    /// `neg`, `zero`, `pos` followed by `:` (contextual: they are not keywords).
    fn at_arm_label(&self) -> Option<&'static str> {
        let colon = matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Punct(":")));
        match self.peek() {
            Tok::Ident(s) if colon && s == "neg" => Some("neg"),
            Tok::Ident(s) if colon && s == "zero" => Some("zero"),
            Tok::Ident(s) if colon && s == "pos" => Some("pos"),
            _ => None,
        }
    }

    /// Statements until one of `stops` (not consumed). `"arm"` stops at a
    /// `sign` arm label.
    fn stmts_until(&mut self, stops: &[&str]) -> R<Vec<Stmt>> {
        let mut v = Vec::new();
        while !stops.iter().any(|s| if *s == "arm" { self.at_arm_label().is_some() } else { self.is(s) }) {
            if *self.peek() == Tok::Eof {
                return self.err("unexpected end of file");
            }
            v.push(self.stmt()?);
        }
        Ok(v)
    }

    fn stmt(&mut self) -> R<Stmt> {
        let pos = self.pos();
        let kind = if self.is("var") {
            StmtKind::Var(self.var_decl()?)
        } else if self.is("{") {
            StmtKind::Block(self.block()?)
        } else if self.eat("if") {
            self.expect("(")?;
            let c = self.expr()?;
            self.expect(")")?;
            let t = Box::new(self.stmt()?);
            let e = if self.eat("else") { Some(Box::new(self.stmt()?)) } else { None };
            StmtKind::If(c, t, e)
        } else if self.eat("while") {
            self.expect("(")?;
            let c = self.expr()?;
            self.expect(")")?;
            StmtKind::While(c, Box::new(self.stmt()?))
        } else if self.eat("loop") {
            StmtKind::Loop(Box::new(self.stmt()?))
        } else if self.eat("for") {
            self.expect("(")?;
            let init = if self.eat(";") {
                None
            } else if self.is("var") {
                Some(Box::new(Stmt { pos: self.pos(), kind: StmtKind::Var(self.var_decl()?) }))
            } else {
                let p = self.pos();
                let e = self.expr()?;
                self.expect(";")?;
                Some(Box::new(Stmt { pos: p, kind: StmtKind::Expr(e) }))
            };
            let cond = if self.is(";") { None } else { Some(self.expr()?) };
            self.expect(";")?;
            let step = if self.is(")") { None } else { Some(self.expr()?) };
            self.expect(")")?;
            StmtKind::For(init, cond, step, Box::new(self.stmt()?))
        } else if self.eat("sign") {
            self.expect("(")?;
            let e = self.expr()?;
            self.expect(")")?;
            self.expect("{")?;
            let (mut neg, mut zero, mut pos_) = (None, None, None);
            while !self.eat("}") {
                let arm = match self.at_arm_label() {
                    Some("neg") => &mut neg,
                    Some("zero") => &mut zero,
                    Some(_) => &mut pos_,
                    None => return self.err(format!("expected `neg:`, `zero:` or `pos:`, found {}", describe(self.peek()))),
                };
                if arm.is_some() {
                    return self.err("this arm appears twice");
                }
                self.next();
                self.expect(":")?;
                *arm = Some(self.stmts_until(&["arm", "}"])?);
            }
            StmtKind::Sign { e, neg: neg.unwrap_or_default(), zero: zero.unwrap_or_default(), pos: pos_.unwrap_or_default() }
        } else if self.eat("switch") {
            self.expect("(")?;
            let e = self.expr()?;
            self.expect(")")?;
            self.expect("{")?;
            let (mut cases, mut default) = (Vec::new(), None);
            while !self.eat("}") {
                if self.eat("default") {
                    self.expect(":")?;
                    if default.is_some() {
                        return self.err("two `default` arms");
                    }
                    default = Some(self.stmts_until(&["case", "default", "}"])?);
                    continue;
                }
                self.expect("case")?;
                let mut vals = vec![self.expr()?];
                while self.eat(",") {
                    vals.push(self.expr()?);
                }
                self.expect(":")?;
                cases.push((vals, self.stmts_until(&["case", "default", "}"])?));
            }
            StmtKind::Switch { e, cases, default }
        } else if self.eat("break") {
            self.expect(";")?;
            StmtKind::Break
        } else if self.eat("continue") {
            self.expect(";")?;
            StmtKind::Continue
        } else if self.eat("return") {
            let e = if self.is(";") { None } else { Some(self.expr()?) };
            self.expect(";")?;
            StmtKind::Return(e)
        } else if self.eat("asm") {
            self.expect("{")?;
            let mut lines = Vec::new();
            while !self.eat("}") {
                match self.next() {
                    Tok::Str(s) => lines.push(s),
                    t => {
                        self.i -= 1;
                        return self.err(format!("expected an assembly string, found {}", describe(&t)));
                    }
                }
            }
            StmtKind::Asm(lines)
        } else {
            let e = self.expr()?;
            self.expect(";")?;
            StmtKind::Expr(e)
        };
        Ok(Stmt { kind, pos })
    }

    // ---- expressions ---------------------------------------------------------

    pub fn expr(&mut self) -> R<Expr> {
        let lhs = self.cond()?;
        let pos = self.pos();
        let op = match self.peek() {
            Tok::Punct("=") => None,
            Tok::Punct("+=") => Some(BinOp::Add),
            Tok::Punct("-=") => Some(BinOp::Sub),
            Tok::Punct("*=") => Some(BinOp::Mul),
            Tok::Punct("/=") => Some(BinOp::Div),
            Tok::Punct("%=") => Some(BinOp::Mod),
            Tok::Punct("<<=") => Some(BinOp::Shl),
            Tok::Punct(">>=") => Some(BinOp::Shr),
            Tok::Punct("&=") => Some(BinOp::Min),
            Tok::Punct("|=") => Some(BinOp::Max),
            _ => return Ok(lhs),
        };
        self.next();
        let rhs = self.expr()?;
        Ok(Expr { kind: ExprKind::Assign(op, Box::new(lhs), Box::new(rhs)), pos })
    }

    fn cond(&mut self) -> R<Expr> {
        let c = self.binary(0)?;
        if self.is("?") {
            let pos = self.pos();
            self.next();
            let a = self.expr()?;
            self.expect(":")?;
            let b = self.cond()?;
            return Ok(Expr { kind: ExprKind::Cond(Box::new(c), Box::new(a), Box::new(b)), pos });
        }
        Ok(c)
    }

    fn binary(&mut self, level: usize) -> R<Expr> {
        const LEVELS: &[&[(&str, BinOp)]] = &[
            &[("||", BinOp::Or)],
            &[("&&", BinOp::And)],
            &[("==", BinOp::Eq), ("!=", BinOp::Ne)],
            &[("<", BinOp::Lt), ("<=", BinOp::Le), (">", BinOp::Gt), (">=", BinOp::Ge)],
            &[("<=>", BinOp::Cmp3)],
            &[("|", BinOp::Max)],
            &[("&", BinOp::Min)],
            &[("<<", BinOp::Shl), (">>", BinOp::Shr)],
            &[("+", BinOp::Add), ("-", BinOp::Sub)],
            &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Mod)],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        'outer: loop {
            for (p, op) in LEVELS[level] {
                if matches!(self.peek(), Tok::Punct(q) if q == p) {
                    let pos = self.pos();
                    self.next();
                    let rhs = self.binary(level + 1)?;
                    lhs = Expr { kind: ExprKind::Binary(*op, Box::new(lhs), Box::new(rhs)), pos };
                    continue 'outer;
                }
            }
            return Ok(lhs);
        }
    }

    /// Prefix operators bind tighter than `as`: `&x as int` = `(&x) as int`.
    fn unary(&mut self) -> R<Expr> {
        let mut e = self.prefix()?;
        while self.is("as") {
            let pos = self.pos();
            self.next();
            let t = self.ty()?;
            e = Expr { kind: ExprKind::Cast(Box::new(e), t), pos };
        }
        Ok(e)
    }

    fn prefix(&mut self) -> R<Expr> {
        let pos = self.pos();
        let op = match self.peek() {
            Tok::Punct("-") | Tok::Punct("~") => Some(UnOp::Neg),
            Tok::Punct("!") => Some(UnOp::Not),
            Tok::Punct("*") => Some(UnOp::Deref),
            Tok::Punct("&") => Some(UnOp::Addr),
            _ => None,
        };
        if let Some(op) = op {
            self.next();
            let e = self.prefix()?;
            return Ok(Expr { kind: ExprKind::Unary(op, Box::new(e)), pos });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.primary()?;
        loop {
            let pos = self.pos();
            if self.eat("(") {
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                e = Expr { kind: ExprKind::Call(Box::new(e), args), pos };
            } else if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr { kind: ExprKind::Index(Box::new(e), Box::new(i)), pos };
            } else if self.eat(".") {
                let f = self.ident()?;
                e = Expr { kind: ExprKind::Field(Box::new(e), f), pos };
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> R<Expr> {
        let pos = self.pos();
        let kind = match self.next() {
            Tok::Int(v) => ExprKind::Int(v),
            Tok::Char(v) => ExprKind::Char(v),
            Tok::Str(s) => ExprKind::Str(s),
            Tok::Kw("true") => ExprKind::Int(1),
            Tok::Kw("false") => ExprKind::Int(0),
            Tok::Kw("null") => ExprKind::Null,
            Tok::Ident(s) => ExprKind::Name(s),
            Tok::Kw("sizeof") => {
                self.expect("(")?;
                let t = self.ty()?;
                self.expect(")")?;
                ExprKind::Sizeof(t)
            }
            Tok::Punct("(") => {
                let e = self.expr()?;
                self.expect(")")?;
                return Ok(e);
            }
            t => {
                self.i -= 1;
                return self.err(format!("expected an expression, found {}", describe(&t)));
            }
        };
        Ok(Expr { kind, pos })
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Int(v) => format!("number {v}"),
        Tok::Char(_) => "a character literal".into(),
        Tok::Str(_) => "a string".into(),
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Kw(k) => format!("`{k}`"),
        Tok::Punct(p) => format!("`{p}`"),
        Tok::Eof => "end of file".into(),
    }
}
