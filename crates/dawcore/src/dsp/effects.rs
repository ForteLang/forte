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
