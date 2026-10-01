//! Type checking and code generation: Trine AST → Setun-386 assembly text.
//!
//! Code generation is deliberately simple (v1): every expression leaves its
//! value in `r1`; intermediate values live on the stack (`push`/`pop`), so
//! nothing needs saving around calls. Locals sit in the frame below `fp`.
//! Globals live in RAM at addresses the compiler assigns; their
//! initialisers run as code in `_trine_init`.

use crate::ast::*;
use crate::lexer::Pos;
use crate::Error;
use std::collections::HashMap;
use std::fmt;

// ---- types --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Tryte,
    Trit,
    Void,
    Null,
    Ptr(Box<Ty>),
    Array(i64, Box<Ty>),
    Struct(usize),
    Fn(Vec<Ty>, Box<Ty>),
}

impl Ty {
    fn is_integer(&self) -> bool {
        matches!(self, Ty::Int | Ty::Tryte | Ty::Trit)
    }
    fn is_pointer(&self) -> bool {
        matches!(self, Ty::Ptr(_) | Ty::Null)
    }
    fn is_scalar(&self) -> bool {
        matches!(self, Ty::Int | Ty::Tryte | Ty::Trit | Ty::Ptr(_) | Ty::Fn(..) | Ty::Null)
    }
    fn is_tryte_sized(&self) -> bool {
        matches!(self, Ty::Tryte | Ty::Trit)
    }
    /// Arrays decay to pointers to their first element.
    fn decay(self) -> Ty {
        match self {
            Ty::Array(_, t) => Ty::Ptr(t),
            t => t,
        }
    }
}

struct StructDef {
    name: String,
    fields: Vec<(String, Ty, i64)>,
    size: i64,
    state: u8, // 0 = declared, 1 = laying out, 2 = done
    decl: Vec<Param>,
    pos: Pos,
}

pub struct TyFmt<'a>(&'a Ty, &'a [StructDef]);

impl fmt::Display for TyFmt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.1;
        match self.0 {
            Ty::Int => write!(f, "int"),
            Ty::Tryte => write!(f, "tryte"),
            Ty::Trit => write!(f, "trit"),
            Ty::Void => write!(f, "void"),
            Ty::Null => write!(f, "null"),
            Ty::Ptr(t) => write!(f, "*{}", TyFmt(t, s)),
            Ty::Array(n, t) => write!(f, "[{n}]{}", TyFmt(t, s)),
            Ty::Struct(i) => write!(f, "{}", s[*i].name),
            Ty::Fn(ps, r) => {
                write!(f, "fn(")?;
                for (i, p) in ps.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", TyFmt(p, s))?;
                }
                write!(f, ") -> {}", TyFmt(r, s))
            }
        }
    }
}

// ---- symbols ------------------------------------------------------------------

#[derive(Clone)]
enum ConstVal {
    Int(i64),
    Str(String),
}

#[derive(Clone)]
enum Global {
    /// A variable at a known address (`label` is an assembler constant).
    Var { ty: Ty, label: String },
    /// `extern var`: an assembler label defined elsewhere.
    Extern { ty: Ty, label: String },
    Const { ty: Ty, val: ConstVal },
    Func { ty: Ty, defined: bool },
}

struct Local {
    name: String,
    ty: Ty,
    /// Object address relative to `fp`.
    off: i64,
}

/// Where an object lives, for initialisers.
#[derive(Clone, Copy)]
enum Place {
    Frame(i64),
    Abs(i64),
}

impl Place {
    fn plus(self, d: i64) -> Place {
        match self {
            Place::Frame(o) => Place::Frame(o + d),
            Place::Abs(a) => Place::Abs(a + d),
        }
    }
    fn operand(self) -> String {
        match self {
            Place::Frame(o) => mem("fp", o),
            Place::Abs(a) => format!("[{a}]"),
        }
    }
}

fn mem(base: &str, off: i64) -> String {
    match off {
        0 => format!("[{base}]"),
        o if o > 0 => format!("[{base}+{o}]"),
        o => format!("[{base}{o}]"),
    }
}

/// Names that would read as registers in the generated assembly.
fn is_register_name(s: &str) -> bool {
    ["sp", "lr", "fp", "zero"].contains(&s) || s.strip_prefix('r').is_some_and(|n| n.parse::<i64>().is_ok())
}

const CREGS: &[&str] = &["MODE", "PTB", "FADDR", "FCAUSE", "ITB", "EPC", "EMODE", "KSP", "SCRATCH", "CPUID", "CYCLE"];

const BUILTINS: &[&str] = &[
    "sgn", "abs", "divr", "modr", "tmul", "tsat", "cons", "mulh", "trit", "ext", "ins", "tlz", "bytes", "in", "out",
    "mfc", "mtc", "hlt", "brk", "tlbflush", "tlbinv", "fence", "syscall",
];

// ---- compiler -------------------------------------------------------------------

pub struct Layout {
    /// First global's address (a word address).
    pub data_org: i64,
}

pub struct Compiled {
    /// Functions, string pool and symbol equates (no `.org`).
    pub text: String,
    pub data_start: i64,
    pub data_end: i64,
}

type R<T> = Result<T, Error>;

pub struct Compiler {
    structs: Vec<StructDef>,
    struct_ids: HashMap<String, usize>,
    globals: HashMap<String, Global>,
    // per function
    locals: Vec<Local>,
    scopes: Vec<usize>,
    next_slot: i64,
    lowest_slot: i64,
    loops: Vec<(Option<String>, String)>, // (continue, break)
    ret_ty: Ty,
    ret_label: String,
    // output
    code: Vec<String>,
    strings: Vec<(String, String)>,
    labels: usize,
    data_cursor: i64,
    data_start: i64,
}

fn err<T>(pos: Pos, msg: impl Into<String>) -> R<T> {
    Err(Error { pos, msg: msg.into() })
}

impl Compiler {
    pub fn new() -> Self {
        Compiler {
            structs: Vec::new(),
            struct_ids: HashMap::new(),
            globals: HashMap::new(),
            locals: Vec::new(),
            scopes: Vec::new(),
            next_slot: -3,
            lowest_slot: -3,
            loops: Vec::new(),
            ret_ty: Ty::Void,
            ret_label: String::new(),
            code: Vec::new(),
            strings: Vec::new(),
            labels: 0,
            data_cursor: 0,
            data_start: 0,
        }
    }

    fn tyname(&self, t: &Ty) -> String {
        TyFmt(t, &self.structs).to_string()
    }

    fn emit(&mut self, s: impl Into<String>) {
        self.code.push(format!("        {}", s.into()));
    }

    fn label(&mut self, l: &str) {
        self.code.push(format!("{l}:"));
    }

    fn new_label(&mut self) -> String {
        self.labels += 1;
        format!(".L{}", self.labels)
    }

    fn intern(&mut self, s: &str) -> String {
        if let Some((l, _)) = self.strings.iter().find(|(_, v)| v == s) {
            return l.clone();
        }
        let l = format!(".Lstr{}", self.strings.len());
        self.strings.push((l.clone(), s.to_string()));
        l
    }

    // ---- sizes and layout ----------------------------------------------------

    fn size(&self, t: &Ty) -> i64 {
        match t {
            Ty::Int | Ty::Ptr(_) | Ty::Fn(..) | Ty::Null => 3,
            Ty::Tryte | Ty::Trit => 1,
            Ty::Void => 0,
            Ty::Array(n, e) => n * self.size(e),
            Ty::Struct(i) => self.structs[*i].size,
        }
    }

    /// Word-addressed objects are addressed by the centre of their first word.
    fn word_addressed(&self, t: &Ty) -> bool {
        match t {
            Ty::Tryte | Ty::Trit | Ty::Void => false,
            Ty::Array(_, e) => self.word_addressed(e),
            _ => true,
        }
    }

    /// Words of storage an object needs.
    fn words(&self, t: &Ty) -> i64 {
        (self.size(t) + 2).div_euclid(3).max(1)
    }

    fn resolve(&mut self, t: &TypeExpr, pos: Pos) -> R<Ty> {
        Ok(match t {
            TypeExpr::Int => Ty::Int,
            TypeExpr::Tryte => Ty::Tryte,
            TypeExpr::Trit => Ty::Trit,
            TypeExpr::Void => Ty::Void,
            TypeExpr::Named(n) => match self.struct_ids.get(n) {
                Some(&i) => Ty::Struct(i),
                None => return err(pos, format!("unknown type `{n}`")),
            },
            TypeExpr::Ptr(t) => Ty::Ptr(Box::new(self.resolve_ptr_target(t, pos)?)),
            TypeExpr::Array(n, t) => {
                let len = self.const_int(n)?;
                if len <= 0 {
                    return err(pos, format!("array length must be positive, not {len}"));
                }
                let e = self.resolve(t, pos)?;
                self.complete(&e, pos)?;
                if e == Ty::Void {
                    return err(pos, "arrays of void are not allowed");
                }
                Ty::Array(len, Box::new(e))
            }
            TypeExpr::Fn(ps, r) => {
                let ps = ps.iter().map(|p| self.resolve(p, pos)).collect::<R<Vec<_>>>()?;
                Ty::Fn(ps, Box::new(self.resolve(r, pos)?))
            }
        })
    }

    /// Pointer targets may be structs that are not laid out yet.
    fn resolve_ptr_target(&mut self, t: &TypeExpr, pos: Pos) -> R<Ty> {
        match t {
            TypeExpr::Named(n) => match self.struct_ids.get(n) {
                Some(&i) => Ok(Ty::Struct(i)),
                None => err(pos, format!("unknown type `{n}`")),
            },
            _ => self.resolve(t, pos),
        }
    }

    /// Make sure a struct used by value has its layout.
    fn complete(&mut self, t: &Ty, _pos: Pos) -> R<()> {
        match t {
            Ty::Struct(i) => self.layout_struct(*i),
            Ty::Array(_, e) => self.complete(e, _pos),
            _ => Ok(()),
        }
    }

    fn layout_struct(&mut self, id: usize) -> R<()> {
        match self.structs[id].state {
            2 => return Ok(()),
            1 => return err(self.structs[id].pos, format!("struct `{}` contains itself", self.structs[id].name)),
            _ => {}
        }
        self.structs[id].state = 1;
        let decl = self.structs[id].decl.clone();
        let spos = self.structs[id].pos;
        let mut fields = Vec::new();
        let mut at = 0i64; // next free tryte, counted from the struct's first tryte
        for p in &decl {
            let t = self.resolve(&p.ty, spos)?;
            self.complete(&t, spos)?;
            if t == Ty::Void {
                return err(spos, format!("field `{}` cannot be void", p.name));
            }
            if fields.iter().any(|(n, _, _)| n == &p.name) {
                return err(spos, format!("field `{}` appears twice", p.name));
            }
            let wa = self.word_addressed(&t);
            if wa {
                at = (at + 2).div_euclid(3) * 3;
            }
            // Struct address = first tryte + 1 (centre of the first word).
            let off = at - 1 + wa as i64;
            fields.push((p.name.clone(), t.clone(), off));
            at += self.size(&t);
        }
        let s = &mut self.structs[id];
        s.fields = fields;
        s.size = ((at + 2).div_euclid(3) * 3).max(3);
        s.state = 2;
        Ok(())
    }

    // ---- constants -------------------------------------------------------------

    fn const_eval(&self, e: &Expr) -> Option<i64> {
        Some(match &e.kind {
            ExprKind::Int(v) | ExprKind::Char(v) => *v,
            ExprKind::Name(n) => match self.lookup_local(n) {
                Some(_) => return None,
                None => match self.globals.get(n) {
                    Some(Global::Const { val: ConstVal::Int(v), .. }) => *v,
                    _ => return None,
                },
            },
            ExprKind::Unary(UnOp::Neg, x) => -self.const_eval(x)?,
            ExprKind::Unary(UnOp::Not, x) => (self.const_eval(x)? == 0) as i64,
            ExprKind::Binary(op, a, b) => {
                let (x, y) = (self.const_eval(a)?, self.const_eval(b)?);
                match op {
                    BinOp::Add => x.checked_add(y)?,
                    BinOp::Sub => x.checked_sub(y)?,
                    BinOp::Mul => x.checked_mul(y)?,
                    BinOp::Div if y != 0 => x / y,
                    BinOp::Mod if y != 0 => x % y,
                    BinOp::Shl if (0..27).contains(&y) => x.checked_mul(3i64.pow(y as u32))?,
                    BinOp::Lt => (x < y) as i64,
                    BinOp::Le => (x <= y) as i64,
                    BinOp::Gt => (x > y) as i64,
                    BinOp::Ge => (x >= y) as i64,
                    BinOp::Eq => (x == y) as i64,
                    BinOp::Ne => (x != y) as i64,
                    BinOp::Cmp3 => (x - y).signum(),
                    _ => return None,
                }
            }
            ExprKind::Cast(x, t) => {
                let v = self.const_eval(x)?;
                match t {
                    TypeExpr::Tryte => ternary::wrap_trits(v as i128, 9) as i64,
                    TypeExpr::Trit => ternary::wrap_trits(v as i128, 1) as i64,
                    TypeExpr::Int | TypeExpr::Ptr(_) => v,
                    _ => return None,
                }
            }
            ExprKind::Sizeof(t) => return self.sizeof_const(t),
            _ => return None,
        })
    }

    fn sizeof_const(&self, t: &TypeExpr) -> Option<i64> {
        Some(match t {
            TypeExpr::Int | TypeExpr::Ptr(_) | TypeExpr::Fn(..) => 3,
            TypeExpr::Tryte | TypeExpr::Trit => 1,
            TypeExpr::Void => return None,
            TypeExpr::Named(n) => {
                let s = &self.structs[*self.struct_ids.get(n)?];
                if s.state != 2 {
                    return None;
                }
                s.size
            }
            TypeExpr::Array(n, e) => self.const_eval(n)? * self.sizeof_const(e)?,
        })
    }

    fn const_int(&self, e: &Expr) -> R<i64> {
        self.const_eval(e).ok_or_else(|| Error { pos: e.pos, msg: "a constant integer expression is required here".into() })
    }

    // ---- names -------------------------------------------------------------------

    fn lookup_local(&self, n: &str) -> Option<&Local> {
        self.locals.iter().rev().find(|l| l.name == n)
    }

    fn alloc_local(&mut self, name: &str, ty: Ty, pos: Pos) -> R<i64> {
        let start = *self.scopes.last().unwrap();
        if self.locals[start..].iter().any(|l| l.name == name) {
            return err(pos, format!("`{name}` is already declared in this block"));
        }
        let words = self.words(&ty);
        let first_centre = self.next_slot - 3 * (words - 1);
        let off = first_centre - (!self.word_addressed(&ty)) as i64;
        self.next_slot = first_centre - 3;
        self.lowest_slot = self.lowest_slot.min(first_centre);
        self.locals.push(Local { name: name.into(), ty, off });
        Ok(off)
    }

    fn alloc_global(&mut self, ty: &Ty, align: Option<i64>, pos: Pos) -> R<i64> {
        let words = self.words(ty);
        if let Some(a) = align {
            if a <= 0 || a % 3 != 0 {
                return err(pos, format!("@align({a}) must be a positive multiple of 3"));
            }
            self.data_cursor = (self.data_cursor + a - 1).div_euclid(a) * a;
        }
        let first_centre = self.data_cursor;
        self.data_cursor += 3 * words;
        Ok(first_centre - (!self.word_addressed(ty)) as i64)
    }

    // ---- program -------------------------------------------------------------------

    pub fn compile(&mut self, items: &[Item], layout: &Layout) -> R<Compiled> {
        self.data_cursor = layout.data_org;
        self.data_start = layout.data_org;
        // Pass 1: struct names.
        for it in items {
            if let Item::Struct { name, fields, pos } = it {
                if self.struct_ids.contains_key(name) {
                    return err(*pos, format!("struct `{name}` is defined twice"));
                }
                self.struct_ids.insert(name.clone(), self.structs.len());
                self.structs.push(StructDef { name: name.clone(), fields: Vec::new(), size: 0, state: 0, decl: fields.clone(), pos: *pos });
            }
        }
        for i in 0..self.structs.len() {
            self.layout_struct(i)?;
        }
        // Pass 2: constants, function signatures, globals (in order).
        let mut inits: Vec<(Place, Ty, Expr)> = Vec::new();
        for it in items {
            match it {
                Item::Const(d) => {
                    let ty = self.resolve(&d.ty, d.pos)?;
                    let e = d.init.as_ref().unwrap();
                    let val = match (&e.kind, &ty) {
                        (ExprKind::Str(s), Ty::Ptr(t)) if **t == Ty::Tryte => ConstVal::Str(self.intern(s)),
                        _ => {
                            let v = self.const_int(e)?;
                            self.check_fits(v, &ty, e.pos)?;
                            ConstVal::Int(v)
                        }
                    };
                    self.define(&d.name, Global::Const { ty, val }, d.pos)?;
                }
                Item::Fn(f) => {
                    let ps = f.params.iter().map(|p| self.resolve(&p.ty, f.pos)).collect::<R<Vec<_>>>()?;
                    let ret = self.resolve(&f.ret, f.pos)?;
                    for p in &ps {
                        if !p.is_scalar() {
                            return err(f.pos, format!("parameter of type {} must be passed as a pointer", self.tyname(p)));
                        }
                    }
                    if !ret.is_scalar() && ret != Ty::Void {
                        return err(f.pos, format!("cannot return {} by value; return a pointer", self.tyname(&ret)));
                    }
                    if f.interrupt && (!ps.is_empty() || ret != Ty::Void) {
                        return err(f.pos, "an interrupt fn takes no parameters and returns nothing");
                    }
                    let ty = Ty::Fn(ps, Box::new(ret));
                    let defined = f.body.is_some();
                    match self.globals.get(&f.name) {
                        Some(Global::Func { ty: t2, defined: d2, .. }) if *t2 == ty && !(defined && *d2) => {
                            let d = defined || *d2;
                            self.globals.insert(f.name.clone(), Global::Func { ty, defined: d });
                        }
                        Some(_) => return err(f.pos, format!("`{}` is already defined", f.name)),
                        None => self.define(&f.name, Global::Func { ty, defined }, f.pos)?,
                    }
                }
                Item::Var { attrs, decl, external } => {
                    let ty = self.resolve(&decl.ty, decl.pos)?;
                    if ty == Ty::Void {
                        return err(decl.pos, "a variable cannot be void");
                    }
                    if *external {
                        self.define(&decl.name, Global::Extern { ty, label: decl.name.clone() }, decl.pos)?;
                        continue;
                    }
                    let mut align = None;
                    let mut at = None;
                    for a in attrs {
                        match a {
                            Attr::Align(e) => align = Some(self.const_int(e)?),
                            Attr::At(e) => at = Some(self.const_int(e)?),
                        }
                    }
                    let addr = match at {
                        Some(a) => a,
                        None => self.alloc_global(&ty, align, decl.pos)?,
                    };
                    if let Some(init) = &decl.init {
                        inits.push((Place::Abs(addr), ty.clone(), init.clone()));
                    }
                    self.define(&decl.name, Global::Var { ty, label: format!("{addr}") }, decl.pos)?;
                }
                Item::Struct { .. } | Item::Import(..) => {}
            }
        }
        // Pass 3: function bodies.
        for it in items {
            if let Item::Fn(f) = it {
                if f.body.is_some() {
                    self.function(f)?;
                }
            }
        }
        // Global initialisers.
        self.label("_trine_init");
        self.emit("push lr");
        self.emit("push fp");
        self.emit("mov fp, sp");
        self.begin_fn(Ty::Void);
        let body_start = self.code.len();
        for (place, ty, init) in &inits {
            self.init_object(*place, ty, init)?;
        }
        let frame = -self.lowest_slot;
        if frame > 0 {
            self.code.insert(body_start, format!("        sub sp, sp, {frame}"));
        }
        self.emit("mov sp, fp");
        self.emit("pop fp");
        self.emit("pop lr");
        self.emit("ret");
        // `extern fn`s may live in assembly; the assembler reports any that are missing.
        if !matches!(self.globals.get("main"), Some(Global::Func { defined: true, .. })) {
            return err(Pos::default(), "no `fn main()` defined");
        }

        let mut text = self.code.join("\n");
        text.push('\n');
        for (l, s) in &self.strings {
            text += &format!("{l}: .string \"{}\"\n", escape(s));
        }
        Ok(Compiled { text, data_start: self.data_start, data_end: self.data_cursor })
    }

    fn define(&mut self, name: &str, g: Global, pos: Pos) -> R<()> {
        if is_register_name(name) || name.starts_with("_trine") {
            return err(pos, format!("`{name}` is a reserved name"));
        }
        if self.globals.contains_key(name) {
            return err(pos, format!("`{name}` is already defined"));
        }
        self.globals.insert(name.into(), g);
        Ok(())
    }

    fn check_fits(&self, v: i64, ty: &Ty, pos: Pos) -> R<()> {
        let lim = match ty {
            Ty::Tryte => 9841,
            Ty::Trit => 1,
            Ty::Int | Ty::Ptr(_) => 3_812_798_742_493,
            _ => return err(pos, format!("a constant of type {} is not supported", self.tyname(ty))),
        };
        if v.abs() > lim {
            return err(pos, format!("{v} does not fit in {}", self.tyname(ty)));
        }
        Ok(())
    }

    fn begin_fn(&mut self, ret: Ty) {
        self.locals.clear();
        self.scopes = vec![0];
        self.next_slot = -3;
        self.lowest_slot = 0; // lowest slot centre in use; 0 = none, so the frame is empty
        self.loops.clear();
        self.ret_ty = ret;
        self.ret_label = self.new_label();
    }

    fn function(&mut self, f: &FnDecl) -> R<()> {
        let Some(Global::Func { ty: Ty::Fn(ps, ret), .. }) = self.globals.get(&f.name).cloned() else { unreachable!() };
        self.begin_fn((*ret).clone());
        self.label(&f.name);
        const SAVED: &[&str] = &["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r-10", "r-11", "r-12", "r-13"];
        if f.interrupt {
            for r in SAVED {
                self.emit(format!("push {r}"));
            }
        }
        self.emit("push lr");
        self.emit("push fp");
        self.emit("mov fp, sp");
        let frame_at = self.code.len();
        // Parameters: r1..r6, then the stack (fp+6, fp+9, ...), copied into slots.
        for (i, (p, t)) in f.params.iter().zip(&ps).enumerate() {
            let off = self.alloc_local(&p.name, t.clone(), f.pos)?;
            let st = if t.is_tryte_sized() { "stt" } else { "stw" };
            if i < 6 {
                self.emit(format!("{st} r{}, {}", i + 1, mem("fp", off)));
            } else {
                self.emit(format!("ldw r7, {}", mem("fp", 6 + 3 * (i as i64 - 6))));
                self.emit(format!("{st} r7, {}", mem("fp", off)));
            }
        }
        for s in f.body.as_ref().unwrap() {
            self.stmt(s)?;
        }
        let frame = -self.lowest_slot;
        if frame > 0 {
            self.code.insert(frame_at, format!("        sub sp, sp, {frame}"));
        }
        let rl = self.ret_label.clone();
        self.label(&rl);
        self.emit("mov sp, fp");
        self.emit("pop fp");
        self.emit("pop lr");
        if f.interrupt {
            for r in SAVED.iter().rev() {
                self.emit(format!("pop {r}"));
            }
            self.emit("iret");
        } else {
            self.emit("ret");
        }
        Ok(())
    }

    // ---- statements ------------------------------------------------------------------

    fn block(&mut self, stmts: &[Stmt]) -> R<()> {
        self.scopes.push(self.locals.len());
        let saved = self.next_slot;
        for s in stmts {
            self.stmt(s)?;
        }
        let start = self.scopes.pop().unwrap();
        self.locals.truncate(start);
        self.next_slot = saved;
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match &s.kind {
            StmtKind::Var(d) => {
                let ty = self.resolve(&d.ty, d.pos)?;
                if ty == Ty::Void {
                    return err(d.pos, "a variable cannot be void");
                }
                // The initialiser cannot see the variable it initialises.
                if let Some(init) = &d.init {
                    // Reserve the slot, generate the initialiser (which must not
                    // see the new name), then declare the name at that slot.
                    let saved = self.next_slot;
                    let words = self.words(&ty);
                    let first = saved - 3 * (words - 1);
                    let off = first - (!self.word_addressed(&ty)) as i64;
                    self.next_slot = first - 3;
                    self.lowest_slot = self.lowest_slot.min(first);
                    self.init_object(Place::Frame(off), &ty, init)?;
                    self.next_slot = saved;
                    let got = self.alloc_local(&d.name, ty, d.pos)?;
                    debug_assert_eq!(got, off);
                } else {
                    self.alloc_local(&d.name, ty, d.pos)?;
                }
            }
            StmtKind::Expr(e) => {
                self.expr(e)?;
            }
            StmtKind::Block(b) => self.block(b)?,
            StmtKind::If(c, t, e) => {
                let (lelse, lend) = (self.new_label(), self.new_label());
                self.cond_jump_false(c, &lelse)?;
                self.block(std::slice::from_ref(t))?;
                if let Some(e) = e {
                    self.emit(format!("b {lend}"));
                    self.label(&lelse);
                    self.block(std::slice::from_ref(e))?;
                    self.label(&lend);
                } else {
                    self.label(&lelse);
                }
            }
            StmtKind::While(c, body) => {
                let (top, end) = (self.new_label(), self.new_label());
                self.label(&top);
                self.cond_jump_false(c, &end)?;
                self.loops.push((Some(top.clone()), end.clone()));
                self.block(std::slice::from_ref(body))?;
                self.loops.pop();
                self.emit(format!("b {top}"));
                self.label(&end);
            }
            StmtKind::Loop(body) => {
                let (top, end) = (self.new_label(), self.new_label());
                self.label(&top);
                self.loops.push((Some(top.clone()), end.clone()));
                self.block(std::slice::from_ref(body))?;
                self.loops.pop();
                self.emit(format!("b {top}"));
                self.label(&end);
            }
            StmtKind::For(init, cond, step, body) => {
                self.scopes.push(self.locals.len());
                let saved = self.next_slot;
                if let Some(i) = init {
                    self.stmt(i)?;
                }
                let (top, cont, end) = (self.new_label(), self.new_label(), self.new_label());
                self.label(&top);
                if let Some(c) = cond {
                    self.cond_jump_false(c, &end)?;
                }
                self.loops.push((Some(cont.clone()), end.clone()));
                self.block(std::slice::from_ref(body))?;
                self.loops.pop();
                self.label(&cont);
                if let Some(st) = step {
                    self.expr(st)?;
                }
                self.emit(format!("b {top}"));
                self.label(&end);
                let start = self.scopes.pop().unwrap();
                self.locals.truncate(start);
                self.next_slot = saved;
            }
            StmtKind::Sign { e, neg, zero, pos } => {
                // `sign (a <=> b)` compares directly (exact); otherwise test against 0.
                if let ExprKind::Binary(BinOp::Cmp3, a, b) = &e.kind {
                    self.compare(a, b)?;
                } else {
                    let t = self.expr(e)?;
                    self.need_integer(&t, e.pos)?;
                    self.emit("cmp r1, 0");
                }
                let (ln, lp, lz, lend) = (self.new_label(), self.new_label(), self.new_label(), self.new_label());
                let (tn, tp) = (self.new_label(), self.new_label());
                self.emit(format!("br3 {tn}, {tp}"));
                self.emit(format!("b {lz}"));
                self.label(&tn);
                self.emit(format!("b {ln}"));
                self.label(&tp);
                self.emit(format!("b {lp}"));
                for (l, arm) in [(&lz, zero), (&ln, neg), (&lp, pos)] {
                    self.label(l);
                    self.block(arm)?;
                    self.emit(format!("b {lend}"));
                }
                self.label(&lend);
            }
            StmtKind::Switch { e, cases, default } => {
                let t = self.expr(e)?;
                self.need_integer(&t, e.pos)?;
                let end = self.new_label();
                let mut arm_labels = Vec::new();
                let mut seen = Vec::new();
                for (vals, _) in cases {
                    let l = self.new_label();
                    for v in vals {
                        let c = self.const_int(v)?;
                        if seen.contains(&c) {
                            return err(v.pos, format!("case {c} appears twice"));
                        }
                        seen.push(c);
                        self.emit(format!("cmp r1, {c}"));
                        self.emit(format!("beq {l}"));
                    }
                    arm_labels.push(l);
                }
                let dl = self.new_label();
                self.emit(format!("b {}", if default.is_some() { &dl } else { &end }));
                let cont = self.loops.last().and_then(|(c, _)| c.clone());
                self.loops.push((cont, end.clone()));
                for ((_, body), l) in cases.iter().zip(&arm_labels) {
                    self.label(l);
                    self.block(body)?;
                    self.emit(format!("b {end}"));
                }
                if let Some(d) = default {
                    self.label(&dl);
                    self.block(d)?;
                }
                self.loops.pop();
                self.label(&end);
            }
            StmtKind::Break => match self.loops.last() {
                Some((_, b)) => {
                    let b = b.clone();
                    self.emit(format!("b {b}"));
                }
                None => return err(s.pos, "`break` outside a loop or switch"),
            },
            StmtKind::Continue => match self.loops.iter().rev().find_map(|(c, _)| c.clone()) {
                Some(c) => self.emit(format!("b {c}")),
                None => return err(s.pos, "`continue` outside a loop"),
            },
            StmtKind::Return(e) => {
                let ret = self.ret_ty.clone();
                match (e, &ret) {
                    (None, Ty::Void) => {}
                    (None, _) => return err(s.pos, format!("this function must return a {}", self.tyname(&ret))),
                    (Some(e), Ty::Void) => return err(e.pos, "a void function cannot return a value"),
                    (Some(e), _) => {
                        let t = self.expr(e)?;
                        self.check_assign(&ret, &t, e)?;
                        self.narrow_to(&ret, &t);
                    }
                }
                let rl = self.ret_label.clone();
                self.emit(format!("b {rl}"));
            }
            StmtKind::Asm(lines) => {
                for l in lines {
                    self.code.push(l.clone());
                }
            }
        }
        Ok(())
    }

    /// Jump to `target` when `c` is false (zero).
    fn cond_jump_false(&mut self, c: &Expr, target: &str) -> R<()> {
        // Comparisons branch directly on the flags.
        if let ExprKind::Binary(op @ (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne), a, b) = &c.kind {
            self.compare(a, b)?;
            let inv = match op {
                BinOp::Lt => "bge",
                BinOp::Le => "bgt",
                BinOp::Gt => "ble",
                BinOp::Ge => "blt",
                BinOp::Eq => "bne",
                _ => "beq",
            };
            self.emit(format!("{inv} {target}"));
            return Ok(());
        }
        let t = self.expr(c)?;
        self.need_condition(&t, c.pos)?;
        self.emit("cmp r1, 0");
        self.emit(format!("beq {target}"));
        Ok(())
    }

    /// Evaluate `a` and `b` and set S = sign(a − b) (exact).
    fn compare(&mut self, a: &Expr, b: &Expr) -> R<()> {
        let ta = self.expr(a)?.decay();
        if let Some(v) = self.const_eval(b) {
            if !(ta.is_integer() || ta.is_pointer() && v == 0) {
                return err(b.pos, format!("cannot compare {} with a number", self.tyname(&ta)));
            }
            self.emit(format!("cmp r1, {v}"));
            return Ok(());
        }
        self.emit("push r1");
        let tb = self.expr(b)?.decay();
        let ok = (ta.is_integer() && tb.is_integer())
            || (ta.is_pointer() && tb.is_pointer())
            || (matches!(ta, Ty::Fn(..) | Ty::Null) && matches!(tb, Ty::Fn(..) | Ty::Null));
        if !ok {
            return err(b.pos, format!("cannot compare {} with {}", self.tyname(&ta), self.tyname(&tb)));
        }
        self.emit("mov r2, r1");
        self.emit("pop r1");
        self.emit("cmp r1, r2");
        Ok(())
    }

    fn need_integer(&self, t: &Ty, pos: Pos) -> R<()> {
        if t.is_integer() {
            Ok(())
        } else {
            err(pos, format!("expected an integer, found {}", self.tyname(t)))
        }
    }

    fn need_condition(&self, t: &Ty, pos: Pos) -> R<()> {
        if t.clone().decay().is_scalar() {
            Ok(())
        } else {
            err(pos, format!("{} cannot be used as a condition", self.tyname(t)))
        }
    }

    // ---- initialisers ------------------------------------------------------------

    fn init_object(&mut self, place: Place, ty: &Ty, init: &Expr) -> R<()> {
        match (ty, &init.kind) {
            (Ty::Array(n, e), ExprKind::Str(s)) if **e == Ty::Tryte => {
                let chars: Vec<i64> = s.chars().map(|c| c as i64).collect();
                if chars.len() as i64 > *n {
                    return err(init.pos, format!("string of {} characters does not fit in [{n}]char", chars.len()));
                }
                for i in 0..*n {
                    let c = chars.get(i as usize).copied().unwrap_or(0);
                    self.emit(format!("li r1, {c}"));
                    self.emit(format!("stt r1, {}", place.plus(i).operand()));
                }
                Ok(())
            }
            (Ty::Array(n, e), ExprKind::List(items)) => {
                if items.len() as i64 > *n {
                    return err(init.pos, format!("{} initialisers for an array of {n}", items.len()));
                }
                let stride = self.size(e);
                for i in 0..*n {
                    let p = place.plus(i * stride);
                    match items.get(i as usize) {
                        Some(it) => self.init_object(p, e, it)?,
                        None => self.zero_object(p, e),
                    }
                }
                Ok(())
            }
            (Ty::Struct(id), ExprKind::List(items)) => {
                let fields = self.structs[*id].fields.clone();
                if items.len() > fields.len() {
                    return err(init.pos, format!("{} initialisers for a struct with {} fields", items.len(), fields.len()));
                }
                for (i, (_, ft, off)) in fields.iter().enumerate() {
                    let p = place.plus(*off);
                    match items.get(i) {
                        Some(it) => self.init_object(p, ft, it)?,
                        None => self.zero_object(p, ft),
                    }
                }
                Ok(())
            }
            (_, ExprKind::List(_)) => err(init.pos, format!("a list cannot initialise {}", self.tyname(ty))),
            (t, _) if t.is_scalar() => {
                let got = self.expr(init)?;
                self.check_assign(t, &got, init)?;
                self.narrow_to(t, &got);
                let st = if t.is_tryte_sized() { "stt" } else { "stw" };
                self.emit(format!("{st} r1, {}", place.operand()));
                Ok(())
            }
            _ => err(init.pos, format!("{} must be initialised with {{ … }}", self.tyname(ty))),
        }
    }

    fn zero_object(&mut self, place: Place, ty: &Ty) {
        match ty {
            Ty::Array(n, e) => {
                let stride = self.size(e);
                for i in 0..*n {
                    self.zero_object(place.plus(i * stride), e);
                }
            }
            Ty::Struct(id) => {
                for (_, ft, off) in self.structs[*id].fields.clone() {
                    self.zero_object(place.plus(off), &ft);
                }
            }
            t => {
                let st = if t.is_tryte_sized() { "stt" } else { "stw" };
                self.emit(format!("{st} r0, {}", place.operand()));
            }
        }
    }

    // ---- type rules ----------------------------------------------------------------

    fn check_assign(&self, dst: &Ty, src: &Ty, e: &Expr) -> R<()> {
        let src = src.clone().decay();
        let ok = match (dst, &src) {
            (a, b) if a == b => true,
            (Ty::Int, s) if s.is_integer() => true,
            (Ty::Tryte, Ty::Trit) => true,
            (Ty::Tryte | Ty::Trit, Ty::Int | Ty::Tryte) => {
                let lim = if *dst == Ty::Tryte { 9841 } else { 1 };
                self.const_eval(e).is_some_and(|v| v.abs() <= lim)
            }
            (Ty::Ptr(_) | Ty::Fn(..), Ty::Null) => true,
            (Ty::Ptr(a), Ty::Ptr(b)) => **a == Ty::Void || **b == Ty::Void || a == b,
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            let hint = if dst.is_integer() && src.is_integer() { " (use `as` to narrow)" } else { "" };
            err(e.pos, format!("expected {}, found {}{hint}", self.tyname(dst), self.tyname(&src)))
        }
    }

    /// After an implicit widening nothing is needed; constants were range-checked.
    fn narrow_to(&mut self, _dst: &Ty, _src: &Ty) {}

    // ---- expressions -----------------------------------------------------------------

    fn load(&mut self, t: &Ty) {
        if t.is_scalar() {
            let op = if t.is_tryte_sized() { "ldt" } else { "ldw" };
            self.emit(format!("{op} r1, [r1]"));
        }
    }

    fn store(&mut self, t: &Ty, val: &str, addr: &str) {
        let op = if t.is_tryte_sized() { "stt" } else { "stw" };
        self.emit(format!("{op} {val}, {addr}"));
    }

    /// Evaluate `e` into r1 and return its type. Arrays and structs evaluate
    /// to their address.
    fn expr(&mut self, e: &Expr) -> R<Ty> {
        if let Some(v) = self.const_eval(e) {
            if !matches!(e.kind, ExprKind::Name(_)) || self.lookup_local_name(e).is_none() {
                let t = match &e.kind {
                    ExprKind::Char(_) => Ty::Tryte,
                    ExprKind::Name(n) => match self.globals.get(n) {
                        Some(Global::Const { ty, .. }) => ty.clone(),
                        _ => Ty::Int,
                    },
                    ExprKind::Cast(_, t) => self.resolve(t, e.pos)?,
                    ExprKind::Binary(BinOp::Cmp3, ..) => Ty::Trit,
                    _ => Ty::Int,
                };
                if !(-3_812_798_742_493..=3_812_798_742_493).contains(&v) {
                    return err(e.pos, format!("constant {v} does not fit in a word"));
                }
                self.emit(format!("li r1, {v}"));
                return Ok(t);
            }
        }
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Char(_) => unreachable!("constants are folded"),
            ExprKind::Str(s) => {
                let l = self.intern(s);
                self.emit(format!("la r1, {l}"));
                self.emit("sub r1, r1, 1");
                Ok(Ty::Ptr(Box::new(Ty::Tryte)))
            }
            ExprKind::Null => {
                self.emit("li r1, 0");
                Ok(Ty::Null)
            }
            ExprKind::Name(n) => {
                if let Some(l) = self.lookup_local(n) {
                    let (ty, off) = (l.ty.clone(), l.off);
                    if ty.is_scalar() {
                        let op = if ty.is_tryte_sized() { "ldt" } else { "ldw" };
                        self.emit(format!("{op} r1, {}", mem("fp", off)));
                    } else {
                        self.emit(format!("add r1, fp, {off}"));
                    }
                    return Ok(ty);
                }
                match self.globals.get(n).cloned() {
                    Some(Global::Var { ty, label }) | Some(Global::Extern { ty, label }) => {
                        if ty.is_scalar() {
                            let op = if ty.is_tryte_sized() { "ldt" } else { "ldw" };
                            self.emit(format!("{op} r1, [{label}]"));
                        } else {
                            self.emit(format!("la r1, {label}"));
                        }
                        Ok(ty)
                    }
                    Some(Global::Const { ty, val: ConstVal::Str(l) }) => {
                        self.emit(format!("la r1, {l}"));
                        self.emit("sub r1, r1, 1");
                        Ok(ty)
                    }
                    Some(Global::Const { .. }) => unreachable!("integer constants are folded"),
                    Some(Global::Func { ty, .. }) => {
                        self.emit(format!("la r1, {n}"));
                        Ok(ty)
                    }
                    None => err(e.pos, format!("unknown name `{n}`")),
                }
            }
            ExprKind::Unary(op, x) => match op {
                UnOp::Neg => {
                    let t = self.expr(x)?;
                    self.need_integer(&t, x.pos)?;
                    self.emit("neg r1, r1");
                    Ok(Ty::Int)
                }
                UnOp::Not => {
                    let t = self.expr(x)?;
                    self.need_condition(&t, x.pos)?;
                    let l = self.new_label();
                    self.emit("cmp r1, 0");
                    self.emit("li r1, 1");
                    self.emit(format!("beq {l}"));
                    self.emit("li r1, 0");
                    self.label(&l);
                    Ok(Ty::Int)
                }
                UnOp::Deref => {
                    let t = self.addr(e)?;
                    self.load(&t);
                    Ok(t)
                }
                UnOp::Addr => {
                    if let ExprKind::Name(n) = &x.kind {
                        if self.lookup_local(n).is_none() {
                            if let Some(Global::Func { ty, .. }) = self.globals.get(n).cloned() {
                                self.emit(format!("la r1, {n}"));
                                return Ok(ty);
                            }
                        }
                    }
                    let t = self.addr(x)?;
                    Ok(Ty::Ptr(Box::new(t)))
                }
            },
            ExprKind::Binary(op, a, b) => self.binary(*op, a, b, e.pos),
            ExprKind::Assign(op, lhs, rhs) => self.assign(*op, lhs, rhs),
            ExprKind::Call(f, args) => self.call(f, args, e.pos),
            ExprKind::Index(..) | ExprKind::Field(..) => {
                let t = self.addr(e)?;
                self.load(&t);
                Ok(t)
            }
            ExprKind::Cast(x, t) => {
                let to = self.resolve(t, e.pos)?;
                let from = self.expr(x)?.decay();
                let ok = (to.is_integer() && (from.is_integer() || from.is_pointer() || matches!(from, Ty::Fn(..))))
                    || (matches!(to, Ty::Ptr(_) | Ty::Fn(..)) && (from.is_integer() || from.is_pointer() || matches!(from, Ty::Fn(..))));
                if !ok {
                    return err(e.pos, format!("cannot convert {} to {}", self.tyname(&from), self.tyname(&to)));
                }
                match (&to, &from) {
                    (Ty::Tryte, Ty::Tryte | Ty::Trit) | (Ty::Trit, Ty::Trit) => {}
                    (Ty::Tryte, _) => self.emit("ext r1, r1, 19, 9"),
                    (Ty::Trit, _) => self.emit("ext r1, r1, 27, 1"),
                    _ => {}
                }
                Ok(to)
            }
            ExprKind::Sizeof(t) => {
                let ty = self.resolve(t, e.pos)?;
                let n = self.size(&ty);
                self.emit(format!("li r1, {n}"));
                Ok(Ty::Int)
            }
            ExprKind::Cond(c, a, b) => {
                let (lelse, lend) = (self.new_label(), self.new_label());
                self.cond_jump_false(c, &lelse)?;
                let ta = self.expr(a)?.decay();
                self.emit(format!("b {lend}"));
                self.label(&lelse);
                let tb = self.expr(b)?.decay();
                self.label(&lend);
                if ta == tb {
                    Ok(ta)
                } else if ta.is_integer() && tb.is_integer() {
                    Ok(Ty::Int)
                } else if ta.is_pointer() && tb == Ty::Null {
                    Ok(ta)
                } else if tb.is_pointer() && ta == Ty::Null {
                    Ok(tb)
                } else {
                    err(e.pos, format!("the branches have different types: {} and {}", self.tyname(&ta), self.tyname(&tb)))
                }
            }
            ExprKind::List(_) => err(e.pos, "a `{ … }` list is only allowed as an initialiser"),
        }
    }

    fn lookup_local_name(&self, e: &Expr) -> Option<&Local> {
        match &e.kind {
            ExprKind::Name(n) => self.lookup_local(n),
            _ => None,
        }
    }

    /// Evaluate the address of an lvalue into r1; returns the object's type.
    fn addr(&mut self, e: &Expr) -> R<Ty> {
        match &e.kind {
            ExprKind::Name(n) => {
                if let Some(l) = self.lookup_local(n) {
                    let (ty, off) = (l.ty.clone(), l.off);
                    self.emit(format!("add r1, fp, {off}"));
                    return Ok(ty);
                }
                match self.globals.get(n).cloned() {
                    Some(Global::Var { ty, label }) | Some(Global::Extern { ty, label }) => {
                        self.emit(format!("la r1, {label}"));
                        Ok(ty)
                    }
                    Some(_) => err(e.pos, format!("`{n}` is not a variable")),
                    None => err(e.pos, format!("unknown name `{n}`")),
                }
            }
            ExprKind::Unary(UnOp::Deref, p) => {
                let t = self.expr(p)?.decay();
                match t {
                    Ty::Ptr(inner) if *inner != Ty::Void => Ok(*inner),
                    Ty::Ptr(_) => err(e.pos, "cannot dereference a *void"),
                    t => err(e.pos, format!("cannot dereference {}", self.tyname(&t))),
                }
            }
            ExprKind::Index(a, i) => {
                let elem = match self.expr(a)? {
                    Ty::Array(_, t) | Ty::Ptr(t) => *t,
                    t => return err(a.pos, format!("cannot index {}", self.tyname(&t))),
                };
                if elem == Ty::Void {
                    return err(a.pos, "cannot index a *void");
                }
                let size = self.size(&elem);
                if let Some(k) = self.const_eval(i) {
                    if k != 0 {
                        self.emit(format!("add r1, r1, {}", k * size));
                    }
                } else {
                    self.emit("push r1");
                    let ti = self.expr(i)?;
                    self.need_integer(&ti, i.pos)?;
                    if size != 1 {
                        self.emit(format!("mul r1, r1, {size}"));
                    }
                    self.emit("mov r2, r1");
                    self.emit("pop r1");
                    self.emit("add r1, r1, r2");
                }
                Ok(elem)
            }
            ExprKind::Field(s, f) => {
                let id = match self.expr(s)? {
                    Ty::Struct(id) => id,
                    Ty::Ptr(t) => match *t {
                        Ty::Struct(id) => id,
                        t => return err(s.pos, format!("{} has no fields", self.tyname(&Ty::Ptr(Box::new(t))))),
                    },
                    t => return err(s.pos, format!("{} has no fields", self.tyname(&t))),
                };
                let Some((_, ft, off)) = self.structs[id].fields.iter().find(|(n, _, _)| n == f).cloned() else {
                    return err(e.pos, format!("struct {} has no field `{f}`", self.structs[id].name));
                };
                if off != 0 {
                    self.emit(format!("add r1, r1, {off}"));
                }
                Ok(ft)
            }
            _ => err(e.pos, "this expression has no address (it is not a variable, element or field)"),
        }
    }

    /// Apply `r1 := r1 op r2` (or `r1 op imm`). `lt`/`rt` are operand types.
    fn arith(&mut self, op: BinOp, lt: &Ty, rt: &Ty, imm: Option<i64>, pos: Pos) -> R<Ty> {
        let operand = |imm: Option<i64>| imm.map_or("r2".to_string(), |v| v.to_string());
        let ptr_elem = |t: &Ty| match t {
            Ty::Ptr(e) => Some((**e).clone()),
            _ => None,
        };
        match op {
            BinOp::Add | BinOp::Sub => {
                if let (Some(e), true) = (ptr_elem(lt), rt.is_integer()) {
                    let size = self.size(&e).max(1);
                    let n = match imm {
                        Some(v) => (v * size).to_string(),
                        None => {
                            if size != 1 {
                                self.emit(format!("mul r2, r2, {size}"));
                            }
                            "r2".into()
                        }
                    };
                    let m = if op == BinOp::Add { "add" } else { "sub" };
                    self.emit(format!("{m} r1, r1, {n}"));
                    return Ok(lt.clone());
                }
                if let (true, Some(e), BinOp::Add) = (lt.is_integer(), ptr_elem(rt), op) {
                    let size = self.size(&e).max(1);
                    if size != 1 {
                        self.emit(format!("mul r1, r1, {size}"));
                    }
                    self.emit(format!("add r1, r1, {}", operand(imm)));
                    return Ok(rt.clone());
                }
                if let (Some(a), Some(b), BinOp::Sub) = (ptr_elem(lt), ptr_elem(rt), op) {
                    if a != b {
                        return err(pos, format!("cannot subtract {} from {}", self.tyname(rt), self.tyname(lt)));
                    }
                    self.emit(format!("sub r1, r1, {}", operand(imm)));
                    let size = self.size(&a).max(1);
                    if size != 1 {
                        self.emit(format!("div r1, r1, {size}"));
                    }
                    return Ok(Ty::Int);
                }
            }
            _ => {}
        }
        if !lt.is_integer() || !rt.is_integer() {
            return err(pos, format!("cannot apply {op:?} to {} and {}", self.tyname(lt), self.tyname(rt)));
        }
        let m = match op {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Mod => "mod",
            BinOp::Min => "min",
            BinOp::Max => "max",
            BinOp::Shl => "sh",
            BinOp::Shr => {
                match imm {
                    Some(v) => self.emit(format!("sh r1, r1, {}", -v)),
                    None => {
                        self.emit("neg r2, r2");
                        self.emit("sh r1, r1, r2");
                    }
                }
                return Ok(Ty::Int);
            }
            _ => unreachable!(),
        };
        if matches!(op, BinOp::Div | BinOp::Mod) && imm == Some(0) {
            return err(pos, "division by zero");
        }
        self.emit(format!("{m} r1, r1, {}", operand(imm)));
        Ok(Ty::Int)
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr, pos: Pos) -> R<Ty> {
        match op {
            BinOp::And | BinOp::Or => {
                let (short, end) = (self.new_label(), self.new_label());
                let (val_short, jump) = if op == BinOp::And { (0, "beq") } else { (1, "bne") };
                for x in [a, b] {
                    let t = self.expr(x)?;
                    self.need_condition(&t, x.pos)?;
                    self.emit("cmp r1, 0");
                    self.emit(format!("{jump} {short}"));
                }
                self.emit(format!("li r1, {}", 1 - val_short));
                self.emit(format!("b {end}"));
                self.label(&short);
                self.emit(format!("li r1, {val_short}"));
                self.label(&end);
                Ok(Ty::Int)
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                self.compare(a, b)?;
                let br = match op {
                    BinOp::Lt => "blt",
                    BinOp::Le => "ble",
                    BinOp::Gt => "bgt",
                    BinOp::Ge => "bge",
                    BinOp::Eq => "beq",
                    _ => "bne",
                };
                let l = self.new_label();
                self.emit("li r1, 1");
                self.emit(format!("{br} {l}"));
                self.emit("li r1, 0");
                self.label(&l);
                Ok(Ty::Int)
            }
            BinOp::Cmp3 => {
                self.compare(a, b)?;
                let l = self.new_label();
                self.emit("li r1, 0");
                self.emit(format!("beq {l}"));
                self.emit("li r1, 1");
                self.emit(format!("bgt {l}"));
                self.emit("li r1, -1");
                self.label(&l);
                Ok(Ty::Trit)
            }
            _ => {
                let lt = self.expr(a)?.decay();
                if let Some(v) = self.const_eval(b) {
                    return self.arith(op, &lt, &Ty::Int, Some(v), pos);
                }
                self.emit("push r1");
                let rt = self.expr(b)?.decay();
                self.emit("mov r2, r1");
                self.emit("pop r1");
                self.arith(op, &lt, &rt, None, pos)
            }
        }
    }

    fn assign(&mut self, op: Option<BinOp>, lhs: &Expr, rhs: &Expr) -> R<Ty> {
        match op {
            None => {
                // Fast path: a named scalar variable.
                if let ExprKind::Name(n) = &lhs.kind {
                    let target = match self.lookup_local(n) {
                        Some(l) => Some((l.ty.clone(), mem("fp", l.off))),
                        None => match self.globals.get(n) {
                            Some(Global::Var { ty, label }) | Some(Global::Extern { ty, label }) => {
                                Some((ty.clone(), format!("[{label}]")))
                            }
                            _ => None,
                        },
                    };
                    if let Some((ty, operand)) = target {
                        if !ty.is_scalar() {
                            return err(lhs.pos, format!("cannot assign a whole {}; copy it field by field", self.tyname(&ty)));
                        }
                        let rt = self.expr(rhs)?;
                        self.check_assign(&ty, &rt, rhs)?;
                        self.store(&ty, "r1", &operand);
                        return Ok(ty);
                    }
                }
                let rt = self.expr(rhs)?;
                self.emit("push r1");
                let lt = self.addr(lhs)?;
                if !lt.is_scalar() {
                    return err(lhs.pos, format!("cannot assign a whole {}; copy it field by field", self.tyname(&lt)));
                }
                self.check_assign(&lt, &rt, rhs)?;
                self.emit("pop r2");
                self.store(&lt, "r2", "[r1]");
                self.emit("mov r1, r2");
                Ok(lt)
            }
            Some(op) => {
                let lt = self.addr(lhs)?;
                if !lt.is_scalar() {
                    return err(lhs.pos, format!("cannot apply an operator to a whole {}", self.tyname(&lt)));
                }
                self.emit("push r1");
                let rt = self.expr(rhs)?.decay();
                self.emit("mov r2, r1");
                self.emit("pop r3");
                self.emit("push r3");
                self.load_from("r3", &lt);
                let res = self.arith(op, &lt, &rt, None, rhs.pos)?;
                if res.is_integer() && lt.is_tryte_sized() {
                    // Compound assignment narrows implicitly (x += 1 on a tryte).
                } else {
                    self.check_assign(&lt, &res, rhs)?;
                }
                self.emit("pop r3");
                self.store(&lt, "r1", "[r3]");
                Ok(lt)
            }
        }
    }

    fn load_from(&mut self, reg: &str, t: &Ty) {
        let op = if t.is_tryte_sized() { "ldt" } else { "ldw" };
        self.emit(format!("{op} r1, [{reg}]"));
    }

    fn call(&mut self, f: &Expr, args: &[Expr], pos: Pos) -> R<Ty> {
        if let ExprKind::Name(n) = &f.kind {
            if BUILTINS.contains(&n.as_str()) && self.lookup_local(n).is_none() && !self.globals.contains_key(n) {
                return self.builtin(n, args, pos);
            }
        }
        // Direct call to a known function, or through a function pointer.
        let direct = match &f.kind {
            ExprKind::Name(n) if self.lookup_local(n).is_none() => match self.globals.get(n) {
                Some(Global::Func { ty, .. }) => Some((n.clone(), ty.clone())),
                _ => None,
            },
            _ => None,
        };
        let fty = match &direct {
            Some((_, t)) => t.clone(),
            None => {
                let t = self.expr(f)?;
                self.emit("push r1");
                match t {
                    Ty::Ptr(inner) if matches!(*inner, Ty::Fn(..)) => *inner,
                    t @ Ty::Fn(..) => t,
                    t => return err(f.pos, format!("cannot call {}", self.tyname(&t))),
                }
            }
        };
        let Ty::Fn(params, ret) = fty else { unreachable!() };
        if params.len() != args.len() {
            return err(pos, format!("expected {} argument(s), found {}", params.len(), args.len()));
        }
        for (a, p) in args.iter().zip(&params) {
            let t = self.expr(a)?;
            self.check_assign(p, &t, a)?;
            self.emit("push r1");
        }
        let n = args.len() as i64;
        for i in 0..n.min(6) {
            self.emit(format!("ldw r{}, {}", i + 1, mem("sp", 3 * (n - 1 - i))));
        }
        // Stack arguments: reverse so arg 7 is at the lowest address.
        let k = (n - 6).max(0);
        for i in 0..k / 2 {
            let (a, b) = (3 * i, 3 * (k - 1 - i));
            self.emit(format!("ldw r7, {}", mem("sp", a)));
            self.emit(format!("ldw r8, {}", mem("sp", b)));
            self.emit(format!("stw r8, {}", mem("sp", a)));
            self.emit(format!("stw r7, {}", mem("sp", b)));
        }
        let mut cleanup = n;
        match &direct {
            Some((name, _)) => self.emit(format!("call {name}")),
            None => {
                self.emit(format!("ldw r7, {}", mem("sp", 3 * n)));
                self.emit("jalr lr, r7");
                cleanup += 1;
            }
        }
        if cleanup > 0 {
            self.emit(format!("add sp, sp, {}", 3 * cleanup));
        }
        Ok(*ret)
    }

    /// Evaluate two operands into r1 (first) and r2 (second).
    fn two(&mut self, a: &Expr, b: &Expr) -> R<(Ty, Ty)> {
        let ta = self.expr(a)?.decay();
        self.emit("push r1");
        let tb = self.expr(b)?.decay();
        self.emit("mov r2, r1");
        self.emit("pop r1");
        Ok((ta, tb))
    }

    fn builtin(&mut self, n: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let want = |k: usize| -> R<()> {
            if args.len() == k {
                Ok(())
            } else {
                err(pos, format!("`{n}` takes {k} argument(s), found {}", args.len()))
            }
        };
        match n {
            "sgn" | "abs" | "tlz" => {
                want(1)?;
                let t = self.expr(&args[0])?;
                self.need_integer(&t, args[0].pos)?;
                self.emit(format!("{n} r1, r1"));
                Ok(if n == "sgn" { Ty::Trit } else { Ty::Int })
            }
            "divr" | "modr" | "tmul" | "tsat" | "cons" | "mulh" => {
                want(2)?;
                let (ta, tb) = self.two(&args[0], &args[1])?;
                self.need_integer(&ta, args[0].pos)?;
                self.need_integer(&tb, args[1].pos)?;
                self.emit(format!("{n} r1, r1, r2"));
                Ok(Ty::Int)
            }
            "trit" => {
                want(2)?;
                let i = self.const_int(&args[1])?;
                if !(1..=27).contains(&i) {
                    return err(args[1].pos, "trit position must be 1..27");
                }
                let t = self.expr(&args[0])?;
                self.need_integer(&t, args[0].pos)?;
                self.emit(format!("trit r1, r1, {i}"));
                Ok(Ty::Trit)
            }
            "ext" => {
                want(3)?;
                let (p, l) = (self.const_int(&args[1])?, self.const_int(&args[2])?);
                let t = self.expr(&args[0])?;
                self.need_integer(&t, args[0].pos)?;
                self.emit(format!("ext r1, r1, {p}, {l}"));
                Ok(Ty::Int)
            }
            "ins" => {
                want(4)?;
                let (p, l) = (self.const_int(&args[2])?, self.const_int(&args[3])?);
                let (ta, tb) = self.two(&args[0], &args[1])?;
                self.need_integer(&ta, args[0].pos)?;
                self.need_integer(&tb, args[1].pos)?;
                self.emit(format!("ins r1, r2, {p}, {l}"));
                Ok(Ty::Int)
            }
            "bytes" => {
                want(1)?;
                let t = self.expr(&args[0])?.decay();
                if !t.is_pointer() {
                    return err(args[0].pos, format!("`bytes` takes a pointer, found {}", self.tyname(&t)));
                }
                self.emit("sub r1, r1, 1");
                Ok(Ty::Ptr(Box::new(Ty::Tryte)))
            }
            "in" => {
                want(1)?;
                match self.const_eval(&args[0]) {
                    Some(p) => self.emit(format!("in r1, {p}")),
                    None => {
                        let t = self.expr(&args[0])?;
                        self.need_integer(&t, args[0].pos)?;
                        self.emit("in r1, [r1]");
                    }
                }
                Ok(Ty::Int)
            }
            "out" => {
                want(2)?;
                match self.const_eval(&args[0]) {
                    Some(p) => {
                        self.expr(&args[1])?;
                        self.emit(format!("out r1, {p}"));
                    }
                    None => {
                        self.two(&args[0], &args[1])?;
                        self.emit("out r2, [r1]");
                    }
                }
                Ok(Ty::Void)
            }
            "mfc" | "mtc" => {
                want(if n == "mfc" { 1 } else { 2 })?;
                let ExprKind::Name(reg) = &args[0].kind else {
                    return err(args[0].pos, format!("`{n}` needs a control register name: {}", CREGS.join(", ")));
                };
                let up = reg.to_ascii_uppercase();
                if !CREGS.contains(&up.as_str()) {
                    return err(args[0].pos, format!("unknown control register `{reg}`; one of {}", CREGS.join(", ")));
                }
                if n == "mfc" {
                    self.emit(format!("mfc r1, {}", up.to_ascii_lowercase()));
                    Ok(Ty::Int)
                } else {
                    let t = self.expr(&args[1])?.decay();
                    if !t.is_scalar() {
                        return err(args[1].pos, "`mtc` needs a word value");
                    }
                    self.emit(format!("mtc {}, r1", up.to_ascii_lowercase()));
                    Ok(Ty::Void)
                }
            }
            "hlt" | "brk" | "tlbflush" | "fence" => {
                want(0)?;
                self.emit(n);
                Ok(Ty::Void)
            }
            "tlbinv" => {
                want(1)?;
                self.expr(&args[0])?;
                self.emit("tlbinv r1");
                Ok(Ty::Void)
            }
            "syscall" => {
                if args.is_empty() || args.len() > 7 {
                    return err(pos, "`syscall` takes a constant number and up to 6 arguments");
                }
                let num = self.const_int(&args[0])?;
                let rest = &args[1..];
                for a in rest {
                    self.expr(a)?;
                    self.emit("push r1");
                }
                let k = rest.len() as i64;
                for i in 0..k {
                    self.emit(format!("ldw r{}, {}", i + 1, mem("sp", 3 * (k - 1 - i))));
                }
                if k > 0 {
                    self.emit(format!("add sp, sp, {}", 3 * k));
                }
                self.emit(format!("sys {num}"));
                Ok(Ty::Int)
            }
            _ => unreachable!(),
        }
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

fn escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            '\0' => o.push_str("\\0"),
            c => o.push(c),
        }
    }
    o
}
