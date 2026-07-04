//! The Grid: a compiled node-graph synthesiser. The UI thread compiles a
//! [`GridGraph`](crate::model::GridGraph) into a [`GridSpec`] (topological
//! order + summed input wiring); the audio thread evaluates it per sample for
//! each active voice. No allocation happens after compilation.

use std::sync::Arc;

use crate::model::{GridConn, GridGraph, GridModuleKind};

use super::envelope::Adsr;
use super::filter::{FilterMode, Svf};
use super::oscillator::{Oscillator, Waveform};
use super::sampler::Sample;
use super::voice::midi_to_freq;

pub const GRID_VOICES: usize = 8;
const MAX_OUTPUTS: usize = 3;

/// Per-node compiled description.
struct NodeSpec {
    kind: GridModuleKind,
    params: Vec<f32>,
    /// For each input port: the list of source (node, output port) pairs (summed).
    inputs: Vec<Vec<(usize, usize)>>,
    /// Resolved audio buffer for a Sample node (None for every other kind).
    sample: Option<Arc<Sample>>,
}

/// Per-voice, per-node runtime state.
#[derive(Clone)]
enum NodeState {
    None,
    Osc(Oscillator),
    Adsr { env: Adsr, prev_gate: f32 },
    Filter(Svf),
    Lfo { phase: f32 },
    /// xorshift32 state — deterministic noise, reseeded per note-on so the
    /// same source renders the same bits everywhere.
    Noise(u32),
    /// read head into the node's sample; pos < 0 = playback finished
    Sample { pos: f64, started: bool },
}

struct GridVoice {
    states: Vec<NodeState>,
    note: u8,
    velocity: f32,
    gate: f32,
    active: bool,
    /// current (possibly gliding) frequency; only meaningful in mono mode
    freq_cur: f32,
}

pub struct GridSynth {
    sample_rate: f32,
    nodes: Vec<NodeSpec>,
    order: Vec<usize>,
    out_node: Option<usize>,
    has_adsr: bool,
    voices: Vec<GridVoice>,
    age: Vec<u64>,
    clock: u64,
    /// scratch: per-node output values, reused across voices each sample
    values: Vec<[f32; MAX_OUTPUTS]>,
    /// mono/legato mode (graph.glide > 0): one voice, overlapping notes glide
    mono: bool,
    glide_coef: f32,
}

fn topo_order(n: usize, conns: &[GridConn]) -> Vec<usize> {
    // Kahn's algorithm; nodes stuck in a cycle are appended in index order so
    // evaluation still terminates (their inputs read last sample's values).
    let mut indeg = vec![0usize; n];
    for c in conns {
        if c.to.0 < n && c.from.0 < n {
            indeg[c.to.0] += 1;
        }
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    let mut qi = 0;
    while qi < queue.len() {
        let v = queue[qi];
        qi += 1;
        order.push(v);
        for c in conns {
            if c.from.0 == v && c.to.0 < n {
                indeg[c.to.0] -= 1;
                if indeg[c.to.0] == 0 {
                    queue.push(c.to.0);
                }
            }
        }
    }
    for i in 0..n {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    order
}

fn fresh_state(kind: GridModuleKind, sr: f32, node_idx: usize) -> NodeState {
    match kind {
        GridModuleKind::Osc => NodeState::Osc(Oscillator::default()),
        GridModuleKind::Adsr => NodeState::Adsr { env: Adsr::new(sr), prev_gate: 0.0 },
        GridModuleKind::Filter => NodeState::Filter(Svf::new(sr)),
        GridModuleKind::Lfo => NodeState::Lfo { phase: 0.0 },
        // two noise nodes in one patch must not correlate: seed by node index
        GridModuleKind::Noise => {
            NodeState::Noise(0x9e37_79b9 ^ (node_idx as u32).wrapping_mul(0x85eb_ca6b))
        }
        GridModuleKind::Sample => NodeState::Sample { pos: 0.0, started: false },
        _ => NodeState::None,
    }
}

/// Compile a graph into node specs + evaluation order (shared by the poly
/// synth and the audio-effect interpreter).
fn build_specs(graph: &GridGraph) -> (Vec<NodeSpec>, Vec<usize>, Option<usize>) {
    let n = graph.modules.len();
    let mut nodes = Vec::with_capacity(n);
    for (i, m) in graph.modules.iter().enumerate() {
        let n_inputs = m.kind.inputs().len();
        let mut inputs = vec![Vec::new(); n_inputs];
        for c in &graph.conns {
            if c.to.0 == i && c.to.1 < n_inputs && c.from.0 < n {
                inputs[c.to.1].push((c.from.0, c.from.1.min(MAX_OUTPUTS - 1)));
            }
        }
        nodes.push(NodeSpec {
            kind: m.kind,
            params: m.params.clone(),
            inputs,
            sample: m.sample.as_ref().and_then(crate::engine::resolve_sample),
        });
    }
    let order = topo_order(n, &graph.conns);
    let out_node = graph.modules.iter().position(|m| m.kind == GridModuleKind::Out);
    (nodes, order, out_node)
}

/// One node, one sample. `note` feeds NoteIn (zeros in an effect context);
/// `audio_in` feeds AudioIn (zero in an instrument context).
#[allow(clippy::too_many_arguments)]
#[inline]
fn eval_node(
    kind: GridModuleKind,
    params: &[f32],
    ins: &[f32; 4],
    connected: &[bool; 4],
    state: &mut NodeState,
    out: &mut [f32; MAX_OUTPUTS],
    sr: f32,
    note: (f32, f32, f32),
    audio_in: f32,
    sample: Option<&Arc<Sample>>,
) {
    match kind {
        GridModuleKind::Sample => {
            out[0] = 0.0;
            let (NodeState::Sample { pos, started }, Some(smp)) = (state, sample) else {
                return;
            };
            let len = smp.data.len() as f64;
            if len < 1.0 {
                return;
            }
            // region [s, e) from Start/End params; direction and loop as flags
            let s = (params[0].clamp(0.0, 1.0) as f64 * len).floor();
            let e = ((params[1].clamp(0.0, 1.0) as f64 * len).floor()).clamp(s + 1.0, len);
            let looping = params[2] > 0.5;
            let reverse = params[3] > 0.5;
            if !*started {
                *pos = if reverse { e - 1.0 } else { s };
                *started = true;
            }
            if *pos < 0.0 {
                return; // one-shot playback finished
            }
            let i = pos.floor() as usize;
            let frac = (*pos - i as f64) as f32;
            let a = smp.data.get(i).copied().unwrap_or(0.0);
            let b = smp.data.get(i + 1).copied().unwrap_or(0.0);
            out[0] = a + (b - a) * frac;
            // repitch against the sample's root (assets root at C4); in an
            // effect graph note.freq is 0 → play at natural speed
            let ratio =
                if note.0 > 0.0 { (note.0 / midi_to_freq(smp.root)) as f64 } else { 1.0 };
            let step = ratio * (smp.sample_rate as f64 / sr as f64);
            *pos += if reverse { -step } else { step };
            let span = e - s;
            if reverse {
                if *pos < s {
                    *pos = if looping { *pos + span } else { -1.0 };
                }
            } else if *pos >= e {
                *pos = if looping { *pos - span } else { -1.0 };
            }
        }
        GridModuleKind::NoteIn => {
            out[0] = note.0;
            out[1] = note.1;
            out[2] = note.2;
        }
        GridModuleKind::AudioIn => {
            out[0] = audio_in;
        }
        GridModuleKind::Osc => {
            let base = if connected[0] { ins[0].max(0.1) } else { 220.0 };
            // pitch mod shifts up to ±4 octaves (mirrors the SVF's
            // cutoff mod) — envelopes make kick drops, LFOs vibrato
            let freq = base * crate::dmath::powf(2.0, ins[1] * 4.0);
            let shape = Waveform::from_index((params[0] * 4.999) as u8);
            // pulse width: base param plus ±0.45 of modulation (PWM)
            let pw_base = if params.len() > 1 { params[1] } else { 0.5 };
            let pw = pw_base + ins[2] * 0.45;
            if let NodeState::Osc(osc) = state {
                out[0] = osc.next_pw(freq, sr, shape, pw);
            }
        }
        GridModuleKind::Noise => {
            if let NodeState::Noise(s) = state {
                *s ^= *s << 13;
                *s ^= *s >> 17;
                *s ^= *s << 5;
                out[0] = (*s as f32 / u32::MAX as f32) * 2.0 - 1.0;
            }
        }
        GridModuleKind::Shaper => {
            let drive = (params[0] + ins[1]).clamp(0.0, 1.0);
            let x = ins[0] * (1.0 + drive * 15.0);
            out[0] = match (params[1] * 2.999) as u8 {
                1 => x.clamp(-1.0, 1.0), // hard clip
                2 => {
                    // triangle wavefolder: reflects instead of clipping
                    let t = (x * 0.25 + 0.25).rem_euclid(1.0);
                    4.0 * (t - 0.5).abs() - 1.0
                }
                _ => crate::dmath::tanh(x),
            };
        }
        GridModuleKind::Lfo => {
            if let NodeState::Lfo { phase } = state {
                let hz = 0.05 + params[0] * 12.0;
                let shape = (params[1] * 3.999) as u8;
                out[0] = match shape {
                    1 => 1.0 - 4.0 * (*phase - 0.5).abs(),
                    2 => *phase * 2.0 - 1.0,
                    3 => if *phase < 0.5 { 1.0 } else { -1.0 },
                    _ => crate::dmath::sin(*phase * std::f32::consts::TAU),
                };
                *phase = (*phase + hz / sr).fract();
            }
        }
        GridModuleKind::Adsr => {
            if let NodeState::Adsr { env, prev_gate } = state {
                let gate = ins[0];
                if gate > 0.5 && *prev_gate <= 0.5 {
                    env.set(
                        0.001 + params[0] * params[0] * 2.0,
                        0.001 + params[1] * params[1] * 2.0,
                        params[2],
                        0.001 + params[3] * params[3] * 2.5,
                    );
                    env.trigger();
                } else if gate <= 0.5 && *prev_gate > 0.5 {
                    env.release();
                }
                *prev_gate = gate;
                out[0] = env.next();
            }
        }
        GridModuleKind::Filter => {
            if let NodeState::Filter(svf) = state {
                let base = 30.0 * crate::dmath::powf(600.0, params[0].clamp(0.0, 1.0));
                // cutoff mod input shifts up to ±4 octaves
                let cutoff = base * crate::dmath::powf(2.0, ins[1] * 4.0);
                svf.set(cutoff, params[1]);
                out[0] = svf.process(ins[0], FilterMode::Lowpass);
            }
        }
        GridModuleKind::Gain => {
            let m = if connected[1] { ins[1].clamp(0.0, 2.0) } else { 1.0 };
            out[0] = ins[0] * params[0] * m;
        }
        GridModuleKind::Mix => {
            out[0] = ins[0] + ins[1];
        }
        GridModuleKind::Out => {
            out[0] = ins[0];
        }
    }
}

impl GridSynth {
    /// Compile a graph. Runs on the UI thread.
    pub fn compile(graph: &GridGraph, sample_rate: f32) -> Self {
        let (nodes, order, out_node) = build_specs(graph);
        let has_adsr = graph.modules.iter().any(|m| m.kind == GridModuleKind::Adsr);

        let voices = (0..GRID_VOICES)
            .map(|_| GridVoice {
                states: graph
                    .modules
                    .iter()
                    .enumerate()
                    .map(|(i, m)| fresh_state(m.kind, sample_rate, i))
                    .collect(),
                note: 0,
                velocity: 0.0,
                gate: 0.0,
                active: false,
                freq_cur: 0.0,
            })
            .collect();

        let n = nodes.len();
        // one-pole coefficient for the mono glide (0 = poly, no smoothing)
        let glide_coef = if graph.glide > 0.0 {
            1.0 - crate::dmath::exp(-1.0 / (graph.glide.max(0.001) * sample_rate))
        } else {
            0.0
        };
        GridSynth {
            sample_rate,
            nodes,
            order,
            out_node,
            has_adsr,
            voices,
            age: vec![0; GRID_VOICES],
            clock: 0,
            values: vec![[0.0; MAX_OUTPUTS]; n],
            mono: graph.glide > 0.0,
            glide_coef,
        }
    }

    pub fn set_param(&mut self, node: usize, param: usize, value: f32) {
        if let Some(n) = self.nodes.get_mut(node) {
            if let Some(p) = n.params.get_mut(param) {
                *p = value;
            }
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.clock += 1;
        if self.mono {
            let v = &mut self.voices[0];
            if v.active && v.gate > 0.0 {
                // legato: retarget the pitch, keep envelopes running — the slide
                v.note = note;
                v.velocity = velocity.clamp(0.0, 1.0);
                self.age[0] = self.clock;
                return;
            }
            let sr = self.sample_rate;
            for (si, st) in v.states.iter_mut().enumerate() {
                *st = fresh_state(self.nodes[si].kind, sr, si);
            }
            v.note = note;
            v.velocity = velocity.clamp(0.0, 1.0);
            v.gate = 1.0;
            v.active = true;
            v.freq_cur = midi_to_freq(note);
            self.age[0] = self.clock;
            return;
        }
        let mut idx = 0;
        let mut oldest = u64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                idx = i;
                break;
            }
            if self.age[i] < oldest {
                oldest = self.age[i];
                idx = i;
            }
        }
        let sr = self.sample_rate;
        let v = &mut self.voices[idx];
        for (si, st) in v.states.iter_mut().enumerate() {
            *st = fresh_state(self.nodes[si].kind, sr, si);
        }
        v.note = note;
        v.velocity = velocity.clamp(0.0, 1.0);
        v.gate = 1.0;
        v.active = true;
        self.age[idx] = self.clock;
    }

    pub fn note_off(&mut self, note: u8) {
        if self.mono {
            let v = &mut self.voices[0];
            // releases of already-superseded notes are ignored — that overlap
            // IS the tie that makes a slide
            if v.active && v.note == note {
                v.gate = 0.0;
                if !self.has_adsr {
                    v.active = false;
                }
            }
            return;
        }
        for v in &mut self.voices {
            if v.active && v.note == note {
                v.gate = 0.0;
                if !self.has_adsr {
                    v.active = false;
                }
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.gate = 0.0;
            if !self.has_adsr {
                v.active = false;
            }
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    #[inline]
    #[allow(clippy::should_implement_trait)] // audio-rate tick, not an Iterator
    pub fn next(&mut self) -> f32 {
        let Some(out_node) = self.out_node else { return 0.0 };
        let mut sum = 0.0f32;
        let sr = self.sample_rate;

        for vi in 0..self.voices.len() {
            if !self.voices[vi].active {
                continue;
            }
            // advance the mono glide once per voice per sample (not per node)
            let voice_freq = if self.mono {
                let v = &mut self.voices[vi];
                let target = midi_to_freq(v.note);
                v.freq_cur += (target - v.freq_cur) * self.glide_coef;
                v.freq_cur
            } else {
                midi_to_freq(self.voices[vi].note)
            };
            // evaluate graph in topological order
            for oi in 0..self.order.len() {
                let ni = self.order[oi];
                let kind = self.nodes[ni].kind;

                // gather summed inputs (reads self.values written this sample,
                // or last sample's value for cycle back-edges)
                let mut ins = [0.0f32; 4];
                let mut connected = [false; 4];
                for (port, sources) in self.nodes[ni].inputs.iter().enumerate() {
                    for &(sn, sp) in sources {
                        ins[port] += self.values[sn][sp];
                        connected[port] = true;
                    }
                }

                let voice = &mut self.voices[vi];
                let note = (voice_freq, voice.gate, voice.velocity);
                eval_node(
                    kind,
                    &self.nodes[ni].params,
                    &ins,
                    &connected,
                    &mut voice.states[ni],
                    &mut self.values[ni],
                    sr,
                    note,
                    0.0,
                    self.nodes[ni].sample.as_ref(),
                );
            }

            let s = self.values[out_node][0];
            let voice = &mut self.voices[vi];
            sum += s * voice.velocity.max(0.05);

            // free the voice when released and every envelope has decayed away
            if voice.gate <= 0.0 {
                let mut still = false;
                for st in &voice.states {
                    if let NodeState::Adsr { env, .. } = st {
                        if env.is_active() {
                            still = true;
                            break;
                        }
                    }
                }
                if !still {
                    voice.active = false;
                }
            }
        }
        sum * 0.25
    }
}

// ---------------------------------------------------------------------------
// GridFx: the same node graph as an audio effect (`device X : Effect`).
// The signal enters through AudioIn; each stereo channel runs its own copy of
// the node states so filters/LFO phases stay per-channel.
// ---------------------------------------------------------------------------

pub struct GridFx {
    sample_rate: f32,
    nodes: Vec<NodeSpec>,
    order: Vec<usize>,
    out_node: Option<usize>,
    states: [Vec<NodeState>; 2],
    values: Vec<[f32; MAX_OUTPUTS]>,
}

impl GridFx {
    pub fn compile(graph: &GridGraph, sample_rate: f32) -> Self {
        let (nodes, order, out_node) = build_specs(graph);
        let states: Vec<NodeState> = graph
            .modules
            .iter()
            .enumerate()
            .map(|(i, m)| fresh_state(m.kind, sample_rate, i))
            .collect();
        let n = nodes.len();
        GridFx {
            sample_rate,
            nodes,
            order,
            out_node,
            states: [states.clone(), states],
            values: vec![[0.0; MAX_OUTPUTS]; n],
        }
    }

    #[inline]
    fn chan(&mut self, ch: usize, x: f32) -> f32 {
        let Some(out_node) = self.out_node else { return x };
        for oi in 0..self.order.len() {
            let ni = self.order[oi];
            let mut ins = [0.0f32; 4];
            let mut connected = [false; 4];
            for (port, sources) in self.nodes[ni].inputs.iter().enumerate() {
                for &(sn, sp) in sources {
                    ins[port] += self.values[sn][sp];
                    connected[port] = true;
                }
            }
            let mut out = self.values[ni];
            eval_node(
                self.nodes[ni].kind,
                &self.nodes[ni].params,
                &ins,
                &connected,
                &mut self.states[ch][ni],
                &mut out,
                self.sample_rate,
                (0.0, 0.0, 0.0),
                x,
                self.nodes[ni].sample.as_ref(),
            );
            self.values[ni] = out;
        }
        self.values[out_node][0]
    }
}

impl crate::device::AudioFx for GridFx {
    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.chan(0, l), self.chan(1, r))
    }
    fn configure(&mut self, _params: &[f32]) {
        // params are baked into the graph at compile time (like PolyGrid)
    }
}
