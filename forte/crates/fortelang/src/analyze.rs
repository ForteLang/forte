//! `forte analyze` — the agent's ears (SRS #128).
//!
//! Everything an agent used to eyeball with ad-hoc scripts (RMS windows,
//! silence censuses, spike detectors) becomes one deterministic,
//! machine-readable report derived from the same offline render the build
//! digests certify: loudness (BS.1770-style), spectral balance and
//! per-track masking, stereo width, onset-vs-score timing, section energy
//! arc, silence map, and a chroma-based key estimate checked against the
//! declared key.
//!
//! Analysis is measurement, not art: values are rounded on the way out
//! (0.01 dB / 0.1 ms grain) so reports diff cleanly in git.

use dawcore::model::{Project, Scale, TrackKind, NOTE_NAMES};
use serde::Serialize;

/// A named span of the song in beats (sections come from the source's
/// `section name = bars(a..b)` statements; callers convert bars → beats).
#[derive(Clone)]
pub struct SectionSpan {
    pub name: String,
    pub start_beat: f64,
    pub end_beat: f64,
}

#[derive(Serialize)]
pub struct Analysis {
    pub seconds: f64,
    pub tempo: f64,
    pub loudness: Loudness,
    pub spectral: Spectral,
    pub stereo: Stereo,
    pub rhythm: Rhythm,
    pub structure: Structure,
    pub tonality: Tonality,
}

#[derive(Serialize)]
pub struct Loudness {
    /// BS.1770-gated program loudness of the stereo mix
    pub integrated_lufs: f64,
    /// 3 s short-term loudness, one value per second
    pub short_term_lufs: Vec<f64>,
    /// inter-sample (4x interpolated) peak
    pub true_peak_db: f64,
    pub rms_db: f64,
    pub crest_db: f64,
}

pub const BAND_NAMES: [&str; 5] = ["sub", "low", "mid", "high", "air"];
/// crossover points between the five bands (Hz)
pub const BAND_EDGES: [f64; 4] = [60.0, 250.0, 2_000.0, 8_000.0];

#[derive(Serialize)]
pub struct Spectral {
    /// mix energy share per band (sub/low/mid/high/air), percent
    pub band_share_pct: [f64; 5],
    /// per-track band occupancy (stems rendered with the same engine)
    pub tracks: Vec<TrackBands>,
    /// pairwise masking overlap of band distributions, worst first
    pub masking: Vec<MaskPair>,
}

#[derive(Serialize)]
pub struct TrackBands {
    pub name: String,
    pub rms_db: f64,
    pub band_share_pct: [f64; 5],
}

#[derive(Serialize)]
pub struct MaskPair {
    pub a: String,
    pub b: String,
    /// Σ min(shareA, shareB) over bands — 1.0 = identical spectral footprint
    pub overlap: f64,
}

#[derive(Serialize)]
pub struct Stereo {
    /// 10·log10(side/mid energy) for the whole mix; -inf when dead mono
    pub side_mid_db: f64,
    /// side/mid energy ratio per second (linear)
    pub width_curve: Vec<f64>,
    /// per-band side/mid dB — where the width actually lives
    pub band_side_mid_db: [f64; 5],
}

#[derive(Serialize)]
pub struct Rhythm {
    /// note-on count written in the score
    pub score_onsets: usize,
    /// transients detected in the rendered audio
    pub audio_onsets: usize,
    /// share of audio onsets landing within ±30 ms of a written note-on
    pub matched_pct: f64,
    /// mean |offset| of the matched onsets, ms
    pub mean_offset_ms: f64,
    /// onsets per second, one entry per section (or the whole song)
    pub density_per_section: Vec<SectionDensity>,
}

#[derive(Serialize)]
pub struct SectionDensity {
    pub name: String,
    pub onsets_per_second: f64,
}

#[derive(Serialize)]
pub struct Structure {
    pub sections: Vec<SectionEnergy>,
    /// runs of ≥40 ms below −48 dBFS (the chop groove made visible)
    pub silences: Vec<Silence>,
    pub silence_total_pct: f64,
}

#[derive(Serialize)]
pub struct SectionEnergy {
    pub name: String,
    pub start_s: f64,
    pub end_s: f64,
    pub rms_db: f64,
    pub peak_db: f64,
}

#[derive(Serialize)]
pub struct Silence {
    pub start_s: f64,
    pub len_ms: f64,
}

#[derive(Serialize)]
pub struct Tonality {
    /// pitch-class energy share (C, C#, … B), percent
    pub chroma_pct: [f64; 12],
    /// best Krumhansl-profile match, e.g. "A minor"
    pub estimated_key: String,
    pub declared_key: String,
    /// None when the declared scale has no major/minor profile to compare
    pub agrees: Option<bool>,
    /// the estimate is the RELATIVE major/minor of the declared key — the
    /// same seven notes, tonic ambiguity rather than a wrong-note problem
    pub relative: bool,
}

// ---------------------------------------------------------------------------

const SR: f64 = 48_000.0;
const SILENCE_FLOOR: f32 = 0.004; // ≈ −48 dBFS
const SILENCE_MIN_MS: f64 = 40.0;

fn db(x: f64) -> f64 {
    10.0 * (x + 1e-15).log10()
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// One biquad, direct form 1, f64 state.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, x: [0.0; 2], y: [0.0; 2] }
    }
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The BS.1770 K-weighting pair at 48 kHz (shelf + high-pass), per channel.
fn k_weight() -> [Biquad; 2] {
    [
        Biquad::new(
            [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85],
            [-1.690_659_293_182_41, 0.732_480_774_215_85],
        ),
        Biquad::new([1.0, -2.0, 1.0], [-1.990_047_454_833_98, 0.990_072_250_366_21]),
    ]
}

/// Five-band splitter: cascaded one-pole pairs at the four crossovers.
/// 12 dB/oct crossovers are soft, but monotone, cheap and deterministic —
/// plenty for balance/masking REPORTING (this is a meter, not a filter bank
/// anyone mixes through).
struct BandSplit {
    lp: [[f64; 2]; 4], // four crossovers × two cascaded poles
    a: [f64; 4],
}

impl BandSplit {
    fn new() -> Self {
        let mut a = [0.0; 4];
        for (i, f) in BAND_EDGES.iter().enumerate() {
            a[i] = 1.0 - (-std::f64::consts::TAU * f / SR).exp();
        }
        Self { lp: [[0.0; 2]; 4], a }
    }
    /// Returns the five band samples for one input sample.
    #[inline]
    fn split(&mut self, x: f64) -> [f64; 5] {
        let mut lows = [0.0f64; 4];
        for (i, low) in lows.iter_mut().enumerate() {
            self.lp[i][0] += (x - self.lp[i][0]) * self.a[i];
            self.lp[i][1] += (self.lp[i][0] - self.lp[i][1]) * self.a[i];
            *low = self.lp[i][1];
        }
        [lows[0], lows[1] - lows[0], lows[2] - lows[1], lows[3] - lows[2], x - lows[3]]
    }
}

fn band_energy(l: &[f32], r: &[f32]) -> ([f64; 5], f64) {
    let mut split = BandSplit::new();
    let mut e = [0.0f64; 5];
    let mut total = 0.0f64;
    for i in 0..l.len() {
        let m = (l[i] as f64 + r[i] as f64) * 0.5;
        let bands = split.split(m);
        for (acc, b) in e.iter_mut().zip(bands.iter()) {
            *acc += b * b;
        }
        total += m * m;
    }
    (e, total)
}

fn share_pct(e: &[f64; 5]) -> [f64; 5] {
    let sum: f64 = e.iter().sum::<f64>().max(1e-15);
    let mut out = [0.0; 5];
    for (o, v) in out.iter_mut().zip(e.iter()) {
        *o = r2(v / sum * 100.0);
    }
    out
}

// --- loudness ---------------------------------------------------------------

fn loudness(l: &[f32], r: &[f32]) -> Loudness {
    // 100 ms frame energies of the K-weighted stereo sum
    let frame = (SR * 0.1) as usize;
    let mut kwl = k_weight();
    let mut kwr = k_weight();
    let mut frames: Vec<f64> = Vec::with_capacity(l.len() / frame + 1);
    let mut acc = 0.0f64;
    for i in 0..l.len() {
        let a1 = kwl[0].process(l[i] as f64);
        let a = kwl[1].process(a1);
        let b1 = kwr[0].process(r[i] as f64);
        let b = kwr[1].process(b1);
        acc += a * a + b * b;
        if (i + 1) % frame == 0 {
            frames.push(acc / frame as f64);
            acc = 0.0;
        }
    }
    // 400 ms gating blocks at 75% overlap = every frame, 4-frame mean
    let block_l = |ms: f64| -0.691 + db(ms);
    let mut blocks: Vec<f64> = Vec::new();
    for w in frames.windows(4) {
        blocks.push(w.iter().sum::<f64>() / 4.0);
    }
    let abs_gated: Vec<f64> = blocks.iter().copied().filter(|&m| block_l(m) > -70.0).collect();
    let integrated = if abs_gated.is_empty() {
        f64::NEG_INFINITY
    } else {
        let mean = abs_gated.iter().sum::<f64>() / abs_gated.len() as f64;
        let gate = block_l(mean) - 10.0;
        let rel: Vec<f64> = abs_gated.iter().copied().filter(|&m| block_l(m) > gate).collect();
        if rel.is_empty() {
            f64::NEG_INFINITY
        } else {
            block_l(rel.iter().sum::<f64>() / rel.len() as f64)
        }
    };
    // short-term: 3 s (30 frames), hop 1 s (10 frames)
    let mut short = Vec::new();
    let mut i = 0;
    while i + 30 <= frames.len() {
        let ms = frames[i..i + 30].iter().sum::<f64>() / 30.0;
        short.push(r2(block_l(ms)));
        i += 10;
    }
    // true peak: 4x windowed-sinc interpolation between samples
    let mut peak = 0.0f64;
    let mut kernel = [[0.0f64; 16]; 3];
    for (pi, phase) in [0.25f64, 0.5, 0.75].iter().enumerate() {
        let mut sum = 0.0;
        for (ki, k) in kernel[pi].iter_mut().enumerate() {
            let t = ki as f64 - 7.0 - phase;
            let sinc = if t.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t) };
            let w = 0.5 + 0.5 * (std::f64::consts::PI * t / 8.0).cos();
            *k = sinc * w.max(0.0);
            sum += *k;
        }
        for k in kernel[pi].iter_mut() {
            *k /= sum;
        }
    }
    let mut sum_sq = 0.0f64;
    for ch in [l, r] {
        for (i, &s) in ch.iter().enumerate() {
            peak = peak.max((s as f64).abs());
            sum_sq += s as f64 * s as f64;
            if i >= 8 && i + 8 < ch.len() {
                for k in &kernel {
                    let mut v = 0.0;
                    for (ki, kv) in k.iter().enumerate() {
                        v += kv * ch[i - 7 + ki] as f64;
                    }
                    peak = peak.max(v.abs());
                }
            }
        }
    }
    let rms = (sum_sq / (l.len().max(1) * 2) as f64).sqrt();
    let true_peak_db = 20.0 * (peak + 1e-15).log10();
    let rms_db = 20.0 * (rms + 1e-15).log10();
    Loudness {
        integrated_lufs: r2(integrated),
        short_term_lufs: short,
        true_peak_db: r2(true_peak_db),
        rms_db: r2(rms_db),
        crest_db: r2(true_peak_db - rms_db),
    }
}

/// Gated integrated loudness of a stereo buffer in the POWER domain
/// (K-weighted mean square with BS.1770-style gating). Everything here is
/// f64 multiply/add/compare over fixed constants — bit-identical on native
/// and wasm — so compile-time gain staging (`level`) may divide two of
/// these and take a sqrt without breaking the determinism promise.
/// Returns 0.0 for silence.
pub fn integrated_power(l: &[f32], r: &[f32]) -> f64 {
    let frame = (SR * 0.1) as usize;
    let mut kwl = k_weight();
    let mut kwr = k_weight();
    let mut frames: Vec<f64> = Vec::with_capacity(l.len() / frame + 1);
    let mut acc = 0.0f64;
    for i in 0..l.len() {
        let a1 = kwl[0].process(l[i] as f64);
        let a = kwl[1].process(a1);
        let b1 = kwr[0].process(r[i] as f64);
        let b = kwr[1].process(b1);
        acc += a * a + b * b;
        if (i + 1) % frame == 0 {
            frames.push(acc / frame as f64);
            acc = 0.0;
        }
    }
    // −70 LUFS absolute gate as a power threshold: 10^((−70 + 0.691)/10)
    const ABS_GATE_POWER: f64 = 1.172_465_304_582_298_1e-7;
    let mut blocks: Vec<f64> = Vec::new();
    for w in frames.windows(4) {
        let p = w.iter().sum::<f64>() / 4.0;
        if p > ABS_GATE_POWER {
            blocks.push(p);
        }
    }
    if blocks.is_empty() {
        return 0.0;
    }
    let mean = blocks.iter().sum::<f64>() / blocks.len() as f64;
    // relative gate: −10 dB below the first-pass mean = ×0.1 in power
    let gate = mean * 0.1;
    let (mut sum, mut n) = (0.0f64, 0usize);
    for &b in &blocks {
        if b > gate {
            sum += b;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

// --- stereo ------------------------------------------------------------------

fn stereo(l: &[f32], r: &[f32]) -> Stereo {
    let sec = SR as usize;
    let mut mid_e = 0.0f64;
    let mut side_e = 0.0f64;
    let mut curve = Vec::new();
    let (mut ms, mut ss) = (0.0f64, 0.0f64);
    let mut split_m = BandSplit::new();
    let mut split_s = BandSplit::new();
    let mut band_m = [0.0f64; 5];
    let mut band_s = [0.0f64; 5];
    for i in 0..l.len() {
        let m = (l[i] as f64 + r[i] as f64) * 0.5;
        let s = (l[i] as f64 - r[i] as f64) * 0.5;
        mid_e += m * m;
        side_e += s * s;
        ms += m * m;
        ss += s * s;
        for (acc, b) in band_m.iter_mut().zip(split_m.split(m).iter()) {
            *acc += b * b;
        }
        for (acc, b) in band_s.iter_mut().zip(split_s.split(s).iter()) {
            *acc += b * b;
        }
        if (i + 1) % sec == 0 {
            curve.push(r2(ss / ms.max(1e-15) * 1000.0) / 1000.0);
            ms = 0.0;
            ss = 0.0;
        }
    }
    let mut band_ratio = [0.0f64; 5];
    for i in 0..5 {
        band_ratio[i] = r2(db(band_s[i]) - db(band_m[i]));
    }
    Stereo {
        side_mid_db: r2(db(side_e) - db(mid_e)),
        width_curve: curve,
        band_side_mid_db: band_ratio,
    }
}

// --- rhythm -------------------------------------------------------------------

/// Note-on beats written in the arrangement (all instrument tracks).
fn score_onsets(project: &Project) -> Vec<f64> {
    let mut beats: Vec<f64> = Vec::new();
    for t in &project.tracks {
        if t.kind != TrackKind::Instrument {
            continue;
        }
        for ac in &t.arranger {
            for n in &ac.clip.notes {
                if n.start < ac.duration {
                    beats.push(ac.start + n.start);
                }
            }
        }
    }
    beats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    beats.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    beats
}

/// Envelope-rise transient detector: 256-sample RMS frames; an onset is a
/// frame ≥ 1.9× the mean of the previous six and above the floor, with a
/// 40 ms refractory. Deliberately simple — the point is a stable, published
/// definition every song is measured by, not psychoacoustic perfection.
fn audio_onsets(l: &[f32], r: &[f32]) -> Vec<f64> {
    const FRAME: usize = 256;
    let n = l.len() / FRAME;
    let mut env = Vec::with_capacity(n);
    for f in 0..n {
        let mut e = 0.0f64;
        for i in f * FRAME..(f + 1) * FRAME {
            let m = (l[i] as f64 + r[i] as f64) * 0.5;
            e += m * m;
        }
        env.push((e / FRAME as f64).sqrt());
    }
    let mut onsets = Vec::new();
    let refractory = (0.04 * SR / FRAME as f64) as usize;
    let mut last = usize::MAX / 2;
    for f in 6..n {
        let prev = env[f - 6..f].iter().sum::<f64>() / 6.0;
        if env[f] > prev * 1.9 && env[f] > 0.005 && f - last >= refractory {
            onsets.push(f as f64 * FRAME as f64 / SR);
            last = f;
        }
    }
    onsets
}

fn rhythm(project: &Project, l: &[f32], r: &[f32], sections: &[SectionSpan]) -> Rhythm {
    let spb = 60.0 / project.tempo; // seconds per beat
    let score: Vec<f64> = score_onsets(project).iter().map(|b| b * spb).collect();
    let audio = audio_onsets(l, r);
    let mut matched = 0usize;
    let mut offset_sum = 0.0f64;
    for &t in &audio {
        // nearest written onset
        let best = score
            .iter()
            .map(|&s| (t - s).abs())
            .fold(f64::INFINITY, f64::min);
        if best <= 0.030 {
            matched += 1;
            offset_sum += best;
        }
    }
    let seconds = l.len() as f64 / SR;
    let spans: Vec<SectionSpan> = if sections.is_empty() {
        vec![SectionSpan { name: "all".into(), start_beat: 0.0, end_beat: seconds / spb }]
    } else {
        sections.to_vec()
    };
    let density = spans
        .iter()
        .map(|s| {
            let (t0, t1) = (s.start_beat * spb, (s.end_beat * spb).min(seconds));
            let count = audio.iter().filter(|&&t| t >= t0 && t < t1).count();
            SectionDensity {
                name: s.name.clone(),
                onsets_per_second: r2(count as f64 / (t1 - t0).max(1e-9)),
            }
        })
        .collect();
    Rhythm {
        score_onsets: score.len(),
        audio_onsets: audio.len(),
        matched_pct: r2(if audio.is_empty() { 0.0 } else { matched as f64 / audio.len() as f64 * 100.0 }),
        mean_offset_ms: r2(if matched == 0 { 0.0 } else { offset_sum / matched as f64 * 1000.0 }),
        density_per_section: density,
    }
}

// --- structure ------------------------------------------------------------------

fn structure(l: &[f32], r: &[f32], tempo: f64, sections: &[SectionSpan]) -> Structure {
    let seconds = l.len() as f64 / SR;
    let spb = 60.0 / tempo;
    let spans: Vec<SectionSpan> = if sections.is_empty() {
        vec![SectionSpan { name: "all".into(), start_beat: 0.0, end_beat: seconds / spb }]
    } else {
        sections.to_vec()
    };
    let mut out = Vec::new();
    for s in &spans {
        let a = ((s.start_beat * spb * SR) as usize).min(l.len());
        let b = ((s.end_beat * spb * SR) as usize).min(l.len());
        if b <= a {
            continue;
        }
        let mut e = 0.0f64;
        let mut peak = 0.0f64;
        for i in a..b {
            let m = l[i] as f64 * l[i] as f64 + r[i] as f64 * r[i] as f64;
            e += m;
            peak = peak.max((l[i] as f64).abs()).max((r[i] as f64).abs());
        }
        out.push(SectionEnergy {
            name: s.name.clone(),
            start_s: r2(a as f64 / SR),
            end_s: r2(b as f64 / SR),
            rms_db: r2(20.0 * ((e / ((b - a) as f64 * 2.0)).sqrt() + 1e-15).log10()),
            peak_db: r2(20.0 * (peak + 1e-15).log10()),
        });
    }
    // silence census: runs where BOTH channels sit under the floor
    let min_run = (SILENCE_MIN_MS / 1000.0 * SR) as usize;
    let mut silences = Vec::new();
    let mut run = 0usize;
    let mut total = 0usize;
    for i in 0..l.len() {
        if l[i].abs() < SILENCE_FLOOR && r[i].abs() < SILENCE_FLOOR {
            run += 1;
        } else {
            if run >= min_run {
                silences.push(Silence {
                    start_s: r2((i - run) as f64 / SR),
                    len_ms: r2(run as f64 / SR * 1000.0),
                });
                total += run;
            }
            run = 0;
        }
    }
    if run >= min_run {
        silences.push(Silence {
            start_s: r2((l.len() - run) as f64 / SR),
            len_ms: r2(run as f64 / SR * 1000.0),
        });
        total += run;
    }
    Structure {
        sections: out,
        silence_total_pct: r2(total as f64 / l.len().max(1) as f64 * 100.0),
        silences,
    }
}

// --- tonality ---------------------------------------------------------------------

/// Krumhansl-Kessler key profiles.
const KK_MAJOR: [f64; 12] =
    [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const KK_MINOR: [f64; 12] =
    [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

fn correlation(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let ma = a.iter().sum::<f64>() / 12.0;
    let mb = b.iter().sum::<f64>() / 12.0;
    let (mut num, mut da, mut db_) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        num += (a[i] - ma) * (b[i] - mb);
        da += (a[i] - ma) * (a[i] - ma);
        db_ += (b[i] - mb) * (b[i] - mb);
    }
    num / (da.sqrt() * db_.sqrt() + 1e-15)
}

fn tonality(project: &Project, l: &[f32], r: &[f32]) -> Tonality {
    // chunked Goertzel over MIDI 36..84, energies folded into pitch classes
    const CHUNK: usize = 8_192;
    let mut chroma = [0.0f64; 12];
    let coefs: Vec<(usize, f64)> = (36u8..84)
        .map(|midi| {
            let freq = 440.0 * 2f64.powf((midi as f64 - 69.0) / 12.0);
            let w = std::f64::consts::TAU * freq / SR;
            ((midi % 12) as usize, 2.0 * w.cos())
        })
        .collect();
    let chunks = l.len() / CHUNK;
    for c in 0..chunks {
        let base = c * CHUNK;
        for &(pc, coef) in &coefs {
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for i in 0..CHUNK {
                let x = (l[base + i] as f64 + r[base + i] as f64) * 0.5;
                let s0 = x + coef * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            let power = s1 * s1 + s2 * s2 - coef * s1 * s2;
            chroma[pc] += power.max(0.0);
        }
    }
    let sum: f64 = chroma.iter().sum::<f64>().max(1e-15);
    let mut chroma_pct = [0.0f64; 12];
    for i in 0..12 {
        chroma_pct[i] = r2(chroma[i] / sum * 100.0);
    }
    // best-correlated Krumhansl rotation
    let mut best = (f64::NEG_INFINITY, 0usize, false);
    for root in 0..12 {
        let mut rot = [0.0f64; 12];
        for i in 0..12 {
            rot[i] = chroma[(root + i) % 12];
        }
        for (minor, profile) in [(false, &KK_MAJOR), (true, &KK_MINOR)] {
            let c = correlation(&rot, profile);
            if c > best.0 {
                best = (c, root, minor);
            }
        }
    }
    let estimated =
        format!("{} {}", NOTE_NAMES[best.1], if best.2 { "minor" } else { "major" });
    let declared_minor = matches!(project.key.scale, Scale::Minor | Scale::HarmonicMinor);
    let declared_comparable = matches!(project.key.scale, Scale::Major | Scale::Minor | Scale::HarmonicMinor);
    let declared = format!(
        "{} {:?}",
        NOTE_NAMES[(project.key.root % 12) as usize],
        project.key.scale
    );
    let agrees = declared_comparable
        .then_some(best.1 == (project.key.root % 12) as usize && best.2 == declared_minor);
    let droot = (project.key.root % 12) as usize;
    let relative = declared_comparable
        && agrees == Some(false)
        && best.2 != declared_minor
        && best.1 == (droot + if declared_minor { 3 } else { 9 }) % 12;
    Tonality { chroma_pct, estimated_key: estimated, declared_key: declared, agrees, relative }
}

// --- entry -------------------------------------------------------------------------

/// Analyze a compiled project. Renders the mix (and per-track stems when
/// `with_stems`) through the same deterministic offline engine as
/// `forte build`, then measures. `sections` name spans of the timeline.
pub fn analyze(project: &Project, sections: &[SectionSpan], with_stems: bool) -> Analysis {
    let (_key, mix) = crate::render_to_sample(project, 0.0, 60);
    let l = &mix.data[..];
    let r: &[f32] = match &mix.right {
        Some(rc) => rc,
        None => l,
    };

    let (mix_bands, _) = band_energy(l, r);
    let mut tracks = Vec::new();
    if with_stems {
        for t in &project.tracks {
            if t.kind != TrackKind::Instrument
                || t.arranger.iter().all(|a| a.clip.notes.is_empty())
            {
                continue;
            }
            let solo = crate::solo_project(project, t.id);
            let (_k, stem) = crate::render_to_sample(&solo, 0.0, 60);
            let sl = &stem.data[..];
            let sr_ch: &[f32] = match &stem.right {
                Some(rc) => rc,
                None => sl,
            };
            let (e, total) = band_energy(sl, sr_ch);
            tracks.push(TrackBands {
                name: t.name.clone(),
                rms_db: r2(10.0 * (total / sl.len().max(1) as f64 + 1e-15).log10()),
                band_share_pct: share_pct(&e),
            });
        }
    }
    let mut masking = Vec::new();
    for i in 0..tracks.len() {
        for j in i + 1..tracks.len() {
            let overlap: f64 = (0..5)
                .map(|b| tracks[i].band_share_pct[b].min(tracks[j].band_share_pct[b]))
                .sum::<f64>()
                / 100.0;
            masking.push(MaskPair {
                a: tracks[i].name.clone(),
                b: tracks[j].name.clone(),
                overlap: r2(overlap),
            });
        }
    }
    masking.sort_by(|x, y| y.overlap.partial_cmp(&x.overlap).unwrap());

    Analysis {
        seconds: r2(l.len() as f64 / SR),
        tempo: project.tempo,
        loudness: loudness(l, r),
        spectral: Spectral { band_share_pct: share_pct(&mix_bands), tracks, masking },
        stereo: stereo(l, r),
        rhythm: rhythm(project, l, r, sections),
        structure: structure(l, r, project.tempo, sections),
        tonality: tonality(project, l, r),
    }
}

impl Analysis {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

// --- reference profiles (#129) -----------------------------------------------

/// A genre target: ranges for the metrics `analyze` measures. Profiles are
/// plain JSON data — checked into packages, forkable, diffable — so "sounds
/// like the genre" becomes numbers an agent can optimize toward.
#[derive(serde::Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub integrated_lufs: Option<(f64, f64)>,
    #[serde(default)]
    pub true_peak_db: Option<(f64, f64)>,
    #[serde(default)]
    pub crest_db: Option<(f64, f64)>,
    #[serde(default)]
    pub side_mid_db: Option<(f64, f64)>,
    #[serde(default)]
    pub silence_total_pct: Option<(f64, f64)>,
    /// audio onsets per second over the whole song
    #[serde(default)]
    pub onsets_per_second: Option<(f64, f64)>,
    /// per-band energy share targets, keyed by band name (sub/low/mid/high/air)
    #[serde(default)]
    pub band_share_pct: std::collections::BTreeMap<String, (f64, f64)>,
}

impl Profile {
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}

/// One metric compared against its target range. `delta` is 0 inside the
/// range, negative by the shortfall below `lo`, positive by the excess
/// above `hi` — the gradient an agent follows.
#[derive(Serialize)]
pub struct Delta {
    pub metric: String,
    pub value: f64,
    pub lo: f64,
    pub hi: f64,
    pub ok: bool,
    pub delta: f64,
}

fn check(out: &mut Vec<Delta>, metric: &str, value: f64, range: Option<(f64, f64)>) {
    let Some((lo, hi)) = range else { return };
    let delta = if value < lo {
        r2(value - lo)
    } else if value > hi {
        r2(value - hi)
    } else {
        0.0
    };
    out.push(Delta { metric: metric.into(), value, lo, hi, ok: delta == 0.0, delta });
}

/// Compare a report against a profile. Only the targets the profile
/// declares are judged; everything else stays informational.
pub fn compare(a: &Analysis, p: &Profile) -> Vec<Delta> {
    let mut out = Vec::new();
    check(&mut out, "integrated_lufs", a.loudness.integrated_lufs, p.integrated_lufs);
    check(&mut out, "true_peak_db", a.loudness.true_peak_db, p.true_peak_db);
    check(&mut out, "crest_db", a.loudness.crest_db, p.crest_db);
    check(&mut out, "side_mid_db", a.stereo.side_mid_db, p.side_mid_db);
    check(&mut out, "silence_total_pct", a.structure.silence_total_pct, p.silence_total_pct);
    let density = r2(a.rhythm.audio_onsets as f64 / a.seconds.max(1e-9));
    check(&mut out, "onsets_per_second", density, p.onsets_per_second);
    for (i, name) in BAND_NAMES.iter().enumerate() {
        if let Some(&range) = p.band_share_pct.get(*name) {
            check(
                &mut out,
                &format!("band_{name}_pct"),
                a.spectral.band_share_pct[i],
                Some(range),
            );
        }
    }
    out
}
