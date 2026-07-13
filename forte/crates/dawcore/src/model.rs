//! Plain-data project model. This is the GUI's source of truth and the unit of
//! save/load. The audio engine keeps its own real-time mirror, updated through
//! [`crate::command::Command`]s. Keeping the two separate means the audio
//! thread never walks these (heap-heavy, lock-taking) structures.

use serde::{Deserialize, Serialize};

pub const MAX_TRACKS: usize = 64;
pub const SCENE_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Instrument,
    Audio,
    Effect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    // Note -> Note
    Arpeggiator,
    NoteTranspose,
    NoteRepeat,
    // Note -> Audio
    Prisma,
    Sampler,
    /// Pitch → sample map (drum kit built from recorded takes).
    Kit,
    PolyMesh,
    // Audio -> Audio
    Filter,
    Delay,
    Reverb,
    Eq,
    Drive,
    Comp,
    Chorus,
    /// Tempo-synced ducker (deterministic sidechain pumping).
    Pump,
    /// Mid/side stereo width.
    Width,
    /// Bit-depth + sample-rate reduction (the lo-fi/glitch crunch).
    Crush,
    /// Tempo-synced buffer repeat — the glitch stutter.
    Stutter,
    /// Tempo-synced pattern chopper (trance gate).
    Gate,
    Limiter,
    Space,
    /// Saturation: tape/tube/fuzz waveshaping with tone control.
    Saturate,
    /// Transient shaper: independent attack/sustain gain.
    Transient,
    /// Parallel (New York) compression: crushed copy blended under the dry.
    ParComp,
    /// Exciter: saturated high band blended on top.
    Exciter,
    /// Ring modulator: sine-carrier multiplication.
    RingMod,
    /// Tape stop: buffered read head slowing to a halt (automatable).
    TapeStop,
    /// Analog-media patina: wow/flutter, crackle, hiss, dust lowpass.
    Vinyl,
    /// Sidechain ducker keyed to another track's hits (compiler bakes the
    /// swung trigger times) — the glitch groove engine.
    Duck,
    /// A user-defined effect: a Grid graph fed by AudioIn (`device X : Effect`).
    MeshFx,
}

/// The three signal-transformation stages every device belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStage {
    NoteFx,     // Note -> Note
    Instrument, // Note -> Audio
    AudioFx,    // Audio -> Audio
}

impl DeviceStage {
    pub fn label(self) -> &'static str {
        match self {
            DeviceStage::NoteFx => "NOTE FX",
            DeviceStage::Instrument => "INSTRUMENTS",
            DeviceStage::AudioFx => "AUDIO FX",
        }
    }
}

impl DeviceKind {
    /// Every device, in stage order. The browser and factories iterate this —
    /// adding a device here is the only registration step the UI needs.
    pub const ALL: [DeviceKind; 29] = [
        DeviceKind::Arpeggiator,
        DeviceKind::NoteTranspose,
        DeviceKind::NoteRepeat,
        DeviceKind::Prisma,
        DeviceKind::Sampler,
        DeviceKind::Kit,
        DeviceKind::PolyMesh,
        DeviceKind::Filter,
        DeviceKind::Eq,
        DeviceKind::Drive,
        DeviceKind::Delay,
        DeviceKind::Reverb,
        DeviceKind::Comp,
        DeviceKind::Chorus,
        DeviceKind::Pump,
        DeviceKind::Width,
        DeviceKind::Crush,
        DeviceKind::Stutter,
        DeviceKind::Gate,
        DeviceKind::Limiter,
        DeviceKind::Space,
        DeviceKind::Saturate,
        DeviceKind::Transient,
        DeviceKind::ParComp,
        DeviceKind::Exciter,
        DeviceKind::RingMod,
        DeviceKind::TapeStop,
        DeviceKind::Vinyl,
        DeviceKind::Duck,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Arpeggiator => "Arpeggiator",
            DeviceKind::NoteTranspose => "Transposer",
            DeviceKind::NoteRepeat => "Note Repeat",
            DeviceKind::Prisma => "Prisma",
            DeviceKind::Sampler => "Sampler",
            DeviceKind::Kit => "Kit",
            DeviceKind::PolyMesh => "Poly Mesh",
            DeviceKind::Filter => "Filter",
            DeviceKind::Delay => "Delay",
            DeviceKind::Reverb => "Reverb",
            DeviceKind::Eq => "EQ",
            DeviceKind::Drive => "Distortion",
            DeviceKind::Comp => "Compressor",
            DeviceKind::Chorus => "Chorus",
            DeviceKind::Pump => "Pump",
            DeviceKind::Width => "Width",
            DeviceKind::Crush => "Crush",
            DeviceKind::Stutter => "Stutter",
            DeviceKind::Gate => "Gate",
            DeviceKind::Limiter => "Limiter",
            DeviceKind::Space => "Space",
            DeviceKind::Saturate => "Saturate",
            DeviceKind::Transient => "Transient",
            DeviceKind::ParComp => "ParComp",
            DeviceKind::Exciter => "Exciter",
            DeviceKind::RingMod => "RingMod",
            DeviceKind::TapeStop => "TapeStop",
            DeviceKind::Vinyl => "Vinyl",
            DeviceKind::Duck => "Duck",
            DeviceKind::MeshFx => "Mesh FX",
        }
    }

    pub fn stage(self) -> DeviceStage {
        match self {
            DeviceKind::Arpeggiator | DeviceKind::NoteTranspose | DeviceKind::NoteRepeat => {
                DeviceStage::NoteFx
            }
            DeviceKind::Prisma | DeviceKind::Sampler | DeviceKind::Kit | DeviceKind::PolyMesh => {
                DeviceStage::Instrument
            }
            _ => DeviceStage::AudioFx,
        }
    }

    pub fn is_instrument(self) -> bool {
        self.stage() == DeviceStage::Instrument
    }

    /// Parameter labels in engine order. The GUI renders a knob per entry.
    pub fn params(self) -> &'static [&'static str] {
        match self {
            DeviceKind::Prisma => &[
                "Wave", "Cutoff", "Reso", "Attack", "Decay", "Sustain", "Release",
                "Detune", "Sub", "FiltEnv", "Unison", "Spread",
            ],
            DeviceKind::Sampler => &[
                "Gain", "Attack", "Decay", "Sustain", "Release", "Pitch", "Start", "End",
                "Loop", "Reverse", "Glide", "Slices", "Choke", "Vary", "Stretch",
            ],
            DeviceKind::Kit => &["Gain", "Attack", "Decay", "Sustain", "Release", "Pitch", "Vary"],
            DeviceKind::PolyMesh => &[],
            DeviceKind::Arpeggiator => &["Rate", "Octaves", "Mode"],
            DeviceKind::NoteTranspose => &["Semi"],
            DeviceKind::NoteRepeat => &["Rate", "Gate"],
            DeviceKind::Filter => &["Type", "Cutoff", "Reso"],
            DeviceKind::Delay => &["Time", "Fdbk", "Mix"],
            DeviceKind::Reverb => &["Size", "Decay", "Mix"],
            DeviceKind::Eq => &["Low", "Mid", "High"],
            DeviceKind::Drive => &["Drive", "OS"],
            DeviceKind::Comp => &["Thresh", "Ratio", "Attack", "Release", "Makeup"],
            DeviceKind::Chorus => &["Rate", "Depth", "Mix"],
            DeviceKind::Pump => &["Amount", "Period"],
            DeviceKind::Width => &["Amount"],
            DeviceKind::Crush => &["Bits", "Rate", "Mix", "OS"],
            DeviceKind::Stutter => &["Period", "Mix"],
            DeviceKind::Gate => &["Depth", "Period", "Duty"],
            DeviceKind::Limiter => &["Ceiling", "Release"],
            DeviceKind::Space => &["Type", "Size", "Decay", "Damp", "Predelay", "Mod", "Width", "Mix"],
            DeviceKind::Saturate => &["Mode", "Drive", "Tone", "Mix", "OS"],
            DeviceKind::Transient => &["Attack", "Sustain"],
            DeviceKind::ParComp => &["Amount", "Drive", "Color", "OS"],
            DeviceKind::Exciter => &["Amount", "Freq"],
            DeviceKind::RingMod => &["Freq", "Mix"],
            DeviceKind::TapeStop => &["Amount"],
            DeviceKind::Vinyl => &["Wow", "Crackle", "Hiss", "Dust"],
            DeviceKind::Duck => &["Amount", "Attack", "Release", "Shape", "Mode"],
            DeviceKind::MeshFx => &[],
        }
    }

    /// Default parameter values, parallel to [`params`].
    pub fn defaults(self) -> Vec<f32> {
        match self {
            // Unison 0 = the bit-exact mono voice; Spread fans the stack
            DeviceKind::Prisma => {
                vec![1.0, 0.65, 0.15, 0.01, 0.3, 0.6, 0.25, 0.12, 0.3, 0.4, 0.0, 0.5]
            }
            // Pitch 0.5 == centre (no transpose); ±24 semitones across the range.
            // Start/End trim the play region; Loop/Reverse are 0/1 switches.
            // Choke: a new trigger hard-cuts running voices (MPC pad);
            // Vary: deterministic per-hit pitch/level drift (anti machine-gun);
            // Stretch: granular time-stretch (0 = off, 0.5 = original tempo)
            DeviceKind::Sampler => {
                vec![0.8, 0.02, 0.3, 0.9, 0.2, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            }
            // Pitch 0.5 = centre; transposes the WRAP layer only (±24 semi);
            // Vary = per-trigger pitch/level drift (anti machine-gun)
            DeviceKind::Kit => vec![0.8, 0.01, 0.3, 1.0, 0.25, 0.5, 0.0],
            DeviceKind::PolyMesh => Vec::new(),
            DeviceKind::Arpeggiator => vec![0.55, 0.0, 0.0], // 1/8, 1 octave, up
            DeviceKind::NoteTranspose => vec![0.5],          // centre = 0 semitones
            DeviceKind::NoteRepeat => vec![0.7, 0.5],        // 1/16, 50% gate
            DeviceKind::Filter => vec![0.0, 0.6, 0.2],
            DeviceKind::Delay => vec![0.3, 0.35, 0.3],
            DeviceKind::Reverb => vec![0.5, 0.5, 0.25],
            DeviceKind::Eq => vec![0.5, 0.5, 0.5],
            DeviceKind::Drive => vec![0.3, 0.0],
            DeviceKind::Comp => vec![0.5, 0.5, 0.1, 0.3, 0.25],
            DeviceKind::Chorus => vec![0.3, 0.5, 0.5],
            // Period is seconds per duck; the compiler overwrites it from tempo
            DeviceKind::Pump => vec![0.6, 0.5],
            DeviceKind::Width => vec![0.75],
            DeviceKind::Crush => vec![0.5, 0.35, 1.0, 0.0],
            // Period is seconds per repeat; the compiler overwrites it from tempo
            DeviceKind::Stutter => vec![0.25, 0.0],
            DeviceKind::Gate => vec![0.9, 0.25, 0.5],
            DeviceKind::Limiter => vec![0.95, 0.3],
            DeviceKind::Space => vec![1.0, 0.5, 0.5, 0.4, 0.1, 0.3, 0.8, 0.3],
            DeviceKind::Saturate => vec![0.0, 0.4, 0.7, 1.0, 0.0],
            DeviceKind::Transient => vec![0.5, 0.5],
            DeviceKind::ParComp => vec![0.35, 0.5, 0.3, 0.0],
            DeviceKind::Exciter => vec![0.3, 0.5],
            DeviceKind::RingMod => vec![0.4, 0.5],
            DeviceKind::TapeStop => vec![0.0],
            // A bare vinyl() is already a record: light warble, soft ticks,
            // a floor of hiss, a touch of rolloff
            DeviceKind::Vinyl => vec![0.25, 0.3, 0.15, 0.25],
            // Amount 0.85 (deep duck), fast attack, medium release, curved
            // shape; Mode 0 = duck (pump), 1 = key (gate opens on triggers)
            DeviceKind::Duck => vec![0.85, 0.3, 0.3, 0.6, 0.0],
            DeviceKind::MeshFx => Vec::new(),
        }
    }

    /// Which parameters are discrete dropdowns rather than knobs.
    pub fn options(self, param: usize) -> Option<&'static [&'static str]> {
        match (self, param) {
            (DeviceKind::Prisma, 0) => Some(&["Sine", "Saw", "Square", "Tri"]),
            (DeviceKind::Filter, 0) => Some(&["LP", "HP", "BP", "Notch"]),
            (DeviceKind::Arpeggiator, 2) => Some(&["Up", "Down", "UpDn"]),
            _ => None,
        }
    }
}

/// Where a sample comes from. Kept serialisable; the engine resolves it into a
/// shared audio buffer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleSource {
    #[default]
    None,
    Builtin(String), // "Kick" / "Snare" / "Hat"
    File(String),    // path on disk
    /// In-memory registered asset (recorded audio), keyed by content hash.
    Asset(String),
}

impl SampleSource {
    pub fn label(&self) -> String {
        match self {
            SampleSource::None => "—".into(),
            SampleSource::Builtin(n) => n.clone(),
            SampleSource::File(p) => std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone()),
            SampleSource::Asset(k) => format!("rec:{}", &k[..k.len().min(8)]),
        }
    }
}

// ---------------------------------------------------------------------------
// The Grid — Bitwig's modular environment. A Poly Grid device hosts a node
// graph; the engine compiles it into a per-voice interpreter.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridModuleKind {
    NoteIn,  // pitch / gate / velocity source (instrument graphs)
    AudioIn, // incoming signal (effect graphs)
    Osc,
    Noise, // deterministic white noise (per-voice xorshift PRNG)
    /// Recorded audio as a graph source — the soundnote: a take inside a patch.
    Sample,
    Lfo,
    Adsr,
    Filter,
    Shaper, // waveshaper: tanh / clip / fold
    /// Tuned modal resonator: rings at a frequency (physical modeling).
    Resonator,
    Gain,
    Mix,
    Out,
}

impl GridModuleKind {
    pub const PALETTE: [GridModuleKind; 6] = [
        GridModuleKind::Osc, GridModuleKind::Lfo, GridModuleKind::Adsr,
        GridModuleKind::Filter, GridModuleKind::Gain, GridModuleKind::Mix,
    ];
    pub fn label(self) -> &'static str {
        match self {
            GridModuleKind::NoteIn => "Note In",
            GridModuleKind::AudioIn => "Audio In",
            GridModuleKind::Osc => "Osc",
            GridModuleKind::Noise => "Noise",
            GridModuleKind::Sample => "Sample",
            GridModuleKind::Shaper => "Shaper",
            GridModuleKind::Lfo => "LFO",
            GridModuleKind::Adsr => "ADSR",
            GridModuleKind::Filter => "SVF",
            GridModuleKind::Resonator => "Resonator",
            GridModuleKind::Gain => "Gain",
            GridModuleKind::Mix => "Mix",
            GridModuleKind::Out => "Audio Out",
        }
    }
    pub fn inputs(self) -> &'static [&'static str] {
        match self {
            GridModuleKind::NoteIn => &[],
            GridModuleKind::AudioIn => &[],
            GridModuleKind::Osc => &["Pitch", "Mod", "Pwm"],
            GridModuleKind::Noise => &[],
            GridModuleKind::Sample => &[],
            GridModuleKind::Lfo => &[],
            GridModuleKind::Adsr => &["Gate"],
            GridModuleKind::Filter => &["In", "Cutoff"],
            GridModuleKind::Resonator => &["In", "Fm"],
            GridModuleKind::Shaper => &["In", "Mod"],
            GridModuleKind::Gain => &["In", "Mod"],
            GridModuleKind::Mix => &["A", "B"],
            GridModuleKind::Out => &["In"],
        }
    }
    pub fn outputs(self) -> &'static [&'static str] {
        match self {
            GridModuleKind::NoteIn => &["Pitch", "Gate", "Vel"],
            GridModuleKind::AudioIn => &["In"],
            GridModuleKind::Osc => &["Out"],
            GridModuleKind::Noise => &["Out"],
            GridModuleKind::Sample => &["Out"],
            GridModuleKind::Shaper => &["Out"],
            GridModuleKind::Lfo => &["Out"],
            GridModuleKind::Adsr => &["Env"],
            GridModuleKind::Filter => &["Out"],
            GridModuleKind::Resonator => &["Out"],
            GridModuleKind::Gain => &["Out"],
            GridModuleKind::Mix => &["Out"],
            GridModuleKind::Out => &[],
        }
    }
    pub fn params(self) -> &'static [&'static str] {
        match self {
            GridModuleKind::Osc => &["Shape"],
            GridModuleKind::Sample => &["Start", "End", "Loop", "Reverse"],
            GridModuleKind::Lfo => &["Rate", "Shape"],
            GridModuleKind::Adsr => &["A", "D", "S", "R"],
            GridModuleKind::Filter => &["Cutoff", "Reso"],
            GridModuleKind::Resonator => &["Freq", "Ring", "Key", "Strike"],
            GridModuleKind::Shaper => &["Drive", "Mode"],
            GridModuleKind::Gain => &["Level"],
            _ => &[],
        }
    }
    pub fn defaults(self) -> Vec<f32> {
        match self {
            GridModuleKind::Osc => vec![0.25], // saw
            GridModuleKind::Sample => vec![0.0, 1.0, 0.0, 0.0],
            GridModuleKind::Lfo => vec![0.3, 0.0],
            GridModuleKind::Adsr => vec![0.05, 0.3, 0.6, 0.25],
            GridModuleKind::Filter => vec![0.65, 0.2],
            GridModuleKind::Resonator => vec![0.5, 0.3, 0.0, 0.0],
            GridModuleKind::Shaper => vec![0.3, 0.1], // tanh
            GridModuleKind::Gain => vec![0.8],
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GridModule {
    pub kind: GridModuleKind,
    pub pos: (f32, f32), // canvas position (UI only)
    pub params: Vec<f32>,
    /// Audio source for a Sample node (None for every other kind).
    #[serde(default)]
    pub sample: Option<SampleSource>,
}

/// A wire: (from module, output port) → (to module, input port).
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct GridConn {
    pub from: (usize, usize),
    pub to: (usize, usize),
}

/// A declared device param exposed for runtime control: (name, initial
/// value, node param slots the value was compiled into).
pub type GridParamBind = (String, f32, Vec<(u32, u32)>);

#[derive(Clone, Serialize, Deserialize)]
pub struct GridGraph {
    pub modules: Vec<GridModule>,
    pub conns: Vec<GridConn>,
    /// Portamento time in seconds. > 0 switches the synth to mono/legato:
    /// overlapping notes glide instead of retriggering (the 303 slide).
    #[serde(default)]
    pub glide: f32,
    /// Exposed params, in the order the device declared them — the same
    /// order as the owning Device's `params`, so modulators and automation
    /// can route to grid instruments exactly like builtin ones.
    #[serde(default)]
    pub param_binds: Vec<GridParamBind>,
}

impl GridGraph {
    /// The default patch: NoteIn → Osc → SVF → Gain(×ADSR) → Out.
    pub fn default_patch() -> Self {
        let m = |kind: GridModuleKind, x: f32, y: f32| GridModule {
            kind,
            pos: (x, y),
            params: kind.defaults(),
            sample: None,
        };
        GridGraph {
            modules: vec![
                m(GridModuleKind::NoteIn, 20.0, 60.0),  // 0
                m(GridModuleKind::Osc, 170.0, 30.0),    // 1
                m(GridModuleKind::Filter, 320.0, 30.0), // 2
                m(GridModuleKind::Adsr, 170.0, 140.0),  // 3
                m(GridModuleKind::Gain, 470.0, 60.0),   // 4
                m(GridModuleKind::Out, 620.0, 70.0),    // 5
            ],
            conns: vec![
                GridConn { from: (0, 0), to: (1, 0) }, // pitch → osc
                GridConn { from: (0, 1), to: (3, 0) }, // gate → adsr
                GridConn { from: (1, 0), to: (2, 0) }, // osc → filter
                GridConn { from: (2, 0), to: (4, 0) }, // filter → gain
                GridConn { from: (3, 0), to: (4, 1) }, // env → gain mod
                GridConn { from: (4, 0), to: (5, 0) }, // gain → out
            ],
            glide: 0.0,
            param_binds: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub kind: DeviceKind,
    pub enabled: bool,
    pub params: Vec<f32>,
    /// LFO modulators living on this device (Bitwig-style per-device modulators).
    pub modulators: Vec<Modulator>,
    /// Sample loaded into a Sampler device (ignored by other device kinds).
    #[serde(default)]
    pub sample: SampleSource,
    /// Node graph for a Poly Grid device.
    #[serde(default)]
    pub grid: Option<GridGraph>,
    /// Pitch → sample map for a Kit device.
    #[serde(default)]
    pub kit: Vec<(u8, SampleSource)>,
    /// Sidechain trigger times in SECONDS (a Duck effect ducks its input at
    /// each of these, keyed to another track's hits). Empty for every other
    /// device; the compiler bakes them from the source track's swung notes.
    #[serde(default)]
    pub sidechain: Vec<f64>,
}

impl Device {
    pub fn new(kind: DeviceKind) -> Self {
        Self {
            kind,
            enabled: true,
            params: kind.defaults(),
            modulators: Vec::new(),
            sample: SampleSource::None,
            grid: None,
            kit: Vec::new(),
            sidechain: Vec::new(),
        }
    }

    pub fn sampler(source: SampleSource) -> Self {
        let mut d = Device::new(DeviceKind::Sampler);
        d.sample = source;
        d
    }

    pub fn poly_grid() -> Self {
        let mut d = Device::new(DeviceKind::PolyMesh);
        d.grid = Some(GridGraph::default_patch());
        d
    }
}

/// A unipolar/bipolar modulator. Bitwig's "unified modulation system": each
/// modulator outputs a signal routed to one or more parameters with a bipolar
/// amount. We model the most common types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModKind {
    Lfo,
    Steps,
    Random,
    Macro,
    /// A note-gate-driven envelope: rises on the track's first sounding note,
    /// releases when the track falls silent. Stage times live in `steps`
    /// as `[a, d, s, r]` (normalized 0..1).
    Adsr,
}

impl ModKind {
    pub fn label(self) -> &'static str {
        match self {
            ModKind::Lfo => "LFO",
            ModKind::Steps => "Steps",
            ModKind::Random => "Random",
            ModKind::Macro => "Macro",
            ModKind::Adsr => "ADSR",
        }
    }
    pub const ALL: [ModKind; 5] =
        [ModKind::Lfo, ModKind::Steps, ModKind::Random, ModKind::Macro, ModKind::Adsr];
    /// Distinct colour per modulator (Bitwig assigns each modulator a colour).
    pub fn color(self) -> [u8; 3] {
        match self {
            ModKind::Lfo => [0x5a, 0xc8, 0x5a],
            ModKind::Steps => [0x4f, 0xb6, 0xc8],
            ModKind::Random => [0xd0, 0x66, 0xa8],
            ModKind::Macro => [0xe0, 0xc6, 0x4f],
            ModKind::Adsr => [0xe8, 0x8a, 0x4c],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Modulator {
    pub kind: ModKind,
    pub rate: f32,        // 0..1 -> Hz (LFO/Steps/Random)
    pub shape: u8,        // LFO: 0 sine 1 tri 2 saw 3 square
    pub steps: Vec<f32>,  // Steps sequencer values 0..1
    pub value: f32,       // Macro knob / Random smoothing
    pub routes: Vec<ModRoute>,
}

impl Modulator {
    pub fn new(kind: ModKind) -> Self {
        Self {
            kind,
            rate: 0.3,
            shape: 0,
            steps: vec![0.0, 0.5, 1.0, 0.5, 0.75, 0.25, 1.0, 0.0],
            value: 0.5,
            routes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct ModRoute {
    pub param: usize,
    pub amount: f32, // bipolar -1..1
    /// Cross-device route (macros reach any device on the track);
    /// `None` targets the device the modulator lives on.
    #[serde(default)]
    pub device: Option<usize>,
}

/// Automation of a modulator's own field, addressed by the track-flat
/// modulator index (device order, then declaration order within a device).
#[derive(Clone, Serialize, Deserialize)]
pub struct ModAutomation {
    pub mod_index: usize,
    /// 0 = depth (scales every route amount), 1 = rate, 2 = value (macro knob)
    pub field: u8,
    pub points: Vec<AutomationPoint>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub start: f64,  // beats from clip start
    pub length: f64, // beats
    pub velocity: u8,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub color: [u8; 3],
    pub length: f64, // beats
    pub notes: Vec<Note>,
}

impl Clip {
    pub fn new(name: impl Into<String>, color: [u8; 3]) -> Self {
        Self { name: name.into(), color, length: 4.0, notes: Vec::new() }
    }
}

/// A clip placed on the Arranger Timeline at an absolute position. Its content
/// (`clip.length` beats) loops within the placed `duration`, as in Bitwig.
#[derive(Clone, Serialize, Deserialize)]
pub struct ArrangerClip {
    pub clip: Clip,
    pub start: f64,    // beats on the timeline
    pub duration: f64, // placed length in beats
    /// 1-based source line of the `play` that placed this clip (0 = unknown).
    /// Imported blocks carry the entry file's import-statement line, so a
    /// click in the arrangement always lands somewhere meaningful.
    #[serde(default)]
    pub src_line: u32,
}

/// An audio clip on the Arranger Timeline: a sample played from its start, at
/// its recorded pitch, for `duration` beats (or until the buffer ends).
#[derive(Clone, Serialize, Deserialize)]
pub struct AudioClip {
    pub name: String,
    pub color: [u8; 3],
    pub source: SampleSource,
    pub start: f64,
    pub duration: f64,
    pub gain: f32,
}

/// One point on an automation lane. `hold` is Bitwig 6's point behaviour:
/// the value stays flat until the next point instead of ramping linearly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamAutomation {
    /// device index within the track, param index within that device
    pub device: usize,
    pub param: usize,
    pub points: Vec<AutomationPoint>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f32, // 0..1
    pub hold: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Track {
    /// Stable engine slot id (0..MAX_TRACKS). Decoupled from display order.
    pub id: usize,
    pub name: String,
    pub kind: TrackKind,
    pub color: [u8; 3],
    pub volume: f32, // 0..1
    pub pan: f32,    // -1..1
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub devices: Vec<Device>,
    pub clips: Vec<Option<Clip>>, // one slot per scene (Clip Launcher)
    pub arranger: Vec<ArrangerClip>, // clips on the Arranger Timeline
    /// Audio clips on the Arranger Timeline.
    #[serde(default)]
    pub audio_clips: Vec<AudioClip>,
    /// Volume automation on the timeline (overrides the fader while playing).
    #[serde(default)]
    pub volume_automation: Vec<AutomationPoint>,
    #[serde(default)]
    pub param_automation: Vec<ParamAutomation>,
    /// Modulator-field automation lanes (depth / rate / macro value).
    #[serde(default)]
    pub mod_automation: Vec<ModAutomation>,
    /// 1-based source line of this track's `track` statement (0 = unknown).
    #[serde(default)]
    pub src_line: u32,
    /// Post-fader sends: (destination effect-track id, level 0..1).
    #[serde(default)]
    pub sends: Vec<(usize, f32)>,
}

impl Track {
    pub fn new(id: usize, name: impl Into<String>, kind: TrackKind, color: [u8; 3]) -> Self {
        let mut devices = Vec::new();
        if kind == TrackKind::Instrument {
            devices.push(Device::new(DeviceKind::Prisma));
        }
        Self {
            id,
            name: name.into(),
            kind,
            color,
            volume: 0.8,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            devices,
            clips: vec![None; SCENE_COUNT],
            arranger: Vec::new(),
            audio_clips: Vec::new(),
            volume_automation: Vec::new(),
            param_automation: Vec::new(),
            mod_automation: Vec::new(),
            src_line: 0,
            sends: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scale {
    Major,
    Minor,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Locrian,
    HarmonicMinor,
    Chromatic,
}

impl Scale {
    pub const ALL: [Scale; 9] = [
        Scale::Major, Scale::Minor, Scale::Dorian, Scale::Phrygian, Scale::Lydian,
        Scale::Mixolydian, Scale::Locrian, Scale::HarmonicMinor, Scale::Chromatic,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Scale::Major => "Major",
            Scale::Minor => "Minor",
            Scale::Dorian => "Dorian",
            Scale::Phrygian => "Phrygian",
            Scale::Lydian => "Lydian",
            Scale::Mixolydian => "Mixolydian",
            Scale::Locrian => "Locrian",
            Scale::HarmonicMinor => "Harmonic Minor",
            Scale::Chromatic => "Chromatic",
        }
    }
    pub fn degrees(self) -> &'static [i32] {
        match self {
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }
    pub fn contains(self, pitch: u8, root: u8) -> bool {
        let deg = ((pitch as i32 - root as i32) % 12 + 12) % 12;
        self.degrees().contains(&deg)
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct KeySignature {
    pub root: u8, // 0..11
    pub scale: Scale,
}

pub const NOTE_NAMES: [&str; 12] =
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn note_name(pitch: u8) -> String {
    format!("{}{}", NOTE_NAMES[(pitch % 12) as usize], pitch as i32 / 12 - 1)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
}

/// A cue marker storing a play position along the Arranger Timeline.
#[derive(Clone, Serialize, Deserialize)]
pub struct CueMarker {
    pub name: String,
    pub position: f64, // beats
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    /// Root block name plus its `desc`/`tags` metadata — shown by forte play
    /// and package catalogs (not part of the audio; digests are unaffected).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub license: String,
    pub tracks: Vec<Track>,
    pub scenes: Vec<Scene>,
    pub tempo: f64,
    pub time_sig: (u32, u32),
    pub key: KeySignature,
    // Arranger transport
    pub loop_enabled: bool,
    pub loop_start: f64, // beats
    pub loop_end: f64,
    pub cue_markers: Vec<CueMarker>,
    /// Launch quantization in beats. 0 = off (immediate). Bitwig default: 1 bar.
    pub launch_quant: f64,
    /// Mastering gain applied to the summed mix BEFORE the master soft
    /// limiter. 1.0 = neutral and bit-identical to projects that never set
    /// it — the song-level `master` statement drives loudness here.
    #[serde(default = "default_master")]
    pub master: f32,
    /// Master-bus insert chain, applied to the summed mix AFTER the master
    /// gain and BEFORE the soft limiter — the glue compressor / EQ /
    /// saturation / limiter of a real 2-bus. Empty = bit-identical bypass.
    #[serde(default)]
    pub master_inserts: Vec<Device>,
    next_id: usize,
}

fn default_master() -> f32 {
    1.0
}

impl Project {
    /// An empty project (no tracks, no loop) — the starting point for
    /// programmatic construction, e.g. by the Forte compiler.
    pub fn empty() -> Self {
        Project {
            name: String::new(),
            desc: String::new(),
            tags: Vec::new(),
            license: String::new(),
            tracks: Vec::new(),
            scenes: (0..SCENE_COUNT)
                .map(|i| Scene { name: format!("Scene {}", i + 1) })
                .collect(),
            tempo: 120.0,
            time_sig: (4, 4),
            key: KeySignature { root: 0, scale: Scale::Major },
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 16.0,
            cue_markers: Vec::new(),
            launch_quant: 0.0,
            master: 1.0,
            master_inserts: Vec::new(),
            next_id: 0,
        }
    }

    pub fn alloc_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

/// Track colour palette approximating Bitwig's defaults.
pub const TRACK_COLORS: [[u8; 3]; 9] = [
    [224, 88, 79],
    [224, 138, 60],
    [224, 198, 79],
    [155, 207, 82],
    [79, 182, 200],
    [90, 138, 208],
    [154, 111, 208],
    [208, 102, 168],
    [138, 138, 138],
];

impl Default for Project {
    fn default() -> Self {
        Self::demo()
    }
}

impl Project {
    /// A small starter project so the app is immediately playable.
    pub fn demo() -> Self {
        let scenes = (0..SCENE_COUNT)
            .map(|i| Scene { name: format!("Scene {}", i + 1) })
            .collect();

        let mut p = Project {
            name: String::new(),
            desc: String::new(),
            tags: Vec::new(),
            license: String::new(),
            tracks: Vec::new(),
            scenes,
            tempo: 120.0,
            time_sig: (4, 4),
            key: KeySignature { root: 0, scale: Scale::Minor },
            loop_enabled: true,
            loop_start: 0.0,
            loop_end: 32.0, // 8 bars
            cue_markers: vec![
                CueMarker { name: "Intro".into(), position: 0.0 },
                CueMarker { name: "Verse".into(), position: 16.0 },
            ],
            launch_quant: 4.0, // 1 bar, Bitwig default
            master: 1.0,
            master_inserts: Vec::new(),
            next_id: 0,
        };

        // Kick drum — a real Sampler playing the built-in kick one-shot.
        let mut drums = Track::new(p.alloc_id(), "Kick", TrackKind::Instrument, TRACK_COLORS[0]);
        drums.devices[0] = Device::sampler(SampleSource::Builtin("Kick".into()));
        let mut beat = Clip::new("Kick", TRACK_COLORS[0]);
        for i in 0..4 {
            beat.notes.push(Note { pitch: 36, start: i as f64, length: 0.2, velocity: 110 });
        }
        drums.clips[0] = Some(beat);

        // Hi-hats — Sampler, eighth-note pattern.
        let mut hats = Track::new(p.alloc_id(), "Hats", TrackKind::Instrument, TRACK_COLORS[2]);
        hats.devices[0] = Device::sampler(SampleSource::Builtin("Hat".into()));
        let mut hclip = Clip::new("Hats", TRACK_COLORS[2]);
        for i in 0..8 {
            hclip.notes.push(Note { pitch: 42, start: i as f64 * 0.5, length: 0.1, velocity: 80 });
        }
        hats.clips[0] = Some(hclip);

        // Snare — Sampler on the backbeat.
        let mut snare = Track::new(p.alloc_id(), "Snare", TrackKind::Instrument, TRACK_COLORS[3]);
        snare.devices[0] = Device::sampler(SampleSource::Builtin("Snare".into()));
        let mut sclip = Clip::new("Snare", TRACK_COLORS[3]);
        for &b in &[1.0, 3.0] {
            sclip.notes.push(Note { pitch: 38, start: b, length: 0.2, velocity: 100 });
        }
        snare.clips[0] = Some(sclip);

        // Bass — built in The Grid (Poly Grid) to showcase the modular engine.
        let mut bass = Track::new(p.alloc_id(), "Bass", TrackKind::Instrument, TRACK_COLORS[5]);
        bass.devices[0] = Device::poly_grid();
        let mut bclip = Clip::new("Bass A", TRACK_COLORS[5]);
        for &(pitch, start) in &[(36, 0.0), (36, 1.0), (43, 2.0), (41, 3.0)] {
            bclip.notes.push(Note { pitch, start, length: 0.9, velocity: 100 });
        }
        bass.clips[0] = Some(bclip);

        // Keys
        let mut keys = Track::new(p.alloc_id(), "Keys", TrackKind::Instrument, TRACK_COLORS[6]);
        keys.devices[0].params[0] = 2.0; // square
        // Demo of the unified modulation system: an LFO sweeping the cutoff.
        {
            let mut lfo = Modulator::new(ModKind::Lfo);
            lfo.rate = 0.22;
            lfo.routes.push(ModRoute { param: 1, amount: 0.3, device: None });
            keys.devices[0].modulators.push(lfo);
        }
        keys.devices.push(Device::new(DeviceKind::Reverb));
        let mut kclip = Clip::new("Chords", TRACK_COLORS[6]);
        for &p2 in &[60u8, 63, 67] {
            kclip.notes.push(Note { pitch: p2, start: 0.0, length: 2.0, velocity: 80 });
        }
        for &p2 in &[58u8, 62, 65] {
            kclip.notes.push(Note { pitch: p2, start: 2.0, length: 2.0, velocity: 80 });
        }
        keys.clips[0] = Some(kclip);

        // Lead
        let mut lead = Track::new(p.alloc_id(), "Lead", TrackKind::Instrument, TRACK_COLORS[1]);
        lead.devices.push(Device::new(DeviceKind::Delay));
        let mut lclip = Clip::new("Riff", TRACK_COLORS[1]);
        for &(pitch, start, len) in &[
            (72u8, 0.0, 0.5), (75, 0.5, 0.5), (79, 1.0, 0.5), (72, 1.5, 0.5),
            (74, 2.0, 0.5), (77, 2.5, 0.5), (81, 3.0, 1.0),
        ] {
            lclip.notes.push(Note { pitch, start, length: len, velocity: 95 });
        }
        lead.clips[1] = Some(lclip);

        // Audio track with audio clips (sampled percussion placed as audio).
        let mut perc = Track::new(p.alloc_id(), "Perc (Audio)", TrackKind::Audio, TRACK_COLORS[4]);
        for bar in 0..8 {
            perc.audio_clips.push(AudioClip {
                name: "snare".into(),
                color: TRACK_COLORS[4],
                source: SampleSource::Builtin("Snare".into()),
                start: bar as f64 * 4.0 + 2.0,
                duration: 1.0,
                gain: 0.7,
            });
        }

        // FX return track (effect track fed by post-fader sends).
        let mut fx = Track::new(p.alloc_id(), "FX Return", TrackKind::Effect, TRACK_COLORS[8]);
        let mut rev = Device::new(DeviceKind::Reverb);
        rev.params = vec![0.7, 0.7, 0.85];
        fx.devices.push(rev);
        lead.sends.push((fx.id, 0.45));
        keys.sends.push((fx.id, 0.3));
        perc.sends.push((fx.id, 0.25));

        // Volume automation demo: fade the lead in over bars 5-8 (v6 hold+ramp).
        lead.volume_automation = vec![
            AutomationPoint { beat: 0.0, value: 0.25, hold: true },
            AutomationPoint { beat: 16.0, value: 0.25, hold: false },
            AutomationPoint { beat: 24.0, value: 0.85, hold: false },
        ];

        // Lay the launcher clips out on the Arranger Timeline as a simple song.
        if let Some(c) = drums.clips[0].clone() {
            drums.arranger.push(ArrangerClip { clip: c, start: 0.0, duration: 32.0 , src_line: 0 });
        }
        if let Some(c) = bass.clips[0].clone() {
            bass.arranger.push(ArrangerClip { clip: c.clone(), start: 8.0, duration: 24.0 , src_line: 0 });
        }
        if let Some(c) = keys.clips[0].clone() {
            keys.arranger.push(ArrangerClip { clip: c, start: 0.0, duration: 16.0 , src_line: 0 });
        }
        if let Some(c) = lead.clips[1].clone() {
            lead.arranger.push(ArrangerClip { clip: c, start: 16.0, duration: 16.0 , src_line: 0 });
        }
        if let Some(c) = hats.clips[0].clone() {
            hats.arranger.push(ArrangerClip { clip: c, start: 0.0, duration: 32.0 , src_line: 0 });
        }
        if let Some(c) = snare.clips[0].clone() {
            snare.arranger.push(ArrangerClip { clip: c, start: 8.0, duration: 24.0 , src_line: 0 });
        }

        p.tracks = vec![drums, hats, snare, bass, keys, lead, perc, fx];
        p
    }
}
