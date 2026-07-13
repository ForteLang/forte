//! Polyphonic synthesiser — the built-in "Polymer" instrument. Fixed voice
//! pool (no allocation on the audio thread) with oldest-voice stealing.

use super::voice::{SynthParams, Voice};

pub const MAX_VOICES: usize = 16;

pub struct PolySynth {
    voices: [Voice; MAX_VOICES],
    /// monotonically increasing trigger stamp, for voice stealing
    age: [u64; MAX_VOICES],
    clock: u64,
    pub params: SynthParams,
}

impl PolySynth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [Voice::new(sample_rate); MAX_VOICES],
            age: [0; MAX_VOICES],
            clock: 0,
            params: SynthParams::default(),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        for v in &mut self.voices {
            v.set_sample_rate(sr);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        self.clock += 1;
        // prefer a free voice, else steal the oldest
        let mut idx = 0;
        let mut oldest_age = u64::MAX;
        let mut found_free = false;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_active() {
                idx = i;
                found_free = true;
                break;
            }
            if self.age[i] < oldest_age {
                oldest_age = self.age[i];
                idx = i;
            }
        }
        let _ = found_free;
        self.voices[idx].trigger(note, velocity, &self.params);
        self.age[idx] = self.clock;
    }

    pub fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.is_active() && v.note() == note {
                v.release();
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.release();
        }
    }

    #[inline]
    #[allow(clippy::should_implement_trait)] // audio-rate tick, not an Iterator
    pub fn next(&mut self) -> f32 {
        let mut sum = 0.0;
        let p = self.params;
        for v in &mut self.voices {
            sum += v.next(&p);
        }
        sum
    }

    /// Stereo tick: with unison off every voice returns its mono tick on
    /// both sides (bit-exact with `next`); with unison on the detuned
    /// stacks fan across the field.
    #[inline]
    pub fn next_lr(&mut self) -> (f32, f32) {
        let mut suml = 0.0;
        let mut sumr = 0.0;
        let p = self.params;
        for v in &mut self.voices {
            let (l, r) = v.next_lr(&p);
            suml += l;
            sumr += r;
        }
        (suml, sumr)
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }
}
