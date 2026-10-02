//! Tokens.

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    /// A decimal real literal, as written (checked).
    Real(String),
    Char(i64),
    Str(String),
    Ident(String),
    Kw(&'static str),
    Punct(&'static str),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub file: u16,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
}

const KEYWORDS: &[&str] = &[
    "fn", "var", "const", "struct", "return", "if", "else", "while", "for", "loop", "break", "continue", "sign",
    "switch", "case", "default", "as", "sizeof", "import", "extern", "interrupt", "asm", "true", "false",
    "null", "void", "trit", "tryte", "int", "char", "real",
];

// Longest first.
const PUNCT: &[&str] = &[
    "<<=", ">>=", "<=>", "->", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "&=",
    "|=", "+", "-", "*", "/", "%", "<", ">", "=", "!", "~", "&", "|", "?", ":", ";", ",", ".", "(", ")", "[", "]",
    "{", "}", "@",
];

pub fn lex(src: &str, file: u16) -> Result<Vec<Token>, Error> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    macro_rules! bump {
        () => {{
            if chars[i] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }
    while i < chars.len() {
        let c = chars[i];
        let pos = Pos { file, line, col };
        let err = |msg: String| Error { pos, msg };
        if c.is_whitespace() {
            bump!();
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                bump!();
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            bump!();
            bump!();
            loop {
                if i >= chars.len() {
                    return Err(err("unterminated /* comment".into()));
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    bump!();
                    bump!();
                    break;
                }
                bump!();
            }
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            if c == '0' && matches!(chars.get(i + 1), Some('t' | 'z' | 'x')) {
                // the whole run of letters, digits, `_` and overlines is the literal
                let base = chars[i + 1];
                bump!();
                bump!();
                let ds = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == ternary::OVERLINE) {
                    bump!();
                }
                let digits: String = chars[ds..i].iter().collect();
                if base == 't' && matches!(chars.get(i), Some('+' | '-')) {
                    // `0t0+0` was once the digits 0, +, 0 (= 3); refuse it rather than read 0 + 0
                    return Err(err(if digits.is_empty() {
                        "0t digits are 1, 0 and T (the old + and - are gone)".into()
                    } else {
                        "a 0t literal followed by + or -: put a space before the operator (the old + and - digits are gone)".into()
                    }));
                }
                let v = ternary::parse_prefixed(base, &digits).map_err(|e| err(format!("bad 0{base} literal: {e}")))?;
                out.push(Token { tok: Tok::Int(v), pos });
                continue;
            }
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                bump!();
            }
            // a real: digits, then a point and digits, and/or an exponent
            let mut real = false;
            if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                real = true;
                bump!();
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    bump!();
                }
            }
            if matches!(chars.get(i), Some('e' | 'E')) {
                let d = if matches!(chars.get(i + 1), Some('+' | '-')) { 2 } else { 1 };
                if chars.get(i + d).is_some_and(|c| c.is_ascii_digit()) {
                    real = true;
                    for _ in 0..d {
                        bump!();
                    }
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        bump!();
                    }
                }
            }
            // a letter right after the number (1x, 2.5q) is a mistake
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                bump!();
            }
            let t: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            if real {
                setun_real::Real::from_decimal(&t).map_err(|_| err(format!("bad number `{t}`")))?;
                out.push(Token { tok: Tok::Real(t), pos });
            } else {
                let v = t.parse::<i64>().map_err(|_| err(format!("bad number `{t}`")))?;
                out.push(Token { tok: Tok::Int(v), pos });
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                bump!();
            }
            let w: String = chars[start..i].iter().collect();
            let tok = match KEYWORDS.iter().find(|k| **k == w) {
                Some(k) => Tok::Kw(k),
                None => Tok::Ident(w),
            };
            out.push(Token { tok, pos });
            continue;
        }
        if c == '\'' || c == '"' {
            let q = c;
            bump!();
            let mut s = String::new();
            loop {
                let Some(&ch) = chars.get(i) else { return Err(err("unterminated literal".into())) };
                if ch == '\n' {
                    return Err(err("unterminated literal".into()));
                }
                bump!();
                if ch == q {
                    break;
                }
                if ch == '\\' {
                    let Some(&e) = chars.get(i) else { return Err(err("unterminated escape".into())) };
                    bump!();
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        'e' => '\x1b',
                        '0' => '\0',
                        '\\' => '\\',
                        '\'' => '\'',
                        '"' => '"',
                        _ => return Err(err(format!("unknown escape `\\{e}`"))),
                    });
                } else {
                    s.push(ch);
                }
            }
            // a character followed by U+0305 is overlined: its negated code point
            let cps = ternary::code_points(&s).map_err(err)?;
            if q == '\'' {
                match cps[..] {
                    [ch] => out.push(Token { tok: Tok::Char(ch), pos }),
                    _ => return Err(err("a character literal holds exactly one character".into())),
                }
            } else {
                out.push(Token { tok: Tok::Str(s), pos });
            }
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        match PUNCT.iter().find(|p| rest.starts_with(**p)) {
            Some(p) => {
                for _ in 0..p.len() {
                    bump!();
                }
                out.push(Token { tok: Tok::Punct(p), pos });
            }
            None => return Err(err(format!("unexpected character `{c}`"))),
        }
    }
    out.push(Token { tok: Tok::Eof, pos: Pos { file, line, col } });
    Ok(out)
}
