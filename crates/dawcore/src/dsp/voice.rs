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
        }
    }
}

fn norm_to_seconds(v: f32) -> f32 {
    // perceptually-ish: 1ms .. ~4s
    0.001 + v * v * 4.0
}

fn norm_to_cutoff(v: f32) -> f32 {
    // 30 Hz .. ~18 kHz, exponential
    30.0 * (600.0_f32).powf(v.clamp(0.0, 1.0))
}

#[derive(Clone, Copy)]
pub struct Voice {
    sample_rate: f32,
    osc1: Oscillator,
    osc2: Oscillator,
    sub_osc: Oscillator,
    filter: Svf,
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
            filter: Svf::new(sample_rate),
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
}

#[inline]
pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}
