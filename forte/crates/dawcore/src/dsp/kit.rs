//! Kit: a pitch → sample map (drum kit built from recorded takes or bounced
//! composites). Each pad plays its sample at natural speed — no repitching —
//! through the shared amplitude envelope. `kit(C2: kickTake, …)` in the
//! language.
//!
//! The `wrap` layer turns a kit into a RACK: unmapped keys play the wrap
//! sample repitched chromatically from its root (the sampler treatment of
//! the whole composite), while mapped pads keep firing their raw component
//! sounds. `pitch` transposes the wrap layer live — automate it and held
//! wrap notes bend in the audio domain, formants and all.

use std::sync::Arc;

use super::envelope::Adsr;
use super::sampler::Sample;

#[derive(Clone)]
struct KitVoice {
    sample: Option<Arc<Sample>>,
    pos: f64,
    step: f64,
    note: u8,
    active: bool,
    env: Adsr,
    /// note-on velocity as gain (1.0 at velocity 100)
    vel: f32,
    /// wrap-layer voice: repitched, follows live `transpose`
    wrap: bool,
    /// the kit's `transpose` at note-on (see Sampler::tp0)
    tp0: f32,
}

impl KitVoice {
    fn new(sr: f32) -> Self {
        Self {
            sample: None,
            pos: 0.0,
            step: 1.0,
            note: 0,
            active: false,
            env: Adsr::new(sr),
            vel: 1.0,
            wrap: false,
            tp0: 0.0,
        }
    }
}

const KIT_VOICES: usize = 16;

pub struct KitSampler {
    sample_rate: f32,
    voices: Vec<KitVoice>,
    age: [u64; KIT_VOICES],
    clock: u64,
    /// exact pitch → sample (sorted by pitch at build time)
    pub map: Vec<(u8, Arc<Sample>)>,
    /// the rack fallback: unmapped keys play this repitched from its root
    pub wrap: Option<Arc<Sample>>,
    pub gain: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// extra transpose in semitones for the WRAP layer (param-controlled,
    /// automatable — bends held wrap voices live)
    pub transpose: f32,
    /// per-trigger variation 0..1 (same semantics as the sampler's `vary`):
    /// deterministic micro-drift of pitch and level keyed to the trigger
    /// counter — pads stop machine-gunning
    pub vary: f32,
}

impl KitSampler {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            voices: vec![KitVoice::new(sample_rate); KIT_VOICES],
            age: [0; KIT_VOICES],
            clock: 0,
            map: Vec::new(),
            wrap: None,
            gain: 0.8,
            attack: 0.005,
            decay: 0.3,
            sustain: 1.0,
            release: 0.2,
            transpose: 0.0,
            vary: 0.0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        // a pad fires on its exact pitch at natural speed; any OTHER pitch
        // falls through to the wrap layer, repitched from the wrap's root
        let (sample, step, wrap) = match self.map.iter().find(|(p, _)| *p == note) {
            Some((_, sample)) => {
                let step = sample.sample_rate as f64 / self.sample_rate as f64;
                (sample.clone(), step, false)
            }
            None => {
                let Some(sample) = &self.wrap else { return };
                let semis = note as f32 + self.transpose;
                let ratio = super::voice::midi_to_freq(semis.round() as u8)
                    / super::voice::midi_to_freq(sample.root);
                let step = ratio as f64 * (sample.sample_rate as f64 / self.sample_rate as f64);
                (sample.clone(), step, true)
            }
        };
        self.clock += 1;
        // steal the QUIETEST voice, not the oldest — inaudible steals
        let mut idx = 0;
        let mut quietest = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                idx = i;
                break;
            }
            let s = v.env.level() * v.vel;
            if s < quietest {
                quietest = s;
                idx = i;
            }
        }
        let v = &mut self.voices[idx];
        v.sample = Some(sample);
        v.pos = 0.0;
        v.step = step;
        v.wrap = wrap;
        v.tp0 = self.transpose;
        // reconstruct the 0..127 step so velocity 100 lands on exactly 1.0 —
        // songs without accents/ghosts stay bit-identical
        v.vel = ((velocity * 127.0 + 0.5) as u32 as f32 / 100.0).clamp(0.0, 1.27);
        let vary_amt = self.vary.clamp(0.0, 1.0);
        if vary_amt > 0.0 {
            let mut h = (self.clock as u32)
                .wrapping_mul(0x9e37_79b9)
                .wrapping_add((note as u32).wrapping_mul(0x85eb_ca6b))
                .max(1);
            h ^= h << 13;
            h ^= h >> 17;
            h ^= h << 5;
            let r1 = (h as f32 / u32::MAX as f32) * 2.0 - 1.0;
            h ^= h << 13;
            h ^= h >> 17;
            h ^= h << 5;
            let r2 = (h as f32 / u32::MAX as f32) * 2.0 - 1.0;
            v.step *= crate::dmath::powf(2.0, r1 * vary_amt * 0.35 / 12.0) as f64;
            v.vel = (v.vel * (1.0 + r2 * vary_amt * 0.12)).clamp(0.0, 1.27);
        }
        v.note = note;
        v.active = true;
        v.env.set(self.attack, self.decay, self.sustain, self.release);
        v.env.trigger();
        self.age[idx] = self.clock;
    }

    pub fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.active && v.note == note {
                v.env.release();
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.env.release();
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    #[inline]
    #[allow(clippy::should_implement_trait)] // audio-rate tick, not an Iterator
    pub fn next(&mut self) -> f32 {
        let mut sum = 0.0f32;
        for v in &mut self.voices {
            if !v.active {
                continue;
            }
            let Some(sample) = &v.sample else {
                v.active = false;
                continue;
            };
            let data = &sample.data;
            let i = v.pos.floor() as usize;
            let frac = (v.pos - i as f64) as f32;
            let a = data.get(i).copied().unwrap_or(0.0);
            let b = data.get(i + 1).copied().unwrap_or(0.0);
            sum += (a + (b - a) * frac) * v.env.next() * v.vel;

            // live transpose: automating `pitch` bends every held WRAP
            // voice; equal to the note-on transpose multiplies by exactly
            // 1.0, so untouched kits render bit-identically
            let eff = if v.wrap && self.transpose != v.tp0 {
                v.step * crate::dmath::powf(2.0, (self.transpose - v.tp0) / 12.0) as f64
            } else {
                v.step
            };
            v.pos += eff;
            // data end: DECLICK instead of truncating — a bounce that still
            // rings at its cutoff (an echo tail) would pop otherwise
            if v.pos >= data.len() as f64 {
                v.pos = (data.len() as f64 - 1.0).max(0.0);
                v.env.cut();
            }
            if !v.env.is_active() {
                v.active = false;
            }
        }
        sum * self.gain * 0.9
    }
}
