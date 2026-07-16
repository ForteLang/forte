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
//! {"op":"set_track","track":"Bass","field":"volume","value":0.7}
//! {"op":"set_send","track":"Bass","dest":"Space","level":0.3}
//! {"op":"set_section","name":"drop","bars":[33,48]}
//! {"op":"add_import","names":["Groove"],"from":"../blocks/groove.forte"}
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
    /// Set a track-level mix statement — `field` is `"volume"`, `"level"`
    /// (LUFS target) or `"pan"`. A present statement has its number
    /// rewritten in place (unit suffix kept); a missing one is inserted as
    /// the track's first statement. The mixer fader's write path.
    SetTrack {
        #[serde(default)]
        path: Vec<String>,
        track: String,
        field: String,
        value: f64,
    },
    /// Set a track's `send <dest> <level>`. An existing send to `dest` has
    /// its level rewritten; otherwise the send is added to the track.
    SetSend {
        #[serde(default)]
        path: Vec<String>,
        track: String,
        dest: String,
        level: f64,
    },
    /// Rewrite a named section's bar range.
    SetSection {
        #[serde(default)]
        path: Vec<String>,
        name: String,
        bars: (u32, u32),
    },
    /// Re-place whatever `play` (placement or track play) or `audio`
    /// statement sits on a 1-based source line. The op the arrange view
    /// speaks: its clips know source lines, not body paths.
    MoveAtLine { line: u32, bars: (u32, u32) },
    /// Append `track <name> { instrument …; play … }` to a body — the
    /// palette's "+トラック" gesture. `instrument` is the call text
    /// (e.g. `sampler(sample: "Kick")`), `play` an optional first play
    /// statement body (e.g. ``beat`x...` at bars(1..4)``); both are
    /// validated by the re-parse guard like everything else.
    AddTrack {
        #[serde(default)]
        path: Vec<String>,
        name: String,
        instrument: String,
        #[serde(default)]
        play: Option<String>,
    },
    /// Replace a track's WHOLE instrument call (name and arguments) —
    /// the "swap the instrument" gesture. `call` is the new call text
    /// (e.g. `AcidBass(cutoff: 0.3)`); the re-parse guard validates it.
    SetInstrument {
        #[serde(default)]
        path: Vec<String>,
        track: String,
        call: String,
    },
    /// Delete a whole `track <name> { … }` (its lines when it owns them).
    RemoveTrack {
        #[serde(default)]
        path: Vec<String>,
        track: String,
    },
    /// Ensure `import { names… } from "from"` exists — missing names merge
    /// into an existing import of the same path; otherwise a new import
    /// line lands above the first statement (leading comments stay on
    /// top). The "place a block from the library" gesture's first half
    /// (`add_place` is the second).
    AddImport { names: Vec<String>, from: String },
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
                    // no tempo statement yet: write one as the body's first statement
                    let open = ctx.body_open_brace(&file, path)?;
                    ctx.insert_after_open(src, open, &format!("tempo {}bpm", fmt_num(*bpm)))
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
        EditOp::SetTrack { path, track, field, value } => {
            let (body, _) = resolve_body(&file, path)?;
            let tr = find_track(body, track)?;
            let slot = match field.as_str() {
                "volume" => &tr.volume,
                "level" => &tr.level,
                "pan" => &tr.pan,
                _ => {
                    return Err(Diag::new(
                        "E-EDIT-001",
                        p0,
                        format!("set_track の field は volume / level / pan です(見つかったのは {field})"),
                    ))
                }
            };
            match slot {
                Some((_, pos)) => {
                    let idx = ctx.tok_at(*pos)?;
                    let (a, b) = ctx.value_span(idx);
                    let num = if matches!(ctx.toks[idx].tok, Tok::Minus) { idx + 1 } else { idx };
                    let text = match &ctx.toks[num].tok {
                        Tok::Num(_, Some(unit)) => format!("{}{}", fmt_num(*value), unit),
                        _ => fmt_num(*value),
                    };
                    splice(src, a, b, &text)
                }
                None => {
                    let open = ctx.track_open_brace(tr)?;
                    ctx.insert_after_open(src, open, &format!("{field} {}", fmt_num(*value)))
                }
            }
        }
        EditOp::SetSend { path, track, dest, level } => {
            let (body, _) = resolve_body(&file, path)?;
            let tr = find_track(body, track)?;
            match tr.sends.iter().find(|(d, _, _)| d == dest) {
                Some((_, _, pos)) => {
                    // tokens: `send` <dest> <level> — pos anchors the dest name
                    let idx = ctx.tok_at(*pos)?;
                    let (a, b) = ctx.value_span(idx + 1);
                    splice(src, a, b, &fmt_num(*level))
                }
                None => {
                    let open = ctx.track_open_brace(tr)?;
                    ctx.insert_after_open(src, open, &format!("send {dest} {}", fmt_num(*level)))
                }
            }
        }
        EditOp::MoveAtLine { line, bars } => {
            let at = find_at_on_line(&ctx, &file, *line)?;
            ctx.splice_at(src, at, *bars)
        }
        EditOp::AddTrack { path, name, instrument, play } => {
            let ok_name = !name.is_empty()
                && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_');
            if !ok_name {
                return Err(Diag::new("E-EDIT-001", p0, format!("track 名が不正です: \"{name}\"")));
            }
            for (what, text) in
                [("instrument", instrument.as_str()), ("play", play.as_deref().unwrap_or(""))]
            {
                if text.contains('{') || text.contains('}') || text.contains('\n') {
                    return Err(Diag::new(
                        "E-EDIT-005",
                        p0,
                        format!("add_track の {what} に {{ }} や改行は書けません"),
                    ));
                }
            }
            let (body, _) = resolve_body(&file, path)?;
            if body.tracks.iter().any(|t| &t.name == name) {
                return Err(Diag::new("E-EDIT-003", p0, format!("track '{name}' は既にあります")));
            }
            let close = ctx.body_close_brace(&file, path)?;
            let close_off = ctx.toks[close].off;
            let line_start = src[..close_off].rfind('\n').map(|i| i + 1).unwrap_or(0);
            // match the last track's indentation, else brace indent + 2
            let indent = match body.tracks.last().and_then(|t| ctx.tok_at(t.pos).ok()) {
                Some(anchor) => {
                    let off = ctx.toks[anchor].off;
                    let ls = src[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    if src[ls..off].trim().is_empty() {
                        src[ls..off].to_string()
                    } else {
                        "  ".into()
                    }
                }
                None => format!("{}  ", &src[line_start..close_off]),
            };
            let play_line = match play {
                Some(pl) => format!("{indent}  play {pl}\n"),
                None => String::new(),
            };
            let stmt = format!(
                "{indent}track {name} {{\n{indent}  instrument {instrument}\n{play_line}{indent}}}\n"
            );
            if src[line_start..close_off].trim().is_empty() {
                splice(src, line_start, line_start, &stmt)
            } else {
                // one-line body: break the line before the closing brace
                splice(src, close_off, close_off, &format!("\n{stmt}"))
            }
        }
        EditOp::SetInstrument { path, track, call } => {
            if call.contains('{') || call.contains('}') || call.contains('\n') || call.contains('`') {
                return Err(Diag::new(
                    "E-EDIT-005",
                    p0,
                    "set_instrument の call に {} や改行、バッククォートは書けません",
                ));
            }
            let (body, _) = resolve_body(&file, path)?;
            let tr = find_track(body, track)?;
            let cur = tr.instrument.as_ref().ok_or_else(|| {
                Diag::new("E-EDIT-003", tr.pos, format!("Track '{track}' に instrument がありません"))
            })?;
            let name_idx = ctx.tok_at(cur.pos)?;
            let end = if matches!(ctx.toks[name_idx + 1].tok, Tok::LParen) {
                let after = ctx.skip_parens(name_idx + 1)?;
                ctx.toks[after - 1].end
            } else {
                ctx.toks[name_idx].end
            };
            splice(src, ctx.toks[name_idx].off, end, call)
        }
        EditOp::RemoveTrack { path, track } => {
            let (body, _) = resolve_body(&file, path)?;
            let tr = find_track(body, track)?;
            let kw = ctx.tok_at(tr.pos)?;
            let open = (kw..ctx.toks.len())
                .find(|&j| matches!(ctx.toks[j].tok, Tok::LBrace))
                .ok_or_else(|| Diag::new("E-EDIT-006", tr.pos, "track の `{` が見つかりません"))?;
            let mut depth = 0usize;
            let mut close = open;
            for j in open..ctx.toks.len() {
                match ctx.toks[j].tok {
                    Tok::LBrace => depth += 1,
                    Tok::RBrace => {
                        depth -= 1;
                        if depth == 0 {
                            close = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if close == open {
                return Err(Diag::new("E-EDIT-006", tr.pos, "track が閉じていません"));
            }
            let start_off = ctx.toks[kw].off;
            let line_start = src[..start_off].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix_blank = src[line_start..start_off].trim().is_empty();
            let close_end = ctx.toks[close].end;
            let rest = &src[close_end..];
            let line_len = rest.find('\n').unwrap_or(rest.len());
            let tail = rest[..line_len].trim_start();
            if prefix_blank && (tail.is_empty() || tail.starts_with("//")) {
                let del_end = (close_end + line_len + 1).min(src.len());
                splice(src, line_start, del_end, "")
            } else {
                splice(src, start_off, close_end, "")
            }
        }
        EditOp::AddImport { names, from } => {
            if names.is_empty() {
                return Err(Diag::new("E-EDIT-001", p0, "add_import には names が最低 1 つ必要です"));
            }
            match file.imports.iter().find(|i| &i.path == from) {
                Some(im) => {
                    let missing: Vec<&String> =
                        names.iter().filter(|n| !im.names.contains(n)).collect();
                    if missing.is_empty() {
                        src.to_string() // already imported: a no-op, not an error
                    } else {
                        // splice ", A, B" after the last name inside the braces
                        let kw = ctx.tok_at(im.pos)?;
                        let rbrace = (kw..ctx.toks.len())
                            .find(|&j| matches!(ctx.toks[j].tok, Tok::RBrace))
                            .ok_or_else(|| Diag::new("E-EDIT-006", im.pos, "import の `}` が見つかりません"))?;
                        let last = rbrace - 1;
                        let add = missing.iter().map(|n| format!(", {n}")).collect::<String>();
                        splice(src, ctx.toks[last].end, ctx.toks[last].end, &add)
                    }
                }
                None => {
                    // a new import line: below the last import, else above the
                    // first statement (file-leading comments stay on top)
                    let stmt = format!("import {{ {} }} from \"{from}\"\n", names.join(", "));
                    let at = match file.imports.last() {
                        Some(last) => {
                            let kw = ctx.tok_at(last.pos)?;
                            ctx.after_token_line(kw)
                        }
                        None => {
                            let first = ctx.toks.first().filter(|t| !matches!(t.tok, Tok::Eof));
                            match first {
                                Some(t) => src[..t.off].rfind('\n').map(|i| i + 1).unwrap_or(0),
                                None => 0,
                            }
                        }
                    };
                    splice(src, at, at, &stmt)
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

    /// Token index of the `{` opening a track's body.
    fn track_open_brace(&self, tr: &TrackAst) -> Result<usize, Diag> {
        let kw = self.tok_at(tr.pos)?;
        (kw..self.toks.len())
            .find(|&j| matches!(self.toks[j].tok, Tok::LBrace))
            .ok_or_else(|| Diag::new("E-EDIT-006", tr.pos, "track の `{` が見つかりません"))
    }

    /// Insert `stmt` as the first statement after an opening brace. When
    /// the `{` ends its line the statement gets its own line below it; a
    /// one-line body (`track T { … }`) gets it spliced inline after the
    /// brace instead — never outside the braces.
    fn insert_after_open(&self, src: &str, open: usize, stmt: &str) -> String {
        let e = self.toks[open].end;
        let rest = &self.src[e..];
        let line_rest = rest[..rest.find('\n').unwrap_or(rest.len())].trim_start();
        if line_rest.is_empty() || line_rest.starts_with("//") {
            let indent = self.first_stmt_indent(open);
            let at = self.after_token_line(open);
            splice(src, at, at, &format!("{indent}{stmt}\n"))
        } else {
            splice(src, e, e, &format!(" {stmt}"))
        }
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

/// Find the at-ref of the play/place/audio statement anchored on `line`,
/// searching every body (song + top-level blocks, nested included).
fn find_at_on_line(ctx: &Ctx, file: &FileAst, line: u32) -> Result<AtSpan, Diag> {
    fn walk(ctx: &Ctx, body: &SongAst, line: u32) -> Option<Result<AtSpan, Diag>> {
        for pl in &body.places {
            if pl.pos.line == line {
                let anchor = match ctx.tok_at(pl.pos) {
                    Ok(a) => a,
                    Err(e) => return Some(Err(e)),
                };
                return Some(ctx.place_at_span(anchor));
            }
        }
        for t in &body.tracks {
            for pl in &t.plays {
                if pl.pos.line == line {
                    let anchor = match ctx.tok_at(pl.pos) {
                        Ok(a) => a,
                        Err(e) => return Some(Err(e)),
                    };
                    let j = match ctx.pattern_end(anchor) {
                        Ok(j) => j,
                        Err(e) => return Some(Err(e)),
                    };
                    return Some(ctx.at_span_after_kw(j));
                }
            }
            for a in &t.audios {
                if a.pos.line == line {
                    // `audio <name> at …` — the anchor is the asset name
                    let anchor = match ctx.tok_at(a.pos) {
                        Ok(x) => x,
                        Err(e) => return Some(Err(e)),
                    };
                    return Some(ctx.at_span_after_kw(anchor + 1));
                }
            }
        }
        for b in &body.blocks {
            if let Some(r) = walk(ctx, &b.body, line) {
                return Some(r);
            }
        }
        None
    }
    let mut found = None;
    if let Some(s) = &file.song {
        found = walk(ctx, s, line);
    }
    if found.is_none() {
        for b in &file.blocks {
            found = walk(ctx, &b.body, line);
            if found.is_some() {
                break;
            }
        }
    }
    found.unwrap_or_else(|| {
        Err(Diag::new(
            "E-EDIT-003",
            Pos { line, col: 1 },
            format!("{line} 行目に play / audio 文がありません(import した block の clip はこのファイルからは動かせません)"),
        ))
    })
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

// ------------------------------------------------------------ read side

/// One editable music literal in the source — what a grid/roll GUI binds to.
/// `path`/`track`/`play` (or `let_name`) are exactly the coordinates the
/// corresponding `set_pattern` op takes, so a GUI can echo them back verbatim.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PatternSite {
    pub path: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub let_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play: Option<usize>,
    /// "beat" | "notes" | "prog"
    pub kind: String,
    /// The literal's current contents (between the backticks).
    pub raw: String,
    /// 1-based source line of the literal (for editor code-jumps).
    pub line: u32,
    /// Where the play sits: "a..b" or a section name (lets have none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// List every editable pattern literal with the coordinates `set_pattern`
/// expects. The read side of the GUI round-trip.
pub fn pattern_sites(src: &str) -> Result<Vec<PatternSite>, Diag> {
    let file = parse(src).map_err(|mut ds| ds.remove(0))?;
    let mut out = Vec::new();
    if let Some(s) = &file.song {
        walk_sites(s, &mut Vec::new(), &mut out);
    }
    for b in &file.blocks {
        let mut path = vec![b.name.clone()];
        walk_sites(&b.body, &mut path, &mut out);
    }
    Ok(out)
}

fn walk_sites(body: &SongAst, path: &mut Vec<String>, out: &mut Vec<PatternSite>) {
    for l in &body.lets {
        out.push(PatternSite {
            path: path.clone(),
            let_name: Some(l.name.clone()),
            track: None,
            play: None,
            kind: l.value.kind.clone(),
            raw: l.value.raw.clone(),
            line: l.value.pos.line,
            at: None,
        });
    }
    for t in &body.tracks {
        for (i, p) in t.plays.iter().enumerate() {
            if let PatternRef::Lit(lit) = &p.pattern {
                out.push(PatternSite {
                    path: path.clone(),
                    let_name: None,
                    track: Some(t.name.clone()),
                    play: Some(i),
                    kind: lit.kind.clone(),
                    raw: lit.raw.clone(),
                    line: lit.pos.line,
                    at: Some(match &p.at {
                        AtRef::Bars(a, b) => format!("{a}..{b}"),
                        AtRef::Section(s, _) => s.clone(),
                    }),
                });
            }
        }
    }
    for b in &body.blocks {
        path.push(b.name.clone());
        walk_sites(&b.body, path, out);
        path.pop();
    }
}

/// One `instrument` / `insert` call in the source — what an inspector GUI
/// binds knobs to. `path`/`track`/`target` are exactly the coordinates the
/// corresponding `set_arg` op takes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArgSite {
    pub path: Vec<String>,
    pub track: String,
    /// "instrument" or "insert:<index>"
    pub target: String,
    /// The call's name (device or effect).
    pub name: String,
    /// 1-based source line of the call (for editor code-jumps).
    pub line: u32,
    /// Present arguments in written order.
    pub args: Vec<ArgSiteArg>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ArgSiteArg {
    pub arg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub str: Option<String>,
}

/// List every instrument/insert call with the coordinates `set_arg`
/// expects. The inspector's read side of the GUI round-trip.
pub fn arg_sites(src: &str) -> Result<Vec<ArgSite>, Diag> {
    let file = parse(src).map_err(|mut ds| ds.remove(0))?;
    let mut out = Vec::new();
    if let Some(s) = &file.song {
        walk_arg_sites(s, &mut Vec::new(), &mut out);
    }
    for b in &file.blocks {
        let mut path = vec![b.name.clone()];
        walk_arg_sites(&b.body, &mut path, &mut out);
    }
    Ok(out)
}

fn walk_arg_sites(body: &SongAst, path: &mut Vec<String>, out: &mut Vec<ArgSite>) {
    let site = |track: &str, target: String, call: &Call, path: &[String]| ArgSite {
        path: path.to_vec(),
        track: track.to_string(),
        target,
        name: call.name.clone(),
        line: call.pos.line,
        args: call
            .args
            .iter()
            .map(|(k, a)| match a {
                Arg::Num(v, _) => ArgSiteArg { arg: k.clone(), num: Some(*v), str: None },
                Arg::Str(s, _) | Arg::Ident(s, _) => {
                    ArgSiteArg { arg: k.clone(), num: None, str: Some(s.clone()) }
                }
            })
            .collect(),
    };
    for t in &body.tracks {
        if let Some(c) = &t.instrument {
            out.push(site(&t.name, "instrument".into(), c, path));
        }
        for (i, c) in t.inserts.iter().enumerate() {
            out.push(site(&t.name, format!("insert:{i}"), c, path));
        }
    }
    for b in &body.blocks {
        path.push(b.name.clone());
        walk_arg_sites(&b.body, path, out);
        path.pop();
    }
}

// ------------------------------------------------ piano-roll round trip

/// One roll note in beats — the JSON a piano-roll GUI speaks. Chords are
/// several events sharing a start (and duration/flags).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteEvent {
    pub start: f64,
    pub dur: f64,
    pub pitch: u8,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub accent: bool,
}

/// A parsed `notes` literal: its events plus the total length in beats
/// (trailing rests live in `len`, not in any event).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotesDoc {
    pub len: f64,
    pub notes: Vec<NoteEvent>,
}

/// Structurally parse a `notes` literal (same grammar as the compiler:
/// `C2:1`, `[D4 F4 A4]:2`, `_:1`, `~` tie, `!` accent, `1/2` durations).
pub fn note_events(raw: &str) -> Result<NotesDoc, Diag> {
    let p0 = Pos { line: 1, col: 1 };
    let mut out = Vec::new();
    let mut cursor = 0.0f64;
    let mut toks = raw.split_whitespace().peekable();
    let mut events: Vec<String> = Vec::new();
    while let Some(t) = toks.next() {
        if t.starts_with('[') && !t.contains(']') {
            let mut acc = t.to_string();
            for u in toks.by_ref() {
                acc.push(' ');
                acc.push_str(u);
                if u.contains(']') {
                    break;
                }
            }
            events.push(acc);
        } else {
            events.push(t.to_string());
        }
    }
    for ev in events {
        let (head, durs) = ev.rsplit_once(':').ok_or_else(|| {
            Diag::new("E-NOTE-005", p0, format!("イベントは `ピッチ:長さ` の形です: {ev}"))
        })?;
        let dur = crate::music::parse_duration(durs, p0)?;
        if head == "_" {
            cursor += dur;
            continue;
        }
        let (head, tie) = match head.strip_suffix('~') {
            Some(h) => (h, true),
            None => (head, false),
        };
        let (head, accent) = match head.strip_suffix('!') {
            Some(h) => (h, true),
            None => (head, false),
        };
        let pitches: Vec<&str> = if head.starts_with('[') && head.ends_with(']') {
            head[1..head.len() - 1].split_whitespace().collect()
        } else {
            vec![head]
        };
        for ps in pitches {
            let pitch = crate::music::parse_pitch(ps, p0)?;
            out.push(NoteEvent { start: cursor, dur, pitch, tie, accent });
        }
        cursor += dur;
    }
    Ok(NotesDoc { len: cursor, notes: out })
}

/// MIDI pitch → written name (sharps).
fn pitch_name(p: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[(p % 12) as usize], (p / 12) as i32 - 1)
}

/// Shortest clean duration text (mirrors what composers write by hand).
fn fmt_dur(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// Serialize a roll document back to idiomatic `notes` text: simultaneous
/// equal-length events become chords, gaps become `_:n` rests, trailing
/// space is padded from `len`. Overlaps that are not exact chords cannot
/// be written in the sequential notes grammar and are refused.
pub fn serialize_notes(doc: &NotesDoc) -> Result<String, Diag> {
    let p0 = Pos { line: 1, col: 1 };
    let mut evs = doc.notes.clone();
    evs.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap().then(a.pitch.cmp(&b.pitch)));
    let mut toks: Vec<String> = Vec::new();
    let mut cursor = 0.0f64;
    let eps = 1e-6;
    let mut i = 0;
    while i < evs.len() {
        let e = evs[i].clone();
        let mut pitches = vec![e.pitch];
        let mut j = i + 1;
        while j < evs.len()
            && (evs[j].start - e.start).abs() < eps
            && (evs[j].dur - e.dur).abs() < eps
            && evs[j].tie == e.tie
            && evs[j].accent == e.accent
        {
            pitches.push(evs[j].pitch);
            j += 1;
        }
        if e.start < cursor - eps {
            return Err(Diag::new(
                "E-EDIT-007",
                p0,
                "音が重なっています(同時に鳴らすには開始と長さを揃えて和音にしてください)",
            ));
        }
        if e.start - cursor > eps {
            toks.push(format!("_:{}", fmt_dur(e.start - cursor)));
        }
        let name = if pitches.len() == 1 {
            pitch_name(pitches[0])
        } else {
            format!("[{}]", pitches.iter().map(|p| pitch_name(*p)).collect::<Vec<_>>().join(" "))
        };
        let accent = if e.accent { "!" } else { "" };
        let tie = if e.tie { "~" } else { "" };
        toks.push(format!("{name}{accent}{tie}:{}", fmt_dur(e.dur)));
        cursor = e.start + e.dur;
        i = j;
    }
    if doc.len - cursor > eps {
        toks.push(format!("_:{}", fmt_dur(doc.len - cursor)));
    }
    if toks.is_empty() {
        return Err(Diag::new("E-NOTE-006", p0, "notes リテラルが空になります"));
    }
    Ok(toks.join(" "))
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
