//! The device system. Every device is one of three signal-transformation
//! stages, mirroring Bitwig's taxonomy:
//!
//! - [`NoteFx`]      Note → Note   (Arpeggiator, Note Transpose, Note Repeat…)
//! - [`Instrument`]  Note → Audio  (Polymer, Sampler, Poly Grid…)
//! - [`AudioFx`]     Audio → Audio (Filter+, EQ-5, Delay-4, Reverb…)
//!
//! A track's chain processes in stage order each block: scheduled + live note
//! events flow through the Note-FX chain (each stage may transform, swallow or
//! emit events), the result drives every instrument, and the summed audio runs
//! through the Audio-FX chain.
//!
//! Adding a device = implement one trait + add a [`crate::model::DeviceKind`]
//! with metadata + one arm in [`build_dsp`]. Nothing else in the engine changes.
//!
//! Real-time rules: trait objects are built on the UI thread and shipped to the
//! audio thread pre-boxed. `process`/`next` must not allocate; bounded pushes
//! only (use [`push_bounded`]).

use crate::dsp::effects::{Chorus, Compressor, Crush, Drive, Eq3, FdnReverb, Gate, Pump, StereoDelay, Stutter, Width};
use crate::dsp::filter::{FilterMode, Svf};
use crate::dsp::grid::GridSynth;
use crate::dsp::sampler::Sampler;
use crate::dsp::synth::PolySynth;
use crate::dsp::voice::SynthParams;
use crate::dsp::oscillator::Waveform;
use crate::engine::resolve_sample;
use crate::model::{Device, DeviceKind, GridGraph};

/// A note event positioned inside the current block.
#[derive(Clone, Copy, Debug)]
pub struct NoteEvent {
    pub sample: u32,
    pub on: bool,
    pub pitch: u8,
    pub velocity: f32, // 0..1
}

/// Per-block timing context handed to note effects.
#[derive(Clone, Copy)]
pub struct BlockCtx {
    pub sample_rate: f32,
    pub frames: usize,
    /// Beat position of the block start/end. When the transport is stopped the
    /// engine substitutes a free-running beat so live performance still works.
    pub start_beat: f64,
    pub end_beat: f64,
    pub samples_per_beat: f64,
    pub playing: bool,
}

impl BlockCtx {
    #[inline]
    pub fn beat_to_sample(&self, beat: f64) -> u32 {
        (((beat - self.start_beat) * self.samples_per_beat).round() as i64)
            .clamp(0, self.frames as i64 - 1) as u32
    }
}

#[inline]
pub fn push_bounded(out: &mut Vec<NoteEvent>, ev: NoteEvent) {
    if out.len() < out.capacity() {
        out.push(ev);
    }
}

// ---------------------------------------------------------------------------
// Stage traits
// ---------------------------------------------------------------------------

pub trait NoteFx: Send {
    /// Transform this block's events. `input` is sorted by sample; the engine
    /// re-sorts `output` afterwards. Must not allocate (bounded pushes only).
    fn process(&mut self, ctx: &BlockCtx, input: &[NoteEvent], output: &mut Vec<NoteEvent>);
    fn configure(&mut self, params: &[f32]);
    /// Transport stopped / panic: clear held state and emit nothing further.
    fn reset(&mut self);
}

pub trait Instrument: Send {
    fn handle(&mut self, on: bool, pitch: u8, velocity: f32);
    /// Render one mono sample.
    fn next(&mut self) -> f32;
    fn configure(&mut self, params: &[f32]);
    fn voices(&self) -> usize;
    fn reset(&mut self);
    /// Extra hook for graph instruments (Poly Grid node params). Default no-op.
    fn set_node_param(&mut self, _node: usize, _param: usize, _value: f32) {}
}

pub trait AudioFx: Send {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32);
    fn configure(&mut self, params: &[f32]);
}

/// A device slot on the audio thread: exactly one stage.
pub enum Dsp {
    Note(Box<dyn NoteFx>),
    Inst(Box<dyn Instrument>),
    Audio(Box<dyn AudioFx>),
}

/// Factory: build the DSP for a device. Runs on the UI thread.
pub fn build_dsp(dev: &Device, sr: f32) -> Dsp {
    match dev.kind {
        // ---- Note → Note ----
        DeviceKind::NoteTranspose => Dsp::Note(Box::new(NoteTranspose::new())),
        DeviceKind::NoteRepeat => Dsp::Note(Box::new(NoteRepeat::new())),
        DeviceKind::Arpeggiator => Dsp::Note(Box::new(Arpeggiator::new())),
        // ---- Note → Audio ----
        DeviceKind::Prisma => Dsp::Inst(Box::new(PolySynth::new(sr))),
        DeviceKind::Sampler => {
            let mut s = Sampler::new(sr);
            s.sample = resolve_sample(&dev.sample);
            Dsp::Inst(Box::new(s))
        }
        DeviceKind::Kit => {
            let mut k = crate::dsp::kit::KitSampler::new(sr);
            k.map = dev
                .kit
                .iter()
                .filter_map(|(p, src)| resolve_sample(src).map(|s| (*p, s)))
                .collect();
            k.map.sort_by_key(|(p, _)| *p);
            Dsp::Inst(Box::new(k))
        }
        DeviceKind::PolyMesh => {
            let graph = dev.grid.clone().unwrap_or_else(GridGraph::default_patch);
            Dsp::Inst(Box::new(GridSynth::compile(&graph, sr)))
        }
        // ---- Audio → Audio ----
        DeviceKind::MeshFx => {
            let graph = dev.grid.clone().unwrap_or_else(GridGraph::default_patch);
            Dsp::Audio(Box::new(crate::dsp::grid::GridFx::compile(&graph, sr)))
        }
        DeviceKind::Filter => Dsp::Audio(Box::new(FilterFx { l: Svf::new(sr), r: Svf::new(sr), mode: FilterMode::Lowpass })),
        DeviceKind::Delay => Dsp::Audio(Box::new(StereoDelay::new(sr))),
        DeviceKind::Reverb => Dsp::Audio(Box::new(FdnReverb::new(sr))),
        DeviceKind::Eq => Dsp::Audio(Box::new(EqFx { l: Eq3::new(sr), r: Eq3::new(sr) })),
        DeviceKind::Drive => Dsp::Audio(Box::new(Drive::new())),
        DeviceKind::Comp => Dsp::Audio(Box::new(Compressor::new(sr))),
        DeviceKind::Chorus => Dsp::Audio(Box::new(Chorus::new(sr))),
        DeviceKind::Pump => Dsp::Audio(Box::new(Pump::new(sr))),
        DeviceKind::Width => Dsp::Audio(Box::new(Width::new())),
        DeviceKind::Crush => Dsp::Audio(Box::new(Crush::new())),
        DeviceKind::Stutter => Dsp::Audio(Box::new(Stutter::new(sr))),
        DeviceKind::Gate => Dsp::Audio(Box::new(Gate::new(sr))),
    }
}

fn map_cutoff(v: f32) -> f32 {
    30.0 * crate::dmath::powf(600.0, v.clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// Note FX implementations
// ---------------------------------------------------------------------------

/// Shifts every note by a fixed number of semitones. Remembers the shifted
/// pitch per held input pitch so a param change mid-note can't strand a voice.
pub struct NoteTranspose {
    semitones: i32,
    map: [i16; 128], // input pitch -> emitted pitch (-1 = not held)
}

impl Default for NoteTranspose {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteTranspose {
    pub fn new() -> Self {
        Self { semitones: 0, map: [-1; 128] }
    }
}

impl NoteFx for NoteTranspose {
    fn process(&mut self, _ctx: &BlockCtx, input: &[NoteEvent], output: &mut Vec<NoteEvent>) {
        for e in input {
            if e.on {
                let t = (e.pitch as i32 + self.semitones).clamp(0, 127) as u8;
                self.map[e.pitch as usize] = t as i16;
                push_bounded(output, NoteEvent { pitch: t, ..*e });
            } else {
                let t = self.map[e.pitch as usize];
                self.map[e.pitch as usize] = -1;
                let pitch = if t >= 0 { t as u8 } else { e.pitch };
                push_bounded(output, NoteEvent { pitch, ..*e });
            }
        }
    }
    fn configure(&mut self, p: &[f32]) {
        // p[0]: 0..1 -> -24..+24 semitones
        self.semitones = ((p.first().copied().unwrap_or(0.5) - 0.5) * 48.0).round() as i32;
    }
    fn reset(&mut self) {
        self.map = [-1; 128];
    }
}

fn rate_to_beats(v: f32) -> f64 {
    // 0..1 -> 1/1, 1/2, 1/4, 1/8, 1/16, 1/32 (in beats: 4, 2, 1, .5, .25, .125)
    let idx = (v.clamp(0.0, 0.999) * 6.0) as usize;
    [4.0, 2.0, 1.0, 0.5, 0.25, 0.125][idx]
}

/// Retriggers held notes on a beat grid (Bitwig's Note Repeat).
pub struct NoteRepeat {
    rate: f32,
    gate: f32,
    held: [f32; 128], // velocity while held, 0 = not held
    pending_offs: Vec<(f64, u8)>, // (beat, pitch)
    free_beat: f64,
}

impl Default for NoteRepeat {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteRepeat {
    pub fn new() -> Self {
        Self { rate: 0.5, gate: 0.5, held: [0.0; 128], pending_offs: Vec::with_capacity(64), free_beat: 0.0 }
    }
}

impl NoteFx for NoteRepeat {
    fn process(&mut self, ctx: &BlockCtx, input: &[NoteEvent], output: &mut Vec<NoteEvent>) {
        // swallow input; track held set. Note-offs cut the sounding repeat.
        for e in input {
            if e.on {
                self.held[e.pitch as usize] = e.velocity.max(0.01);
            } else {
                self.held[e.pitch as usize] = 0.0;
                push_bounded(output, *e);
                self.pending_offs.retain(|&(_, p)| p != e.pitch);
            }
        }

        // beat window: transport when playing, free-running otherwise
        let (start, end) = if ctx.playing {
            (ctx.start_beat, ctx.end_beat)
        } else {
            let s = self.free_beat;
            let e = s + ctx.frames as f64 / ctx.samples_per_beat;
            self.free_beat = e;
            (s, e)
        };
        let to_sample = |beat: f64| -> u32 {
            (((beat - start) * ctx.samples_per_beat).round() as i64)
                .clamp(0, ctx.frames as i64 - 1) as u32
        };

        // due note-offs from previous blocks
        self.pending_offs.retain(|&(beat, pitch)| {
            if beat < end {
                push_bounded(output, NoteEvent { sample: to_sample(beat.max(start)), on: false, pitch, velocity: 0.0 });
                false
            } else {
                true
            }
        });

        let interval = rate_to_beats(self.rate);
        let mut b = (start / interval).ceil() * interval;
        while b < end {
            for pitch in 0..128u8 {
                let vel = self.held[pitch as usize];
                if vel > 0.0 {
                    push_bounded(output, NoteEvent { sample: to_sample(b), on: true, pitch, velocity: vel });
                    let off = b + interval * (0.1 + self.gate as f64 * 0.85);
                    if off < end {
                        push_bounded(output, NoteEvent { sample: to_sample(off), on: false, pitch, velocity: 0.0 });
                    } else if self.pending_offs.len() < self.pending_offs.capacity() {
                        self.pending_offs.push((off, pitch));
                    }
                }
            }
            b += interval;
        }
    }
    fn configure(&mut self, p: &[f32]) {
        self.rate = p.first().copied().unwrap_or(0.5);
        self.gate = p.get(1).copied().unwrap_or(0.5);
    }
    fn reset(&mut self) {
        self.held = [0.0; 128];
        self.pending_offs.clear();
    }
}

/// Steps through held notes (expanded over octaves) on a beat grid.
pub struct Arpeggiator {
    rate: f32,
    octaves: usize, // 1..=3
    mode: u8,       // 0 up, 1 down, 2 up-down
    held: Vec<(u8, f32)>, // sorted by pitch, with velocity
    step: usize,
    sounding: Option<u8>,
    pending_off: Option<(f64, u8)>,
    free_beat: f64,
}

impl Default for Arpeggiator {
    fn default() -> Self {
        Self::new()
    }
}

impl Arpeggiator {
    pub fn new() -> Self {
        Self {
            rate: 0.5,
            octaves: 1,
            mode: 0,
            held: Vec::with_capacity(32),
            step: 0,
            sounding: None,
            pending_off: None,
            free_beat: 0.0,
        }
    }

    fn sequence_len(&self) -> usize {
        let base = self.held.len() * self.octaves;
        match self.mode {
            2 if base > 1 => base * 2 - 2,
            _ => base,
        }
    }

    fn pitch_at(&self, step: usize) -> Option<(u8, f32)> {
        let base = self.held.len() * self.octaves;
        if base == 0 {
            return None;
        }
        let idx = match self.mode {
            1 => base - 1 - (step % base),               // down
            2 if base > 1 => {
                let cycle = base * 2 - 2;
                let s = step % cycle;
                if s < base { s } else { cycle - s }      // up-down
            }
            _ => step % base,                             // up
        };
        let (p, v) = self.held[idx % self.held.len()];
        let oct = (idx / self.held.len()) as i32;
        Some((((p as i32) + oct * 12).clamp(0, 127) as u8, v))
    }
}

impl NoteFx for Arpeggiator {
    fn process(&mut self, ctx: &BlockCtx, input: &[NoteEvent], output: &mut Vec<NoteEvent>) {
        // swallow input into the held chord
        for e in input {
            if e.on {
                if !self.held.iter().any(|&(p, _)| p == e.pitch) && self.held.len() < 32 {
                    self.held.push((e.pitch, e.velocity.max(0.01)));
                    self.held.sort_unstable_by_key(|&(p, _)| p);
                }
            } else {
                self.held.retain(|&(p, _)| p != e.pitch);
                if self.held.is_empty() {
                    // chord released: stop whatever is sounding
                    if let Some(p) = self.sounding.take() {
                        push_bounded(output, NoteEvent { sample: e.sample, on: false, pitch: p, velocity: 0.0 });
                    }
                    self.pending_off = None;
                    self.step = 0;
                }
            }
        }

        let (start, end) = if ctx.playing {
            (ctx.start_beat, ctx.end_beat)
        } else {
            let s = self.free_beat;
            let e = s + ctx.frames as f64 / ctx.samples_per_beat;
            self.free_beat = e;
            (s, e)
        };
        let to_sample = |beat: f64| -> u32 {
            (((beat - start) * ctx.samples_per_beat).round() as i64)
                .clamp(0, ctx.frames as i64 - 1) as u32
        };

        if let Some((beat, pitch)) = self.pending_off {
            if beat < end {
                push_bounded(output, NoteEvent { sample: to_sample(beat.max(start)), on: false, pitch, velocity: 0.0 });
                if self.sounding == Some(pitch) {
                    self.sounding = None;
                }
                self.pending_off = None;
            }
        }

        if self.held.is_empty() {
            return;
        }
        let interval = rate_to_beats(self.rate);
        let mut b = (start / interval).ceil() * interval;
        while b < end {
            // release the previous step if still sounding
            if let Some(p) = self.sounding.take() {
                push_bounded(output, NoteEvent { sample: to_sample(b), on: false, pitch: p, velocity: 0.0 });
                self.pending_off = None;
            }
            if let Some((pitch, vel)) = self.pitch_at(self.step) {
                push_bounded(output, NoteEvent { sample: to_sample(b), on: true, pitch, velocity: vel });
                self.sounding = Some(pitch);
                let off = b + interval * 0.9;
                if off < end {
                    push_bounded(output, NoteEvent { sample: to_sample(off), on: false, pitch, velocity: 0.0 });
                    self.sounding = None;
                } else {
                    self.pending_off = Some((off, pitch));
                }
            }
            self.step = (self.step + 1) % self.sequence_len().max(1);
            b += interval;
        }
    }
    fn configure(&mut self, p: &[f32]) {
        self.rate = p.first().copied().unwrap_or(0.5);
        self.octaves = 1 + (p.get(1).copied().unwrap_or(0.0) * 2.999) as usize;
        self.mode = (p.get(2).copied().unwrap_or(0.0)).round() as u8;
    }
    fn reset(&mut self) {
        self.held.clear();
        self.step = 0;
        self.sounding = None;
        self.pending_off = None;
    }
}

// ---------------------------------------------------------------------------
// Instrument impls for the existing synth engines
// ---------------------------------------------------------------------------

impl Instrument for PolySynth {
    fn handle(&mut self, on: bool, pitch: u8, velocity: f32) {
        if on { self.note_on(pitch, velocity) } else { self.note_off(pitch) }
    }
    fn next(&mut self) -> f32 {
        PolySynth::next(self)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 10 {
            self.params = SynthParams {
                wave: Waveform::from_index(p[0] as u8),
                cutoff: p[1],
                resonance: p[2],
                attack: p[3],
                decay: p[4],
                sustain: p[5],
                release: p[6],
                detune: p[7],
                sub: p[8],
                filter_env: p[9],
            };
        }
    }
    fn voices(&self) -> usize {
        self.active_voices()
    }
    fn reset(&mut self) {
        self.all_notes_off();
    }
}

impl Instrument for Sampler {
    fn handle(&mut self, on: bool, pitch: u8, velocity: f32) {
        if on { self.note_on(pitch, velocity) } else { self.note_off(pitch) }
    }
    fn next(&mut self) -> f32 {
        Sampler::next(self)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 6 {
            self.gain = p[0];
            self.attack = 0.001 + p[1] * p[1] * 2.0;
            self.decay = 0.001 + p[2] * p[2] * 2.0;
            self.sustain = p[3];
            self.release = 0.001 + p[4] * p[4] * 2.5;
            self.transpose = (p[5] - 0.5) * 48.0;
        }
        if p.len() >= 10 {
            self.start = p[6];
            self.end = p[7];
            self.loop_on = p[8] > 0.5;
            self.reverse = p[9] > 0.5;
        }
        if p.len() >= 11 {
            // knob is 0..1 → 0..0.5 s of slide
            self.glide = p[10] * 0.5;
        }
        if p.len() >= 12 {
            // knob is 0..1 → 0..32 slices (0 = slice mode off)
            self.slices = (p[11] * 32.0).round() as u8;
        }
    }
    fn voices(&self) -> usize {
        self.active_voices()
    }
    fn reset(&mut self) {
        self.all_notes_off();
    }
}

impl Instrument for crate::dsp::kit::KitSampler {
    fn handle(&mut self, on: bool, pitch: u8, velocity: f32) {
        if on { self.note_on(pitch, velocity) } else { self.note_off(pitch) }
    }
    fn next(&mut self) -> f32 {
        crate::dsp::kit::KitSampler::next(self)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 5 {
            self.gain = p[0];
            self.attack = 0.001 + p[1] * p[1] * 2.0;
            self.decay = 0.001 + p[2] * p[2] * 2.0;
            self.sustain = p[3];
            self.release = 0.001 + p[4] * p[4] * 2.5;
        }
    }
    fn voices(&self) -> usize {
        self.active_voices()
    }
    fn reset(&mut self) {
        self.all_notes_off();
    }
}

impl Instrument for GridSynth {
    fn handle(&mut self, on: bool, pitch: u8, velocity: f32) {
        if on { self.note_on(pitch, velocity) } else { self.note_off(pitch) }
    }
    fn next(&mut self) -> f32 {
        GridSynth::next(self)
    }
    fn configure(&mut self, p: &[f32]) {
        // exposed device params (declaration order) → their node slots, so
        // modulators and automation drive grid instruments like builtins
        self.apply_exposed_params(p);
    }
    fn voices(&self) -> usize {
        self.active_voices()
    }
    fn reset(&mut self) {
        self.all_notes_off();
    }
    fn set_node_param(&mut self, node: usize, param: usize, value: f32) {
        self.set_param(node, param, value);
    }
}

// ---------------------------------------------------------------------------
// Audio FX impls
// ---------------------------------------------------------------------------

pub struct FilterFx {
    pub l: Svf,
    pub r: Svf,
    pub mode: FilterMode,
}

impl AudioFx for FilterFx {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.l.process(l, self.mode), self.r.process(r, self.mode))
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            self.mode = FilterMode::from_index(p[0] as u8);
            let c = map_cutoff(p[1]);
            self.l.set(c, p[2]);
            self.r.set(c, p[2]);
        }
    }
}

pub struct EqFx {
    pub l: Eq3,
    pub r: Eq3,
}

impl AudioFx for EqFx {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.l.process(l), self.r.process(r))
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            for ch in [&mut self.l, &mut self.r] {
                ch.low_gain = p[0];
                ch.mid_gain = p[1];
                ch.high_gain = p[2];
            }
        }
    }
}

impl AudioFx for StereoDelay {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        StereoDelay::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            self.time = p[0];
            self.feedback = p[1];
            self.mix = p[2];
        }
    }
}

impl AudioFx for FdnReverb {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        FdnReverb::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            self.size = p[0];
            self.decay = p[1];
            self.mix = p[2];
        }
    }
}

impl AudioFx for Compressor {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Compressor::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 5 {
            self.thresh = p[0];
            self.ratio = p[1];
            self.attack = p[2];
            self.release = p[3];
            self.makeup = p[4];
            self.update_coefs();
        }
    }
}

impl AudioFx for Chorus {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Chorus::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            self.rate = p[0];
            self.depth = p[1];
            self.mix = p[2];
        }
    }
}

impl AudioFx for Crush {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Crush::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            self.bits = p[0];
            self.rate = p[1];
            self.mix = p[2];
        }
    }
}

impl AudioFx for Stutter {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Stutter::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 2 {
            self.period = p[0];
            self.mix = p[1];
        }
    }
}

impl AudioFx for Gate {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Gate::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 3 {
            self.depth = p[0];
            self.period = p[1];
            self.duty = p[2];
        }
    }
}

impl AudioFx for Pump {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Pump::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if p.len() >= 2 {
            self.amount = p[0];
            self.period = p[1];
        }
    }
}

impl AudioFx for Width {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        Width::process(self, l, r)
    }
    fn configure(&mut self, p: &[f32]) {
        if !p.is_empty() {
            self.amount = p[0];
        }
    }
}

impl AudioFx for Drive {
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (Drive::process(self, l), Drive::process(self, r))
    }
    fn configure(&mut self, p: &[f32]) {
        if let Some(&d) = p.first() {
            self.amount = d;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(frames: usize) -> BlockCtx {
        BlockCtx {
            sample_rate: 48_000.0,
            frames,
            start_beat: 0.0,
            end_beat: frames as f64 / 24_000.0, // 120 bpm at 48k
            samples_per_beat: 24_000.0,
            playing: true,
        }
    }

    fn on(pitch: u8) -> NoteEvent {
        NoteEvent { sample: 0, on: true, pitch, velocity: 0.8 }
    }

    #[test]
    fn transpose_shifts_and_restores() {
        let mut t = NoteTranspose::new();
        t.configure(&[1.0]); // +24
        let mut out = Vec::with_capacity(16);
        t.process(&ctx(64), &[on(60)], &mut out);
        assert_eq!(out[0].pitch, 84);
        // param change mid-note must not strand the voice
        t.configure(&[0.5]); // 0
        out.clear();
        t.process(&ctx(64), &[NoteEvent { sample: 0, on: false, pitch: 60, velocity: 0.0 }], &mut out);
        assert_eq!(out[0].pitch, 84, "note-off must release the transposed pitch");
    }

    #[test]
    fn arp_steps_through_held_chord() {
        let mut a = Arpeggiator::new();
        a.configure(&[0.99, 0.0, 0.0]); // 1/32, 1 octave, up
        let mut out = Vec::with_capacity(256);
        // hold a C-minor triad, render one beat (24k samples = 8 steps at 1/32)
        let c = BlockCtx { frames: 24_000, end_beat: 1.0, ..ctx(24_000) };
        a.process(&c, &[on(60), on(63), on(67)], &mut out);
        let ons: Vec<u8> = out.iter().filter(|e| e.on).map(|e| e.pitch).collect();
        assert!(ons.len() >= 6, "expected several arp steps, got {}", ons.len());
        assert_eq!(&ons[0..3], &[60, 63, 67], "arp must cycle the chord upward");
    }

    #[test]
    fn note_repeat_retriggers() {
        let mut r = NoteRepeat::new();
        r.configure(&[0.7, 0.5]); // 1/16
        let mut out = Vec::with_capacity(256);
        let c = BlockCtx { frames: 24_000, end_beat: 1.0, ..ctx(24_000) };
        r.process(&c, &[on(36)], &mut out);
        let ons = out.iter().filter(|e| e.on && e.pitch == 36).count();
        assert!(ons >= 3, "expected repeated retriggers, got {ons}");
    }
}
