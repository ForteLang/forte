//! Recursive-descent parser for the v0 slice grammar:
//!
//! ```text
//! file   := song
//! song   := "song" STRING "{" { songItem } "}"
//! item   := "tempo" NUM["bpm"] | "swing" NUM | "meter" NUM "/" NUM | "key" IDENT IDENT
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

pub fn parse(src: &str) -> Result<FileAst, Vec<Diag>> {
    let toks = lex(src).map_err(|d| vec![d])?;
    let mut p = Parser { toks, i: 0, diags: Vec::new() };
    let mut imports = Vec::new();
    let mut assets = Vec::new();
    let mut devices = Vec::new();
    let mut blocks = Vec::new();
    loop {
        match p.peek().clone() {
            Tok::Ident(s) if s == "import" => match p.import() {
                Some(ImportKind::Module(im)) => imports.push(im),
                Some(ImportKind::Asset(a)) => assets.push(a),
                None => break,
            },
            Tok::Ident(s) if s == "device" => {
                if let Some(d) = p.device() {
                    devices.push(d);
                } else {
                    break;
                }
            }
            Tok::Ident(s) if s == "block" => {
                if let Some(b) = p.block_def() {
                    blocks.push(b);
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    // a file may be a pure device/block library (no song)
    let song = if *p.peek() == Tok::Eof { None } else { p.song() };
    if p.diags.is_empty() {
        Ok(FileAst { imports, assets, devices, blocks, song })
    } else {
        Err(p.diags)
    }
}

enum ImportKind {
    Module(ImportAst),
    Asset(AssetImportAst),
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
            self.err("E-PARSE-004", "ファイルは `song \"名前\" { … }` か `block 名前 { … }` で始めてください");
            return None;
        }
        let name = if let Tok::Str(s) = self.peek().clone() {
            self.bump();
            s
        } else {
            self.err("E-PARSE-005", "song の名前(文字列)が必要です");
            return None;
        };
        self.body(name)
    }

    /// `block Name { … }` — same body as a song; the name is an identifier
    /// so blocks can be imported and placed by name.
    fn block_def(&mut self) -> Option<BlockAst> {
        let pos = self.pos();
        self.bump(); // "block"
        let name = self.ident("block の名前")?;
        // `block Child : Parent { … }` — inherit and override
        let parent = if *self.peek() == Tok::Colon {
            self.bump();
            let ppos = self.pos();
            Some((self.ident("継承元 block の名前")?, ppos))
        } else {
            None
        };
        let body = self.body(name.clone())?;
        Some(BlockAst { name, parent, body, pos })
    }

    /// The shared body of `song`/`block`: header, lets, sections, tracks,
    /// returns, nested blocks, and block placements.
    fn body(&mut self, name: String) -> Option<SongAst> {
        self.expect(Tok::LBrace, "`{`");

        let mut song = SongAst {
            name,
            desc: None,
            tags: Vec::new(),
            license: None,
            version: None,
            requires: Vec::new(),
            artist: None,
            sponsor: None,
            place_autos: Vec::new(),
            params: Vec::new(),
            tempo: None,
            swing: None,
            meter: None,
            key: None,
            lets: Vec::new(),
            sections: Vec::new(),
            tracks: Vec::new(),
            returns: Vec::new(),
            blocks: Vec::new(),
            places: Vec::new(),
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
                    "desc" => {
                        self.bump();
                        if let Tok::Str(d) = self.peek().clone() {
                            self.bump();
                            song.desc = Some(d);
                        } else {
                            self.err("E-PARSE-023", "desc には文字列が必要です(例: desc \"A 4-bar acid line\")");
                        }
                    }
                    "tags" => {
                        self.bump();
                        if let Tok::Str(t) = self.peek().clone() {
                            self.bump();
                            song.tags = t
                                .split(',')
                                .map(|x| x.trim().to_string())
                                .filter(|x| !x.is_empty())
                                .collect();
                        } else {
                            self.err("E-PARSE-023", "tags には文字列が必要です(例: tags \"acid, bass, 303\")");
                        }
                    }
                    "license" => {
                        self.bump();
                        if let Tok::Str(l) = self.peek().clone() {
                            self.bump();
                            song.license = Some(l);
                        } else {
                            self.err("E-PARSE-023", "license には文字列が必要です(例: license \"CC-BY-NC-SA-4.0\")");
                        }
                    }
                    "version" => {
                        self.bump();
                        if let Tok::Str(v) = self.peek().clone() {
                            self.bump();
                            song.version = Some(v);
                        } else {
                            self.err("E-PARSE-023", "version には文字列が必要です(例: version \"0.6.0\")");
                        }
                    }
                    "requires" => {
                        self.bump();
                        if let Tok::Str(r) = self.peek().clone() {
                            self.bump();
                            song.requires.push(r);
                        } else {
                            self.err("E-PARSE-023", "requires には文字列が必要です(例: requires \"github:owner/repo@0.6.0\")");
                        }
                    }
                    "artist" => {
                        self.bump();
                        if let Tok::Str(a) = self.peek().clone() {
                            self.bump();
                            song.artist = Some(a);
                        } else {
                            self.err("E-PARSE-023", "artist には文字列が必要です(例: artist \"Forte Essentials\")");
                        }
                    }
                    "sponsor" => {
                        self.bump();
                        if let Tok::Str(u) = self.peek().clone() {
                            self.bump();
                            song.sponsor = Some(u);
                        } else {
                            self.err("E-PARSE-023", "sponsor には文字列が必要です(例: sponsor \"https://github.com/sponsors/you\")");
                        }
                    }
                    // the block's public knobs — device syntax:
                    //   param cutoff = 0.5 in 0..1
                    "param" => {
                        self.bump();
                        let ppos = self.pos();
                        if let Some(name) = self.ident("param の名前") {
                            self.expect(Tok::Eq, "`=`");
                            if let Some((default, _, _)) = self.number("param の既定値") {
                                let mut range = None;
                                if self.keyword("in") {
                                    let a = self.number("範囲の下限");
                                    self.expect(Tok::DotDot, "`..`");
                                    let b = self.number("範囲の上限");
                                    if let (Some(a), Some(b)) = (a, b) {
                                        range = Some((a.0, b.0));
                                    }
                                }
                                song.params.push(DevParam { name, default, range, pos: ppos });
                            }
                        }
                    }
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
                    "swing" => {
                        self.bump();
                        if let Some((n, _unit, pos)) = self.number("swing") {
                            song.swing = Some((n, pos));
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
                    "section" => {
                        self.bump();
                        let pos = self.pos();
                        let name = self.ident("section の名前")?;
                        self.expect(Tok::Eq, "`=`");
                        if !self.keyword("bars") {
                            self.err("E-PARSE-013", "section は `= bars(a..b)` で定義します");
                        }
                        self.expect(Tok::LParen, "`(`");
                        let a = self.number("開始小節")?;
                        self.expect(Tok::DotDot, "`..`");
                        let b = self.number("終了小節")?;
                        self.expect(Tok::RParen, "`)`");
                        song.sections.push(SectionAst { name, bars: (a.0 as u32, b.0 as u32), pos });
                    }
                    "track" => {
                        let t = self.track()?;
                        song.tracks.push(t);
                    }
                    "return" => {
                        let r = self.return_block()?;
                        song.returns.push(r);
                    }
                    "block" => {
                        let b = self.block_def()?;
                        song.blocks.push(b);
                    }
                    // body-level play = a block placement:
                    //   play Groove(key: "E minor", from: 2) at bars(9..16)
                    "play" => {
                        self.bump();
                        let pos = self.pos();
                        let block = self.ident("配置する block の名前")?;
                        let mut key = None;
                        let mut from = None;
                        let mut to = None;
                        let mut volume = None;
                        let mut swing = None;
                        let mut stretch = None;
                        let mut params = Vec::new();
                        if *self.peek() == Tok::LParen {
                            self.bump();
                            loop {
                                if *self.peek() == Tok::RParen {
                                    self.bump();
                                    break;
                                }
                                let apos = self.pos();
                                let arg = self.ident("配置引数(key / from / to / volume)")?;
                                self.expect(Tok::Colon, "`:`");
                                match arg.as_str() {
                                    "key" => {
                                        if let Tok::Str(s) = self.peek().clone() {
                                            self.bump();
                                            let mut it = s.split_whitespace();
                                            match (it.next(), it.next(), it.next()) {
                                                (Some(r), Some(sc), None) => {
                                                    key = Some(((r.to_string(), sc.to_string()), apos));
                                                }
                                                _ => self.err(
                                                    "E-PARSE-022",
                                                    format!("key は \"ルート スケール\" で書きます(例: \"E minor\"。見つかったのは \"{s}\")"),
                                                ),
                                            }
                                        } else {
                                            self.err("E-PARSE-022", "key: には文字列が必要です(例: key: \"E minor\")");
                                        }
                                    }
                                    "from" => {
                                        let n = self.number("from(小節)")?;
                                        from = Some(n.0 as u32);
                                    }
                                    "to" => {
                                        let n = self.number("to(小節)")?;
                                        to = Some(n.0 as u32);
                                    }
                                    "volume" => {
                                        let n = self.number("volume(0..1)")?;
                                        volume = Some((n.0, apos));
                                    }
                                    "swing" => {
                                        let n = self.number("swing(0.5..0.8)")?;
                                        swing = Some((n.0, apos));
                                    }
                                    "stretch" => {
                                        let n = self.number("stretch(倍率)")?;
                                        stretch = Some((n.0, apos));
                                    }
                                    // anything else is a block param value:
                                    //   play Riff(cutoff: 0.7)
                                    other => {
                                        let n = self.number(&format!("{other}(param 値)"))?;
                                        params.push((other.to_string(), n.0, apos));
                                    }
                                }
                                if *self.peek() == Tok::Comma {
                                    self.bump();
                                }
                            }
                        }
                        // `as Alias` — share one lane across variant placements
                        let alias = if self.keyword("as") {
                            self.ident("配置インスタンス名(as の後)")
                        } else {
                            None
                        };
                        if !self.keyword("at") {
                            self.err("E-PARSE-022", "block の配置には `at bars(a..b)` か `at セクション名` が必要です");
                        }
                        let at = if self.keyword("bars") {
                            self.expect(Tok::LParen, "`(`");
                            let a = self.number("開始小節")?;
                            self.expect(Tok::DotDot, "`..`");
                            let b = self.number("終了小節")?;
                            self.expect(Tok::RParen, "`)`");
                            AtRef::Bars(a.0 as u32, b.0 as u32)
                        } else {
                            let spos = self.pos();
                            let name = self.ident("区間(bars(a..b) かセクション名)")?;
                            AtRef::Section(name, spos)
                        };
                        song.places.push(PlaceAst {
                            block,
                            key,
                            from,
                            to,
                            volume,
                            swing,
                            stretch,
                            alias,
                            params,
                            at,
                            pos,
                        });
                    }
                    // body-level automate: fade a placed block instance
                    //   automate Riff.volume from 0 to 1 over intro
                    "automate" => {
                        self.bump();
                        let apos = self.pos();
                        let target = self.param_target("automate 対象(<配置名>.volume)")?;
                        if !self.keyword("from") {
                            self.err("E-PARSE-020", "automate は `from A to B over 区間` で書きます");
                        }
                        let from = self.number("開始値")?;
                        if !self.keyword("to") {
                            self.err("E-PARSE-020", "`to` が必要です");
                        }
                        let to = self.number("終了値")?;
                        if !self.keyword("over") {
                            self.err("E-PARSE-020", "`over bars(a..b)` か `over セクション名` が必要です");
                        }
                        let at = if self.keyword("bars") {
                            self.expect(Tok::LParen, "`(`");
                            let a = self.number("開始小節")?;
                            self.expect(Tok::DotDot, "`..`");
                            let b = self.number("終了小節")?;
                            self.expect(Tok::RParen, "`)`");
                            AtRef::Bars(a.0 as u32, b.0 as u32)
                        } else {
                            let spos = self.pos();
                            let name = self.ident("区間(bars(a..b) かセクション名)")?;
                            AtRef::Section(name, spos)
                        };
                        song.place_autos.push(AutomateAst {
                            target,
                            from: from.0,
                            to: to.0,
                            at,
                            pos: apos,
                        });
                    }
                    other => {
                        self.err(
                            "E-PARSE-007",
                            format!("song/block 内で使えない要素です: {other}(tempo/meter/key/let/section/track/return/block/play/automate)"),
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
        let kind = self.ident("音楽リテラルの種類(beat / notes / prog)")?;
        if kind != "beat" && kind != "notes" && kind != "prog" {
            self.diags.push(Diag::new(
                "E-PARSE-009",
                pos,
                format!("音楽リテラルは beat`…` / notes`…` / prog`…` です(見つかったのは {kind})"),
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
            sends: Vec::new(),
            audios: Vec::new(),
            automations: Vec::new(),
            modulations: Vec::new(),
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
                        let pattern = self.pattern_expr()?;
                        let at = self.at_ref("play")?;
                        t.plays.push(PlayAst { pattern, at, pos: ppos });
                    }
                    "audio" => {
                        self.bump();
                        let apos = self.pos();
                        let name = self.ident("録音アセット名(import した名前)")?;
                        let at = self.at_ref("audio")?;
                        t.audios.push(AudioPlayAst { name, at, pos: apos });
                    }
                    "send" => {
                        self.bump();
                        let spos = self.pos();
                        let dest = self.ident("send 先(return の名前)")?;
                        if let Some((level, _, _)) = self.number("send レベル") {
                            t.sends.push((dest, level, spos));
                        }
                    }
                    "automate" => {
                        self.bump();
                        let apos = self.pos();
                        let target = self.param_target("automate 対象(volume / パラメータ / insert名.パラメータ)")?;
                        if !self.keyword("from") {
                            self.err("E-PARSE-020", "automate は `from A to B over 区間` で書きます");
                        }
                        let from = self.number("開始値")?;
                        if !self.keyword("to") {
                            self.err("E-PARSE-020", "`to` が必要です");
                        }
                        let to = self.number("終了値")?;
                        if !self.keyword("over") {
                            self.err("E-PARSE-020", "`over bars(a..b)` か `over セクション名` が必要です");
                        }
                        let at = if self.keyword("bars") {
                            self.expect(Tok::LParen, "`(`");
                            let a = self.number("開始小節")?;
                            self.expect(Tok::DotDot, "`..`");
                            let b = self.number("終了小節")?;
                            self.expect(Tok::RParen, "`)`");
                            AtRef::Bars(a.0 as u32, b.0 as u32)
                        } else {
                            let spos = self.pos();
                            let name = self.ident("区間(bars(a..b) かセクション名)")?;
                            AtRef::Section(name, spos)
                        };
                        t.automations.push(AutomateAst { target, from: from.0, to: to.0, at, pos: apos });
                    }
                    "modulate" => {
                        self.bump();
                        let mpos = self.pos();
                        let param = self.param_target("modulate 対象のパラメータ名(または insert名.パラメータ)")?;
                        if !self.keyword("with") {
                            self.err("E-PARSE-021", "modulate は `with lfo(rate: …, amount: …)` で書きます");
                        }
                        let call = self.call()?;
                        if !matches!(call.name.as_str(), "lfo" | "steps" | "random" | "adsr") {
                            self.err(
                                "E-PARSE-021",
                                format!(
                                    "modulate に使えるのは lfo / steps / random / adsr です(見つかったのは {})",
                                    call.name
                                ),
                            );
                        }
                        t.modulations.push(ModulateAst {
                            param,
                            kind: call.name.clone(),
                            args: call.args,
                            pos: mpos,
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
                                "track 内で使えない要素です: {other}(instrument/insert/play/send/volume/pan)"
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

    /// `import { A, B } from "./lib.forte"` or `import take from "./t.frec"`
    fn import(&mut self) -> Option<ImportKind> {
        let pos = self.pos();
        self.bump(); // "import"
        // default import = recorded asset
        if let Tok::Ident(name) = self.peek().clone() {
            self.bump();
            if !self.keyword("from") {
                self.err("E-PARSE-019", "import には from \"パス\" が必要です");
            }
            let Tok::Str(path) = self.peek().clone() else {
                self.err("E-PARSE-019", "import 元のパス(文字列)が必要です");
                return None;
            };
            self.bump();
            if !path.ends_with(".frec") {
                self.err(
                    "E-PROV-002",
                    "デフォルト import は録音アセット(.frec)専用です(モジュールは import { 名前 } from …)",
                );
            }
            return Some(ImportKind::Asset(AssetImportAst { name, path, pos }));
        }
        self.expect(Tok::LBrace, "`{`(import { 名前, … } from \"…\")");
        let mut names = Vec::new();
        loop {
            match self.peek().clone() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Ident(n) => {
                    self.bump();
                    names.push(n);
                    if *self.peek() == Tok::Comma {
                        self.bump();
                    }
                }
                _ => {
                    self.err("E-PARSE-019", "import する名前が必要です");
                    return None;
                }
            }
        }
        if !self.keyword("from") {
            self.err("E-PARSE-019", "import には from \"パス\" が必要です");
        }
        let path = if let Tok::Str(s) = self.peek().clone() {
            self.bump();
            s
        } else {
            self.err("E-PARSE-019", "import 元のパス(文字列)が必要です");
            return None;
        };
        Some(ImportKind::Module(ImportAst { names, path, pos }))
    }

    /// `device Name : Instrument { param … / node … / out … }`
    fn device(&mut self) -> Option<DeviceAst> {
        let pos = self.pos();
        self.bump(); // "device"
        let name = self.ident("device の名前")?;
        let mut dkind = "Instrument".to_string();
        if *self.peek() == Tok::Colon {
            self.bump();
            let kind = self.ident("device の種類")?;
            if kind != "Instrument" && kind != "Effect" {
                self.err("E-GRID-005", format!("device は Instrument か Effect です(見つかったのは {kind})"));
            } else {
                dkind = kind;
            }
        }
        self.expect(Tok::LBrace, "`{`");
        let mut d = DeviceAst {
            name,
            pos,
            kind: dkind,
            params: Vec::new(),
            takes: Vec::new(),
            nodes: Vec::new(),
            out: None,
        };
        loop {
            match self.peek().clone() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Eof => {
                    self.err("E-PARSE-006", "device ブロックが閉じていません(`}` が必要)");
                    break;
                }
                Tok::Ident(kw) => match kw.as_str() {
                    "param" => {
                        self.bump();
                        let ppos = self.pos();
                        let name = self.ident("param の名前")?;
                        self.expect(Tok::Eq, "`=`");
                        let (default, _, _) = self.number("param の既定値")?;
                        let mut range = None;
                        if self.keyword("in") {
                            let a = self.number("範囲の下限")?;
                            self.expect(Tok::DotDot, "`..`");
                            let b = self.number("範囲の上限")?;
                            range = Some((a.0, b.0));
                        }
                        d.params.push(DevParam { name, default, range, pos: ppos });
                    }
                    "take" => {
                        self.bump();
                        let tpos = self.pos();
                        let name = self.ident("take の名前")?;
                        d.takes.push((name, tpos));
                    }
                    "node" => {
                        self.bump();
                        let npos = self.pos();
                        let name = self.ident("node の名前")?;
                        self.expect(Tok::Eq, "`=`");
                        let expr = self.node_expr()?;
                        d.nodes.push((name, expr, npos));
                    }
                    "out" => {
                        self.bump();
                        let expr = self.node_expr()?;
                        if d.out.is_some() {
                            self.err("E-GRID-006", "out は device に 1 つだけです");
                        }
                        d.out = Some(expr);
                    }
                    other => {
                        self.err(
                            "E-PARSE-018",
                            format!("device 内で使えない要素です: {other}(param/take/node/out)"),
                        );
                        self.bump();
                    }
                },
                _ => {
                    self.err("E-PARSE-008", "device 内で解釈できないトークンです");
                    self.bump();
                }
            }
        }
        Some(d)
    }

    /// An automate/modulate target: `cutoff` or `delay.mix` (insert.param),
    /// returned as one dotted string for the compiler to resolve.
    fn param_target(&mut self, what: &str) -> Option<String> {
        let head = self.ident(what)?;
        if *self.peek() == Tok::Dot {
            self.bump();
            let tail = self.ident("パラメータ名(insert名. の後)")?;
            return Some(format!("{head}.{tail}"));
        }
        Some(head)
    }

    /// DSP expression: `osc(shape: "saw")` / `note.freq` / a node or param name.
    fn node_expr(&mut self) -> Option<NodeExpr> {
        let pos = self.pos();
        let name = self.ident("DSP 式(osc()/svf()/… か node 名)")?;
        if name == "note" && *self.peek() == Tok::Dot {
            self.bump();
            let port = self.ident("note のポート(freq/gate/vel)")?;
            return Some(NodeExpr::NotePort(port, pos));
        }
        if name == "audio" && *self.peek() == Tok::Dot {
            self.bump();
            let port = self.ident("audio のポート(in)")?;
            if port != "in" {
                self.err("E-GRID-003", format!("audio.{port} はありません(audio.in のみ)"));
            }
            return Some(NodeExpr::AudioIn(pos));
        }
        if *self.peek() == Tok::LParen {
            self.bump();
            let mut args = Vec::new();
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
                                NodeArg::Str(s, apos)
                            }
                            Tok::Num(..) | Tok::Minus => {
                                let (n, _, p) = self.number("引数の値")?;
                                NodeArg::Num(n, p)
                            }
                            _ => NodeArg::Expr(self.node_expr()?),
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
            return Some(NodeExpr::Call { name, args, pos });
        }
        Some(NodeExpr::Ref(name, pos))
    }

    /// `at bars(a..b)` or `at <section-name>`.
    fn at_ref(&mut self, what: &str) -> Option<AtRef> {
        if !self.keyword("at") {
            self.err("E-PARSE-012", format!("{what} には `at bars(a..b)` か `at セクション名` が必要です"));
        }
        if self.keyword("bars") {
            self.expect(Tok::LParen, "`(`");
            let a = self.number("開始小節")?;
            self.expect(Tok::DotDot, "`..`");
            let b = self.number("終了小節")?;
            self.expect(Tok::RParen, "`)`");
            Some(AtRef::Bars(a.0 as u32, b.0 as u32))
        } else {
            let spos = self.pos();
            let name = self.ident("配置先(bars(a..b) かセクション名)")?;
            Some(AtRef::Section(name, spos))
        }
    }

    /// Pattern expression: a literal, a `let` name, or a pattern function
    /// `chords(x)` / `arp(x, rate: 0.25, style: "up")` / `bass(x, rate: 0.5)`.
    fn pattern_expr(&mut self) -> Option<PatternRef> {
        if let Tok::Ident(id) = self.peek().clone() {
            if id == "beat" || id == "notes" || id == "prog" {
                return Some(PatternRef::Lit(self.music_lit()?));
            }
            let pos = self.pos();
            self.bump();
            if *self.peek() == Tok::LParen {
                self.bump();
                let inner = self.pattern_expr()?;
                let mut args = Vec::new();
                loop {
                    match self.peek().clone() {
                        Tok::RParen => {
                            self.bump();
                            break;
                        }
                        Tok::Comma => {
                            self.bump();
                            let key = self.ident("引数名")?;
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
                        }
                        _ => {
                            self.err("E-PARSE-015", "引数は `, 名前: 値` の形で書いてください");
                            self.bump();
                        }
                    }
                }
                return Some(PatternRef::Fn { name: id, inner: Box::new(inner), args, pos });
            }
            return Some(PatternRef::Name(id, pos));
        }
        self.err("E-PARSE-016", "パターン(名前・リテラル・chords()/arp()/bass())が必要です");
        None
    }

    /// `return Name { insert … / volume / pan }` — an effect return track.
    fn return_block(&mut self) -> Option<ReturnAst> {
        let pos = self.pos();
        self.bump(); // "return"
        let name = self.ident("return の名前")?;
        self.expect(Tok::LBrace, "`{`");
        let mut r = ReturnAst { name, pos, inserts: Vec::new(), volume: None, pan: None };
        loop {
            match self.peek().clone() {
                Tok::RBrace => {
                    self.bump();
                    break;
                }
                Tok::Eof => {
                    self.err("E-PARSE-006", "return ブロックが閉じていません(`}` が必要)");
                    break;
                }
                Tok::Ident(kw) => match kw.as_str() {
                    "insert" => {
                        self.bump();
                        let c = self.call()?;
                        r.inserts.push(c);
                    }
                    "volume" => {
                        self.bump();
                        if let Some((n, _, p)) = self.number("volume") {
                            r.volume = Some((n, p));
                        }
                    }
                    "pan" => {
                        self.bump();
                        if let Some((n, _, p)) = self.number("pan") {
                            r.pan = Some((n, p));
                        }
                    }
                    other => {
                        self.err(
                            "E-PARSE-017",
                            format!("return 内で使えない要素です: {other}(insert/volume/pan)"),
                        );
                        self.bump();
                    }
                },
                _ => {
                    self.err("E-PARSE-008", "return 内で解釈できないトークンです");
                    self.bump();
                }
            }
        }
        Some(r)
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
                            Tok::Ident(name) => {
                                self.bump();
                                Arg::Ident(name, apos)
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
