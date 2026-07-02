//! AST for the Forte v0 slice: one `song` per file, tracks, lets, plays.

use crate::diag::Pos;

#[derive(Clone, Debug)]
pub struct SongAst {
    pub name: String,
    pub tempo: Option<(f64, Pos)>,
    pub meter: Option<((u32, u32), Pos)>,
    pub key: Option<((String, String), Pos)>, // (root, scale) as written
    pub lets: Vec<LetAst>,
    pub tracks: Vec<TrackAst>,
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
    /// Inclusive 1-based bar range from `bars(a..b)`.
    pub bars: (u32, u32),
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub enum PatternRef {
    Name(String, Pos),
    Lit(PatternLit),
}
