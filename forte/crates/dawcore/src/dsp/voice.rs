//! A single polyphonic synth voice: two detuned oscillators + sub, through a
//! state-variable filter shaped by its own envelope, then an amp envelope.

use super::envelope::Adsr;
use super::filter::{FilterMode, Svf};
use super::oscillator::{Oscillator, Waveform};

#[derive(Clone, Copy)]
pub struct SynthParams {
    pub wave: Waveform,
    pub cutoff: f32,   // 0..1 normalised
    pub resonance: f32,
    pub attack: f32,   // 0..1
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub detune: f32,   // 0..1
    pub sub: f32,      // 0..1
    pub filter_env: f32, // 0..1 amount filter env adds to cutoff
    /// 0..1 → 1..7 unison voices. At ≥2 the classic osc pair is REPLACED by
    /// n oscillators fanned by `detune` and panned by `spread` — the width
    /// of modern melodic production. 0 keeps the bit-exact mono voice.
    pub unison: f32,
    /// 0..1 stereo fan of the unison stack (0 = all center)
    pub spread: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            wave: Waveform::Saw,
            cutoff: 0.6,
            resonance: 0.15,
            attack: 0.01,
            decay: 0.3,
            sustain: 0.6,
            release: 0.25,
            detune: 0.12,
            sub: 0.3,
            filter_env: 0.4,
            unison: 0.0,
            spread: 0.5,
        }
    }
}

/// knob 0..1 → 1..7 voices
#[inline]
pub fn unison_count(v: f32) -> usize {
    1 + (v.clamp(0.0, 1.0) * 6.0).round() as usize
}

/// maximum unison detune fan at `detune` = 1.0, in cents to each side
const UNISON_CENTS: f32 = 45.0;

fn norm_to_seconds(v: f32) -> f32 {
    // perceptually-ish: 1ms .. ~4s
    0.001 + v * v * 4.0
}

fn norm_to_cutoff(v: f32) -> f32 {
    // 30 Hz .. ~18 kHz, exponential
    30.0 * crate::dmath::powf(600.0, v.clamp(0.0, 1.0))
}

pub const MAX_UNISON: usize = 7;

#[derive(Clone, Copy)]
pub struct Voice {
    sample_rate: f32,
    osc1: Oscillator,
    osc2: Oscillator,
    sub_osc: Oscillator,
    /// the unison stack (used when params.unison ≥ 2 voices)
    uni: [Oscillator; MAX_UNISON],
    filter: Svf,
    /// right-channel filter — only ticked on the stereo unison path, so the
    /// mono path stays bit-exact
    filter_r: Svf,
    amp_env: Adsr,
    filt_env: Adsr,
    freq: f32,
    velocity: f32,
    note: u8,
    active: bool,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            osc1: Oscillator::default(),
            osc2: Oscillator::default(),
            sub_osc: Oscillator::default(),
            uni: [Oscillator::default(); MAX_UNISON],
            filter: Svf::new(sample_rate),
            filter_r: Svf::new(sample_rate),
            amp_env: Adsr::new(sample_rate),
            filt_env: Adsr::new(sample_rate),
            freq: 440.0,
            velocity: 1.0,
            note: 0,
            active: false,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.filter.set_sample_rate(sr);
        self.filter_r.set_sample_rate(sr);
        self.amp_env.set_sample_rate(sr);
        self.filt_env.set_sample_rate(sr);
    }

    pub fn note(&self) -> u8 {
        self.note
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn trigger(&mut self, note: u8, velocity: f32, p: &SynthParams) {
        self.note = note;
        self.freq = midi_to_freq(note);
        self.velocity = velocity.clamp(0.0, 1.0);
        self.osc2.set_phase(0.33);
        self.sub_osc.set_phase(0.0);
        // golden-ratio phase scatter: no two unison voices start aligned,
        // and every trigger scatters the same way (determinism)
        for (k, o) in self.uni.iter_mut().enumerate() {
            o.set_phase((k as f32 * 0.618_034).fract());
        }
        self.apply_env_params(p);
        self.amp_env.trigger();
        self.filt_env.trigger();
        self.active = true;
    }

    pub fn release(&mut self) {
        self.amp_env.release();
        self.filt_env.release();
    }

    fn apply_env_params(&mut self, p: &SynthParams) {
        self.amp_env.set(
            norm_to_seconds(p.attack),
            norm_to_seconds(p.decay),
            p.sustain,
            norm_to_seconds(p.release),
        );
        self.filt_env.set(
            norm_to_seconds(p.attack) * 0.5,
            norm_to_seconds(p.decay),
            p.sustain * 0.5,
            norm_to_seconds(p.release),
        );
    }

    #[inline]
    pub fn next(&mut self, p: &SynthParams) -> f32 {
        if !self.active {
            return 0.0;
        }
        let detune_ratio = 1.0 + p.detune * 0.03;
        let s1 = self.osc1.next(self.freq, self.sample_rate, p.wave);
        let s2 = self.osc2.next(self.freq * detune_ratio, self.sample_rate, p.wave);
        let sub = self.sub_osc.next(self.freq * 0.5, self.sample_rate, super::oscillator::Waveform::Sine);

        let mut mix = (s1 + s2) * 0.5 + sub * p.sub * 0.7;

        let fenv = self.filt_env.next();
        let base = norm_to_cutoff(p.cutoff);
        let mod_amt = p.filter_env * fenv * 8000.0;
        self.filter.set((base + mod_amt).min(self.sample_rate * 0.45), p.resonance);
        mix = self.filter.process(mix, FilterMode::Lowpass);

        let amp = self.amp_env.next();
        if !self.amp_env.is_active() {
            self.active = false;
        }
        mix * amp * self.velocity * 0.3
    }

    /// Stereo tick. With unison < 2 voices this IS `next` duplicated (same
    /// ops, same order — bit-exact); at ≥2 the classic osc pair is replaced
    /// by the unison stack: each oscillator detuned across ±`detune`·45 ct
    /// and equal-power panned across ±`spread`, filtered per channel.
    #[inline]
    pub fn next_lr(&mut self, p: &SynthParams) -> (f32, f32) {
        let n = unison_count(p.unison);
        if n < 2 {
            let m = self.next(p);
            return (m, m);
        }
        if !self.active {
            return (0.0, 0.0);
        }
        let mut suml = 0.0f32;
        let mut sumr = 0.0f32;
        for k in 0..n {
            // symmetric fan −1..+1 (odd counts keep one voice dead center)
            let off = (2 * k) as f32 / (n - 1) as f32 - 1.0;
            let cents = p.detune.clamp(0.0, 1.0) * UNISON_CENTS * off;
            let ratio = crate::dmath::powf(2.0, cents / 1200.0);
            let s = self.uni[k].next(self.freq * ratio, self.sample_rate, p.wave);
            let pan = (p.spread.clamp(0.0, 1.0) * off).clamp(-1.0, 1.0);
            // equal-power via sqrt — IEEE-exact on every platform
            suml += s * ((1.0 - pan) * 0.5).sqrt();
            sumr += s * ((1.0 + pan) * 0.5).sqrt();
        }
        // keep the stack near the loudness of the classic two-osc voice
        let scale = 0.85 / (n as f32).sqrt();
        let sub = self.sub_osc.next(self.freq * 0.5, self.sample_rate, super::oscillator::Waveform::Sine)
            * p.sub
            * 0.7
            * std::f32::consts::FRAC_1_SQRT_2;
        let (mut l, mut r) = (suml * scale + sub, sumr * scale + sub);

        let fenv = self.filt_env.next();
        let base = norm_to_cutoff(p.cutoff);
        let mod_amt = p.filter_env * fenv * 8000.0;
        let cutoff = (base + mod_amt).min(self.sample_rate * 0.45);
        self.filter.set(cutoff, p.resonance);
        self.filter_r.set(cutoff, p.resonance);
        l = self.filter.process(l, FilterMode::Lowpass);
        r = self.filter_r.process(r, FilterMode::Lowpass);

        let amp = self.amp_env.next();
        if !self.amp_env.is_active() {
            self.active = false;
        }
        let g = amp * self.velocity * 0.3;
        (l * g, r * g)
    }
}

#[inline]
pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * crate::dmath::powf(2.0, (note as f32 - 69.0) / 12.0)
}
