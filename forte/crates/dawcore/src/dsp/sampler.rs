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
    /// glide target: `step` slews toward this exponentially (the 808 slide)
    step_target: f64,
    /// per-sample multiplicative slew factor (1.0 = no glide in flight)
    step_factor: f64,
    /// gate currently held (note-on seen, no note-off yet) — glide engages
    /// only into a held voice, the tie/legato semantics of the 303
    gate: bool,
    /// the sampler's `transpose` at note-on — automating `pitch` bends the
    /// running voice relative to this, so a CONSTANT transpose multiplies by
    /// exactly 1.0 and leaves the sound bit-identical
    tp0: f32,
    region: (f64, f64), // play region [start, end) in source samples
    looping: bool,
    note: u8,
    active: bool,
    env: Adsr,
    /// note-on velocity as gain (1.0 at velocity 100, so accents lift and
    /// ghosts duck around the nominal level)
    vel: f32,
}

impl SampleVoice {
    fn new(sr: f32) -> Self {
        Self {
            pos: 0.0,
            step: 1.0,
            step_target: 1.0,
            step_factor: 1.0,
            gate: false,
            tp0: 0.0,
            region: (0.0, 0.0),
            looping: false,
            vel: 1.0,
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
    /// glide time in seconds: >0 makes overlapping notes slide the pitch of
    /// the running voice (mono/legato, the 808/303 slide) instead of
    /// retriggering
    pub glide: f32,
    /// slice mode: >0 chops the play region into this many equal slices;
    /// the incoming note picks the slice (root = slice 0, root+1 = slice 1,
    /// wrapping) and plays it at ORIGINAL speed — the MPC chop
    pub slices: u8,
    /// choke: every new trigger hard-cuts all running voices (~3 ms fade) —
    /// the MPC mono pad. The unnatural cut and the silence it leaves IS the
    /// groove; without it overlapping slices smear into legato mush.
    pub choke: bool,
    /// per-trigger variation 0..1: deterministic micro-drift of pitch
    /// (±35 cents at 1.0) and level (±12%) keyed to the trigger counter, so
    /// no two hits are bit-identical — kills the machine-gun effect that
    /// makes repeated samples read as MIDI
    pub vary: f32,
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
            glide: 0.0,
            slices: 0,
            choke: false,
            vary: 0.0,
        }
    }

    /// Per-trigger drift for `vary`: two deterministic −1..1 draws keyed to
    /// the trigger counter and note (xorshift32, same family as the noise
    /// node) — the same song renders the same bits everywhere.
    fn vary_draws(&self, note: u8) -> (f32, f32) {
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
        (r1, r2)
    }

    /// choke: hard-cut every running voice before the new trigger starts
    fn choke_voices(&mut self) {
        for v in &mut self.voices {
            if v.active {
                v.env.cut();
                v.gate = false;
            }
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        for v in &mut self.voices {
            v.env.set_sample_rate(sr);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: f32) {
        let Some(sample) = self.sample.clone() else { return };
        self.clock += 1;
        // vary: draws are taken even when the amount is 0 so the code path
        // stays branch-light; they only touch the voice when vary_amt > 0
        let vary_amt = self.vary.clamp(0.0, 1.0);
        let (r1, r2) = self.vary_draws(note);
        // slice mode: the note picks a chunk of the region, played at
        // original speed — the unnatural cut is the instrument
        if self.slices > 0 {
            let len = sample.data.len() as f64;
            let rs = (self.start.clamp(0.0, 1.0) as f64 * len).floor();
            let re = ((self.end.clamp(0.0, 1.0) as f64 * len).floor()).clamp(rs + 1.0, len);
            let n = self.slices as f64;
            // slice 0 sits at the note that plays the sample untouched
            let played = (note as f32 + self.transpose).round() as i32;
            let idx = (played - sample.root as i32).rem_euclid(self.slices as i32) as f64;
            let w = (re - rs) / n;
            let (ss, se) = (rs + idx * w, rs + (idx + 1.0) * w);
            if self.choke {
                self.choke_voices();
            }
            let mut best = 0;
            let mut oldest = u64::MAX;
            for (i, v) in self.voices.iter().enumerate() {
                if !v.active {
                    best = i;
                    break;
                }
                if self.age[i] < oldest {
                    oldest = self.age[i];
                    best = i;
                }
            }
            let v = &mut self.voices[best];
            v.region = (ss, se);
            v.looping = false;
            v.pos = if self.reverse { se - 1.0 } else { ss };
            v.step = sample.sample_rate as f64 / self.sample_rate as f64;
            if self.reverse {
                v.step = -v.step;
            }
            v.step_target = v.step;
            v.step_factor = 1.0;
            v.gate = true;
            v.tp0 = self.transpose;
            v.vel = ((velocity * 127.0 + 0.5) as u32 as f32 / 100.0).clamp(0.0, 1.27);
            if vary_amt > 0.0 {
                v.step *= crate::dmath::powf(2.0, r1 * vary_amt * 0.35 / 12.0) as f64;
                v.step_target = v.step;
                v.vel = (v.vel * (1.0 + r2 * vary_amt * 0.12)).clamp(0.0, 1.27);
            }
            v.note = note;
            v.active = true;
            v.env.set(self.attack, self.decay, self.sustain, self.release);
            v.env.trigger();
            self.age[best] = self.clock;
            return;
        }
        // glide: a new note while another is HELD slides the running voice's
        // pitch instead of starting a new one (mono legato, the 808 slide)
        if self.glide > 0.0001 {
            let semis = note as f32 + self.transpose;
            let ratio = midi_to_freq(semis.round() as u8) / midi_to_freq(sample.root);
            let target = {
                let t = ratio as f64 * (sample.sample_rate as f64 / self.sample_rate as f64);
                if self.reverse { -t } else { t }
            };
            if let Some(v) = self.voices.iter_mut().filter(|v| v.active && v.gate).last() {
                v.step_target = target;
                let n = (self.glide as f64 * self.sample_rate as f64).max(1.0);
                // exponential slew: equal semitone speed regardless of range
                v.step_factor = crate::dmath::powf((target / v.step) as f32, (1.0 / n) as f32) as f64;
                if !v.step_factor.is_finite() || v.step_factor <= 0.0 {
                    v.step = target;
                    v.step_factor = 1.0;
                }
                v.tp0 = self.transpose;
                v.note = note;
                return;
            }
        }
        if self.choke {
            self.choke_voices();
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
        v.step_target = v.step;
        v.step_factor = 1.0;
        v.gate = true;
        v.tp0 = self.transpose;
        // velocity arrives 0..1 (127-normalised); 100/127 → unity so plain
        // `x` hits keep their nominal level, `X` lifts, `.` ghosts duck
        // reconstruct the 0..127 step so velocity 100 lands on exactly 1.0 —
        // songs without accents/ghosts stay bit-identical
        v.vel = ((velocity * 127.0 + 0.5) as u32 as f32 / 100.0).clamp(0.0, 1.27);
        if vary_amt > 0.0 {
            v.step *= crate::dmath::powf(2.0, r1 * vary_amt * 0.35 / 12.0) as f64;
            v.step_target = v.step;
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
                v.gate = false;
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.env.release();
            v.gate = false;
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    #[inline]
    #[allow(clippy::should_implement_trait)] // audio-rate tick, not an Iterator
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
            sum += s * amp * v.vel;

            if v.step_factor != 1.0 {
                v.step *= v.step_factor;
                // stop on crossing the target (either direction)
                if (v.step_factor > 1.0 && v.step >= v.step_target)
                    || (v.step_factor < 1.0 && v.step <= v.step_target)
                {
                    v.step = v.step_target;
                    v.step_factor = 1.0;
                }
            }
            // live transpose: automating `pitch` bends every held voice.
            // Equal to the note-on transpose → multiply by exactly 1.0, so a
            // constant `pitch` renders bit-identically to before this existed.
            let eff = if self.transpose == v.tp0 {
                v.step
            } else {
                v.step * crate::dmath::powf(2.0, (self.transpose - v.tp0) / 12.0) as f64
            };
            v.pos += eff;
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
