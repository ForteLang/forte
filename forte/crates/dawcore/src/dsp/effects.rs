//! Insert effects operating on interleaved stereo. Each effect owns its own
//! pre-allocated buffers so `process` never allocates on the audio thread.

use super::filter::OnePole;

/// Stereo delay with feedback and high-frequency damping in the feedback path.
pub struct StereoDelay {
    sr: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    damp_l: OnePole,
    damp_r: OnePole,
    pub time: f32,     // 0..1
    pub feedback: f32, // 0..1
    pub mix: f32,      // 0..1
}

impl StereoDelay {
    pub fn new(sr: f32) -> Self {
        let max = (sr * 2.0) as usize + 4;
        let mut damp_l = OnePole::new();
        let mut damp_r = OnePole::new();
        damp_l.set_lowpass(4000.0, sr);
        damp_r.set_lowpass(4000.0, sr);
        Self {
            sr,
            buf_l: vec![0.0; max],
            buf_r: vec![0.0; max],
            write: 0,
            damp_l,
            damp_r,
            time: 0.3,
            feedback: 0.35,
            mix: 0.3,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let len = self.buf_l.len();
        let delay_samples = (0.05 + self.time * 0.9) * self.sr;
        let read = (self.write as f32 - delay_samples).rem_euclid(len as f32) as usize % len;

        let dl = self.buf_l[read];
        let dr = self.buf_r[read];

        let fb = self.feedback.min(0.95);
        // ping-pong: cross the feedback channels
        self.buf_l[self.write] = l + self.damp_r.process(dr) * fb;
        self.buf_r[self.write] = r + self.damp_l.process(dl) * fb;

        self.write = (self.write + 1) % len;

        let m = self.mix;
        (l * (1.0 - m * 0.5) + dl * m, r * (1.0 - m * 0.5) + dr * m)
    }
}

/// 4-line feedback delay network (Stautner–Puckette style) for a smooth,
/// diffuse reverb tail.
pub struct FdnReverb {
    lines: [Vec<f32>; 4],
    idx: [usize; 4],
    delays: [usize; 4],
    damp: [OnePole; 4],
    pub size: f32,  // 0..1
    pub decay: f32, // 0..1
    pub mix: f32,   // 0..1
}

impl FdnReverb {
    pub fn new(sr: f32) -> Self {
        // mutually-prime-ish delay lengths in ms for a dense tail
        let base_ms = [29.7, 37.1, 41.1, 43.7];
        let mut lines: [Vec<f32>; 4] = Default::default();
        let mut delays = [0usize; 4];
        let mut damp: [OnePole; 4] = [OnePole::new(); 4];
        for i in 0..4 {
            let n = ((base_ms[i] / 1000.0) * sr) as usize + 1;
            lines[i] = vec![0.0; (sr * 0.2) as usize + n + 4];
            delays[i] = n;
            damp[i].set_lowpass(6000.0, sr);
        }
        let _ = sr;
        Self {
            lines,
            idx: [0; 4],
            delays,
            damp,
            size: 0.5,
            decay: 0.5,
            mix: 0.25,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let input = (l + r) * 0.5;
        let size_scale = 0.4 + self.size * 1.6;
        let g = 0.5 + self.decay * 0.49; // feedback gain

        let mut out = [0.0f32; 4];
        for (i, o) in out.iter_mut().enumerate() {
            let buf = &self.lines[i];
            let n = ((self.delays[i] as f32) * size_scale) as usize;
            let n = n.min(buf.len() - 1).max(1);
            let read = (self.idx[i] + buf.len() - n) % buf.len();
            *o = buf[read];
        }

        // Householder feedback matrix mixing for diffusion
        let s = (out[0] + out[1] + out[2] + out[3]) * 0.5;
        let fb = [
            out[0] - s,
            out[1] - s,
            out[2] - s,
            out[3] - s,
        ];

        for i in 0..4 {
            let v = input + self.damp[i].process(fb[i]) * g;
            let buf = &mut self.lines[i];
            buf[self.idx[i]] = v;
            self.idx[i] = (self.idx[i] + 1) % buf.len();
        }

        let wet_l = (out[0] + out[2]) * 0.5;
        let wet_r = (out[1] + out[3]) * 0.5;
        let m = self.mix;
        (l * (1.0 - m * 0.5) + wet_l * m, r * (1.0 - m * 0.5) + wet_r * m)
    }
}

/// Soft-clipping waveshaper drive.
pub struct Drive {
    pub amount: f32, // 0..1
}

impl Default for Drive {
    fn default() -> Self {
        Self::new()
    }
}

impl Drive {
    pub fn new() -> Self {
        Self { amount: 0.3 }
    }
    #[inline]
    pub fn process(&self, x: f32) -> f32 {
        let k = 1.0 + self.amount * 20.0;
        let shaped = crate::dmath::tanh(x * k);
        shaped / (1.0 + self.amount * 1.5)
    }
}

/// Three-band shelving/peaking EQ.
pub struct Eq3 {
    low: OnePole,
    high: OnePole,
    pub low_gain: f32,  // 0..1 (0.5 = flat)
    pub mid_gain: f32,
    pub high_gain: f32,
}

impl Eq3 {
    pub fn new(sr: f32) -> Self {
        let mut low = OnePole::new();
        let mut high = OnePole::new();
        low.set_lowpass(250.0, sr);
        high.set_lowpass(3000.0, sr);
        Self { low, high, low_gain: 0.5, mid_gain: 0.5, high_gain: 0.5 }
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let lo = self.low.process(x);
        let hi = x - self.high.process(x);
        let mid = x - lo - hi;
        let lg = self.low_gain * 2.0;
        let mg = self.mid_gain * 2.0;
        let hg = self.high_gain * 2.0;
        lo * lg + mid * mg + hi * hg
    }
}

/// Stereo-linked compressor. Level detection and gain computation stay in the
/// linear domain (no log/exp per sample) so native and wasm agree bit-for-bit.
pub struct Compressor {
    sr: f32,
    env: f32,
    att_coef: f32,
    rel_coef: f32,
    pub thresh: f32,  // 0..1 → linear amplitude 0.05..1.0
    pub ratio: f32,   // 0..1 → 1:1..20:1
    pub attack: f32,  // 0..1 → 0.5ms..100ms
    pub release: f32, // 0..1 → 20ms..800ms
    pub makeup: f32,  // 0..1 → x1..x4
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        let mut c = Self {
            sr,
            env: 0.0,
            att_coef: 0.0,
            rel_coef: 0.0,
            thresh: 0.5,
            ratio: 0.5,
            attack: 0.1,
            release: 0.3,
            makeup: 0.25,
        };
        c.update_coefs();
        c
    }

    pub fn update_coefs(&mut self) {
        let att_s = 0.0005 + self.attack * 0.0995;
        let rel_s = 0.02 + self.release * 0.78;
        self.att_coef = crate::dmath::exp(-1.0 / (att_s * self.sr));
        self.rel_coef = crate::dmath::exp(-1.0 / (rel_s * self.sr));
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let level = l.abs().max(r.abs());
        let coef = if level > self.env { self.att_coef } else { self.rel_coef };
        self.env = coef * self.env + (1.0 - coef) * level;

        let thr = 0.05 + self.thresh * 0.95;
        let ratio = 1.0 + self.ratio * 19.0;
        let g = if self.env > thr {
            let target = thr + (self.env - thr) / ratio;
            target / self.env
        } else {
            1.0
        };
        let mk = 1.0 + self.makeup * 3.0;
        (l * g * mk, r * g * mk)
    }
}

/// Stereo chorus: one short delay line per channel, modulated by LFOs in
/// quadrature so the two sides move against each other (the width comes from
/// the phase offset, not from random spread — deterministic by construction).
pub struct Chorus {
    sr: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    phase: f32,
    pub rate: f32,  // 0..1 → 0.05..3 Hz
    pub depth: f32, // 0..1 → 0..8ms of sweep
    pub mix: f32,   // 0..1
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        let max = (sr * 0.06) as usize + 4; // 12ms base + 8ms sweep + headroom
        Self {
            sr,
            buf_l: vec![0.0; max],
            buf_r: vec![0.0; max],
            write: 0,
            phase: 0.0,
            rate: 0.3,
            depth: 0.5,
            mix: 0.5,
        }
    }

    #[inline]
    fn read(buf: &[f32], write: usize, delay_samples: f32) -> f32 {
        let len = buf.len() as f32;
        let pos = (write as f32 - delay_samples).rem_euclid(len);
        let i0 = pos as usize % buf.len();
        let i1 = (i0 + 1) % buf.len();
        let frac = pos - pos as usize as f32;
        buf[i0] * (1.0 - frac) + buf[i1] * frac
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.buf_l[self.write] = l;
        self.buf_r[self.write] = r;

        let hz = 0.05 + self.rate * 2.95;
        self.phase += hz / self.sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let tau = 2.0 * core::f32::consts::PI;
        let lfo_l = crate::dmath::sin(self.phase * tau);
        let lfo_r = crate::dmath::sin(self.phase * tau + core::f32::consts::FRAC_PI_2);

        let base = 0.012 * self.sr;
        let sweep = self.depth * 0.008 * self.sr;
        let dl = Self::read(&self.buf_l, self.write, base + sweep * (0.5 + 0.5 * lfo_l));
        let dr = Self::read(&self.buf_r, self.write, base + sweep * (0.5 + 0.5 * lfo_r));

        self.write = (self.write + 1) % self.buf_l.len();

        let m = self.mix;
        (l * (1.0 - m * 0.5) + dl * m, r * (1.0 - m * 0.5) + dr * m)
    }
}

/// Tempo-synced ducker: the deterministic take on sidechain pumping. Instead
/// of following another track's level, it dips on a fixed beat grid (which is
/// what the classic "pump" is musically — tied to the kick pattern).
pub struct Pump {
    sr: f32,
    pos: f32,
    pub amount: f32, // 0..1 → duck depth
    pub period: f32, // seconds per duck cycle (compiler sets this from tempo)
}

impl Pump {
    pub fn new(sr: f32) -> Self {
        Self { sr, pos: 0.0, amount: 0.6, period: 0.5 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let period_samples = (self.period.max(0.05)) * self.sr;
        let t = self.pos / period_samples; // 0..1 through the cycle
        // instant dip at the beat, smooth quadratic recovery
        let g = 1.0 - self.amount * (1.0 - t) * (1.0 - t);
        self.pos += 1.0;
        if self.pos >= period_samples {
            self.pos -= period_samples;
        }
        (l * g, r * g)
    }
}

/// Mid/side stereo width. 0.5 is unity; below narrows towards mono, above
/// pushes the sides up (capped ×2 so it cannot blow up the mix).
pub struct Width {
    pub amount: f32,
}

impl Width {
    pub fn new() -> Self {
        Self { amount: 0.5 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5 * (self.amount * 2.0);
        (mid + side, mid - side)
    }
}

impl Default for Width {
    fn default() -> Self {
        Self::new()
    }
}

/// Bit-depth + sample-rate reduction — the lo-fi/glitch crunch. `bits`
/// sweeps 16 (clean) down to 1 (square-ish grit); `rate` holds each sample
/// for 1..64 input samples. Everything is a pure function of the input and
/// the phase counter: deterministic on every backend.
pub struct Crush {
    pub bits: f32, // 0..1 → 16..1 effective bits
    pub rate: f32, // 0..1 → hold 1..64 samples
    pub mix: f32,
    phase: f32,
    held: (f32, f32),
}

impl Crush {
    pub fn new() -> Self {
        Self { bits: 0.5, rate: 0.35, mix: 1.0, phase: 0.0, held: (0.0, 0.0) }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let hold = 1.0 + self.rate.clamp(0.0, 1.0) * 63.0;
        self.phase += 1.0;
        if self.phase >= hold {
            self.phase -= hold;
            // quantize to 2^bits amplitude levels
            let bits = 16.0 - self.bits.clamp(0.0, 1.0) * 15.0;
            let levels = crate::dmath::powf(2.0, bits - 1.0);
            self.held = ((l * levels).round() / levels, (r * levels).round() / levels);
        }
        let m = self.mix.clamp(0.0, 1.0);
        (l * (1.0 - m) + self.held.0 * m, r * (1.0 - m) + self.held.1 * m)
    }
}

impl Default for Crush {
    fn default() -> Self {
        Self::new()
    }
}

/// Tempo-synced buffer repeat — THE glitch stutter. The last `period`
/// seconds of dry signal loop while `mix` is up; automate `stutter.mix`
/// for fills. The buffer is preallocated (2 s) so the audio thread never
/// allocates.
pub struct Stutter {
    sr: f32,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    pos: f32,
    pub period: f32, // seconds per repeat cycle (compiler sets from tempo)
    pub mix: f32,
}

impl Stutter {
    pub fn new(sr: f32) -> Self {
        let cap = (sr * 2.0) as usize;
        Self {
            sr,
            buf_l: vec![0.0; cap],
            buf_r: vec![0.0; cap],
            write: 0,
            pos: 0.0,
            period: 0.125,
            mix: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let cap = self.buf_l.len();
        self.buf_l[self.write] = l;
        self.buf_r[self.write] = r;
        let loop_len = ((self.period.max(0.01) * self.sr) as usize).clamp(1, cap);
        // read the loop that ends at the write head: freeze-repeat the
        // most recent chunk
        let idx = (self.write + cap - loop_len + self.pos as usize % loop_len) % cap;
        let (wl, wr) = (self.buf_l[idx], self.buf_r[idx]);
        self.write = (self.write + 1) % cap;
        self.pos += 1.0;
        if self.pos >= loop_len as f32 {
            self.pos -= loop_len as f32;
        }
        let m = self.mix.clamp(0.0, 1.0);
        (l * (1.0 - m) + wl * m, r * (1.0 - m) + wr * m)
    }
}

/// Tempo-synced chopper (trance gate): open for `duty` of each period,
/// attenuated by `depth` for the rest, with a 1 ms slew so the edges click
/// only as much as you want them to (crank depth for hard chops).
pub struct Gate {
    sr: f32,
    pos: f32,
    g: f32,
    pub depth: f32,  // 0..1 → how far closed
    pub period: f32, // seconds per cycle (compiler sets from tempo)
    pub duty: f32,   // 0..1 open fraction
}

impl Gate {
    pub fn new(sr: f32) -> Self {
        Self { sr, pos: 0.0, g: 1.0, depth: 0.9, period: 0.125, duty: 0.5 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let period_samples = self.period.max(0.01) * self.sr;
        let frac = self.pos / period_samples;
        let target = if frac < self.duty.clamp(0.02, 0.98) { 1.0 } else { 1.0 - self.depth };
        // ~1 ms one-pole slew keeps the chop tight but unclicked
        // (linear approx of 1 - e^(-1/(0.001*sr)) — deterministic, no libm)
        let a = (1.0 / (0.001 * self.sr)).min(1.0);
        self.g += (target - self.g) * a;
        self.pos += 1.0;
        if self.pos >= period_samples {
            self.pos -= period_samples;
        }
        (l * self.g, r * self.g)
    }
}
