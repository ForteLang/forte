//! Hand-written lexer. No dependencies: the toolchain must stay deterministic
//! and portable to wasm32 unchanged.

use crate::diag::{Diag, Pos};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Str(String),
    /// Number with an optional unit suffix written directly after the digits
    /// (`96bpm`, `0.5`, `4`).
    Num(f64, Option<String>),
    /// Contents of a backtick literal (the preceding ident says which kind).
    Backtick(String),
    LBrace,
    RBrace,
    LParen,
    RParen,
    Colon,
    Comma,
    Slash,
    Minus,
    DotDot,
    Dot,
    Eq,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Spanned {
    pub tok: Tok,
    pub pos: Pos,
}

pub fn lex(src: &str) -> Result<Vec<Spanned>, Diag> {
    let mut out = Vec::new();
    let b: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;

    macro_rules! pos {
        () => {
            Pos { line, col }
        };
    }

    while i < b.len() {
        let c = b[i];
        let p = pos!();
        match c {
            ' ' | '\t' | '\r' => {
                i += 1;
                col += 1;
            }
            '\n' => {
                i += 1;
                line += 1;
                col = 1;
            }
            '/' if i + 1 < b.len() && b[i + 1] == '/' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' => {
                out.push(Spanned { tok: Tok::Slash, pos: p });
                i += 1;
                col += 1;
            }
            '{' | '}' | '(' | ')' | ':' | ',' | '-' | '=' => {
                let tok = match c {
                    '{' => Tok::LBrace,
                    '}' => Tok::RBrace,
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    ':' => Tok::Colon,
                    ',' => Tok::Comma,
                    '-' => Tok::Minus,
                    _ => Tok::Eq,
                };
                out.push(Spanned { tok, pos: p });
                i += 1;
                col += 1;
            }
            '.' if i + 1 < b.len() && b[i + 1] == '.' => {
                out.push(Spanned { tok: Tok::DotDot, pos: p });
                i += 2;
                col += 2;
            }
            '.' => {
                out.push(Spanned { tok: Tok::Dot, pos: p });
                i += 1;
                col += 1;
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                col += 1;
                loop {
                    if i >= b.len() || b[i] == '\n' {
                        return Err(Diag::new("E-LEX-001", p, "文字列リテラルが閉じていません"));
                    }
                    if b[i] == '"' {
                        i += 1;
                        col += 1;
                        break;
                    }
                    s.push(b[i]);
                    i += 1;
                    col += 1;
                }
                out.push(Spanned { tok: Tok::Str(s), pos: p });
            }
            '`' => {
                let mut s = String::new();
                i += 1;
                col += 1;
                loop {
                    if i >= b.len() {
                        return Err(Diag::new("E-LEX-002", p, "音楽リテラル(`...`)が閉じていません"));
                    }
                    if b[i] == '`' {
                        i += 1;
                        col += 1;
                        break;
                    }
                    if b[i] == '\n' {
                        line += 1;
                        col = 1;
                    } else {
                        col += 1;
                    }
                    s.push(b[i]);
                    i += 1;
                }
                out.push(Spanned { tok: Tok::Backtick(s), pos: p });
            }
            '0'..='9' => {
                let mut s = String::new();
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                    // stop at `..` (range), which is not a decimal point
                    if b[i] == '.' && i + 1 < b.len() && b[i + 1] == '.' {
                        break;
                    }
                    s.push(b[i]);
                    i += 1;
                    col += 1;
                }
                let n: f64 = s
                    .parse()
                    .map_err(|_| Diag::new("E-LEX-003", p, format!("数値として読めません: {s}")))?;
                // unit suffix written flush against the digits: 96bpm, -0.3dB
                let mut unit = String::new();
                while i < b.len() && (b[i].is_ascii_alphabetic()) {
                    unit.push(b[i]);
                    i += 1;
                    col += 1;
                }
                let unit = if unit.is_empty() { None } else { Some(unit) };
                out.push(Spanned { tok: Tok::Num(n, unit), pos: p });
            }
            c if c.is_ascii_alphabetic() || c == '_' || c == '@' => {
                let mut s = String::new();
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || b[i] == '_' || b[i] == '@' || b[i] == '#')
                {
                    s.push(b[i]);
                    i += 1;
                    col += 1;
                }
                out.push(Spanned { tok: Tok::Ident(s), pos: p });
            }
            other => {
                return Err(Diag::new(
                    "E-LEX-004",
                    p,
                    format!("使えない文字です: '{other}'"),
                ));
            }
        }
    }
    out.push(Spanned { tok: Tok::Eof, pos: pos!() });
    Ok(out)
}
