//! AST for the Forte v0 slice: one `song` per file, tracks, lets, plays.

use crate::diag::Pos;

/// A whole `.forte` file: imports, device definitions, then (optionally) one
/// song. A file without a song is a device library, importable from songs.
#[derive(Clone, Debug)]
pub struct FileAst {
    pub imports: Vec<ImportAst>,
    pub assets: Vec<AssetImportAst>,
    pub devices: Vec<DeviceAst>,
    /// Top-level `block Name { … }` definitions. A file of blocks is a block
    /// library; `forte build` on such a file builds the LAST block as root.
    pub blocks: Vec<BlockAst>,
    pub song: Option<SongAst>,
}

/// `block Groove { … }` — a self-contained piece of music, the universal
/// composition unit. A song is just the outermost block; blocks nest via
/// placements, and the settings of the block ABOVE always win (the root's
/// tempo/key govern the render; a block's own key is the reference its
/// transposition is computed from).
#[derive(Clone, Debug)]
pub struct BlockAst {
    pub name: String,
    /// `block Child : Parent { … }` — OOP-style inheritance: the child
    /// starts from the parent's body and overrides (instrument swaps,
    /// insert param changes, added effects, replaced patterns).
    pub parent: Option<(String, Pos)>,
    pub body: SongAst,
    pub pos: Pos,
}

/// `play BlockName(key: "E minor", from: 2, to: 5) at bars(9..16)` — place a
/// block on this body's timeline. `key` transposes (relative to the placed
/// block's own key), `from`/`to` pick a bar window inside the block, and the
/// content loops when the placement span is longer than the block.
#[derive(Clone, Debug)]
pub struct PlaceAst {
    pub block: String,
    pub key: Option<((String, String), Pos)>,
    pub from: Option<u32>,
    pub to: Option<u32>,
    /// `volume: 0.6` — scales the whole instance (every track's fader)
    /// across this placement's span only.
    pub volume: Option<(f64, Pos)>,
    /// `cutoff: 0.7` — values for the block's declared `param`s.
    pub params: Vec<(String, f64, Pos)>,
    /// `swing: 0.66` — local swing for this instance's subtree (grid 16ths).
    pub swing: Option<(f64, Pos)>,
    /// `stretch: 2` — scale the block's time: 2 = half-time (beats double),
    /// 0.5 = double-time. Windows/loops apply AFTER the stretch.
    pub stretch: Option<(f64, Pos)>,
    pub at: AtRef,
    pub pos: Pos,
}

/// `import { WarmLead, SubBass } from "./devices/warm.forte"`
#[derive(Clone, Debug)]
pub struct ImportAst {
    pub names: Vec<String>,
    pub path: String,
    pub pos: Pos,
}

/// `import take from "./take1.frec"` — a recorded-audio asset (provenance
/// is validated when the bytes are loaded).
#[derive(Clone, Debug)]
pub struct AssetImportAst {
    pub name: String,
    pub path: String,
    pub pos: Pos,
}

/// `device WarmLead : Instrument { param … / node … / out … }` — a synth (or
/// with `: Effect`, an audio effect) defined in the language itself; lowered
/// to a Grid node graph.
#[derive(Clone, Debug)]
pub struct DeviceAst {
    pub name: String,
    pub pos: Pos,
    /// "Instrument" | "Effect"
    pub kind: String,
    pub params: Vec<DevParam>,
    /// `take voice` — recorded-audio slots the caller binds with an imported
    /// take (`instrument MyVox(voice: myTake)`); used by `sample()` nodes.
    pub takes: Vec<(String, Pos)>,
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
    /// `audio.in` — the incoming signal (Effect devices only)
    AudioIn(Pos),
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
    /// `desc "…"` — one-line description shown by forte play / catalogs.
    pub desc: Option<String>,
    /// `tags "acid, bass, 303"` — search keywords for packages/browsing.
    pub tags: Vec<String>,
    /// `license "CC-BY-NC-SA-4.0"` — the content license this body is
    /// published under (packages declare it; players display it).
    pub license: Option<String>,
    /// `version "0.6.0"` — the package version (used in the vendored
    /// directory name `<name>_<version>`).
    pub version: Option<String>,
    /// `requires "github:owner/repo@ref"` — package dependencies, resolved
    /// FLAT into the consumer's packages/ by `forte package add`.
    pub requires: Vec<String>,
    /// `artist "…"` — who made this (albums declare it; players display it).
    pub artist: Option<String>,
    /// `sponsor "https://…"` — where listeners can support the author
    /// (package lists, the catalog and players surface it).
    pub sponsor: Option<String>,
    /// Body-level `automate <配置名>.volume from A to B over 区間` — fade a
    /// placed block instance from the outside (target keeps the dot form).
    pub place_autos: Vec<AutomateAst>,
    /// `param cutoff = 0.5 in 0..1` — the block's public knobs (device
    /// syntax). Referenced by name inside the block's instrument/insert
    /// args and set from a placement: `play Riff(cutoff: 0.7)`.
    pub params: Vec<DevParam>,
    pub tempo: Option<(f64, Pos)>,
    pub swing: Option<(f64, Pos)>,
    pub meter: Option<((u32, u32), Pos)>,
    pub key: Option<((String, String), Pos)>, // (root, scale) as written
    pub lets: Vec<LetAst>,
    pub sections: Vec<SectionAst>,
    pub tracks: Vec<TrackAst>,
    pub returns: Vec<ReturnAst>,
    /// Nested block definitions, local to this body.
    pub blocks: Vec<BlockAst>,
    /// Block placements on this body's timeline.
    pub places: Vec<PlaceAst>,
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
    /// `audio take at bars(2..3)` — recorded assets placed on the timeline.
    pub audios: Vec<AudioPlayAst>,
    /// `automate volume from 0.2 to 0.8 over bars(1..8)`
    pub automations: Vec<AutomateAst>,
    /// `modulate cutoff with lfo(rate: 0.3, amount: 0.4)`
    pub modulations: Vec<ModulateAst>,
}

#[derive(Clone, Debug)]
pub struct AutomateAst {
    pub target: String,
    pub from: f64,
    pub to: f64,
    pub at: AtRef,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct ModulateAst {
    pub param: String,
    /// modulator kind: "lfo" | "steps" | "random"
    pub kind: String,
    pub args: Vec<(String, Arg)>,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub struct AudioPlayAst {
    pub name: String,
    pub at: AtRef,
    pub pos: Pos,
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
    /// Bare name: an imported take (`take: myTake`) or a note (`root: A3`).
    Ident(String, Pos),
}

impl Arg {
    pub fn pos(&self) -> Pos {
        match self {
            Arg::Num(_, p) | Arg::Str(_, p) | Arg::Ident(_, p) => *p,
        }
    }
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
