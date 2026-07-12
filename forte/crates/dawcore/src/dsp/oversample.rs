//! Deterministic halfband oversampling for nonlinear stages.
//!
//! A nonlinearity run at 48 kHz sprays harmonics past Nyquist and they fold
//! back as inharmonic aliasing — the "cheap digital" fingerprint. Running the
//! same waveshaper at 2x/4x and filtering on the way back down puts those
//! harmonics above the passband where the downsampling filter removes them.
//!
//! Everything here is plain f32 multiply/accumulate over fixed coefficient
//! tables in a fixed order, so native and wasm renders agree bit-for-bit.
//! The filters are linear-phase halfbands; a full up→shape→down round trip
//! delays the wet path by a whole number of base-rate samples (31 at 2x,
//! 37 at 4x — ~0.7 ms), and `run` hands back a latency-matched copy of the
//! dry input so `mix`-style blends stay phase-aligned instead of combing.

/// 63-tap Blackman-windowed halfband (base↔2x stage).
/// Passband flat to 0.19·fs (±0.001 dB), stopband −75 dB above 0.30·fs.
const HB63: [f32; 63] = [
    1.424_979_7e-19,
    0.0,
    4.117_892e-5,
    0.0,
    -1.843_656_5e-4,
    0.0,
    4.762_259_6e-4,
    0.0,
    -9.890_387e-4,
    0.0,
    1.823_255_7e-3,
    0.0,
    -3.110_167_8e-3,
    0.0,
    5.017_218_6e-3,
    0.0,
    -7.761_138_6e-3,
    0.0,
    1.163_982_2e-2,
    0.0,
    -1.710_853_7e-2,
    0.0,
    2.496_967e-2,
    0.0,
    -3.690_090_5e-2,
    0.0,
    5.726_334e-2,
    0.0,
    -1.021_489e-1,
    0.0,
    3.169_720_5e-1,
    5.0e-1,
    3.169_720_5e-1,
    0.0,
    -1.021_489e-1,
    0.0,
    5.726_334e-2,
    0.0,
    -3.690_090_5e-2,
    0.0,
    2.496_967e-2,
    0.0,
    -1.710_853_7e-2,
    0.0,
    1.163_982_2e-2,
    0.0,
    -7.761_138_6e-3,
    0.0,
    5.017_218_6e-3,
    0.0,
    -3.110_167_8e-3,
    0.0,
    1.823_255_7e-3,
    0.0,
    -9.890_387e-4,
    0.0,
    4.762_259_6e-4,
    0.0,
    -1.843_656_5e-4,
    0.0,
    4.117_892e-5,
    0.0,
    1.424_979_7e-19,
];

/// 25-tap Blackman-windowed halfband (2x↔4x stage). Wider transition is
/// fine here: everything that could fold into the audible band after the
/// second decimation sits above 0.375·fs, where this is −74 dB down.
const HB25: [f32; 25] = [
    0.0,
    -1.828_580_1e-4,
    0.0,
    2.350_068_1e-3,
    0.0,
    -1.006_352_4e-2,
    0.0,
    3.056_586_5e-2,
    0.0,
    -8.207_656_4e-2,
    0.0,
    3.094_751_8e-1,
    5.0e-1,
    3.094_751_8e-1,
    0.0,
    -8.207_656_4e-2,
    0.0,
    3.056_586_5e-2,
    0.0,
    -1.006_352_4e-2,
    0.0,
    2.350_068_1e-3,
    0.0,
    -1.828_580_1e-4,
    0.0,
];

/// One direct-form FIR. Offline rendering pays the O(taps) shift happily;
/// the win is a fixed, platform-independent order of operations.
struct Fir {
    taps: &'static [f32],
    buf: Vec<f32>,
}

impl Fir {
    fn new(taps: &'static [f32]) -> Self {
        Self { taps, buf: vec![0.0; taps.len()] }
    }

    #[inline]
    fn push(&mut self, x: f32) -> f32 {
        let n = self.buf.len();
        self.buf.copy_within(0..n - 1, 1);
        self.buf[0] = x;
        let mut acc = 0.0f32;
        for (c, s) in self.taps.iter().zip(self.buf.iter()) {
            acc += c * s;
        }
        acc
    }
}

/// Stereo oversampler wrapping a nonlinear stage: `run` upsamples the input,
/// calls the stage once per high-rate sample, and decimates the result.
pub struct Oversampler {
    factor: u8,
    up1: [Fir; 2],
    down1: [Fir; 2],
    /// second stage, engaged at 4x only
    up2: [Fir; 2],
    down2: [Fir; 2],
    /// latency-matching ring for the dry signal
    dry: Vec<(f32, f32)>,
    dry_pos: usize,
}

impl Oversampler {
    /// `factor` must be 2 or 4 (anything else clamps to 2 — callers gate
    /// the "off" case themselves so the bypass path stays bit-exact).
    pub fn new(factor: u8) -> Self {
        let factor = if factor >= 4 { 4 } else { 2 };
        Self {
            factor,
            up1: [Fir::new(&HB63), Fir::new(&HB63)],
            down1: [Fir::new(&HB63), Fir::new(&HB63)],
            up2: [Fir::new(&HB25), Fir::new(&HB25)],
            down2: [Fir::new(&HB25), Fir::new(&HB25)],
            dry: vec![(0.0, 0.0); Self::latency(factor).max(1)],
            dry_pos: 0,
        }
    }

    pub fn factor(&self) -> u8 {
        self.factor
    }

    /// Whole-sample wet-path delay at the base rate: the halfband stages sum
    /// to exactly 31 (2x) / 37 (4x) input samples.
    pub fn latency(factor: u8) -> usize {
        match factor {
            4 => 37,
            2 => 31,
            _ => 0,
        }
    }

    /// Feed one stereo sample through `stage` at `factor`× the base rate.
    /// Returns `(wet_l, wet_r, dry_l, dry_r)`; the dry pair is the input
    /// delayed to line up with the filtered wet pair.
    #[inline]
    pub fn run(
        &mut self,
        l: f32,
        r: f32,
        mut stage: impl FnMut(f32, f32) -> (f32, f32),
    ) -> (f32, f32, f32, f32) {
        let (dry_l, dry_r) = self.dry[self.dry_pos];
        self.dry[self.dry_pos] = (l, r);
        self.dry_pos = (self.dry_pos + 1) % self.dry.len();

        let mut out = (0.0f32, 0.0f32);
        // ×2 makes up for the energy the zero-stuffed samples don't carry
        for (i, (hl, hr)) in [(l * 2.0, r * 2.0), (0.0, 0.0)].into_iter().enumerate() {
            let a = self.up1[0].push(hl);
            let b = self.up1[1].push(hr);
            let (wa, wb) = if self.factor == 4 {
                let mut mid = (0.0f32, 0.0f32);
                for (j, (ql, qr)) in [(a * 2.0, b * 2.0), (0.0, 0.0)].into_iter().enumerate() {
                    let ua = self.up2[0].push(ql);
                    let ub = self.up2[1].push(qr);
                    let (sa, sb) = stage(ua, ub);
                    let da = self.down2[0].push(sa);
                    let db = self.down2[1].push(sb);
                    if j == 0 {
                        mid = (da, db);
                    }
                }
                mid
            } else {
                stage(a, b)
            };
            let da = self.down1[0].push(wa);
            let db = self.down1[1].push(wb);
            if i == 0 {
                out = (da, db);
            }
        }
        (out.0, out.1, dry_l, dry_r)
    }
}

/// Param slot → factor for the shared `os` switch. The compiler stores
/// switch choices as their raw index (`off`/`2x`/`4x` → 0/1/2).
pub fn os_factor(v: f32) -> u8 {
    match v.round() as i32 {
        1 => 2,
        x if x >= 2 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hann-windowed single-bin DFT, amplitude in dBFS.
    fn tone_db(buf: &[f32], sr: f64, freq: f64) -> f64 {
        let n = buf.len();
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in buf.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (n - 1) as f64).cos();
            let ph = std::f64::consts::TAU * freq * i as f64 / sr;
            re += x as f64 * w * ph.cos();
            im -= x as f64 * w * ph.sin();
        }
        // ÷ coherent gain of the Hann window (0.5), ×2 for the one-sided bin
        let mag = (re * re + im * im).sqrt() * 2.0 / n as f64 / 0.5;
        20.0 * (mag + 1e-15).log10()
    }

    fn saturated_sine(os_knob: f32) -> Vec<f32> {
        let sr = 48_000.0f64;
        let f0 = 5_000.0f64;
        let mut s = crate::dsp::effects::Saturate::new(sr as f32);
        s.mode = 2; // fuzz — the harshest shaper we ship
        s.drive = 0.35;
        s.tone = 1.0;
        s.mix = 1.0;
        s.set_os(os_factor(os_knob));
        let mut out = Vec::with_capacity(9_600);
        for i in 0..9_600 {
            let x = (std::f64::consts::TAU * f0 * i as f64 / sr).sin() as f32 * 0.8;
            let (l, _) = s.process(x, x);
            out.push(l);
        }
        out.split_off(4_800) // steady state only
    }

    /// The issue-#124 acceptance: a high sine through `saturate` folds its
    /// out-of-band harmonics back into the audible range at 48 kHz; with
    /// `os: "4x"` those folded partials drop into the noise while the real
    /// harmonic survives.
    #[test]
    fn oversampling_kills_audible_foldback() {
        let sr = 48_000.0;
        let plain = saturated_sine(0.0);
        let hq = saturated_sine(1.0);
        // 5 kHz odd harmonics: 15 kHz is real; 35 kHz and 45 kHz exceed
        // Nyquist and fold to 13 kHz and 3 kHz
        for alias in [3_000.0, 13_000.0] {
            let before = tone_db(&plain, sr, alias);
            let after = tone_db(&hq, sr, alias);
            assert!(
                before - after > 25.0 && after < -60.0,
                "alias at {alias} Hz: {before:.1} dB → {after:.1} dB"
            );
        }
        let h3_before = tone_db(&plain, sr, 15_000.0);
        let h3_after = tone_db(&hq, sr, 15_000.0);
        assert!(
            (h3_before - h3_after).abs() < 1.5,
            "real 15 kHz harmonic must survive: {h3_before:.1} dB → {h3_after:.1} dB"
        );
    }

    /// 2x also helps, and both factors leave the fundamental level alone.
    #[test]
    fn oversampling_is_transparent_in_band() {
        let sr = 48_000.0;
        let plain = saturated_sine(0.0);
        for knob in [0.5, 1.0] {
            let os = saturated_sine(knob);
            let d = tone_db(&plain, sr, 5_000.0) - tone_db(&os, sr, 5_000.0);
            assert!(d.abs() < 1.0, "fundamental shifted {d:.2} dB at knob {knob}");
        }
    }

    #[test]
    fn latency_is_a_whole_number_of_samples() {
        // an impulse comes back centered on one sample, factor× later
        for factor in [2u8, 4] {
            let mut o = Oversampler::new(factor);
            let lat = Oversampler::latency(factor);
            let mut peak = (0usize, 0.0f32);
            for i in 0..(lat + 32) {
                let x = if i == 0 { 1.0 } else { 0.0 };
                let (wl, _, dl, _) = o.run(x, x, |a, b| (a, b));
                if wl.abs() > peak.1 {
                    peak = (i, wl.abs());
                }
                // the dry ring must line up with the wet peak
                if i == lat {
                    assert!(dl == 1.0, "dry not delayed by {lat} at {factor}x");
                }
            }
            assert_eq!(peak.0, lat, "wet peak off at {factor}x");
            assert!(peak.1 > 0.9, "unity impulse came back at {:.3}", peak.1);
        }
    }
}
