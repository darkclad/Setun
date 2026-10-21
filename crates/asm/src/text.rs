//! Setun-11̅ assembly language.
//!
//! ```text
//! ; comment                        NAME = expr   /   .equ NAME, expr
//!         .org  -59049             start a segment at a word address
//! start:  li    r1, 42             LIQ if it fits ±364, else LI
//!         add   r1, r2, r3         register form
//!         add   r1, r2, 5          ALUQ if the constant fits ±13, else ALUI
//!         add.l r1, r2, 5          `.l` forces the long (2-word) form
//!         ldw   r1, [sp+6]         offsets in trytes; 1-word form when possible
//!         in    r2, [r3+2]         port = register + small offset
//!         in    r2, 486            port as a constant
//!         blt   loop               short (BRQ) when in range, else long (BR)
//!         br3   less, more         3-way branch on S
//!         call  func / ret / push r1 / pop r1 / mov r1, r2 / la r1, label
//!         .word 1, label, -3       .tryte 1, 'A', 0t1T0     .string "Привет\n"
//!         .zero 10                 10 zero words
//! ```
//!
//! Literals: decimal; `0t1T0` (balanced ternary: `1`, `0`, and `T` or an
//! overlined `1` for −1); `0zDD5̅` (balanced base 27: `0`–`9`, `A`–`D` = 0..13,
//! a negative digit overlined with U+0305); `0x1F` (hexadecimal, for values
//! from binary systems); `'c'` (a Unicode character). `$` is the current address.
//! Registers: `r-13`..`r13`, `zero` = r0, `fp` = r11, `lr` = r12, `sp` = r13.

use setun_core::isa::{self, alu, cond, cr, falu, fcvt, fld, fmem, mem, memr, op, sys, valu, vmem, vmov};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use ternary::Word;

use crate::pack_trytes;

/// A contiguous run of words starting at a word address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub origin: i64,
    pub words: Vec<Word>,
}

/// One assembled source line, for listings.
#[derive(Debug, Clone)]
pub struct ListingLine {
    pub line: usize,
    pub addr: i64,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub segments: Vec<Segment>,
    pub symbols: BTreeMap<String, i64>,
    pub listing: Vec<ListingLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for Error {}

fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, Error> {
    Err(Error { line, msg: msg.into() })
}

// ---- expressions -------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Num(i64),
    Sym(String),
    Here,
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
}

impl Expr {
    fn eval(&self, syms: &HashMap<String, i64>, here: i64) -> Result<i64, String> {
        Ok(match self {
            Expr::Num(v) => *v,
            Expr::Here => here,
            Expr::Sym(s) => *syms.get(s).ok_or_else(|| format!("undefined symbol `{s}`"))?,
            Expr::Neg(e) => -e.eval(syms, here)?,
            Expr::Bin(o, a, b) => {
                let (x, y) = (a.eval(syms, here)?, b.eval(syms, here)?);
                match o {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    _ => {
                        if y == 0 {
                            return Err("division by zero".into());
                        }
                        x / y
                    }
                }
            }
        })
    }

    /// Value if it depends only on constants known before layout.
    fn constant(&self, consts: &HashMap<String, i64>) -> Option<i64> {
        match self {
            Expr::Here => None,
            _ => self.eval(consts, 0).ok(),
        }
    }
}

struct Lexer<'a> {
    s: &'a [char],
    i: usize,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn expr(&mut self) -> Result<Expr, String> {
        let mut e = self.term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(c @ ('+' | '-')) => {
                    self.i += 1;
                    e = Expr::Bin(c, Box::new(e), Box::new(self.term()?));
                }
                _ => return Ok(e),
            }
        }
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut e = self.factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(c @ ('*' | '/')) => {
                    self.i += 1;
                    e = Expr::Bin(c, Box::new(e), Box::new(self.factor()?));
                }
                _ => return Ok(e),
            }
        }
    }
    fn factor(&mut self) -> Result<Expr, String> {
        self.skip_ws();
        let c = self.peek().ok_or("expression expected")?;
        if c == '-' {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.factor()?)));
        }
        if c == '(' {
            self.i += 1;
            let e = self.expr()?;
            self.skip_ws();
            if self.peek() != Some(')') {
                return Err("`)` expected".into());
            }
            self.i += 1;
            return Ok(e);
        }
        if c == '$' {
            self.i += 1;
            return Ok(Expr::Here);
        }
        if c == '\'' {
            self.i += 1;
            let ch = self.char_in_quotes('\'')?;
            if self.peek() != Some('\'') {
                return Err("`'` expected".into());
            }
            self.i += 1;
            return Ok(Expr::Num(ch as i64));
        }
        if c.is_ascii_digit() {
            let start = self.i;
            if c == '0' && matches!(self.s.get(self.i + 1), Some('t' | 'z' | 'x')) {
                // the whole run of letters, digits, `_` and overlines is the literal
                let base = self.s[self.i + 1];
                self.i += 2;
                let ds = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == ternary::OVERLINE) {
                    self.i += 1;
                }
                let digits: String = self.s[ds..self.i].iter().collect();
                if base == 't' && matches!(self.peek(), Some('+' | '-')) {
                    // `0t0+0` was once the digits 0, +, 0 (= 3); refuse it rather than read 0 + 0
                    return Err(if digits.is_empty() {
                        "0t digits are 1, 0 and T (the old + and - are gone)".into()
                    } else {
                        "a 0t literal followed by + or -: put a space before the operator (the old + and - digits are gone)".into()
                    });
                }
                if digits.is_empty() {
                    return Err(format!("empty 0{base} literal"));
                }
                return ternary::parse_prefixed(base, &digits).map(Expr::Num).map_err(|e| format!("bad 0{base} literal: {e}"));
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.i += 1;
            }
            let t: String = self.s[start..self.i].iter().filter(|c| **c != '_').collect();
            return t.parse().map(Expr::Num).map_err(|_| format!("bad number `{t}`"));
        }
        if is_ident_start(c) {
            let start = self.i;
            while self.peek().is_some_and(is_ident_char) {
                self.i += 1;
            }
            return Ok(Expr::Sym(self.s[start..self.i].iter().collect()));
        }
        Err(format!("unexpected `{c}`"))
    }
    /// One (possibly escaped) character inside quotes.
    fn char_in_quotes(&mut self, q: char) -> Result<char, String> {
        let c = self.peek().ok_or("unterminated literal")?;
        self.i += 1;
        if c == q {
            return Err("empty character literal".into());
        }
        if c != '\\' {
            return Ok(c);
        }
        let e = self.peek().ok_or("unterminated escape")?;
        self.i += 1;
        Ok(match e {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            _ => return Err(format!("unknown escape `\\{e}`")),
        })
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '.'
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

fn parse_expr(s: &str) -> Result<Expr, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut l = Lexer { s: &chars, i: 0 };
    let e = l.expr()?;
    l.skip_ws();
    match l.peek() {
        None => Ok(e),
        Some(c) => Err(format!("unexpected `{c}` in expression")),
    }
}

fn parse_string(s: &str) -> Result<String, String> {
    let chars: Vec<char> = s.trim().chars().collect();
    if chars.first() != Some(&'"') {
        return Err("string literal expected".into());
    }
    let mut l = Lexer { s: &chars, i: 1 };
    let mut out = String::new();
    loop {
        match l.peek() {
            None => return Err("unterminated string".into()),
            Some('"') => {
                l.i += 1;
                break;
            }
            Some(_) => out.push(l.char_in_quotes('"').or_else(|e| if e == "empty character literal" { Ok('"') } else { Err(e) })?),
        }
    }
    if l.i != chars.len() {
        return Err("text after string".into());
    }
    Ok(out)
}

// ---- operands ----------------------------------------------------------------

#[derive(Debug, Clone)]
enum Operand {
    Reg(i64),
    /// An FPU register, f−13..f13.
    FReg(i64),
    /// A vector register, v−13..v13.
    VReg(i64),
    Mem(i64, Expr),
    Expr(Expr),
    /// A real literal (decimal with a point or an exponent).
    Real(setun_real::Real),
}

/// An FPU register name: `f-13` … `f13`.
pub fn fregister(s: &str) -> Option<i64> {
    let n: i64 = s.trim().strip_prefix('f')?.parse().ok()?;
    (-13..=13).contains(&n).then_some(n)
}

/// A vector register name: `v-13` … `v13`.
pub fn vregister(s: &str) -> Option<i64> {
    let n: i64 = s.trim().strip_prefix('v')?.parse().ok()?;
    (-13..=13).contains(&n).then_some(n)
}

pub fn register(s: &str) -> Option<i64> {
    let s = s.trim();
    match s {
        "zero" => return Some(0),
        "fp" => return Some(11),
        "lr" => return Some(12),
        "sp" => return Some(13),
        _ => {}
    }
    let n: i64 = s.strip_prefix('r')?.parse().ok()?;
    (-13..=13).contains(&n).then_some(n)
}

fn parse_operand(s: &str) -> Result<Operand, String> {
    let s = s.trim();
    if s.starts_with('"') {
        return Err("a string is only allowed after .string".into());
    }
    if let Some(inner) = s.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or("`]` expected")?.trim();
        // [reg], [reg + expr], [reg - expr], or [expr] (base r0). Registers
        // may contain a minus (`r-5`), so try each +/- as the split point.
        if let Some(r) = register(inner) {
            return Ok(Operand::Mem(r, Expr::Num(0)));
        }
        for (i, c) in inner.char_indices().skip(1) {
            if c == '+' || c == '-' {
                if let Some(r) = register(&inner[..i]) {
                    return Ok(Operand::Mem(r, parse_expr(&format!("0{}", &inner[i..]))?));
                }
            }
        }
        return Ok(Operand::Mem(0, parse_expr(inner)?));
    }
    if let Some(r) = register(s) {
        return Ok(Operand::Reg(r));
    }
    if let Some(f) = fregister(s) {
        return Ok(Operand::FReg(f));
    }
    if let Some(v) = vregister(s) {
        return Ok(Operand::VReg(v));
    }
    match parse_expr(s) {
        Ok(e) => Ok(Operand::Expr(e)),
        Err(msg) => real_literal(s).map(Operand::Real).ok_or(msg),
    }
}

/// A decimal real literal (it has a point or an exponent), rounded to real81.
fn real_literal(s: &str) -> Option<setun_real::Real> {
    let t = s.trim();
    if !t.contains(['.', 'e', 'E']) || t.starts_with("0x") {
        return None;
    }
    setun_real::Real::from_decimal(t).ok().map(|(r, _)| r)
}

/// Split operands on top-level commas (not inside quotes or brackets).
fn split_operands(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut depth, mut quote) = (String::new(), 0, None::<char>);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                continue;
            }
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[' | '(') => depth += 1,
            (None, ']' | ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|o| o.trim().to_string()).collect()
}

/// Remove a `;` comment, respecting quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut esc = false;
    for (i, c) in line.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match (quote, c) {
            (Some(_), '\\') => esc = true,
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, ';') => return &line[..i],
            _ => {}
        }
    }
    line
}

// ---- statements --------------------------------------------------------------

#[derive(Debug, Clone)]
enum Kind {
    Label(String),
    Org(Expr),
    Equ(String, Expr),
    Words(Vec<Expr>),
    /// `.real27` (one word each) or `.real81` (three words each)
    Reals(Vec<setun_real::Real>, bool),
    Trytes(Vec<Expr>),
    /// Code points (`ternary::code_points`), without the NUL.
    Str(Vec<i64>),
    Zero(Expr),
    Fill(Expr, Expr),
    Align(Expr),
    Inst { m: String, long: bool, ops: Vec<Operand> },
}

#[derive(Debug, Clone)]
struct Stmt {
    line: usize,
    kind: Kind,
}

fn parse(src: &str) -> Result<Vec<Stmt>, Error> {
    let mut out = Vec::new();
    for (n, raw) in src.lines().enumerate() {
        let line = n + 1;
        let mut s = strip_comment(raw).trim();
        let e = |msg: String| Error { line, msg };
        // Leading labels.
        while let Some(colon) = s.find(':') {
            let name = s[..colon].trim();
            if name.is_empty() || !name.chars().next().is_some_and(is_ident_start) || !name.chars().all(is_ident_char) {
                break;
            }
            out.push(Stmt { line, kind: Kind::Label(name.to_string()) });
            s = s[colon + 1..].trim();
        }
        if s.is_empty() {
            continue;
        }
        // NAME = expr
        if let Some(eq) = s.find('=') {
            let name = s[..eq].trim();
            if !name.is_empty() && name.chars().next().is_some_and(is_ident_start) && name.chars().all(is_ident_char) {
                let ex = parse_expr(&s[eq + 1..]).map_err(e)?;
                out.push(Stmt { line, kind: Kind::Equ(name.to_string(), ex) });
                continue;
            }
        }
        let (m, rest) = match s.find(char::is_whitespace) {
            Some(i) => (&s[..i], s[i..].trim()),
            None => (s, ""),
        };
        let m = m.to_ascii_lowercase();
        let ops = split_operands(rest);
        let exprs = |ops: &[String]| -> Result<Vec<Expr>, Error> {
            ops.iter().map(|o| parse_expr(o).map_err(|msg| Error { line, msg })).collect()
        };
        let kind = match m.as_str() {
            ".org" => Kind::Org(one(&exprs(&ops)?, line)?),
            ".equ" => {
                if ops.len() != 2 {
                    return err(line, ".equ NAME, value");
                }
                Kind::Equ(ops[0].clone(), parse_expr(&ops[1]).map_err(e)?)
            }
            ".word" => Kind::Words(exprs(&ops)?),
            ".real27" | ".real81" => {
                let mut v = Vec::new();
                for o in &ops {
                    let r = match parse_expr(o) {
                        Ok(Expr::Num(n)) => setun_real::Real::from_i64(n),
                        Ok(Expr::Neg(inner)) if matches!(*inner, Expr::Num(_)) => {
                            let Expr::Num(n) = *inner else { unreachable!() };
                            setun_real::Real::from_i64(-n)
                        }
                        _ => real_literal(o).ok_or_else(|| Error { line, msg: format!("real literal expected, found `{o}`") })?,
                    };
                    v.push(r);
                }
                Kind::Reals(v, m == ".real81")
            }
            ".tryte" => Kind::Trytes(exprs(&ops)?),
            ".string" => Kind::Str(parse_string(rest).and_then(|t| ternary::code_points(&t)).map_err(e)?),
            ".zero" => Kind::Zero(one(&exprs(&ops)?, line)?),
            ".align" => Kind::Align(one(&exprs(&ops)?, line)?),
            ".fill" => match exprs(&ops)?.as_slice() {
                [n, v] => Kind::Fill(n.clone(), v.clone()),
                _ => return err(line, ".fill COUNT, VALUE"),
            },
            _ if m.starts_with('.') && !m.ends_with(".l") => return err(line, format!("unknown directive `{m}`")),
            _ => {
                let (base, long) = match m.strip_suffix(".l") {
                    Some(b) => (b.to_string(), true),
                    None => (m.clone(), false),
                };
                let ops = ops.iter().map(|o| parse_operand(o).map_err(|msg| Error { line, msg })).collect::<Result<_, _>>()?;
                Kind::Inst { m: base, long, ops }
            }
        };
        // Consecutive `.tryte` lines pack as one run (no padding between).
        if let (Kind::Trytes(more), Some(Stmt { kind: Kind::Trytes(prev), .. })) = (&kind, out.last_mut()) {
            prev.extend(more.iter().cloned());
            continue;
        }
        out.push(Stmt { line, kind });
    }
    Ok(out)
}

fn one(v: &[Expr], line: usize) -> Result<Expr, Error> {
    match v {
        [e] => Ok(e.clone()),
        _ => err(line, "exactly one value expected"),
    }
}

// ---- instruction tables ------------------------------------------------------

fn alu_binary(m: &str) -> Option<i64> {
    Some(match m {
        "add" => alu::ADD,
        "sub" => alu::SUB,
        "adc" => alu::ADC,
        "sbc" => alu::SBC,
        "mul" => alu::MUL,
        "mulh" => alu::MULH,
        "div" => alu::DIV,
        "mod" => alu::MOD,
        "divr" => alu::DIVR,
        "modr" => alu::MODR,
        "min" => alu::MIN,
        "max" => alu::MAX,
        "tmul" => alu::TMUL,
        "tsat" => alu::TSAT,
        "cons" => alu::CONS,
        "sh" => alu::SH,
        "trit" => alu::TRIT,
        "band" => alu::BAND,
        "bor" => alu::BOR,
        "bxor" => alu::BXOR,
        "bandn" => alu::BANDN,
        "bsh" => alu::BSH,
        "mrg" => alu::MRG,
        "tcp" => alu::TCP,
        "tcn" => alu::TCN,
        "trev" => alu::TREV,
        _ => return None,
    })
}

/// A lane-wise vector operation: `vadd` (9 tryte lanes), `vaddw` (3 word
/// lanes), …; the VALU function and whether it takes two sources.
pub fn valu_lanes(m: &str) -> Option<(i64, bool)> {
    let s = m.strip_prefix('v')?;
    let (s, words) = match s.strip_suffix('w') {
        Some(t) => (t, true),
        None => (s, false),
    };
    let (f, binary) = match s {
        "add" => (alu::ADD, true),
        "sub" => (alu::SUB, true),
        "min" => (alu::MIN, true),
        "max" => (alu::MAX, true),
        "tmul" => (alu::TMUL, true),
        "tsat" => (alu::TSAT, true),
        "cons" => (alu::CONS, true),
        "neg" => (alu::NEG, false),
        "abs" => (alu::ABS, false),
        "sgn" => (alu::SGN, false),
        _ => return None,
    };
    Some((if words { f + valu::WORD_LANES } else { f }, binary))
}

fn alu_unary(m: &str) -> Option<i64> {
    Some(match m {
        "neg" | "not" => alu::NEG,
        "sgn" => alu::SGN,
        "abs" => alu::ABS,
        "tlz" => alu::TLZ,
        _ => return None,
    })
}

pub fn condition(m: &str) -> Option<i64> {
    Some(match m {
        "b" => cond::ALWAYS,
        "blt" => cond::LT,
        "beq" => cond::EQ,
        "bgt" => cond::GT,
        "ble" => cond::LE,
        "bge" => cond::GE,
        "bne" => cond::NE,
        "bov" => cond::OV,
        "bnov" => cond::NOV,
        "bnever" => cond::NEVER,
        _ => return None,
    })
}

fn mem_op(m: &str) -> Option<(i64, i64)> {
    Some(match m {
        "ldt" => (mem::LDT, 1),
        "ldw" => (mem::LDW, 3),
        "stt" => (mem::STT, 1),
        "stw" => (mem::STW, 3),
        "swap" => (mem::SWAP, 3),
        _ => return None,
    })
}

/// FPU loads and stores: (B function, access size in trytes).
fn fmem_op(m: &str) -> Option<(i64, i64)> {
    Some(match m {
        "flds" => (fmem::FLDS, 3),
        "fldd" => (fmem::FLDD, 9),
        "fsts" => (fmem::FSTS, 3),
        "fstd" => (fmem::FSTD, 9),
        _ => return None,
    })
}

/// FALU functions taking (fd, fa, fb) and (fd, fa).
fn falu_binary(m: &str) -> Option<i64> {
    Some(match m {
        "fadd" => falu::FADD,
        "fsub" => falu::FSUB,
        "fmul" => falu::FMUL,
        "fdiv" => falu::FDIV,
        "frem" => falu::FREM,
        "fmin" => falu::FMIN,
        "fmax" => falu::FMAX,
        _ => return None,
    })
}

fn falu_unary(m: &str) -> Option<i64> {
    Some(match m {
        "fsqrt" => falu::FSQRT,
        "fneg" => falu::FNEG,
        "fabs" => falu::FABS,
        "frnd" => falu::FRND,
        "ftrunc" => falu::FTRUNC,
        _ => return None,
    })
}

/// FCONST names (or a number 0..6).
pub fn fconst_name(s: &str) -> Option<i64> {
    Some(match s {
        "zero" => 0,
        "one" => 1,
        "pi" => 2,
        "e" => 3,
        "ln3" => 4,
        "log3e" => 5,
        "log3_10" => 6,
        _ => return None,
    })
}

pub fn control_register(s: &str) -> Option<i64> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "mode" => cr::MODE,
        "ptb" => cr::PTB,
        "faddr" => cr::FADDR,
        "fcause" => cr::FCAUSE,
        "itb" => cr::ITB,
        "epc" => cr::EPC,
        "emode" => cr::EMODE,
        "ksp" => cr::KSP,
        "scratch" => cr::SCRATCH,
        "cpuid" => cr::CPUID,
        "cycle" => cr::CYCLE,
        other => return other.strip_prefix("cr")?.parse().ok().filter(|n: &i64| n.abs() <= 13),
    })
}

fn sys_simple(m: &str) -> Option<i64> {
    Some(match m {
        "nop" => sys::NOP,
        "hlt" => sys::HLT,
        "brk" => sys::BRK,
        "iret" => sys::IRET,
        "tlbflush" => sys::TLBFLUSH,
        "fence" => sys::FENCE,
        "ill" => sys::ILL,
        _ => return None,
    })
}

// ---- assembly ----------------------------------------------------------------

struct Env<'a> {
    syms: &'a HashMap<String, i64>,
    here: i64,
    line: usize,
}

impl Env<'_> {
    fn val(&self, e: &Expr) -> Result<i64, Error> {
        e.eval(self.syms, self.here).map_err(|msg| Error { line: self.line, msg })
    }
    fn range(&self, what: &str, v: i64, lim: i64) -> Result<i64, Error> {
        if v.abs() <= lim {
            Ok(v)
        } else {
            err(self.line, format!("{what} {v} out of range ±{lim}"))
        }
    }
}

/// Size in words of an instruction. `long` = forced or relaxed to the long form.
fn inst_size(m: &str, ops: &[Operand], long: bool, consts: &HashMap<String, i64>) -> usize {
    let small = |e: &Expr, lim: i64| !long && e.constant(consts).is_some_and(|v| v.abs() <= lim);
    if alu_binary(m).is_some() || m == "cmp" {
        return match ops.last() {
            Some(Operand::Expr(e)) if !small(e, 13) => 2,
            _ => 1,
        };
    }
    if let Some((_, size)) = mem_op(m).or(fmem_op(m)) {
        return match ops.get(1) {
            Some(Operand::Mem(_, e)) if !long && e.constant(consts).is_some_and(|v| v % size == 0 && (v / size).abs() <= 13) => 1,
            _ => 2,
        };
    }
    match m {
        "li" | "mov" => match ops.get(1) {
            Some(Operand::Expr(e)) if !small(e, 364) => 2,
            _ => 1,
        },
        "la" | "lea" | "jal" | "call" | "br3" | "ext" | "ins" | "bext" | "brot" | "fli" => 2,
        "in" | "out" => match ops.get(1) {
            Some(Operand::Mem(..)) => 1,
            _ => 2,
        },
        "push" | "pop" => 2,
        _ if condition(m).is_some() && long => 2,
        _ => 1,
    }
}

/// Encode one instruction. Returns `Ok(None)` if a short branch is out of
/// range (the caller relaxes it to the long form and retries).
fn encode(m: &str, ops: &[Operand], size: usize, env: &Env) -> Result<Option<Vec<Word>>, Error> {
    let line = env.line;
    let bad = || err(line, format!("bad operands for `{m}`"));
    let reg = |o: &Operand| match o {
        Operand::Reg(r) => Ok(*r),
        _ => err(line, format!("register expected for `{m}`")),
    };
    let exp = |o: &Operand| match o {
        Operand::Expr(e) => env.val(e),
        _ => err(line, format!("value expected for `{m}`")),
    };
    let freg = |o: &Operand| match o {
        Operand::FReg(f) => Ok(*f),
        _ => err(line, format!("FPU register expected for `{m}`")),
    };
    let vreg = |o: &Operand| match o {
        Operand::VReg(v) => Ok(*v),
        _ => err(line, format!("vector register expected for `{m}`")),
    };
    let rel = |target: i64| -> Result<i64, Error> {
        let d = target - env.here;
        if d % 3 != 0 {
            return err(line, format!("branch target {target} is not word-aligned"));
        }
        Ok(d / 3)
    };
    let two = |w: [Word; 2]| Ok(Some(w.to_vec()));
    let one = |w: Word| Ok(Some(vec![w]));

    // ALU
    if let Some(f) = alu_binary(m).or((m == "cmp").then_some(alu::CMP)) {
        let (rd, ra, b) = match (m, ops) {
            ("cmp", [a, b]) => (0, reg(a)?, b),
            (_, [d, a, b]) => (reg(d)?, reg(a)?, b),
            _ => return bad(),
        };
        return match b {
            Operand::Reg(rb) => one(isa::rrr(op::ALU, f, rd, ra, *rb)),
            Operand::Expr(_) if size == 1 => one(isa::rrr(op::ALUQ, f, rd, ra, env.range("immediate", exp(b)?, 13)?)),
            Operand::Expr(_) => two(isa::wide(op::ALUI, f, rd, ra, env.range("immediate", exp(b)?, Word::MAX.value())?)),
            _ => bad(),
        };
    }
    if let Some(f) = alu_unary(m) {
        return match ops {
            [d, a] => one(isa::rrr(op::ALU, f, reg(d)?, reg(a)?, 0)),
            _ => bad(),
        };
    }
    if let Some((f, sz)) = mem_op(m) {
        let [d, Operand::Mem(base, off)] = ops else { return bad() };
        let off = env.val(off)?;
        return if size == 1 {
            one(isa::rrr(op::MEM, f, reg(d)?, *base, off / sz))
        } else {
            two(isa::wide(op::MEMD, f, reg(d)?, *base, off))
        };
    }
    if let Some(f) = falu_binary(m) {
        return match ops {
            [d, a, b] => one(isa::rrr(op::FALU, f, freg(d)?, freg(a)?, freg(b)?)),
            _ => bad(),
        };
    }
    if let Some(f) = falu_unary(m) {
        return match ops {
            [d, a] => one(isa::rrr(op::FALU, f, freg(d)?, freg(a)?, 0)),
            _ => bad(),
        };
    }
    if let Some((f, sz)) = fmem_op(m) {
        let [d, Operand::Mem(base, off)] = ops else { return bad() };
        let off = env.val(off)?;
        return if size == 1 {
            one(isa::rrr(op::FMEM, f, freg(d)?, *base, off / sz))
        } else {
            two(isa::wide(op::FMEMD, f, freg(d)?, *base, off))
        };
    }
    if let Some((f, binary)) = valu_lanes(m) {
        return match (binary, ops) {
            (true, [d, a, b]) => one(isa::rrr(op::VALU, f, vreg(d)?, vreg(a)?, vreg(b)?)),
            (false, [d, a]) => one(isa::rrr(op::VALU, f, vreg(d)?, vreg(a)?, 0)),
            _ => bad(),
        };
    }
    let vm = |b: i64, x: i64, y: i64, z: i64| one(isa::rrr(op::VMOV, b, x, y, z));
    let lane = |o: &Operand, n: i64| -> Result<i64, Error> {
        let k = exp(o)?;
        if !(0..n).contains(&k) {
            return err(line, format!("lane {k} out of range for `{m}` (0..{})", n - 1));
        }
        Ok(k)
    };
    match (m, ops) {
        ("vmrg", [d, k, s]) => return one(isa::rrr(op::VALU, valu::VMRG, vreg(d)?, vreg(k)?, vreg(s)?)),
        ("vsum", [d, a]) => return one(isa::rrr(op::VALU, valu::VSUM, reg(d)?, vreg(a)?, 0)),
        ("vdotw", [d, a, b]) => return one(isa::rrr(op::VALU, valu::VDOTW, reg(d)?, vreg(a)?, vreg(b)?)),
        ("vld" | "vst", [d, Operand::Mem(base, off)]) => {
            // 9 trytes; the offset counts in vectors
            let off = env.val(off)?;
            if off % 9 != 0 {
                return err(line, format!("`{m}` offset {off} is not a multiple of 9"));
            }
            let f = if m == "vld" { vmem::VLD } else { vmem::VST };
            return one(isa::rrr(op::VMEM, f, vreg(d)?, *base, env.range("offset/9", off / 9, 13)?));
        }
        ("vget", [d, a, k]) => return vm(vmov::VGET, reg(d)?, vreg(a)?, lane(k, 9)?),
        ("vput", [d, a, k]) => return vm(vmov::VPUT, vreg(d)?, reg(a)?, lane(k, 9)?),
        ("vgetw", [d, a, k]) => return vm(vmov::VGETW, reg(d)?, vreg(a)?, lane(k, 3)?),
        ("vputw", [d, a, k]) => return vm(vmov::VPUTW, vreg(d)?, reg(a)?, lane(k, 3)?),
        ("vbcst", [d, a]) => return vm(vmov::VBCST, vreg(d)?, reg(a)?, 0),
        ("vbcstw", [d, a]) => return vm(vmov::VBCSTW, vreg(d)?, reg(a)?, 0),
        _ if matches!(m, "vmrg" | "vsum" | "vdotw" | "vld" | "vst" | "vget" | "vput" | "vgetw" | "vputw" | "vbcst" | "vbcstw") => {
            return bad()
        }
        _ => {}
    }
    let fcv = |b: i64, x: i64, y: i64, z: i64| one(isa::rrr(op::FCVT, b, x, y, z));
    match (m, ops) {
        // masked store: the address in a register, no offset (R format)
        ("stwm" | "cas3", [d, Operand::Mem(base, off), k]) => {
            if env.val(off)? != 0 {
                return err(line, format!("`{m}` takes no offset: [ra] only"));
            }
            let f = if m == "stwm" { memr::STWM } else { memr::CAS3 };
            return one(isa::rrr(op::MEMR, f, reg(d)?, *base, reg(k)?));
        }
        ("stwm" | "cas3", _) => return bad(),
        ("fcmp", [a, b]) => return one(isa::rrr(op::FALU, falu::FCMP, 0, freg(a)?, freg(b)?)),
        ("fmov", [d, a]) => return fcv(fcvt::FMOV, freg(d)?, freg(a)?, 0),
        ("fcvtif", [d, a]) => return fcv(fcvt::FCVTIF, freg(d)?, reg(a)?, 0),
        ("fcvtfi", [d, a]) => return fcv(fcvt::FCVTFI, reg(d)?, freg(a)?, 0),
        ("ftow", [d, a]) => return fcv(fcvt::FTOW, reg(d)?, freg(a)?, 0),
        ("ffrw", [d, a]) => return fcv(fcvt::FFRW, freg(d)?, reg(a)?, 0),
        ("fexp", [d, a]) => return fcv(fcvt::FEXP, reg(d)?, freg(a)?, 0),
        ("fscl", [d, a, b]) => return fcv(fcvt::FSCL, freg(d)?, freg(a)?, reg(b)?),
        ("fmfs", [d]) => return fcv(fcvt::FMFS, reg(d)?, 0, 0),
        ("fmts", [a]) => return fcv(fcvt::FMTS, 0, reg(a)?, 0),
        ("fclass", [d, a]) => return fcv(fcvt::FCLASS, reg(d)?, freg(a)?, 0),
        ("fconst", [d, k]) => {
            let k = match k {
                Operand::Expr(Expr::Sym(name)) => fconst_name(name).ok_or_else(|| Error { line, msg: format!("unknown constant `{name}`") })?,
                Operand::Reg(0) => 0, // `zero` reads as the register name
                Operand::Expr(_) => env.range("constant number", exp(k)?, 6)?,
                _ => return bad(),
            };
            return fcv(fcvt::FCONST, freg(d)?, k, 0);
        }
        ("fli", [d, v]) => {
            let r = match v {
                Operand::Real(r) => *r,
                Operand::Expr(_) => setun_real::Real::from_i64(exp(v)?),
                _ => return bad(),
            };
            return two(isa::wide(op::FLI, 0, freg(d)?, 0, r.to_word27().0));
        }
        _ if matches!(
            m,
            "fcmp" | "fmov" | "fcvtif" | "fcvtfi" | "ftow" | "ffrw" | "fexp" | "fscl" | "fmfs" | "fmts" | "fclass" | "fconst" | "fli"
        ) =>
        {
            return bad()
        }
        _ => {}
    }
    if let Some(c) = condition(m) {
        let [t] = ops else { return bad() };
        let off = rel(exp(t)?)?;
        if size == 1 {
            if off.abs() > 364 {
                return Ok(None);
            }
            return one(isa::s6(op::BRQ, 0, c, off));
        }
        return two(isa::wide(op::BR, 0, c, 0, off));
    }
    if let Some(b) = sys_simple(m) {
        return if ops.is_empty() { one(isa::rrr(op::SYS, b, 0, 0, 0)) } else { bad() };
    }
    match (m, ops) {
        ("mov", [d, Operand::Reg(a)]) => one(isa::rrr(op::ALU, alu::ADD, reg(d)?, *a, 0)),
        ("li" | "mov", [d, v @ Operand::Expr(_)]) => {
            let v = exp(v)?;
            if size == 1 {
                one(isa::s6(op::LIQ, 0, reg(d)?, v))
            } else {
                two(isa::wide(op::LI, 0, reg(d)?, 0, env.range("constant", v, Word::MAX.value())?))
            }
        }
        ("la", [d, v]) => two(isa::wide(op::LI, 0, reg(d)?, 0, exp(v)?)),
        ("lea", [d, v]) => two(isa::wide(op::LEA, 0, reg(d)?, 0, exp(v)? - env.here)),
        ("jal", [d, t]) => two(isa::wide(op::JAL, 0, reg(d)?, 0, rel(exp(t)?)?)),
        ("call", [t]) => two(isa::wide(op::JAL, 0, 12, 0, rel(exp(t)?)?)),
        ("jalr", [d, a]) => one(isa::rrr(op::JALR, 0, reg(d)?, reg(a)?, 0)),
        ("jalr", [d, a, q]) => one(isa::rrr(op::JALR, 0, reg(d)?, reg(a)?, env.range("offset", exp(q)?, 13)?)),
        ("ret", []) => one(isa::rrr(op::JALR, 0, 0, 12, 0)),
        ("br3", [n, p]) => {
            let (n, p) = (rel(exp(n)?)?, rel(exp(p)?)?);
            two(isa::br3(env.range("BR3 offset", n, 9841)?, env.range("BR3 offset", p, 9841)?))
        }
        ("in" | "out", [d, port]) => {
            let b = if m == "in" { isa::io::IN } else { isa::io::OUT };
            match port {
                Operand::Mem(base, off) => {
                    one(isa::rrr(op::IO, b, reg(d)?, *base, env.range("port offset", env.val(off)?, 13)?))
                }
                Operand::Expr(_) => two(isa::wide(op::IOI, b, reg(d)?, 0, exp(port)?)),
                _ => bad(),
            }
        }
        ("push", [r]) => Ok(Some(vec![isa::rrr(op::ALUQ, alu::SUB, 13, 13, 3), isa::rrr(op::MEM, mem::STW, reg(r)?, 13, 0)])),
        ("pop", [r]) => Ok(Some(vec![isa::rrr(op::MEM, mem::LDW, reg(r)?, 13, 0), isa::rrr(op::ALUQ, alu::ADD, 13, 13, 3)])),
        ("ext" | "ins", [d, a, pos, len]) => {
            let b = if m == "ext" { fld::EXT } else { fld::INS };
            let (pos, len) = (exp(pos)?, exp(len)?);
            if pos < 1 || len < 1 || pos + len - 1 > 27 {
                return err(line, format!("trit field {pos}+{len} outside 1..27"));
            }
            two(isa::fld(b, reg(d)?, reg(a)?, pos, len))
        }
        ("bext" | "brot", [d, a, pos, len]) => {
            let b = if m == "bext" { fld::BEXT } else { fld::BROT };
            let (pos, len) = (exp(pos)?, exp(len)?);
            if isa::bin_field(b, 0, pos, len).is_none() {
                return err(line, format!("bit field {pos}+{len} out of range for {m}"));
            }
            two(isa::fld(b, reg(d)?, reg(a)?, pos, len))
        }
        ("tlbinv", [a]) => one(isa::rrr(op::SYS, sys::TLBINV, 0, reg(a)?, 0)),
        ("sys", [n]) => one(isa::s9(op::SYS, sys::SYS, env.range("system call number", exp(n)?, 9841)?)),
        ("mfc", [d, Operand::Expr(Expr::Sym(c))]) => {
            let c = control_register(c).ok_or_else(|| Error { line, msg: format!("unknown control register `{c}`") })?;
            one(isa::rrr(op::SYS, sys::MFC, reg(d)?, c, 0))
        }
        ("mtc", [Operand::Expr(Expr::Sym(c)), a]) => {
            let c = control_register(c).ok_or_else(|| Error { line, msg: format!("unknown control register `{c}`") })?;
            one(isa::rrr(op::SYS, sys::MTC, c, reg(a)?, 0))
        }
        _ if matches!(
            m,
            "mov" | "li" | "la" | "lea" | "jal" | "call" | "jalr" | "ret" | "br3" | "in" | "out" | "push" | "pop" | "ext" | "ins"
                | "bext" | "brot" | "tlbinv" | "sys" | "mfc" | "mtc"
        ) =>
        {
            bad()
        }
        _ => err(line, format!("unknown instruction `{m}`")),
    }
}

/// Assemble source text.
pub fn assemble(src: &str) -> Result<Program, Error> {
    let stmts = parse(src)?;

    // Constants: `.equ`s whose values need no labels, resolved in order.
    let mut consts = HashMap::new();
    for s in &stmts {
        if let Kind::Equ(name, e) = &s.kind {
            if let Some(v) = e.constant(&consts) {
                consts.insert(name.clone(), v);
            }
        }
    }

    let mut long: HashSet<usize> = stmts
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(&s.kind, Kind::Inst { long: true, .. }))
        .map(|(i, _)| i)
        .collect();

    for _round in 0..50 {
        // Layout.
        let mut syms: HashMap<String, i64> = HashMap::new();
        let mut addrs = Vec::with_capacity(stmts.len());
        let mut sizes = Vec::with_capacity(stmts.len());
        let mut at = 0i64;
        for (i, s) in stmts.iter().enumerate() {
            let size = match &s.kind {
                Kind::Org(e) => {
                    at = e
                        .constant(&consts)
                        .ok_or_else(|| Error { line: s.line, msg: ".org needs a constant".into() })?;
                    if at.rem_euclid(3) != 0 {
                        return err(s.line, format!(".org {at} is not word-aligned (a multiple of 3)"));
                    }
                    0
                }
                Kind::Label(name) => {
                    if syms.insert(name.clone(), at).is_some() || consts.contains_key(name) {
                        return err(s.line, format!("`{name}` defined twice"));
                    }
                    0
                }
                Kind::Equ(..) => 0,
                Kind::Words(v) => v.len(),
                Kind::Reals(v, wide) => v.len() * if *wide { 3 } else { 1 },
                Kind::Trytes(v) => v.len().div_ceil(3),
                Kind::Str(t) => (t.len() + 1).div_ceil(3),
                Kind::Align(e) => {
                    let n = e.constant(&consts).ok_or_else(|| Error { line: s.line, msg: ".align needs a constant".into() })?;
                    if n <= 0 || n % 3 != 0 {
                        return err(s.line, format!(".align {n}: must be a positive multiple of 3"));
                    }
                    ((n - at.rem_euclid(n)) % n / 3) as usize
                }
                Kind::Zero(e) | Kind::Fill(e, _) => {
                    let n = e.constant(&consts).ok_or_else(|| Error { line: s.line, msg: "count needs a constant".into() })?;
                    if n < 0 {
                        return err(s.line, "count is negative");
                    }
                    n as usize
                }
                Kind::Inst { m, ops, .. } => inst_size(m, ops, long.contains(&i), &consts),
            };
            addrs.push(at);
            sizes.push(size);
            at += 3 * size as i64;
        }
        for s in &stmts {
            if let Kind::Equ(name, e) = &s.kind {
                let v = e.eval(&syms.iter().chain(consts.iter()).map(|(k, v)| (k.clone(), *v)).collect(), 0);
                let v = v.map_err(|msg| Error { line: s.line, msg })?;
                if syms.insert(name.clone(), v).is_some() && !consts.contains_key(name) {
                    return err(s.line, format!("`{name}` defined twice"));
                }
            }
        }
        for (k, v) in &consts {
            syms.insert(k.clone(), *v);
        }

        // Encode.
        let mut segments: Vec<Segment> = Vec::new();
        let mut listing = Vec::new();
        let mut relaxed = false;
        for (i, s) in stmts.iter().enumerate() {
            let env = Env { syms: &syms, here: addrs[i], line: s.line };
            let words: Vec<Word> = match &s.kind {
                Kind::Org(_) => {
                    segments.push(Segment { origin: addrs[i], words: Vec::new() });
                    continue;
                }
                Kind::Label(_) | Kind::Equ(..) => continue,
                Kind::Words(v) => v
                    .iter()
                    .map(|e| env.val(e).and_then(|x| Word::new(x).ok_or_else(|| Error { line: s.line, msg: format!("word {x} out of range") })))
                    .collect::<Result<_, _>>()?,
                Kind::Reals(v, wide) => {
                    let mut out = Vec::new();
                    for r in v {
                        if *wide {
                            out.extend(r.to_words().iter().map(|&x| Word::new(x).unwrap()));
                        } else {
                            out.push(Word::new(r.to_word27().0).unwrap());
                        }
                    }
                    out
                }
                Kind::Trytes(v) => {
                    let t = v
                        .iter()
                        .map(|e| env.val(e).and_then(|x| env.range("tryte", x, 9841)))
                        .collect::<Result<Vec<_>, _>>()?;
                    pack_trytes(&t)
                }
                Kind::Str(t) => {
                    let mut t = t.clone();
                    t.push(0);
                    pack_trytes(&t)
                }
                Kind::Zero(_) | Kind::Align(_) => vec![Word::ZERO; sizes[i]],
                Kind::Fill(_, v) => {
                    let x = env.val(v)?;
                    let w = Word::new(x).ok_or_else(|| Error { line: s.line, msg: format!("word {x} out of range") })?;
                    vec![w; sizes[i]]
                }
                Kind::Inst { m, ops, .. } => match encode(m, ops, sizes[i], &env)? {
                    Some(w) => w,
                    None => {
                        long.insert(i);
                        relaxed = true;
                        continue;
                    }
                },
            };
            if words.is_empty() {
                continue;
            }
            if segments.is_empty() {
                segments.push(Segment { origin: 0, words: Vec::new() });
            }
            debug_assert_eq!(words.len(), sizes[i], "size mismatch on line {}", s.line);
            listing.push(ListingLine { line: s.line, addr: addrs[i], words: words.clone() });
            segments.last_mut().unwrap().words.extend(words);
        }
        if relaxed {
            continue;
        }
        segments.retain(|s| !s.words.is_empty());
        let symbols = syms.into_iter().collect();
        return Ok(Program { segments, symbols, listing });
    }
    err(0, "branch relaxation did not converge")
}

impl Program {
    /// Listing: address, words (base 27), source line.
    pub fn listing_text(&self, src: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        let mut out = String::new();
        for l in &self.listing {
            let words: Vec<String> = l.words.iter().take(2).map(|w| w.to_b27()).collect();
            let more = if l.words.len() > 2 { "…" } else { "" };
            out += &format!(
                "{:>14}  {:<20}{:<2} {}\n",
                l.addr,
                words.join(" "),
                more,
                lines.get(l.line - 1).map_or("", |s| s.trim_end())
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use setun_core::{Machine, Step};

    fn asm1(src: &str) -> Vec<Word> {
        let p = assemble(src).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.segments.len(), 1);
        p.segments[0].words.clone()
    }

    fn run(src: &str) -> Machine {
        let p = assemble(src).unwrap_or_else(|e| panic!("{e}"));
        let mut m = Machine::new(3, 0);
        for s in &p.segments {
            m.mem.load_words(s.origin, &s.words);
        }
        m.cpu.pc = Word::new(p.segments[0].origin).unwrap();
        for _ in 0..1_000_000 {
            if m.step() == Step::Halted {
                break;
            }
        }
        m
    }

    #[test]
    fn literals_and_expressions() {
        let w = asm1(".word 42, -7, 0t1T0, 0zDDD, 'Ж', 2*(3+4)-1, 10/3, -0t1");
        let v: Vec<i64> = w.iter().map(|w| w.value()).collect();
        assert_eq!(v, vec![42, -7, 6, 9841, 'Ж' as i64, 13, 3, -1]);
        // overlined digits, T as the ASCII -1, case-insensitive letters, hex
        let w = asm1(".word 0z1D\u{305}, 0z5\u{305}0, 0t11\u{305}0, 0tT1, 0zabc, 0x1F, -0x10, 0zB007AB13");
        let v: Vec<i64> = w.iter().map(|w| w.value()).collect();
        assert_eq!(v, vec![27 - 13, -135, 9 - 3, -3 + 1, 10 * 729 + 11 * 27 + 12, 31, -16, 115_067_810_199]);
        let p = assemble("X = 5\n.equ Y, X*2\n.word Y+1").unwrap();
        assert_eq!(p.segments[0].words[0].value(), 11);
    }

    #[test]
    fn form_selection() {
        // add with small imm → 1 word; big imm → 2; .l forces 2.
        assert_eq!(asm1("add r1, r2, 5").len(), 1);
        assert_eq!(asm1("add r1, r2, 14").len(), 2);
        assert_eq!(asm1("add.l r1, r2, 5").len(), 2);
        assert_eq!(asm1("li r1, 364").len(), 1);
        assert_eq!(asm1("li r1, -365").len(), 2);
        assert_eq!(asm1("ldw r1, [sp+39]").len(), 1, "13 words");
        assert_eq!(asm1("ldw r1, [sp+42]").len(), 2);
        assert_eq!(asm1("ldw r1, [sp+4]").len(), 2, "not a word multiple");
        assert_eq!(asm1("ldt r1, [r2-13]").len(), 1);
        assert_eq!(asm1("in r1, [r3+2]").len(), 1);
        assert_eq!(asm1("in r1, 486").len(), 2);
        // A label value is unknown before layout → long form.
        assert_eq!(asm1("x: li r1, x").len(), 2);
    }

    #[test]
    fn branch_relaxation() {
        let mut src = String::from("start: beq far\n");
        src += &"nop\n".repeat(400);
        src += "far: b start\n";
        let w = asm1(&src);
        assert_eq!(w.len(), 2 + 400 + 2, "both branches relaxed to 2 words");
        let w = asm1("top: nop\n b top");
        assert_eq!(w.len(), 2, "short branch stays short");
    }

    #[test]
    fn a_program_runs() {
        let m = run(r#"
            .org 300
            start:  li   r1, 10          ; count down
                    li   r2, 0
            loop:   add  r2, r2, r1
                    sub  r1, r1, 1
                    bgt  loop
                    la   r3, msg
                    sub  r3, r3, 1       ; first tryte of the string
                    call strlen
                    hlt
            strlen: li   r4, 0
            next:   ldt  r5, [r3]
                    cmp  r5, 0
                    beq  done
                    add  r3, r3, 1
                    add  r4, r4, 1
                    b    next
            done:   ret
            msg:    .string "Привет, мир!"
        "#);
        assert_eq!(m.cpu.reg(2).value(), 55);
        assert_eq!(m.cpu.reg(4).value(), 12);
    }

    #[test]
    fn stack_io_and_three_way() {
        let m = run(r#"
            .org 0
                    li   sp, 9000
                    li   r1, 7
                    push r1
                    li   r1, 0
                    pop  r2
                    li   r5, -3
                    cmp  r5, 0
                    br3  neg, pos
                    li   r6, 0
                    hlt
            neg:    li   r6, -1
                    li   r7, 'H'
                    out  r7, 162        ; serial DATA
                    hlt
            pos:    li   r6, 1
                    hlt
        "#);
        assert_eq!(m.cpu.reg(2).value(), 7);
        assert_eq!(m.cpu.reg(6).value(), -1);
    }

    #[test]
    fn errors_have_line_numbers() {
        let e = |src: &str| assemble(src).unwrap_err();
        assert_eq!(e("nop\nfrob r1").line, 2);
        assert!(e("frob r1").msg.contains("unknown instruction"));
        assert!(e("b nowhere").msg.contains("undefined symbol"));
        assert!(e("x: nop\nx: nop").msg.contains("defined twice"));
        assert!(e("add r1, r2").msg.contains("bad operands"));
        assert!(e(".org 1").msg.contains("word-aligned"));
        assert!(e("jalr r1, r2, 14").msg.contains("out of range"));
        assert!(e(".string \"unterminated").msg.contains("unterminated"));
        assert!(e(".word 0t1x").msg.contains("invalid ternary digit 'x'"));
        // the new notation's refusals
        assert!(e(".word 0t+-0").msg.contains("the old + and - are gone"));
        // 0t0+0 was 3 in the old notation: refused, not read as 0 + 0
        for src in [".word 0t0+0", ".word 0t0+-", ".word 0t1-1", "li r1, 0t1T+3"] {
            assert!(e(src).msg.contains("put a space before the operator"), "{src}");
        }
        assert_eq!(asm1(".word 0t1 - 1, 0t1T + 3")[..], [Word::new(0).unwrap(), Word::new(5).unwrap()]);
        assert!(e(".word 0zMMM").msg.contains("invalid ternary digit 'M'"), "the old letters are gone");
        assert!(e(".word 0zE").msg.contains("invalid ternary digit 'E'"));
        assert!(e(".word 0z10\u{305}").msg.contains("'0' cannot be overlined"));
        assert!(e(".word 0zE\u{305}").msg.contains("'E' cannot be overlined"));
        assert!(e(".word 0z1\u{305}\u{305}").msg.contains("two overlines"));
        assert!(e(".word 0x1\u{305}").msg.contains("hexadecimal digits cannot be overlined"));
        assert!(e(".word 0xG").msg.contains("invalid hexadecimal digit 'G'"));
        assert!(e(".word 0z").msg.contains("empty 0z literal"));
        assert!(e(".word 0z1DDDDDDDDDDDDDDDDDDDDDD").msg.contains("does not fit"));
        assert!(e("ext r1, r2, 20, 9").msg.contains("outside 1..27"));
        assert!(e("bext r1, r2, 0, 42").msg.contains("out of range for bext"));
        assert!(e("brot r1, r2, 32, 32").msg.contains("out of range for brot"));
        assert!(e("bext r1, r2, -1, 8").msg.contains("out of range"));
        assert!(e("mfc r1, bogus").msg.contains("unknown control register"));
        assert!(e(".tryte 9842").msg.contains("out of range"));
        assert!(e(".bogus 1").msg.contains("unknown directive"));
        assert!(e("in r1, [r3+20]").msg.contains("out of range"));
    }

    #[test]
    fn comments_and_strings_with_semicolons() {
        let p = assemble(".string \"a;b\" ; real comment\n.tryte ';'").unwrap();
        let t = crate::words_to_trytes(&p.segments[0].words);
        assert_eq!(&t[..4], &['a' as i16, ';' as i16, 'b' as i16, 0]);
    }
}
