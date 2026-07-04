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
    step: f64,  // per-output-sample advance (pitch ratio; negative = reverse)
    region: (f64, f64), // play region [start, end) in source samples
    looping: bool,
    note: u8,
    active: bool,
    env: Adsr,
}

impl SampleVoice {
    fn new(sr: f32) -> Self {
        Self {
            pos: 0.0,
            step: 1.0,
            region: (0.0, 0.0),
            looping: false,
            note: 0,
            active: false,
            env: Adsr::new(sr),
        }
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
    /// play region as fractions of the sample (sound design: trim a take)
    pub start: f32,
    pub end: f32,
    /// sustain-loop the region while the note is held
    pub loop_on: bool,
    /// play the region backwards
    pub reverse: bool,
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
            start: 0.0,
            end: 1.0,
            loop_on: false,
            reverse: false,
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
        // the trimmed play region, resolved at note-on (param changes don't
        // yank running voices around)
        let len = sample.data.len() as f64;
        let s = (self.start.clamp(0.0, 1.0) as f64 * len).floor();
        let e = ((self.end.clamp(0.0, 1.0) as f64 * len).floor()).clamp(s + 1.0, len);
        let v = &mut self.voices[idx];
        v.region = (s, e);
        v.looping = sample.loop_enabled || self.loop_on;
        v.pos = if self.reverse { e - 1.0 } else { s };
        v.step = ratio as f64 * (sample.sample_rate as f64 / self.sample_rate as f64);
        if self.reverse {
            v.step = -v.step;
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
            let (rs, re) = v.region;
            let span = (re - rs).max(1.0);
            if v.step >= 0.0 {
                if v.pos >= re {
                    if v.looping {
                        v.pos -= span;
                    } else {
                        v.active = false;
                    }
                }
            } else if v.pos < rs {
                if v.looping {
                    v.pos += span;
                } else {
                    v.active = false;
                }
            }
            if !v.env.is_active() {
                v.active = false;
            }
        }
        sum * self.gain * 0.9
    }
}
