//! Insert effects operating on interleaved stereo. Each effect owns its own
//! pre-allocated buffers so `process` never allocates on the audio thread.

use super::filter::OnePole;
use super::oversample::Oversampler;

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
    /// oversampling factor 1 (off, bit-exact legacy path) / 2 / 4
    pub os: u8,
    ovs: Option<Oversampler>,
}

impl Default for Drive {
    fn default() -> Self {
        Self::new()
    }
}

impl Drive {
    pub fn new() -> Self {
        Self { amount: 0.3, os: 1, ovs: None }
    }
    #[inline]
    pub fn process(&self, x: f32) -> f32 {
        let k = 1.0 + self.amount * 20.0;
        let shaped = crate::dmath::tanh(x * k);
        shaped / (1.0 + self.amount * 1.5)
    }
    #[inline]
    pub fn process_lr(&mut self, l: f32, r: f32) -> (f32, f32) {
        match self.ovs.as_mut() {
            Some(o) => {
                let amount = self.amount;
                let shape = |x: f32| {
                    let k = 1.0 + amount * 20.0;
                    crate::dmath::tanh(x * k) / (1.0 + amount * 1.5)
                };
                let (wl, wr, _, _) = o.run(l, r, |a, b| (shape(a), shape(b)));
                (wl, wr)
            }
            None => (self.process(l), self.process(r)),
        }
    }
    /// Rebuilds the oversampler only when the factor actually changes —
    /// `configure` runs every block and must not allocate in steady state.
    pub fn set_os(&mut self, factor: u8) {
        if factor != self.os {
            self.os = factor;
            self.ovs = (factor > 1).then(|| Oversampler::new(factor));
        }
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
    /// oversampling factor 1 (off, bit-exact legacy path) / 2 / 4
    pub os: u8,
    phase: f32,
    held: (f32, f32),
    ovs: Option<Oversampler>,
}

impl Crush {
    pub fn new() -> Self {
        Self { bits: 0.5, rate: 0.35, mix: 1.0, os: 1, phase: 0.0, held: (0.0, 0.0), ovs: None }
    }

    #[inline]
    fn step(
        phase: &mut f32,
        held: &mut (f32, f32),
        hold: f32,
        bits: f32,
        l: f32,
        r: f32,
    ) -> (f32, f32) {
        *phase += 1.0;
        if *phase >= hold {
            *phase -= hold;
            // quantize to 2^bits amplitude levels
            let levels = crate::dmath::powf(2.0, bits - 1.0);
            *held = ((l * levels).round() / levels, (r * levels).round() / levels);
        }
        *held
    }

    pub fn set_os(&mut self, factor: u8) {
        if factor != self.os {
            self.os = factor;
            self.ovs = (factor > 1).then(|| Oversampler::new(factor));
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let bits = 16.0 - self.bits.clamp(0.0, 1.0) * 15.0;
        let hold = 1.0 + self.rate.clamp(0.0, 1.0) * 63.0;
        let m = self.mix.clamp(0.0, 1.0);
        let (phase, held) = (&mut self.phase, &mut self.held);
        match self.ovs.as_mut() {
            // whole sample-and-hold core at the high rate (hold length scaled
            // so the crunch stays the same speed in seconds); the decimation
            // filter then strips the fold-back off both the hold steps and
            // the quantize edges
            Some(o) => {
                let hold_hi = hold * o.factor() as f32;
                let (wl, wr, dl, dr) =
                    o.run(l, r, |a, b| Self::step(phase, held, hold_hi, bits, a, b));
                (dl * (1.0 - m) + wl * m, dr * (1.0 - m) + wr * m)
            }
            None => {
                let (wl, wr) = Self::step(phase, held, hold, bits, l, r);
                (l * (1.0 - m) + wl * m, r * (1.0 - m) + wr * m)
            }
        }
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
        // ~4 ms one-pole slew: still a hard chop to the ear, but the edge
        // stops reading as a click on loud sustained material
        // (linear approx of 1 - e^(-1/(0.004*sr)) — deterministic, no libm)
        let a = (1.0 / (0.004 * self.sr)).min(1.0);
        self.g += (target - self.g) * a;
        self.pos += 1.0;
        if self.pos >= period_samples {
            self.pos -= period_samples;
        }
        (l * self.g, r * self.g)
    }
}

/// Saturation — the harmonic-richness workhorse. Three characters:
/// 0 = tape (symmetric tanh, compresses peaks warmly), 1 = tube
/// (asymmetric: adds even harmonics, "expensive" sheen), 2 = fuzz
/// (hard-driven tanh, guitar-pedal aggression). `tone` darkens the result
/// (one-pole lowpass) so heavy drive stays musical; `mix` blends parallel.
pub struct Saturate {
    sr: f32,
    lp: (f32, f32),
    pub mode: u8,
    pub drive: f32, // 0..1
    pub tone: f32,  // 0..1 → dark..open
    pub mix: f32,
    /// oversampling factor 1 (off, bit-exact legacy path) / 2 / 4
    pub os: u8,
    ovs: Option<Oversampler>,
}

impl Saturate {
    pub fn new(sr: f32) -> Self {
        Self { sr, lp: (0.0, 0.0), mode: 0, drive: 0.4, tone: 0.7, mix: 1.0, os: 1, ovs: None }
    }

    #[inline]
    fn shape_with(mode: u8, drive: f32, x: f32) -> f32 {
        let d = 1.0 + drive * 9.0;
        match mode {
            // tube: asymmetric — a touch of x² rectification before the tanh
            1 => {
                let y = x * d;
                crate::dmath::tanh(y + 0.28 * y * y * if y > 0.0 { 1.0 } else { 0.4 }) / crate::dmath::tanh(d)
            }
            // fuzz: driven hard, normalized less — it BITES
            2 => crate::dmath::tanh(x * d * 3.0) * 0.85,
            // tape: symmetric soft clip, normalized so low drive ≈ unity
            _ => crate::dmath::tanh(x * d) / crate::dmath::tanh(d),
        }
    }

    #[inline]
    fn shape(&self, x: f32) -> f32 {
        Self::shape_with(self.mode, self.drive, x)
    }

    pub fn set_os(&mut self, factor: u8) {
        if factor != self.os {
            self.os = factor;
            self.ovs = (factor > 1).then(|| Oversampler::new(factor));
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // waveshape (the aliasing source) — oversampled when `os` is on;
        // the tone filter and mix blend stay at the base rate either way
        let (mut wl, mut wr, l, r) = match self.ovs.as_mut() {
            Some(o) => {
                let (mode, drive) = (self.mode, self.drive);
                o.run(l, r, |a, b| {
                    (Self::shape_with(mode, drive, a), Self::shape_with(mode, drive, b))
                })
            }
            None => (self.shape(l), self.shape(r), l, r),
        };
        // tone: one-pole lowpass, 800 Hz (dark) .. 18 kHz (open)
        let cutoff = 800.0 * crate::dmath::powf(22.5, self.tone.clamp(0.0, 1.0));
        let a = (cutoff / self.sr * std::f32::consts::TAU).min(1.0);
        self.lp.0 += (wl - self.lp.0) * a;
        self.lp.1 += (wr - self.lp.1) * a;
        wl = self.lp.0;
        wr = self.lp.1;
        let m = self.mix.clamp(0.0, 1.0);
        (l * (1.0 - m) + wl * m, r * (1.0 - m) + wr * m)
    }
}

/// Transient shaper: two envelope followers (fast/slow) split every hit
/// into attack and sustain, each with its own gain. attack/sustain knobs
/// are 0..1 with 0.5 = neutral (up to ±12 dB).
pub struct Transient {
    sr: f32,
    fast: f32,
    slow: f32,
    pub attack: f32,  // 0..1, 0.5 neutral
    pub sustain: f32, // 0..1, 0.5 neutral
}

impl Transient {
    pub fn new(sr: f32) -> Self {
        Self { sr, fast: 0.0, slow: 0.0, attack: 0.5, sustain: 0.5 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let x = (l.abs() + r.abs()) * 0.5;
        // ~1 ms and ~80 ms follower time constants (linear approximations)
        let af = (1.0 / (0.001 * self.sr)).min(1.0);
        let asl = (1.0 / (0.08 * self.sr)).min(1.0);
        self.fast += (x - self.fast) * af;
        self.slow += (x - self.slow) * asl;
        // transient part = fast over slow; body = the rest
        let t = (self.fast - self.slow).max(0.0);
        let denom = self.fast.max(1e-6);
        let t_frac = (t / denom).clamp(0.0, 1.0);
        // ±12 dB at the knob extremes
        let ag = crate::dmath::powf(10.0, (self.attack - 0.5) * 1.2);
        let sg = crate::dmath::powf(10.0, (self.sustain - 0.5) * 1.2);
        let g = ag * t_frac + sg * (1.0 - t_frac);
        (l * g, r * g)
    }
}

/// Parallel (New York) compression in one insert: a hard-compressed copy —
/// fast attack/release, 8:1 over a low threshold, makeup, with a
/// "smiley" tilt (`color`: lows+highs up on the wet bus) — blended under
/// the dry signal by `amount`. Punch and glue without losing dynamics.
pub struct ParComp {
    sr: f32,
    env: f32,
    low: (f32, f32),
    pub amount: f32, // wet blend 0..1
    pub drive: f32,  // input gain into the crushed bus
    pub color: f32,  // 0..1 smiley tilt on the wet bus
    /// oversampling factor 1 (off, bit-exact legacy path) / 2 / 4
    pub os: u8,
    ovs: Option<Oversampler>,
}

impl ParComp {
    pub fn new(sr: f32) -> Self {
        Self { sr, env: 0.0, low: (0.0, 0.0), amount: 0.35, drive: 0.5, color: 0.3, os: 1, ovs: None }
    }

    /// The crushed bus for one sample at rate `sr` — followers, gain
    /// computer, tilt and the final tanh (the aliasing source).
    #[inline]
    fn wet(
        env: &mut f32,
        low: &mut (f32, f32),
        sr: f32,
        g_in: f32,
        c: f32,
        l: f32,
        r: f32,
    ) -> (f32, f32) {
        let (xl, xr) = (l * g_in, r * g_in);
        let level = (xl.abs() + xr.abs()) * 0.5;
        // fast follower: ~2 ms up, ~60 ms down
        let up = (1.0 / (0.002 * sr)).min(1.0);
        let down = (1.0 / (0.06 * sr)).min(1.0);
        *env += (level - *env) * if level > *env { up } else { down };
        // 8:1 over −24 dBFS (0.063 linear)
        const THRESH: f32 = 0.063;
        let gr = if *env > THRESH {
            crate::dmath::powf(*env / THRESH, -(1.0 - 1.0 / 8.0))
        } else {
            1.0
        };
        let makeup = 2.2;
        let (mut wl, mut wr) = (xl * gr * makeup, xr * gr * makeup);
        // smiley tilt: split lows with a one-pole, lift lows and the residue
        let a = (180.0 / sr * std::f32::consts::TAU).min(1.0);
        low.0 += (wl - low.0) * a;
        low.1 += (wr - low.1) * a;
        wl += low.0 * c + (wl - low.0) * c * 0.6;
        wr += low.1 * c + (wr - low.1) * c * 0.6;
        // safety: the crushed bus must never explode
        (crate::dmath::tanh(wl), crate::dmath::tanh(wr))
    }

    pub fn set_os(&mut self, factor: u8) {
        if factor != self.os {
            self.os = factor;
            self.ovs = (factor > 1).then(|| Oversampler::new(factor));
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let g_in = 1.0 + self.drive * 7.0;
        let c = self.color.clamp(0.0, 1.0) * 0.7;
        let m = self.amount.clamp(0.0, 1.0);
        let (env, low) = (&mut self.env, &mut self.low);
        match self.ovs.as_mut() {
            // the whole crushed bus runs at the high rate (followers scaled
            // to the effective sample rate) so the tanh's harmonics land
            // above the decimation filter instead of folding back
            Some(o) => {
                let sr_hi = self.sr * o.factor() as f32;
                let (wl, wr, dl, dr) =
                    o.run(l, r, |a, b| Self::wet(env, low, sr_hi, g_in, c, a, b));
                (dl + wl * m, dr + wr * m)
            }
            None => {
                let (wl, wr) = Self::wet(env, low, self.sr, g_in, c, l, r);
                (l + wl * m, r + wr * m)
            }
        }
    }
}

/// Exciter: high band → saturation → blended back on top. The "sparkle"
/// convention — synthesized harmonics where the source has none.
pub struct Exciter {
    sr: f32,
    lp: (f32, f32),
    pub amount: f32,
    pub freq: f32, // 0..1 → 1.5 kHz .. 9 kHz corner
}

impl Exciter {
    pub fn new(sr: f32) -> Self {
        Self { sr, lp: (0.0, 0.0), amount: 0.3, freq: 0.5 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let corner = 1500.0 * crate::dmath::powf(6.0, self.freq.clamp(0.0, 1.0));
        let a = (corner / self.sr * std::f32::consts::TAU).min(1.0);
        self.lp.0 += (l - self.lp.0) * a;
        self.lp.1 += (r - self.lp.1) * a;
        let (hl, hr) = (l - self.lp.0, r - self.lp.1);
        let m = self.amount.clamp(0.0, 1.0) * 1.5;
        (
            l + crate::dmath::tanh(hl * 4.0) * m,
            r + crate::dmath::tanh(hr * 4.0) * m,
        )
    }
}

/// Ring modulator: multiply by a sine carrier — inharmonic, metallic,
/// the classic "broken machine voice". Deterministic phase accumulator.
pub struct RingMod {
    sr: f32,
    phase: f32,
    pub freq: f32, // 0..1 → 20 Hz .. 4 kHz (log)
    pub mix: f32,
}

impl RingMod {
    pub fn new(sr: f32) -> Self {
        Self { sr, phase: 0.0, freq: 0.4, mix: 0.5 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let hz = 20.0 * crate::dmath::powf(200.0, self.freq.clamp(0.0, 1.0));
        self.phase += hz / self.sr;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let c = crate::dmath::sin(self.phase * std::f32::consts::TAU);
        let m = self.mix.clamp(0.0, 1.0);
        (l * (1.0 - m) + l * c * m, r * (1.0 - m) + r * c * m)
    }
}

/// Tape stop: `amount` 0 = bypass, rising toward 1 slows a buffered read
/// head down to a halt — pitch falls with speed, exactly like power-cut
/// vinyl/tape. Automate `tapestop.amount` from 0 to 1 over the last bar.
pub struct TapeStop {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    read: f64,
    pub amount: f32,
}

impl TapeStop {
    pub fn new(sr: f32) -> Self {
        let cap = (sr * 4.0) as usize;
        Self { buf_l: vec![0.0; cap], buf_r: vec![0.0; cap], write: 0, read: 0.0, amount: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let cap = self.buf_l.len();
        self.buf_l[self.write] = l;
        self.buf_r[self.write] = r;
        self.write = (self.write + 1) % cap;
        if self.amount <= 0.0001 {
            // bypass: pin the read head to the write head (bit-exact dry)
            self.read = self.write as f64;
            return (l, r);
        }
        // playback rate falls to zero as amount → 1 (quadratic feels like tape)
        let rate = (1.0 - self.amount.clamp(0.0, 1.0) as f64).powi(2);
        self.read += rate;
        while self.read >= cap as f64 {
            self.read -= cap as f64;
        }
        let i = self.read.floor() as usize % cap;
        let j = (i + 1) % cap;
        let frac = (self.read - self.read.floor()) as f32;
        (
            self.buf_l[i] + (self.buf_l[j] - self.buf_l[i]) * frac,
            self.buf_r[i] + (self.buf_r[j] - self.buf_r[i]) * frac,
        )
    }
}

/// Sidechain ducker — the glitch groove engine. The compiler bakes the
/// trigger times (another track's swung hits, in seconds) into `triggers`;
/// at each one the gain slams down by `amount` over `attack` seconds, then
/// recovers over `release`. Deep amounts carve the audio to near-silence
/// between hits — the unnatural cut, and the space it leaves, IS the groove.
/// A free-running sample counter walks the trigger list (deterministic on
/// the offline render path that produces every .fortesong and digest).
pub struct Duck {
    sr: f32,
    /// trigger sample positions (absolute from song start), sorted
    triggers: Vec<u32>,
    cursor: usize,
    counter: u32,
    /// current gain, slewing toward 1.0 between hits
    gain: f32,
    pub amount: f32,  // 0..1 duck depth (1 = to silence)
    pub attack: f32,  // seconds to reach the ducked floor
    pub release: f32, // seconds to recover to unity
    pub shape: f32,   // 0 = linear recovery, 1 = exponential (snappy)
    /// keyed-gate mode: the polarity flips — SILENT by default, each
    /// trigger slams the gate OPEN over `attack`, then it falls back to
    /// the floor over `release`. Audio exists only where the key track
    /// plays: the sidechain CHOP (pads carved by a hat pattern), where
    /// plain duck mode is the sidechain PUMP.
    pub key: bool,
    /// samples since the last trigger, for the envelope
    since: f32,
}

impl Duck {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            triggers: Vec::new(),
            cursor: 0,
            counter: 0,
            gain: 1.0,
            amount: 0.85,
            attack: 0.02,
            release: 0.18,
            shape: 0.6,
            key: false,
            since: f32::MAX,
        }
    }

    /// Bake trigger times (seconds) into sample positions.
    pub fn set_triggers(&mut self, secs: &[f64]) {
        self.triggers = secs.iter().map(|&t| (t * self.sr as f64).round().max(0.0) as u32).collect();
        self.triggers.sort_unstable();
        self.triggers.dedup();
        self.cursor = 0;
        self.counter = 0;
        self.since = f32::MAX;
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // fire every trigger reached this sample (cursor never rewinds)
        while self.cursor < self.triggers.len() && self.counter >= self.triggers[self.cursor] {
            self.since = 0.0;
            self.cursor += 1;
        }
        // envelope: dip to (1-amount) over attack, recover to 1 over release
        let floor = 1.0 - self.amount.clamp(0.0, 1.0);
        let atk = (self.attack.max(0.0005) * self.sr).max(1.0);
        let rel = (self.release.max(0.001) * self.sr).max(1.0);
        let target = if self.since < atk {
            // slam down
            1.0 - (1.0 - floor) * (self.since / atk)
        } else {
            // recover
            let t = ((self.since - atk) / rel).min(1.0);
            let curve = if self.shape > 0.5 {
                // exponential ease-out: snappy at first, then settles
                let k = 1.0 + self.shape * 6.0;
                1.0 - crate::dmath::powf(1.0 - t, k)
            } else {
                t
            };
            floor + (1.0 - floor) * curve
        };
        // key mode mirrors the envelope: idle sits at the floor and the
        // trigger opens toward unity instead of dipping away from it
        self.gain = if self.key { 1.0 + floor - target } else { target };
        self.since += 1.0;
        self.counter = self.counter.wrapping_add(1);
        (l * self.gain, r * self.gain)
    }
}

/// Vinyl — the analog-media patina. Digital sources read as MIDI because
/// nothing between the hits moves or breathes; a record does four things a
/// DAC never does, and this stamps all four on a bus: `wow` (slow ±pitch
/// drift + 6.5 Hz flutter, the warped-record warble), `crackle` (sparse
/// deterministic ticks and pops), `hiss` (shaped surface noise floor) and
/// `dust` (a darkening lowpass, the worn-pressing rolloff). Every stage is
/// gated on its knob, so an all-zero vinyl is a bit-exact bypass.
pub struct Vinyl {
    sr: f32,
    // wow/flutter: a short buffer read through a moving fractional head
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write: usize,
    ph_wow: f32,
    ph_flut: f32,
    // crackle: xorshift32 draws + a fast-decaying pop state, band-limited
    // so a pop reads as vinyl, not as a digital defect
    rng: u32,
    pop: f32,
    pop_lp: f32,
    // hiss shaping + dust lowpass states
    hiss_lp: f32,
    lp: (f32, f32),
    pub wow: f32,
    pub crackle: f32,
    pub hiss: f32,
    pub dust: f32,
}

impl Vinyl {
    pub fn new(sr: f32) -> Self {
        let cap = (sr * 0.05) as usize; // 50 ms is plenty for the deepest wow
        Self {
            sr,
            buf_l: vec![0.0; cap],
            buf_r: vec![0.0; cap],
            write: 0,
            ph_wow: 0.0,
            ph_flut: 0.25,
            rng: 0x2545_f491,
            pop: 0.0,
            pop_lp: 0.0,
            hiss_lp: 0.0,
            lp: (0.0, 0.0),
            wow: 0.0,
            crackle: 0.0,
            hiss: 0.0,
            dust: 0.0,
        }
    }

    #[inline]
    fn next_rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let cap = self.buf_l.len();
        self.buf_l[self.write] = l;
        self.buf_r[self.write] = r;
        self.write = (self.write + 1) % cap;

        // ---- wow/flutter: both channels share one head (the platter moves
        // the whole record, not each channel) ----
        let (mut ol, mut or) = if self.wow > 0.0001 {
            let w = self.wow.clamp(0.0, 1.0);
            self.ph_wow = (self.ph_wow + 0.5 / self.sr).fract(); // 0.5 Hz wow
            self.ph_flut = (self.ph_flut + 6.5 / self.sr).fract(); // 6.5 Hz flutter
            // peak deviation ≈ 12 cents of wow + 2 cents of flutter at 1.0
            let a_wow = 0.007 * self.sr / (std::f32::consts::TAU * 0.5);
            let a_flut = 0.0012 * self.sr / (std::f32::consts::TAU * 6.5);
            let m = crate::dmath::sin(self.ph_wow * std::f32::consts::TAU) * a_wow * w
                + crate::dmath::sin(self.ph_flut * std::f32::consts::TAU) * a_flut * w;
            // center the head deep enough that modulation never overtakes it
            let center = a_wow + a_flut + 4.0;
            let read = (self.write as f32 + cap as f32) - 1.0 - center + m;
            let i = read.floor() as usize % cap;
            let j = (i + 1) % cap;
            let frac = read - read.floor();
            (
                self.buf_l[i] + (self.buf_l[j] - self.buf_l[i]) * frac,
                self.buf_r[i] + (self.buf_r[j] - self.buf_r[i]) * frac,
            )
        } else {
            (l, r)
        };

        // ---- crackle: sparse ticks with a ~1 ms tail, mono like real dust.
        // The pop runs through a ~3.5 kHz one-pole so it lands as a vinyl
        // "puh", not a full-spectrum digital click ----
        if self.crackle > 0.0001 {
            let c = self.crackle.clamp(0.0, 1.0);
            let d = self.next_rand();
            // probability rises with the square of the knob: sparse → frying
            if d.abs() > 1.0 - c * c * 0.0004 {
                self.pop = d.signum() * (0.15 + d.abs() * 0.5) * c;
            }
            self.pop *= 0.92;
            let a = 1.0 - crate::dmath::exp(-std::f32::consts::TAU * 3_500.0 / self.sr);
            self.pop_lp += (self.pop - self.pop_lp) * a;
            ol += self.pop_lp;
            or += self.pop_lp;
        }

        // ---- hiss: the shaped noise floor ----
        if self.hiss > 0.0001 {
            let n = self.next_rand();
            // one-pole lowpass ~6 kHz softens white noise into tape/surface hiss
            let a = 1.0 - crate::dmath::exp(-std::f32::consts::TAU * 6000.0 / self.sr);
            self.hiss_lp += (n - self.hiss_lp) * a;
            let h = self.hiss_lp * self.hiss.clamp(0.0, 1.0) * 0.012;
            ol += h;
            or += h;
        }

        // ---- dust: the worn-pressing rolloff ----
        if self.dust > 0.0001 {
            let d = self.dust.clamp(0.0, 1.0);
            let fc = 1_500.0 + 16_500.0 * (1.0 - d) * (1.0 - d);
            let a = 1.0 - crate::dmath::exp(-std::f32::consts::TAU * fc / self.sr);
            self.lp.0 += (ol - self.lp.0) * a;
            self.lp.1 += (or - self.lp.1) * a;
            ol = self.lp.0;
            or = self.lp.1;
        }

        (ol, or)
    }
}

/// Master-grade peak limiter: instant attack, exponential release. The
/// envelope tracks the stereo peak and gain never lets the output exceed
/// the ceiling — loudness without the soft-clip crunch. Deterministic
/// (one-pole linear-approx release, no libm).
pub struct Limiter {
    sr: f32,
    env: f32,
    pub ceiling: f32, // output ceiling (linear, 0..1)
    pub release: f32, // seconds back to unity
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        Self { sr, env: 0.0, ceiling: 0.95, release: 0.12 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        if peak > self.env {
            self.env = peak; // instant attack: nothing gets past
        } else {
            let rel = (1.0 / (self.release.max(0.005) * self.sr)).min(1.0);
            self.env += (peak - self.env) * rel;
        }
        let c = self.ceiling.clamp(0.05, 1.0);
        let g = if self.env > c { c / self.env } else { 1.0 };
        (l * g, r * g)
    }
}

/// `space` — the new-generation reverb (#123): input diffusion into an
/// 8-line FDN with Hadamard mixing, frequency-dependent decay, and slow
/// deterministic delay-line modulation (fixed-phase LFOs) so the tail is
/// alive instead of metallic. Three characters (room / plate / hall) pick
/// the delay-length sets and diffusion. The old `reverb` is untouched —
/// its digests are load-bearing.
pub struct Space {
    sr: f32,
    pub kind: u8,      // 0 room, 1 plate, 2 hall
    pub size: f32,     // 0..1 scales the line lengths (0.5..1.5x)
    pub decay: f32,    // 0..1 → T60 0.2..12 s
    pub damp: f32,     // 0..1 high-frequency decay in the feedback path
    pub predelay: f32, // 0..1 → 0..150 ms
    pub depth: f32,    // modulation depth 0..1 (→ 0..10 samples)
    pub width: f32,
    pub mix: f32,
    pre: Vec<f32>,
    pre_pos: usize,
    ap_buf: [Vec<f32>; 4],
    ap_pos: [usize; 4],
    lines: [Vec<f32>; 8],
    pos: [usize; 8],
    len: [usize; 8],
    lp: [f32; 8],
    lfo: [f32; 8],
    cfg: (u8, f32), // (kind, size) the buffers were tuned for
}

/// Mutually-prime base lengths (samples at 48 kHz, size = 1.0).
const SPACE_SETS: [[usize; 8]; 3] = [
    [571, 683, 811, 929, 1039, 1153, 1259, 1361],           // room
    [887, 1019, 1129, 1249, 1381, 1499, 1613, 1733],        // plate
    [1687, 1861, 2053, 2251, 2399, 2687, 2903, 3181],       // hall
];
const SPACE_APS: [usize; 4] = [107, 142, 277, 379];
const SPACE_LFO_HZ: [f32; 8] = [0.11, 0.13, 0.17, 0.19, 0.23, 0.29, 0.31, 0.37];

impl Space {
    pub fn new(sr: f32) -> Self {
        let mut s = Self {
            sr,
            kind: 2,
            size: 0.5,
            decay: 0.5,
            damp: 0.4,
            predelay: 0.1,
            depth: 0.3,
            width: 0.8,
            mix: 0.3,
            pre: vec![0.0; (sr * 0.16) as usize + 1],
            pre_pos: 0,
            ap_buf: std::array::from_fn(|i| vec![0.0; SPACE_APS[i] * 2 + 8]),
            ap_pos: [0; 4],
            lines: std::array::from_fn(|_| Vec::new()),
            pos: [0; 8],
            len: [0; 8],
            lp: [0.0; 8],
            lfo: [0.0; 8],
            cfg: (255, -1.0),
        };
        s.retune();
        s
    }

    /// Rebuild line lengths for (kind, size). Buffers keep 16 samples of
    /// headroom past the longest modulated read.
    pub fn retune(&mut self) {
        if self.cfg == (self.kind, self.size) {
            return;
        }
        self.cfg = (self.kind, self.size);
        let set = &SPACE_SETS[(self.kind as usize).min(2)];
        let scale = (0.5 + self.size * 1.0) as f64 * (self.sr as f64 / 48_000.0);
        for (i, &base) in set.iter().enumerate() {
            self.len[i] = ((base as f64 * scale) as usize).max(32);
            self.lines[i] = vec![0.0; self.len[i] + 16];
            self.pos[i] = 0;
            self.lp[i] = 0.0;
            // fixed, distinct starting phases — deterministic everywhere
            self.lfo[i] = i as f32 * std::f32::consts::FRAC_PI_4;
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // predelay on the mono sum
        let dry_l = l;
        let dry_r = r;
        let pd = ((self.predelay * 0.15 * self.sr) as usize).min(self.pre.len() - 1);
        self.pre[self.pre_pos] = (l + r) * 0.5;
        let read = (self.pre_pos + self.pre.len() - pd) % self.pre.len();
        let mut x = self.pre[read];
        self.pre_pos = (self.pre_pos + 1) % self.pre.len();

        // input diffusion: 4 series allpasses (g = 0.7)
        for (i, &n) in SPACE_APS.iter().enumerate() {
            let buf = &mut self.ap_buf[i];
            let p = self.ap_pos[i];
            let d = buf[(p + buf.len() - n) % buf.len()];
            let y = d - 0.7 * x;
            buf[p] = x + 0.7 * y;
            self.ap_pos[i] = (p + 1) % buf.len();
            x = y;
        }

        // FDN read (modulated taps), per-line damping, T60 gains
        let t60 = 0.2 + self.decay * self.decay * 11.8;
        let dmp = self.damp * 0.85;
        let mdep = self.depth * 10.0;
        let mut v = [0.0f32; 8];
        for i in 0..8 {
            self.lfo[i] += 2.0 * std::f32::consts::PI * SPACE_LFO_HZ[i] / self.sr;
            if self.lfo[i] > 2.0 * std::f32::consts::PI {
                self.lfo[i] -= 2.0 * std::f32::consts::PI;
            }
            let m = (crate::dmath::sin(self.lfo[i]) + 1.0) * 0.5 * mdep;
            let fpos = self.len[i] as f32 - 1.0 - m;
            let ip = fpos.floor() as usize;
            let frac = fpos - ip as f32;
            let buf = &self.lines[i];
            let blen = buf.len();
            let a = buf[(self.pos[i] + blen - 1 - ip) % blen];
            let b = buf[(self.pos[i] + blen - 2 - ip) % blen];
            let tap = a + (b - a) * frac;
            // frequency-dependent decay: high frequencies die first
            self.lp[i] += (tap - self.lp[i]) * (1.0 - dmp);
            let g = crate::dmath::powf(10.0, -3.0 * self.len[i] as f32 / (t60 * self.sr));
            v[i] = self.lp[i] * g;
        }
        // Hadamard mixing (fast Walsh–Hadamard, normalized)
        for stride in [1usize, 2, 4] {
            let mut i = 0;
            while i < 8 {
                for j in i..i + stride {
                    let a = v[j];
                    let b = v[j + stride];
                    v[j] = a + b;
                    v[j + stride] = a - b;
                }
                i += stride * 2;
            }
        }
        for w in &mut v {
            *w *= 0.353_553_4; // 1/sqrt(8)
        }
        // write back with the diffused input (alternating sign decorrelates)
        for (i, &vi) in v.iter().enumerate() {
            let inj = if i % 2 == 0 { x } else { -x };
            let p = self.pos[i];
            self.lines[i][p] = vi + inj;
            self.pos[i] = (p + 1) % self.lines[i].len();
        }
        // stereo taps: even lines left, odd lines right
        let wl = (v[0] + v[2] + v[4] + v[6]) * 0.7;
        let wr = (v[1] + v[3] + v[5] + v[7]) * 0.7;
        // width: mid/side around the wet signal
        let mid = (wl + wr) * 0.5;
        let side = (wl - wr) * 0.5 * self.width;
        let (wl, wr) = (mid + side, mid - side);
        (
            dry_l + (wl - dry_l) * self.mix,
            dry_r + (wr - dry_r) * self.mix,
        )
    }
}

/// The glue: a program-dependent bus compressor (#127). Where `comp` is a
/// static one-pole gain computer, this one behaves like the desk glue a mix
/// actually leans on:
///
/// - hybrid detector: RMS body + peak overshoot, after a sidechain highpass
///   (`sc_hpf`) so the kick's sub doesn't pump the whole bus
/// - soft knee: reduction fades in across the knee width instead of kinking
/// - program-dependent release: a fast and a slow recovery run in parallel
///   and the slow one only charges while reduction PERSISTS — transients
///   recover in tens of ms, sustained squash lets go slowly (the "breathing
///   with the music" behavior no static release reproduces)
/// - lookahead: the audio path runs 2.5 ms late so the attack has already
///   seen the transient it is catching (offline rendering makes this free);
///   the dry path of `mix` is latency-matched
/// - `mix` under 1.0 is parallel compression on the same insert
///
/// Linear-domain math + dmath::powf only — native/wasm bit-identical.
pub struct Glue {
    sr: f32,
    /// sidechain HP state (per channel)
    sc: (f32, f32),
    /// RMS accumulator (one-pole of the squared detector)
    ms: f32,
    /// smoothed gain-reduction envelope (1.0 = no reduction)
    gr: f32,
    /// slow recovery channel: charges while reduction persists
    slow: f32,
    /// lookahead ring
    buf: Vec<(f32, f32)>,
    pos: usize,
    pub thresh: f32,  // 0..1 → linear 0.05..1.0
    pub ratio: f32,   // 0..1 → 1:1..20:1
    pub attack: f32,  // 0..1 → 0.1..30 ms
    pub release: f32, // 0..1 → 60 ms..1.2 s (the FAST stage; slow is ~6x)
    pub knee: f32,    // 0..1 → hard..±12 dB-ish soft zone
    pub sc_hpf: f32,  // 0..1 → off..300 Hz
    pub makeup: f32,  // 0..1 → x1..x4
    pub mix: f32,     // parallel blend
}

/// 2.5 ms at 48 kHz.
const GLUE_LOOKAHEAD: usize = 120;

impl Glue {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            sc: (0.0, 0.0),
            ms: 0.0,
            gr: 1.0,
            slow: 1.0,
            buf: vec![(0.0, 0.0); GLUE_LOOKAHEAD],
            pos: 0,
            thresh: 0.5,
            ratio: 0.3,
            attack: 0.3,
            release: 0.3,
            knee: 0.5,
            sc_hpf: 0.0,
            makeup: 0.15,
            mix: 1.0,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // ---- detector (on the UNDELAYED input = lookahead) ----
        let (dl, dr) = if self.sc_hpf > 0.0 {
            // one-pole highpass at 20..300 Hz
            let f = 20.0 + self.sc_hpf.clamp(0.0, 1.0) * 280.0;
            let a = (f / self.sr * std::f32::consts::TAU).min(1.0);
            self.sc.0 += (l - self.sc.0) * a;
            self.sc.1 += (r - self.sc.1) * a;
            (l - self.sc.0, r - self.sc.1)
        } else {
            (l, r)
        };
        let inst = (dl.abs() + dr.abs()) * 0.5;
        // RMS body (~10 ms window) + a taste of the raw peak on top
        let rms_a = (1.0 / (0.010 * self.sr)).min(1.0);
        self.ms += (inst * inst - self.ms) * rms_a;
        let level = self.ms.sqrt() * 0.8 + inst * 0.2;

        // ---- gain computer: soft knee in the linear domain ----
        let t = 0.05 + self.thresh.clamp(0.0, 1.0) * 0.95;
        let ratio = 1.0 + self.ratio.clamp(0.0, 1.0) * 19.0;
        let exp = -(1.0 - 1.0 / ratio);
        // knee: reduction blends in between t/w and t*w
        let w = 1.0 + self.knee.clamp(0.0, 1.0) * 1.5;
        let x = level / t.max(1e-6);
        let target = if x <= 1.0 / w {
            1.0
        } else {
            let full = crate::dmath::powf(x.max(1e-6), exp);
            if x >= w || w <= 1.0 + 1e-6 {
                full
            } else {
                // fade the reduction in across the knee zone
                let k = (x - 1.0 / w) / (w - 1.0 / w);
                1.0 + (full - 1.0) * k * k
            }
        };

        // ---- program-dependent ballistics ----
        // `slow` charges toward the reduction with a ~0.4 s constant: a
        // transient barely moves it, sustained squash fills it up. The
        // RELEASE COEFFICIENT itself then slides from fast (uncharged)
        // to 8x slower (fully charged) — recovery breathes with how long
        // the compressor has been working, not just how hard
        let att = (1.0 / ((0.0001 + self.attack * 0.03) * self.sr)).min(1.0);
        let rel_fast = (1.0 / ((0.06 + self.release * 1.14) * self.sr)).min(1.0);
        let charge_coef = (1.0 / (0.4 * self.sr)).min(1.0);
        let charge = (1.0 - self.slow).clamp(0.0, 1.0);
        let rel = rel_fast * (1.0 - charge) + (rel_fast * 0.125) * charge;
        if target < self.gr {
            self.gr += (target - self.gr) * att;
        } else {
            self.gr += (target - self.gr) * rel;
        }
        self.slow += (self.gr - self.slow) * charge_coef;
        let gain = self.gr;

        // ---- apply to the DELAYED audio, blend the delayed dry ----
        let (wl_in, wr_in) = self.buf[self.pos];
        self.buf[self.pos] = (l, r);
        self.pos = (self.pos + 1) % GLUE_LOOKAHEAD;
        let makeup = 1.0 + self.makeup.clamp(0.0, 1.0) * 3.0;
        let (wl, wr) = (wl_in * gain * makeup, wr_in * gain * makeup);
        let m = self.mix.clamp(0.0, 1.0);
        (wl_in * (1.0 - m) + wl * m, wr_in * (1.0 - m) + wr * m)
    }
}

#[cfg(test)]
mod glue_tests {
    use super::Glue;

    fn make() -> Glue {
        let mut g = Glue::new(48_000.0);
        g.thresh = 0.15; // low threshold so the test tones compress
        g.ratio = 0.35;
        g.attack = 0.1;
        g.release = 0.2;
        g.mix = 1.0;
        g.makeup = 0.0;
        g
    }

    /// Feed `secs` of a 200 Hz tone at `amp`, return the last output amp.
    fn run_tone(g: &mut Glue, amp: f32, secs: f32) -> f32 {
        let n = (secs * 48_000.0) as usize;
        let mut peak = 0.0f32;
        for i in 0..n {
            let x = amp * crate::dmath::sin(i as f32 * 200.0 / 48_000.0 * std::f32::consts::TAU);
            let (l, _) = g.process(x, x);
            if i > n.saturating_sub(2_000) {
                peak = peak.max(l.abs());
            }
        }
        peak
    }

    #[test]
    fn release_is_program_dependent() {
        // recovery after a SHORT burst vs after LONG sustain: the slow
        // channel only charges while reduction persists, so the burst
        // case must come back noticeably faster
        let recovered_after = |loud_secs: f32| -> f32 {
            let mut g = make();
            run_tone(&mut g, 0.9, loud_secs);
            // 120 ms into recovery, measure a quiet probe tone
            run_tone(&mut g, 0.05, 0.12)
        };
        let after_burst = recovered_after(0.05);
        let after_sustain = recovered_after(2.0);
        assert!(
            after_burst > after_sustain * 1.02,
            "burst must recover faster than sustain ({after_burst} vs {after_sustain})"
        );
    }

    #[test]
    fn sidechain_hpf_keeps_bass_from_pumping() {
        // a 30 Hz sub at the same level must pull far less gain with the
        // sidechain highpass engaged
        let gr_with = |hpf: f32| -> f32 {
            let mut g = make();
            g.sc_hpf = hpf;
            let mut min_out = f32::MAX;
            for i in 0..48_000 {
                let x = 0.9 * crate::dmath::sin(i as f32 * 30.0 / 48_000.0 * std::f32::consts::TAU);
                let (l, _) = g.process(x, x);
                if i > 40_000 {
                    min_out = min_out.min(l.abs() + (1.0 - x.abs()));
                }
            }
            // steady-state gain estimate via output/input peak
            let mut peak_in = 0.0f32;
            let mut peak_out = 0.0f32;
            for i in 0..9_600 {
                let x = 0.9 * crate::dmath::sin(i as f32 * 30.0 / 48_000.0 * std::f32::consts::TAU);
                let (l, _) = g.process(x, x);
                peak_in = peak_in.max(x.abs());
                peak_out = peak_out.max(l.abs());
            }
            peak_out / peak_in
        };
        let ducked = gr_with(0.0);
        let spared = gr_with(1.0);
        assert!(
            spared > ducked * 1.3,
            "the HPF must spare the sub ({spared} vs {ducked})"
        );
    }

    #[test]
    fn glue_is_deterministic_and_bounded() {
        let render = || -> Vec<u32> {
            let mut g = make();
            (0..4_800)
                .map(|i| {
                    let x = if i % 480 < 40 { 0.9 } else { 0.1 };
                    g.process(x, -x).0.to_bits()
                })
                .collect()
        };
        assert_eq!(render(), render(), "same input, same bits");
        let mut g = make();
        for i in 0..48_000 {
            let x = if i % 480 < 40 { 1.5 } else { 0.0 };
            let (l, r) = g.process(x, x);
            assert!(l.is_finite() && r.is_finite() && l.abs() < 8.0, "bounded output");
        }
    }
}
