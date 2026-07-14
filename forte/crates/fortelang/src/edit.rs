//! Lossless structured edits over `.forte` source — the Studio P0 spike
//! (issue #135, DAW-FR "GUI projection" rows).
//!
//! The contract: a GUI gesture becomes a minimal text splice. We parse with
//! the REAL parser (validation + `Pos` anchors), locate the anchored token in
//! a byte-span-preserving token stream, and replace ONLY the bytes of the
//! tokens the edit touches. Comments, blank lines and layout survive by
//! construction, because everything outside the splice is never rewritten.
//! After every operation the result must re-parse cleanly; a failure there is
//! an internal error and the edit is refused — this module never emits source
//! it cannot read back.
//!
//! Operations arrive as JSON (one object or an array, applied in order):
//!
//! ```json
//! {"op":"set_tempo","bpm":118}
//! {"op":"set_pattern","track":"Drums","play":0,"value":"x... x... x... x..."}
//! {"op":"set_pattern","let_name":"K","value":"x... x... x... x..."}
//! {"op":"move_place","place":1,"block":"Drop","bars":[17,24]}
//! {"op":"move_play","track":"Bass","play":0,"bars":[5,8]}
//! {"op":"add_place","path":["Build"],"block":"Riser","bars":[1,4],"alias":"Rise"}
//! {"op":"remove_place","place":2}
//! {"op":"set_arg","track":"Bass","target":"instrument","arg":"cutoff","value":0.62}
//! {"op":"set_arg","track":"Mix","target":"insert:1","arg":"type","value":"hp"}
//! {"op":"set_section","name":"drop","bars":[33,48]}
//! ```
//!
//! `path` names the body the edit applies to: `[]` (default) is the file's
//! song (or the last top-level block of a block library); `["A", "B"]`
//! descends into nested `block` definitions.

use crate::ast::*;
use crate::diag::{Diag, Pos};
use crate::lexer::{lex, Spanned, Tok};
use crate::parser::parse;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOp {
    /// Rewrite the body's `tempo` value (unit suffix is preserved).
    SetTempo {
        #[serde(default)]
        path: Vec<String>,
        bpm: f64,
    },
    /// Replace the contents of a music literal (the text between backticks)
    /// — either a body-level `let <let_name> = …` or the literal played
    /// inline by `track` / `play` (0-based index among that track's plays).
    SetPattern {
        #[serde(default)]
        path: Vec<String>,
        #[serde(default)]
        let_name: Option<String>,
        #[serde(default)]
        track: Option<String>,
        #[serde(default)]
        play: usize,
        value: String,
    },
    /// Re-place a block placement (0-based index among the body's `play`
    /// placements). `block` is an optional guard: if given it must match the
    /// placement's block name or alias (protects against stale GUI indices).
    MovePlace {
        #[serde(default)]
        path: Vec<String>,
        place: usize,
        #[serde(default)]
        block: Option<String>,
        bars: (u32, u32),
    },
    /// Re-place a track-level pattern play.
    MovePlay {
        #[serde(default)]
        path: Vec<String>,
        track: String,
        play: usize,
        bars: (u32, u32),
    },
    /// Append `play <block> at bars(a..b) [as <alias>]` to a body.
    AddPlace {
        #[serde(default)]
        path: Vec<String>,
        block: String,
        bars: (u32, u32),
        #[serde(default)]
        alias: Option<String>,
    },
    /// Delete a block placement (its whole line when it owns the line).
    RemovePlace {
        #[serde(default)]
        path: Vec<String>,
        place: usize,
        #[serde(default)]
        block: Option<String>,
    },
    /// Set a named argument on a track's `instrument` (target
    /// `"instrument"`) or on an insert (target `"insert:<index>"`). Missing
    /// arguments are added; present ones are rewritten in place.
    SetArg {
        #[serde(default)]
        path: Vec<String>,
        track: String,
        target: String,
        arg: String,
        value: ArgValue,
    },
    /// Rewrite a named section's bar range.
    SetSection {
        #[serde(default)]
        path: Vec<String>,
        name: String,
        bars: (u32, u32),
    },
}

/// Parse an op payload: a single JSON object or an array of them.
pub fn parse_ops(json: &str) -> Result<Vec<EditOp>, Diag> {
    let p0 = Pos { line: 1, col: 1 };
    if let Ok(ops) = serde_json::from_str::<Vec<EditOp>>(json) {
        return Ok(ops);
    }
    match serde_json::from_str::<EditOp>(json) {
        Ok(op) => Ok(vec![op]),
        Err(e) => Err(Diag::new("E-EDIT-001", p0, format!("編集オペレーションのJSONが読めません: {e}"))),
    }
}

/// Apply ops in order. Each op re-parses the current source (offsets shift
/// between ops), splices, and verifies the result still parses.
pub fn apply_ops(src: &str, ops: &[EditOp]) -> Result<String, Diag> {
    let mut cur = src.to_string();
    for op in ops {
        cur = apply_one(&cur, op)?;
    }
    Ok(cur)
}

fn apply_one(src: &str, op: &EditOp) -> Result<String, Diag> {
    let p0 = Pos { line: 1, col: 1 };
    let toks = lex(src)?;
    let file = parse(src).map_err(|mut ds| ds.remove(0))?;
    let ctx = Ctx { src, toks: &toks };

    let out = match op {
        EditOp::SetTempo { path, bpm } => {
            let (body, _) = resolve_body(&file, path)?;
            match body.tempo {
                Some((_, pos)) => {
                    let idx = ctx.tok_at(pos)?;
                    let text = match &ctx.toks[idx].tok {
                        Tok::Num(_, Some(unit)) => format!("{}{}", fmt_num(*bpm), unit),
                        _ => fmt_num(*bpm),
                    };
                    let (a, b) = ctx.value_span(idx);
                    splice(src, a, b, &text)
                }
                None => {
                    // no tempo statement yet: write one as the body's first line
                    let open = ctx.body_open_brace(&file, path)?;
                    let indent = ctx.first_stmt_indent(open);
                    let insert_at = ctx.after_token_line(open);
                    splice(src, insert_at, insert_at, &format!("{indent}tempo {}bpm\n", fmt_num(*bpm)))
                }
            }
        }
        EditOp::SetPattern { path, let_name, track, play, value } => {
            if value.contains('`') {
                return Err(Diag::new("E-EDIT-005", p0, "パターン値にバッククォートは書けません"));
            }
            let (body, _) = resolve_body(&file, path)?;
            let lit_pos = match (let_name, track) {
                (Some(l), None) => {
                    let letd = body.lets.iter().find(|x| &x.name == l).ok_or_else(|| {
                        Diag::new("E-EDIT-003", p0, format!("let {l} が見つかりません"))
                    })?;
                    letd.value.pos
                }
                (None, Some(t)) => {
                    let tr = find_track(body, t)?;
                    let pl = tr.plays.get(*play).ok_or_else(|| {
                        Diag::new("E-EDIT-003", tr.pos, format!("Track '{t}' に play #{play} がありません(play は {} 個)", tr.plays.len()))
                    })?;
                    match &pl.pattern {
                        PatternRef::Lit(lit) => lit.pos,
                        _ => {
                            return Err(Diag::new(
                                "E-EDIT-005",
                                pl.pos,
                                "このplayはリテラルではありません(let名やパターン関数の中身は set_pattern let_name で編集してください)",
                            ))
                        }
                    }
                }
                _ => return Err(Diag::new("E-EDIT-001", p0, "set_pattern には let_name か track のどちらか一方が必要です")),
            };
            let kind_idx = ctx.tok_at(lit_pos)?;
            let bt = kind_idx + 1;
            let Tok::Backtick(_) = &ctx.toks[bt].tok else {
                return Err(Diag::new("E-EDIT-006", lit_pos, "リテラル本体(バッククォート)が見つかりません"));
            };
            // replace the text BETWEEN the backticks
            splice(src, ctx.toks[bt].off + 1, ctx.toks[bt].end - 1, value)
        }
        EditOp::MovePlace { path, place, block, bars } => {
            let (body, _) = resolve_body(&file, path)?;
            let pl = select_place(body, *place, block.as_deref())?;
            let anchor = ctx.tok_at(pl.pos)?;
            let at = ctx.place_at_span(anchor)?;
            ctx.splice_at(src, at, *bars)
        }
        EditOp::MovePlay { path, track, play, bars } => {
            let (body, _) = resolve_body(&file, path)?;
            let tr = find_track(body, track)?;
            let pl = tr.plays.get(*play).ok_or_else(|| {
                Diag::new("E-EDIT-003", tr.pos, format!("Track '{track}' に play #{play} がありません(play は {} 個)", tr.plays.len()))
            })?;
            let anchor = ctx.tok_at(pl.pos)?;
            let j = ctx.pattern_end(anchor)?;
            let at = ctx.at_span_after_kw(j)?;
            ctx.splice_at(src, at, *bars)
        }
        EditOp::AddPlace { path, block, bars, alias } => {
            let close = ctx.body_close_brace(&file, path)?;
            let stmt = match alias {
                Some(a) => format!("play {block} as {a} at bars({}..{})", bars.0, bars.1),
                None => format!("play {block} at bars({}..{})", bars.0, bars.1),
            };
            let close_off = ctx.toks[close].off;
            let line_start = src[..close_off].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if src[line_start..close_off].trim().is_empty() {
                // `}` opens its line: insert the statement as its own line above
                let (body, _) = resolve_body(&file, path)?;
                let indent = place_indent(&ctx, body, line_start, close_off, src);
                splice(src, line_start, line_start, &format!("{indent}{stmt}\n"))
            } else {
                // `}` shares a line (e.g. `block X { }`): insert inline
                splice(src, close_off, close_off, &format!("{stmt} "))
            }
        }
        EditOp::RemovePlace { path, place, block } => {
            let (body, _) = resolve_body(&file, path)?;
            let pl = select_place(body, *place, block.as_deref())?;
            let anchor = ctx.tok_at(pl.pos)?;
            if anchor == 0 || !matches!(&ctx.toks[anchor - 1].tok, Tok::Ident(s) if s == "play") {
                return Err(Diag::new("E-EDIT-006", pl.pos, "play 文の先頭が見つかりません"));
            }
            let kw = anchor - 1;
            let at = ctx.place_at_span(anchor)?;
            let last_end = match at {
                AtSpan::Bars { whole, .. } => ctx.toks[whole.1].end,
                AtSpan::Section(j) => ctx.toks[j].end,
            };
            let start_off = ctx.toks[kw].off;
            let line_start = src[..start_off].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix_blank = src[line_start..start_off].trim().is_empty();
            // does anything besides whitespace / a trailing comment follow on the line?
            let rest = &src[last_end..];
            let line_len = rest.find('\n').unwrap_or(rest.len());
            let tail = rest[..line_len].trim_start();
            let tail_removable = tail.is_empty() || tail.starts_with("//");
            if prefix_blank && tail_removable {
                let del_end = (last_end + line_len + 1).min(src.len());
                splice(src, line_start, del_end, "")
            } else {
                splice(src, start_off, last_end, "")
            }
        }
        EditOp::SetArg { path, track, target, arg, value } => {
            let (body, _) = resolve_body(&file, path)?;
            let tr = find_track(body, track)?;
            let call: &Call = if target == "instrument" {
                tr.instrument.as_ref().ok_or_else(|| {
                    Diag::new("E-EDIT-003", tr.pos, format!("Track '{track}' に instrument がありません"))
                })?
            } else if let Some(nstr) = target.strip_prefix("insert:") {
                let n: usize = nstr.parse().map_err(|_| {
                    Diag::new("E-EDIT-001", p0, format!("target は \"instrument\" か \"insert:<番号>\" です(見つかったのは {target})"))
                })?;
                tr.inserts.get(n).ok_or_else(|| {
                    Diag::new("E-EDIT-003", tr.pos, format!("Track '{track}' に insert #{n} がありません(insert は {} 個)", tr.inserts.len()))
                })?
            } else {
                return Err(Diag::new("E-EDIT-001", p0, format!("target は \"instrument\" か \"insert:<番号>\" です(見つかったのは {target})")));
            };
            let text = match value {
                ArgValue::Num(v) => fmt_num(*v),
                ArgValue::Str(s) => {
                    if s.contains('"') || s.contains('\n') {
                        return Err(Diag::new("E-EDIT-005", p0, "文字列引数に引用符や改行は書けません"));
                    }
                    format!("\"{s}\"")
                }
            };
            match call.args.iter().find(|(k, _)| k == arg) {
                Some((_, a)) => {
                    let idx = ctx.tok_at(a.pos())?;
                    let (s, e) = ctx.value_span(idx);
                    splice(src, s, e, &text)
                }
                None => {
                    // add the argument before the closing paren (or add parens)
                    let name_idx = ctx.tok_at(call.pos)?;
                    if matches!(ctx.toks[name_idx + 1].tok, Tok::LParen) {
                        let after = ctx.skip_parens(name_idx + 1)?;
                        let rparen_off = ctx.toks[after - 1].off;
                        let ins = if call.args.is_empty() {
                            format!("{arg}: {text}")
                        } else {
                            format!(", {arg}: {text}")
                        };
                        splice(src, rparen_off, rparen_off, &ins)
                    } else {
                        let e = ctx.toks[name_idx].end;
                        splice(src, e, e, &format!("({arg}: {text})"))
                    }
                }
            }
        }
        EditOp::SetSection { path, name, bars } => {
            let (body, _) = resolve_body(&file, path)?;
            let sec = body.sections.iter().find(|s| &s.name == name).ok_or_else(|| {
                Diag::new("E-EDIT-003", p0, format!("section {name} が見つかりません"))
            })?;
            // tokens: <name> = bars ( a .. b )
            let idx = ctx.tok_at(sec.pos)?;
            let a = idx + 4;
            let b = idx + 6;
            let ok = matches!(ctx.toks[idx + 1].tok, Tok::Eq)
                && matches!(&ctx.toks[idx + 2].tok, Tok::Ident(s) if s == "bars")
                && matches!(ctx.toks[idx + 3].tok, Tok::LParen)
                && matches!(ctx.toks[a].tok, Tok::Num(..))
                && matches!(ctx.toks[a + 1].tok, Tok::DotDot)
                && matches!(ctx.toks[b].tok, Tok::Num(..));
            if !ok {
                return Err(Diag::new("E-EDIT-006", sec.pos, "section 文の形が想定と違います"));
            }
            splice(src, ctx.toks[a].off, ctx.toks[b].end, &format!("{}..{}", bars.0, bars.1))
        }
    };

    // the hard guarantee: never hand back source we cannot read
    if let Err(ds) = parse(&out) {
        let d = &ds[0];
        let line = out.lines().nth(d.pos.line.saturating_sub(1) as usize).unwrap_or("");
        return Err(Diag::new(
            "E-EDIT-004",
            d.pos,
            format!("編集結果がパースできません(編集は取り消されました): {} — {line}", d.message),
        ));
    }
    Ok(out)
}

// ---------------------------------------------------------------- helpers

struct Ctx<'a> {
    src: &'a str,
    toks: &'a [Spanned],
}

enum AtSpan {
    /// `bars ( a .. b )` — token indices of the two numbers and of the
    /// `bars` keyword / closing paren.
    Bars { nums: (usize, usize), whole: (usize, usize) },
    /// A section name reference (single ident token).
    Section(usize),
}

impl<'a> Ctx<'a> {
    fn tok_at(&self, pos: Pos) -> Result<usize, Diag> {
        self.toks
            .iter()
            .position(|t| t.pos == pos)
            .ok_or_else(|| Diag::new("E-EDIT-006", pos, "編集アンカーのトークンが見つかりません"))
    }

    /// Byte span of a value that may be a negative number (Minus + Num).
    fn value_span(&self, idx: usize) -> (usize, usize) {
        if matches!(self.toks[idx].tok, Tok::Minus) {
            (self.toks[idx].off, self.toks[idx + 1].end)
        } else {
            (self.toks[idx].off, self.toks[idx].end)
        }
    }

    /// From an opening paren token index, return the index AFTER the
    /// matching closing paren.
    fn skip_parens(&self, mut j: usize) -> Result<usize, Diag> {
        let mut depth = 0usize;
        loop {
            match self.toks[j].tok {
                Tok::LParen => depth += 1,
                Tok::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(j + 1);
                    }
                }
                Tok::Eof => {
                    return Err(Diag::new("E-EDIT-006", self.toks[j].pos, "括弧が閉じていません"))
                }
                _ => {}
            }
            j += 1;
        }
    }

    /// From a placement's block-name token, walk the placement grammar
    /// (args, `as`) to its `at` reference.
    fn place_at_span(&self, anchor: usize) -> Result<AtSpan, Diag> {
        let mut j = anchor + 1;
        if matches!(self.toks[j].tok, Tok::LParen) {
            j = self.skip_parens(j)?;
        }
        if matches!(&self.toks[j].tok, Tok::Ident(s) if s == "as") {
            j += 2;
        }
        self.at_span_after_kw(j)
    }

    /// `j` must sit on the `at` keyword; returns the span of what follows.
    fn at_span_after_kw(&self, j: usize) -> Result<AtSpan, Diag> {
        if !matches!(&self.toks[j].tok, Tok::Ident(s) if s == "at") {
            return Err(Diag::new("E-EDIT-006", self.toks[j].pos, "`at` が見つかりません"));
        }
        let k = j + 1;
        match &self.toks[k].tok {
            Tok::Ident(s) if s == "bars" => {
                let ok = matches!(self.toks[k + 1].tok, Tok::LParen)
                    && matches!(self.toks[k + 2].tok, Tok::Num(..))
                    && matches!(self.toks[k + 3].tok, Tok::DotDot)
                    && matches!(self.toks[k + 4].tok, Tok::Num(..))
                    && matches!(self.toks[k + 5].tok, Tok::RParen);
                if !ok {
                    return Err(Diag::new("E-EDIT-006", self.toks[k].pos, "bars(a..b) の形が想定と違います"));
                }
                Ok(AtSpan::Bars { nums: (k + 2, k + 4), whole: (k, k + 5) })
            }
            Tok::Ident(_) => Ok(AtSpan::Section(k)),
            _ => Err(Diag::new("E-EDIT-006", self.toks[k].pos, "配置先(bars かセクション名)が見つかりません")),
        }
    }

    /// End of a pattern expression starting at `anchor` (index of the token
    /// AFTER it). Literals are `kind` + backtick; names may carry a
    /// balanced argument list (pattern functions).
    fn pattern_end(&self, anchor: usize) -> Result<usize, Diag> {
        if matches!(&self.toks[anchor].tok, Tok::Ident(k) if k == "beat" || k == "notes" || k == "prog")
            && matches!(self.toks[anchor + 1].tok, Tok::Backtick(_))
        {
            return Ok(anchor + 2);
        }
        let mut j = anchor + 1;
        if matches!(self.toks[j].tok, Tok::LParen) {
            j = self.skip_parens(j)?;
        }
        Ok(j)
    }

    fn splice_at(&self, src: &str, at: AtSpan, bars: (u32, u32)) -> String {
        match at {
            AtSpan::Bars { nums, .. } => splice(
                src,
                self.toks[nums.0].off,
                self.toks[nums.1].end,
                &format!("{}..{}", bars.0, bars.1),
            ),
            AtSpan::Section(j) => splice(
                src,
                self.toks[j].off,
                self.toks[j].end,
                &format!("bars({}..{})", bars.0, bars.1),
            ),
        }
    }

    /// Token index of the `{` opening the body named by `path`.
    fn body_open_brace(&self, file: &FileAst, path: &[String]) -> Result<usize, Diag> {
        let (_, block_pos) = resolve_body(file, path)?;
        let from = match block_pos {
            Some(pos) => self.tok_at(pos)?,
            None => self
                .toks
                .iter()
                .position(|t| matches!(&t.tok, Tok::Ident(s) if s == "song"))
                .ok_or_else(|| Diag::new("E-EDIT-002", Pos { line: 1, col: 1 }, "song が見つかりません"))?,
        };
        (from..self.toks.len())
            .find(|&j| matches!(self.toks[j].tok, Tok::LBrace))
            .ok_or_else(|| Diag::new("E-EDIT-006", self.toks[from].pos, "`{` が見つかりません"))
    }

    /// Token index of the `}` closing the body named by `path`.
    fn body_close_brace(&self, file: &FileAst, path: &[String]) -> Result<usize, Diag> {
        let open = self.body_open_brace(file, path)?;
        let mut depth = 0usize;
        for j in open..self.toks.len() {
            match self.toks[j].tok {
                Tok::LBrace => depth += 1,
                Tok::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(j);
                    }
                }
                _ => {}
            }
        }
        Err(Diag::new("E-EDIT-006", self.toks[open].pos, "body が閉じていません"))
    }

    /// Byte offset just after the newline that ends `tok`'s line (for
    /// inserting a fresh first line inside a body).
    fn after_token_line(&self, tok: usize) -> usize {
        let e = self.toks[tok].end;
        match self.src[e..].find('\n') {
            Some(i) => e + i + 1,
            None => self.src.len(),
        }
    }

    /// Indentation of the first statement after the opening brace, or two
    /// spaces past the brace line's own indent when the body is empty.
    fn first_stmt_indent(&self, open: usize) -> String {
        let next = &self.toks[open + 1];
        if !matches!(next.tok, Tok::RBrace | Tok::Eof) {
            let off = next.off;
            let line_start = self.src[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if self.src[line_start..off].trim().is_empty() {
                return self.src[line_start..off].to_string();
            }
        }
        let brace_off = self.toks[open].off;
        let line_start = self.src[..brace_off].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let own: String =
            self.src[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        format!("{own}  ")
    }
}

/// Indentation for a new placement line: match the body's last placement
/// if there is one, else fall back to the closing brace line's indent + 2.
fn place_indent(ctx: &Ctx, body: &SongAst, close_line_start: usize, close_off: usize, src: &str) -> String {
    if let Some(last) = body.places.last() {
        if let Ok(anchor) = ctx.tok_at(last.pos) {
            if anchor > 0 {
                let off = ctx.toks[anchor - 1].off;
                let ls = src[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
                if src[ls..off].trim().is_empty() {
                    return src[ls..off].to_string();
                }
            }
        }
    }
    format!("{}  ", &src[close_line_start..close_off])
}

fn find_track<'b>(body: &'b SongAst, name: &str) -> Result<&'b TrackAst, Diag> {
    body.tracks.iter().find(|t| t.name == name).ok_or_else(|| {
        Diag::new("E-EDIT-003", Pos { line: 1, col: 1 }, format!("Track '{name}' が見つかりません"))
    })
}

fn select_place<'b>(body: &'b SongAst, index: usize, guard: Option<&str>) -> Result<&'b PlaceAst, Diag> {
    let pl = body.places.get(index).ok_or_else(|| {
        Diag::new(
            "E-EDIT-003",
            Pos { line: 1, col: 1 },
            format!("配置 #{index} がありません(配置は {} 個)", body.places.len()),
        )
    })?;
    if let Some(g) = guard {
        let alias_ok = pl.alias.as_deref() == Some(g);
        if pl.block != g && !alias_ok {
            return Err(Diag::new(
                "E-EDIT-003",
                pl.pos,
                format!("配置 #{index} は {}(guard は {g})— インデックスが古い可能性があります", pl.block),
            ));
        }
    }
    Ok(pl)
}

/// Resolve a body path. Returns the body and, when the body is a `block`,
/// the position of its `block` keyword (None = the file's `song`).
fn resolve_body<'f>(file: &'f FileAst, path: &[String]) -> Result<(&'f SongAst, Option<Pos>), Diag> {
    let p0 = Pos { line: 1, col: 1 };
    let mut rest: &[String] = path;
    let (mut body, mut pos): (&SongAst, Option<Pos>) = if rest.is_empty() {
        if let Some(s) = &file.song {
            (s, None)
        } else if let Some(b) = file.blocks.last() {
            (&b.body, Some(b.pos))
        } else {
            return Err(Diag::new("E-EDIT-002", p0, "song も block もないファイルです"));
        }
    } else if let Some(b) = file.blocks.iter().find(|b| b.name == rest[0]) {
        rest = &rest[1..];
        (&b.body, Some(b.pos))
    } else if let Some(s) = &file.song {
        (s, None)
    } else {
        return Err(Diag::new("E-EDIT-002", p0, format!("block {} が見つかりません", rest[0])));
    };
    for name in rest {
        let b = body.blocks.iter().find(|b| &b.name == name).ok_or_else(|| {
            Diag::new("E-EDIT-002", p0, format!("block {name} が見つかりません"))
        })?;
        body = &b.body;
        pos = Some(b.pos);
    }
    Ok((body, pos))
}

fn splice(src: &str, from: usize, to: usize, text: &str) -> String {
    let mut out = String::with_capacity(src.len() + text.len());
    out.push_str(&src[..from]);
    out.push_str(text);
    out.push_str(&src[to..]);
    out
}

/// Shortest clean decimal for a value (integers without the trailing `.0`).
fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e12 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
