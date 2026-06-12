//! The Grid: a compiled node-graph synthesiser. The UI thread compiles a
//! [`GridGraph`](crate::model::GridGraph) into a [`GridSpec`] (topological
//! order + summed input wiring); the audio thread evaluates it per sample for
//! each active voice. No allocation happens after compilation.

use crate::model::{GridConn, GridGraph, GridModuleKind};

use super::envelope::Adsr;
use super::filter::{FilterMode, Svf};
use super::oscillator::{Oscillator, Waveform};
use super::voice::midi_to_freq;

pub const GRID_VOICES: usize = 8;
const MAX_OUTPUTS: usize = 3;

/// Per-node compiled description.
struct NodeSpec {
    kind: GridModuleKind,
    params: Vec<f32>,
    /// For each input port: the list of source (node, output port) pairs (summed).
    inputs: Vec<Vec<(usize, usize)>>,
}

/// Per-voice, per-node runtime state.
#[derive(Clone, Copy)]
enum NodeState {
    None,
    Osc(Oscillator),
    Adsr { env: Adsr, prev_gate: f32 },
    Filter(Svf),
    Lfo { phase: f32 },
}

struct GridVoice {
    states: Vec<NodeState>,
    note: u8,
    velocity: f32,
    gate: f32,
    active: bool,
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

fn fresh_state(kind: GridModuleKind, sr: f32) -> NodeState {
    match kind {
        GridModuleKind::Osc => NodeState::Osc(Oscillator::default()),
        GridModuleKind::Adsr => NodeState::Adsr { env: Adsr::new(sr), prev_gate: 0.0 },
        GridModuleKind::Filter => NodeState::Filter(Svf::new(sr)),
        GridModuleKind::Lfo => NodeState::Lfo { phase: 0.0 },
        _ => NodeState::None,
    }
}

impl GridSynth {
    /// Compile a graph. Runs on the UI thread.
    pub fn compile(graph: &GridGraph, sample_rate: f32) -> Self {
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
            nodes.push(NodeSpec { kind: m.kind, params: m.params.clone(), inputs });
        }
        let order = topo_order(n, &graph.conns);
        let out_node = graph.modules.iter().position(|m| m.kind == GridModuleKind::Out);
        let has_adsr = graph.modules.iter().any(|m| m.kind == GridModuleKind::Adsr);

        let voices = (0..GRID_VOICES)
            .map(|_| GridVoice {
                states: graph.modules.iter().map(|m| fresh_state(m.kind, sample_rate)).collect(),
                note: 0,
                velocity: 0.0,
                gate: 0.0,
                active: false,
            })
            .collect();

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
            *st = fresh_state(self.nodes[si].kind, sr);
        }
        v.note = note;
        v.velocity = velocity.clamp(0.0, 1.0);
        v.gate = 1.0;
        v.active = true;
        self.age[idx] = self.clock;
    }

    pub fn note_off(&mut self, note: u8) {
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
    pub fn next(&mut self) -> f32 {
        let Some(out_node) = self.out_node else { return 0.0 };
        let mut sum = 0.0f32;
        let sr = self.sample_rate;

        for vi in 0..self.voices.len() {
            if !self.voices[vi].active {
                continue;
            }
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
                let params = &self.nodes[ni].params;
                let out = &mut self.values[ni];
                match kind {
                    GridModuleKind::NoteIn => {
                        out[0] = midi_to_freq(voice.note);
                        out[1] = voice.gate;
                        out[2] = voice.velocity;
                    }
                    GridModuleKind::Osc => {
                        let freq = if connected[0] { ins[0].max(0.1) } else { 220.0 };
                        let shape = Waveform::from_index((params[0] * 3.999) as u8);
                        if let NodeState::Osc(osc) = &mut voice.states[ni] {
                            out[0] = osc.next(freq, sr, shape);
                        }
                    }
                    GridModuleKind::Lfo => {
                        if let NodeState::Lfo { phase } = &mut voice.states[ni] {
                            let hz = 0.05 + params[0] * 12.0;
                            let shape = (params[1] * 3.999) as u8;
                            out[0] = match shape {
                                1 => 1.0 - 4.0 * (*phase - 0.5).abs(),
                                2 => *phase * 2.0 - 1.0,
                                3 => if *phase < 0.5 { 1.0 } else { -1.0 },
                                _ => (*phase * std::f32::consts::TAU).sin(),
                            };
                            *phase = (*phase + hz / sr).fract();
                        }
                    }
                    GridModuleKind::Adsr => {
                        if let NodeState::Adsr { env, prev_gate } = &mut voice.states[ni] {
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
                        if let NodeState::Filter(svf) = &mut voice.states[ni] {
                            let base = 30.0 * 600.0_f32.powf(params[0].clamp(0.0, 1.0));
                            // cutoff mod input shifts up to ±4 octaves
                            let cutoff = base * 2.0_f32.powf(ins[1] * 4.0);
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
