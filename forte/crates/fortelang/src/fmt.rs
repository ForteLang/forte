//! `forte fmt` — the single canonical layout (SRS-LANG-002): stable diffs and
//! clean merges depend on there being exactly one way to format a file.
//!
//! v0 normalizes lines (indentation from brace depth, trailing whitespace,
//! blank-line runs) without touching strings, music literals or comments.
//! Safety property, checked on every run: the formatted output must lex to
//! the exact same token stream as the input — formatting can never change
//! meaning.

use crate::diag::{Diag, Pos};
use crate::lexer::{lex, Tok};

pub fn format(src: &str) -> Result<String, Diag> {
    let out = normalize(src);
    // meaning-preservation guarantee: identical token streams
    let before: Vec<Tok> = lex(src)?.into_iter().map(|s| s.tok).collect();
    let after: Vec<Tok> = lex(&out)?.into_iter().map(|s| s.tok).collect();
    if before != after {
        return Err(Diag::new(
            "E-FMT-001",
            Pos { line: 1, col: 1 },
            "フォーマッタが意味を変えてしまうためこのファイルには適用しません(バグ報告を歓迎します)",
        ));
    }
    Ok(out)
}

fn normalize(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut depth: i32 = 0;
    let mut in_backtick = false;
    let mut in_block_comment = false;
    let mut blank_run = 0usize;

    for raw_line in src.lines() {
        // lines inside multi-line music literals or block comments are verbatim
        if in_backtick || in_block_comment {
            out.push_str(raw_line.trim_end());
            out.push('\n');
            scan_state(raw_line, &mut depth, &mut in_backtick, &mut in_block_comment);
            continue;
        }
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run == 1 {
                out.push('\n');
            }
            continue;
        }
        blank_run = 0;
        // closing braces at line start outdent that line
        let leading_closes = trimmed.chars().take_while(|&c| c == '}').count() as i32;
        let indent = (depth - leading_closes).max(0) as usize;
        for _ in 0..indent {
            out.push_str("  ");
        }
        out.push_str(trimmed);
        out.push('\n');
        scan_state(trimmed, &mut depth, &mut in_backtick, &mut in_block_comment);
    }

    // exactly one trailing newline, no leading blank line
    let s = out.trim_start_matches('\n').trim_end_matches('\n');
    format!("{s}\n")
}

/// Update brace depth / literal / comment state across one line.
fn scan_state(line: &str, depth: &mut i32, in_backtick: &mut bool, in_block_comment: &mut bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if *in_block_comment {
            if c == '*' && chars.get(i + 1) == Some(&'/') {
                *in_block_comment = false;
                i += 1;
            }
        } else if *in_backtick {
            if c == '`' {
                *in_backtick = false;
            }
        } else if in_string {
            if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '/' if chars.get(i + 1) == Some(&'/') => break, // line comment
                '/' if chars.get(i + 1) == Some(&'*') => {
                    *in_block_comment = true;
                    i += 1;
                }
                '`' => *in_backtick = true,
                '"' => in_string = true,
                '{' => *depth += 1,
                '}' => *depth = (*depth - 1).max(0),
                _ => {}
            }
        }
        i += 1;
    }
}
