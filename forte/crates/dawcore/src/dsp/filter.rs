//! Topology-preserving (TPT) state-variable filter after Andrew Simper / Vadim
//! Zavalishin. Stable when modulated, gives LP/HP/BP/Notch from one structure.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

impl FilterMode {
    pub fn from_index(i: u8) -> Self {
        match i {
            0 => FilterMode::Lowpass,
            1 => FilterMode::Highpass,
            2 => FilterMode::Bandpass,
            _ => FilterMode::Notch,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Svf {
    sample_rate: f32,
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Svf {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        };
        s.set(1000.0, 0.5);
        s
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// `cutoff` Hz, `resonance` 0..1 (mapped to Q).
    #[inline]
    pub fn set(&mut self, cutoff: f32, resonance: f32) {
        let cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);
        let q = 0.5 + resonance.clamp(0.0, 0.99) * 9.5;
        self.g = crate::dmath::tan(std::f32::consts::PI * cutoff / self.sample_rate);
        self.k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    #[inline]
    pub fn process(&mut self, x: f32, mode: FilterMode) -> f32 {
        let v3 = x - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        match mode {
            FilterMode::Lowpass => v2,
            FilterMode::Highpass => x - self.k * v1 - v2,
            FilterMode::Bandpass => v1,
            FilterMode::Notch => x - self.k * v1,
        }
    }
}

/// Simple one-pole low/high shelf used by the EQ device.
#[derive(Clone, Copy)]
pub struct OnePole {
    a0: f32,
    b1: f32,
    z1: f32,
}

impl Default for OnePole {
    fn default() -> Self {
        Self::new()
    }
}

impl OnePole {
    pub fn new() -> Self {
        Self { a0: 1.0, b1: 0.0, z1: 0.0 }
    }
    pub fn set_lowpass(&mut self, cutoff: f32, sr: f32) {
        let x = crate::dmath::exp(-2.0 * std::f32::consts::PI * cutoff / sr);
        self.a0 = 1.0 - x;
        self.b1 = x;
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z1 = x * self.a0 + self.z1 * self.b1;
        self.z1
    }
}
