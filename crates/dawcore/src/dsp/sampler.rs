//! Sampler: pitched, looping playback of a shared audio buffer with linear
//! interpolation and an amplitude envelope. The buffer is shared via `Arc` so
//! the audio thread never copies or allocates sample data.

use std::sync::Arc;

use super::envelope::Adsr;
use super::voice::midi_to_freq;

/// Immutable mono sample data plus its natural pitch and loop region.
pub struct Sample {
    pub data: Arc<[f32]>,
    pub sample_rate: f32,
    /// MIDI note at which the sample plays back at its recorded pitch.
    pub root: u8,
    pub loop_enabled: bool,
    pub loop_start: usize,
    pub loop_end: usize,
}

impl Sample {
    pub fn one_shot(data: Arc<[f32]>, sample_rate: f32, root: u8) -> Self {
        let len = data.len();
        Self { data, sample_rate, root, loop_enabled: false, loop_start: 0, loop_end: len }
    }
}

#[derive(Clone, Copy)]
struct SampleVoice {
    pos: f64,   // fractional read position in source samples
    step: f64,  // per-output-sample advance (pitch ratio)
    note: u8,
    active: bool,
    env: Adsr,
}

impl SampleVoice {
    fn new(sr: f32) -> Self {
        Self { pos: 0.0, step: 1.0, note: 0, active: false, env: Adsr::new(sr) }
    }
}

const SAMPLER_VOICES: usize = 16;

pub struct Sampler {
    sample_rate: f32,
    voices: [SampleVoice; SAMPLER_VOICES],
    age: [u64; SAMPLER_VOICES],
    clock: u64,
    pub sample: Option<Arc<Sample>>,
    pub gain: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// extra transpose in semitones (param-controlled)
    pub transpose: f32,
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            voices: [SampleVoice::new(sample_rate); SAMPLER_VOICES],
            age: [0; SAMPLER_VOICES],
            clock: 0,
            sample: None,
            gain: 0.8,
            attack: 0.002,
            decay: 0.2,
            sustain: 0.8,
            release: 0.15,
            transpose: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        for v in &mut self.voices {
            v.env.set_sample_rate(sr);
        }
    }

    pub fn note_on(&mut self, note: u8, _velocity: f32) {
        let Some(sample) = &self.sample else { return };
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
        // pitch ratio: target freq vs root freq, times source/host sample-rate.
        let semis = note as f32 + self.transpose;
        let ratio = midi_to_freq(semis.round() as u8) / midi_to_freq(sample.root);
        let v = &mut self.voices[idx];
        v.pos = sample.loop_start as f64 * 0.0; // start at 0
        v.pos = 0.0;
        v.step = ratio as f64 * (sample.sample_rate as f64 / self.sample_rate as f64);
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
    pub fn next(&mut self) -> f32 {
        let Some(sample) = &self.sample else { return 0.0 };
        let data = &sample.data;
        if data.is_empty() {
            return 0.0;
        }
        let mut sum = 0.0f32;
        for v in &mut self.voices {
            if !v.active {
                continue;
            }
            // linear interpolation
            let i = v.pos.floor() as usize;
            let frac = (v.pos - i as f64) as f32;
            let a = data.get(i).copied().unwrap_or(0.0);
            let b = data.get(i + 1).copied().unwrap_or(0.0);
            let s = a + (b - a) * frac;

            let amp = v.env.next();
            sum += s * amp;

            v.pos += v.step;
            if sample.loop_enabled && v.pos >= sample.loop_end as f64 {
                let span = (sample.loop_end - sample.loop_start).max(1) as f64;
                v.pos -= span;
            } else if v.pos >= data.len() as f64 {
                v.active = false;
            }
            if !v.env.is_active() {
                v.active = false;
            }
        }
        sum * self.gain * 0.9
    }
}
