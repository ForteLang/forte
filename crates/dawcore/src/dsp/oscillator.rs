//! Band-limited oscillators using polyBLEP to suppress aliasing on the
//! discontinuous saw/square waveforms.

use std::f32::consts::PI;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
    /// Variable-width pulse (PWM via [`Oscillator::next_pw`]).
    Pulse,
}

impl Waveform {
    pub fn from_index(i: u8) -> Self {
        match i {
            0 => Waveform::Sine,
            1 => Waveform::Saw,
            2 => Waveform::Square,
            4 => Waveform::Pulse,
            _ => Waveform::Triangle,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Oscillator {
    phase: f32, // 0..1
    /// running integrator state for the band-limited triangle
    tri_state: f32,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self { phase: 0.0, tri_state: 0.0 }
    }
}

/// polyBLEP residual for a single discontinuity at the wrap point.
#[inline]
fn poly_blep(mut t: f32, dt: f32) -> f32 {
    if t < dt {
        t /= dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

impl Oscillator {
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.tri_state = 0.0;
    }

    pub fn set_phase(&mut self, p: f32) {
        self.phase = p.rem_euclid(1.0);
    }

    /// Advance one sample at `freq` Hz given `sample_rate`.
    #[inline]
    pub fn next(&mut self, freq: f32, sample_rate: f32, wave: Waveform) -> f32 {
        self.next_pw(freq, sample_rate, wave, 0.5)
    }

    /// Like [`next`], with an explicit pulse width for [`Waveform::Pulse`]
    /// (other shapes ignore it — Square stays the fixed 50% wave so existing
    /// renders are bit-identical).
    #[inline]
    pub fn next_pw(&mut self, freq: f32, sample_rate: f32, wave: Waveform, pw: f32) -> f32 {
        let dt = (freq / sample_rate).clamp(0.0, 0.5);
        let p = self.phase;

        let out = match wave {
            Waveform::Sine => crate::dmath::sin(p * TAU),
            Waveform::Saw => {
                // naive saw is 2t-1; subtract the BLEP residual at the wrap
                let mut v = 2.0 * p - 1.0;
                v -= poly_blep(p, dt);
                v
            }
            Waveform::Pulse => {
                let w = pw.clamp(0.05, 0.95);
                let mut v = if p < w { 1.0 } else { -1.0 };
                v += poly_blep(p, dt);
                v -= poly_blep((p + 1.0 - w).rem_euclid(1.0), dt);
                v
            }
            Waveform::Square => {
                let mut v = if p < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(p, dt);
                v -= poly_blep((p + 0.5).rem_euclid(1.0), dt);
                v
            }
            Waveform::Triangle => {
                // band-limited square, then leaky-integrate into a triangle
                let mut sq = if p < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(p, dt);
                sq -= poly_blep((p + 0.5).rem_euclid(1.0), dt);
                self.tri_state = dt * 4.0 * sq + (1.0 - 0.001) * self.tri_state;
                self.tri_state * (PI / 2.0) // approx normalisation
            }
        };

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}
