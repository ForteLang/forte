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
}

impl Saturate {
    pub fn new(sr: f32) -> Self {
        Self { sr, lp: (0.0, 0.0), mode: 0, drive: 0.4, tone: 0.7, mix: 1.0 }
    }

    #[inline]
    fn shape(&self, x: f32) -> f32 {
        let d = 1.0 + self.drive * 9.0;
        match self.mode {
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
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (mut wl, mut wr) = (self.shape(l), self.shape(r));
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
}

impl ParComp {
    pub fn new(sr: f32) -> Self {
        Self { sr, env: 0.0, low: (0.0, 0.0), amount: 0.35, drive: 0.5, color: 0.3 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let g_in = 1.0 + self.drive * 7.0;
        let (xl, xr) = (l * g_in, r * g_in);
        let level = (xl.abs() + xr.abs()) * 0.5;
        // fast follower: ~2 ms up, ~60 ms down
        let up = (1.0 / (0.002 * self.sr)).min(1.0);
        let down = (1.0 / (0.06 * self.sr)).min(1.0);
        self.env += (level - self.env) * if level > self.env { up } else { down };
        // 8:1 over −24 dBFS (0.063 linear)
        const THRESH: f32 = 0.063;
        let gr = if self.env > THRESH {
            crate::dmath::powf(self.env / THRESH, -(1.0 - 1.0 / 8.0))
        } else {
            1.0
        };
        let makeup = 2.2;
        let (mut wl, mut wr) = (xl * gr * makeup, xr * gr * makeup);
        // smiley tilt: split lows with a one-pole, lift lows and the residue
        let a = (180.0 / self.sr * std::f32::consts::TAU).min(1.0);
        self.low.0 += (wl - self.low.0) * a;
        self.low.1 += (wr - self.low.1) * a;
        let c = self.color.clamp(0.0, 1.0) * 0.7;
        wl += self.low.0 * c + (wl - self.low.0) * c * 0.6;
        wr += self.low.1 * c + (wr - self.low.1) * c * 0.6;
        // safety: the crushed bus must never explode
        wl = crate::dmath::tanh(wl);
        wr = crate::dmath::tanh(wr);
        let m = self.amount.clamp(0.0, 1.0);
        (l + wl * m, r + wr * m)
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
