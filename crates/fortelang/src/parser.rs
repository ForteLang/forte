//! Recursive-descent parser for the v0 slice grammar:
//!
//! ```text
//! file   := song
//! song   := "song" STRING "{" { songItem } "}"
//! item   := "tempo" NUM["bpm"] | "meter" NUM "/" NUM | "key" IDENT IDENT
//!         | "let" IDENT "=" musicLit | track
//! track  := "track" IDENT "{" { trackItem } "}"
//! titem  := "instrument" call | "insert" call
//!         | "play" (IDENT | musicLit) "at" "bars" "(" NUM ".." NUM ")"
//!         | "volume" num | "pan" num
//! call   := IDENT "(" [ IDENT ":" (num | STRING) {"," ...} ] ")"
//! musicLit := ("beat"|"notes") BACKTICK
//! num    := ["-"] NUM
//! ```

use crate::ast::*;
use crate::diag::{Diag, Pos};
use crate::lexer::{lex, Spanned, Tok};

pub fn parse(src: &str) -> Result<SongAst, Vec<Diag>> {
    let toks = lex(src).map_err(|d| vec![d])?;
    let mut p = Parser { toks, i: 0, diags: Vec::new() };
    match p.song() {
        Some(song) if p.diags.is_empty() => Ok(song),
        _ => Err(p.diags),
    }
}

struct Parser {
    toks: Vec<Spanned>,
    i: usize,
    diags: Vec<Diag>,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i].tok
    }
    fn pos(&self) -> Pos {
        self.toks[self.i].pos
    }
    fn bump(&mut self) -> Tok {
        let t = self.toks[self.i].tok.clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }
    fn err(&mut self, code: &'static str, msg: impl Into<String>) {
        let pos = self.pos();
        self.diags.push(Diag::new(code, pos, msg));
    }
    fn expect(&mut self, want: Tok, what: &str) -> bool {
        if *self.peek() == want {
            self.bump();
            true
        } else {
            self.err("E-PARSE-001", format!("{what} が必要です(見つかったのは {:?})", self.peek()));
            false
        }
    }
    fn ident(&mut self, what: &str) -> Option<String> {
        if let Tok::Ident(s) = self.peek().clone() {
            self.bump();
            Some(s)
        } else {
            self.err("E-PARSE-002", format!("{what} が必要です"));
            None
        }
    }
    fn keyword(&mut self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == kw) && {
            self.bump();
            true
        }
    }
    /// Signed plain number; unit suffix optional and returned.
    fn number(&mut self, what: &str) -> Option<(f64, Option<String>, Pos)> {
        let pos = self.pos();
        let neg = if *self.peek() == Tok::Minus {
            self.bump();
            true
        } else {
            false
        };
        if let Tok::Num(n, u) = self.peek().clone() {
            self.bump();
            Some((if neg { -n } else { n }, u, pos))
        } else {
            self.err("E-PARSE-003", format!("{what}(数値)が必要です"));
            None
        }
    }

    fn song(&mut self) -> Option<SongAst> {
        if !self.keyword("song") {
            self.err("E-PARSE-004", "ファイルは `song \"名前\" { … }` で始めてください");
            return None;
        }
        let name = if let Tok::Str(s) = self.peek().clone() {
            self.bump();
            s
        } else {
            self.err("E-PARSE-005", "song の名前(文字列)が必要です");
            return None;
        };
        self.expect(Tok::LBrace, "`{`");

        let mut song = SongAst {
            name,
            tempo: None,
            meter: None,
            key: None,
            lets: Vec::new(),
            tracks: Vec::new(),
        };

        loop {
            match self.peek().clone() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Eof => {
                    self.err("E-PARSE-006", "song ブロックが閉じていません(`}` が必要)");
                    break;
                }
                Tok::Ident(kw) => match kw.as_str() {
                    "tempo" => {
                        self.bump();
                        if let Some((n, unit, pos)) = self.number("tempo") {
                            if let Some(u) = &unit {
                                if u != "bpm" {
                                    self.diags.push(Diag::new(
                                        "E-TYPE-001",
                                        pos,
                                        format!("tempo の単位は bpm です(見つかったのは {u})"),
                                    ));
                                }
                            }
                            song.tempo = Some((n, pos));
                        }
                    }
                    "meter" => {
                        self.bump();
                        let pos = self.pos();
                        let num = self.number("拍子の分子")?;
                        self.expect(Tok::Slash, "拍子の `/`");
                        let den = self.number("拍子の分母")?;
                        song.meter = Some(((num.0 as u32, den.0 as u32), pos));
                    }
                    "key" => {
                        self.bump();
                        let pos = self.pos();
                        let root = self.ident("キーのルート音(例: D)")?;
                        let scale = self.ident("スケール名(例: minor)")?;
                        song.key = Some(((root, scale), pos));
                    }
                    "let" => {
                        self.bump();
                        let pos = self.pos();
                        let name = self.ident("let の名前")?;
                        self.expect(Tok::Eq, "`=`");
                        let lit = self.music_lit()?;
                        song.lets.push(LetAst { name, value: lit, pos });
                    }
                    "track" => {
                        let t = self.track()?;
                        song.tracks.push(t);
                    }
                    other => {
                        self.err(
                            "E-PARSE-007",
                            format!("song 内で使えない要素です: {other}(tempo/meter/key/let/track)"),
                        );
                        self.bump();
                    }
                },
                _ => {
                    self.err("E-PARSE-008", "song 内で解釈できないトークンです");
                    self.bump();
                }
            }
        }
        Some(song)
    }

    fn music_lit(&mut self) -> Option<PatternLit> {
        let pos = self.pos();
        let kind = self.ident("音楽リテラルの種類(beat / notes)")?;
        if kind != "beat" && kind != "notes" {
            self.diags.push(Diag::new(
                "E-PARSE-009",
                pos,
                format!("音楽リテラルは beat`…` か notes`…` です(見つかったのは {kind})"),
            ));
        }
        if let Tok::Backtick(raw) = self.peek().clone() {
            self.bump();
            Some(PatternLit { kind, raw, pos })
        } else {
            self.err("E-PARSE-010", "バッククォート(`…`)のリテラル本体が必要です");
            None
        }
    }

    fn track(&mut self) -> Option<TrackAst> {
        let pos = self.pos();
        self.bump(); // "track"
        let name = self.ident("track の名前")?;
        self.expect(Tok::LBrace, "`{`");
        let mut t = TrackAst {
            name,
            pos,
            instrument: None,
            inserts: Vec::new(),
            plays: Vec::new(),
            volume: None,
            pan: None,
        };
        loop {
            match self.peek().clone() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Eof => {
                    self.err("E-PARSE-006", "track ブロックが閉じていません(`}` が必要)");
                    break;
                }
                Tok::Ident(kw) => match kw.as_str() {
                    "instrument" => {
                        self.bump();
                        let c = self.call()?;
                        if t.instrument.is_some() {
                            self.diags.push(Diag::new(
                                "E-PARSE-011",
                                c.pos,
                                format!("Track '{}' に instrument が 2 つあります", t.name),
                            ));
                        }
                        t.instrument = Some(c);
                    }
                    "insert" => {
                        self.bump();
                        let c = self.call()?;
                        t.inserts.push(c);
                    }
                    "play" => {
                        self.bump();
                        let ppos = self.pos();
                        let pattern = match self.peek().clone() {
                            Tok::Ident(id) if id != "beat" && id != "notes" => {
                                let p = self.pos();
                                self.bump();
                                PatternRef::Name(id, p)
                            }
                            _ => PatternRef::Lit(self.music_lit()?),
                        };
                        if !self.keyword("at") {
                            self.err("E-PARSE-012", "play には `at bars(a..b)` が必要です");
                        }
                        if !self.keyword("bars") {
                            self.err("E-PARSE-013", "v0 では配置は bars(a..b) のみ対応です");
                        }
                        self.expect(Tok::LParen, "`(`");
                        let a = self.number("開始小節")?;
                        self.expect(Tok::DotDot, "`..`");
                        let b = self.number("終了小節")?;
                        self.expect(Tok::RParen, "`)`");
                        t.plays.push(PlayAst {
                            pattern,
                            bars: (a.0 as u32, b.0 as u32),
                            pos: ppos,
                        });
                    }
                    "volume" => {
                        self.bump();
                        if let Some((n, _, p)) = self.number("volume") {
                            t.volume = Some((n, p));
                        }
                    }
                    "pan" => {
                        self.bump();
                        if let Some((n, _, p)) = self.number("pan") {
                            t.pan = Some((n, p));
                        }
                    }
                    other => {
                        self.err(
                            "E-PARSE-014",
                            format!(
                                "track 内で使えない要素です: {other}(instrument/insert/play/volume/pan)"
                            ),
                        );
                        self.bump();
                    }
                },
                _ => {
                    self.err("E-PARSE-008", "track 内で解釈できないトークンです");
                    self.bump();
                }
            }
        }
        Some(t)
    }

    fn call(&mut self) -> Option<Call> {
        let pos = self.pos();
        let name = self.ident("デバイス名")?;
        let mut args = Vec::new();
        if *self.peek() == Tok::LParen {
            self.bump();
            loop {
                match self.peek().clone() {
                    Tok::RParen => {
                        self.bump();
                        break;
                    }
                    Tok::Ident(key) => {
                        self.bump();
                        self.expect(Tok::Colon, "引数の `:`");
                        let apos = self.pos();
                        let arg = match self.peek().clone() {
                            Tok::Str(s) => {
                                self.bump();
                                Arg::Str(s, apos)
                            }
                            _ => {
                                let (n, _unit, p) = self.number("引数の値")?;
                                Arg::Num(n, p)
                            }
                        };
                        args.push((key, arg));
                        if *self.peek() == Tok::Comma {
                            self.bump();
                        }
                    }
                    _ => {
                        self.err("E-PARSE-015", "引数は `名前: 値` の形で書いてください");
                        self.bump();
                    }
                }
            }
        }
        Some(Call { name, args, pos })
    }
}
