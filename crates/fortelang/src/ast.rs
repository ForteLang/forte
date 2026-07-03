//! AST for the Forte v0 slice: one `song` per file, tracks, lets, plays.

use crate::diag::Pos;

/// A whole `.forte` file: imports, device definitions, then (optionally) one
/// song. A file without a song is a device library, importable from songs.
#[derive(Clone, Debug)]
pub struct FileAst {
    pub imports: Vec<ImportAst>,
    pub devices: Vec<DeviceAst>,
    pub song: Option<SongAst>,
}

/// `import { WarmLead, SubBass } from "./devices/warm.forte"`
#[derive(Clone, Debug)]
pub struct ImportAst {
    pub names: Vec<String>,
    pub path: String,
    pub pos: Pos,
}

/// `device WarmLead : Instrument { param … / node … / out … }` — a synth
/// defined in the language itself; lowered to a Grid node graph.
#[derive(Clone, Debug)]
pub struct DeviceAst {
    pub name: String,
    pub pos: Pos,
    pub params: Vec<DevParam>,
    pub nodes: Vec<(String, NodeExpr, Pos)>,
    pub out: Option<NodeExpr>,
}

/// `param cutoff = 0.65 in 0.0..1.0`
#[derive(Clone, Debug)]
pub struct DevParam {
    pub name: String,
    pub default: f64,
    pub range: Option<(f64, f64)>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub enum NodeExpr {
    /// DSP primitive: `osc(shape: "saw")`, `svf(in: o, cutoff: cutoff)` …
    Call { name: String, args: Vec<(String, NodeArg)>, pos: Pos },
    /// A previously declared `node` name, or (in numeric positions) a `param`.
    Ref(String, Pos),
    /// `note.freq` / `note.gate` / `note.vel`
    NotePort(String, Pos),
}

#[derive(Clone, Debug)]
pub enum NodeArg {
    Num(f64, Pos),
    Str(String, Pos),
    Expr(NodeExpr),
}

#[derive(Clone, Debug)]
pub struct SongAst {
    pub name: String,
    pub tempo: Option<(f64, Pos)>,
    pub meter: Option<((u32, u32), Pos)>,
    pub key: Option<((String, String), Pos)>, // (root, scale) as written
    pub lets: Vec<LetAst>,
    pub sections: Vec<SectionAst>,
    pub tracks: Vec<TrackAst>,
    pub returns: Vec<ReturnAst>,
}

/// `section verse = bars(1..8)` — a named, reusable bar range.
#[derive(Clone, Debug)]
pub struct SectionAst {
    pub name: String,
    pub bars: (u32, u32),
    pub pos: Pos,
}

/// `return Space { insert reverb(...) }` — an effect (return) track fed by
/// post-fader sends.
#[derive(Clone, Debug)]
pub struct ReturnAst {
    pub name: String,
    pub pos: Pos,
    pub inserts: Vec<Call>,
    pub volume: Option<(f64, Pos)>,
    pub pan: Option<(f64, Pos)>,
}

#[derive(Clone, Debug)]
pub struct LetAst {
    pub name: String,
    pub value: PatternLit,
    pub pos: Pos,
}

/// A music literal: the kind ident (`beat` / `notes`) plus raw contents.
#[derive(Clone, Debug)]
pub struct PatternLit {
    pub kind: String,
    pub raw: String,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct TrackAst {
    pub name: String,
    pub pos: Pos,
    pub instrument: Option<Call>,
    pub inserts: Vec<Call>,
    pub plays: Vec<PlayAst>,
    pub volume: Option<(f64, Pos)>,
    pub pan: Option<(f64, Pos)>,
    /// `send Space 0.3` — (return name, level).
    pub sends: Vec<(String, f64, Pos)>,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub name: String,
    pub args: Vec<(String, Arg)>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub enum Arg {
    Num(f64, Pos),
    Str(String, Pos),
}

#[derive(Clone, Debug)]
pub struct PlayAst {
    pub pattern: PatternRef,
    pub at: AtRef,
    pub pos: Pos,
}

/// Where a play is placed: an explicit bar range or a named section.
#[derive(Clone, Debug)]
pub enum AtRef {
    /// Inclusive 1-based bar range from `bars(a..b)`.
    Bars(u32, u32),
    Section(String, Pos),
}

#[derive(Clone, Debug)]
pub enum PatternRef {
    Name(String, Pos),
    Lit(PatternLit),
    /// Pattern function: `chords(x)`, `arp(x, rate: 0.25, style: "up")`,
    /// `bass(x, rate: 0.5)`.
    Fn { name: String, inner: Box<PatternRef>, args: Vec<(String, Arg)>, pos: Pos },
}
