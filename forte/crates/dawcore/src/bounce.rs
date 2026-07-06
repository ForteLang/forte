//! Offline rendering: bounce the project's Arranger Timeline to a WAV file.
//! Reuses the exact real-time engine, just driven faster than real time, then
//! masters the result so a finished song lands where a listener expects it.
//!
//! Two problems make a raw bounce sound wrong next to a streaming track:
//!
//!  * **Balance.** Instrument presets are not loudness-matched (a 303 bass
//!    leaves the voice hotter than a pad) and low end carries more energy, so
//!    one part can bury the rest. A single capture pass separates every track's
//!    contribution, measures how loud each is *while playing* ([`presence_lufs`])
//!    and gently nudges the outliers toward the mix median — the bass stops
//!    swallowing the hats without the composer hand-balancing every fader.
//!  * **Level.** The raw mix sits 10+ LU under a commercial master (a fader is
//!    a perceptual, squared taper and the master only soft-limits peaks), so
//!    the song plays quiet. After balancing, the bounce normalises the whole
//!    mix to streaming loudness ([`TARGET_LUFS`]) and catches the boosted peaks
//!    with a transparent soft clip.
//!
//! The mastering lives here, in the bounce — the build digest still covers the
//! raw engine render, and the makeup / per-track gains are reported so the
//! mastered file stays reproducible from the manifest.

use std::path::Path;

use crate::command::Command;
use crate::engine::Engine;
use crate::model::{Project, TrackKind};
use crate::sync::full_sync;

const BLOCK: usize = 512;
const SR: f32 = 48_000.0;

/// Loudness the mastered bounce aims for. −14 LUFS is the reference streaming
/// services (Spotify, YouTube, Tidal) normalise to, so a file at this level
/// plays back at the same volume as everything else in a listener's queue.
pub const TARGET_LUFS: f32 = -14.0;
/// Cap on the master makeup gain — bounds how hard a near-silent or very
/// sparse mix is pushed rather than slamming it.
const MAX_MAKEUP_DB: f32 = 12.0;
/// Floor on the master makeup gain, so an already-hot mix is pulled *down* to
/// the target for consistency rather than left blaring.
const MIN_MAKEUP_DB: f32 = -24.0;

/// A track may sit this many LU above the mix-median *presence* before it gets
/// tamed — keeps the kick and bass forward, only reins in a part that is
/// genuinely burying the others.
const LEVEL_ABOVE: f32 = 5.0;
/// A track this far *below* the median presence gets lifted toward it.
const LEVEL_BELOW: f32 = 6.0;
/// Most a single track is turned down / up by the auto-leveller. Kept modest so
/// the mix is rebalanced, not flattened — musical dynamics survive.
const LEVEL_MAX_CUT: f32 = 9.0;
const LEVEL_MAX_BOOST: f32 = 6.0;
/// Fraction of the ideal correction actually applied — a nudge toward balance,
/// not a hard snap to the target.
const LEVEL_STRENGTH: f32 = 0.7;
/// Percentile of a track's active-block loudness taken as its "presence" — how
/// loud it is *while playing*, so sparse parts (risers, fills) aren't judged
/// quiet just for sitting out most of the song.
const PRESENCE_PCT: f32 = 0.9;

/// Length of the arrangement in beats (end of the last clip).
pub fn arrangement_len(project: &Project) -> f64 {
    let mut end: f64 = 4.0;
    for t in &project.tracks {
        for a in &t.arranger {
            end = end.max(a.start + a.duration);
        }
    }
    end
}

/// One source track's rendered contribution to the mix, plus the loudness the
/// auto-leveller measured and the gain it chose.
pub struct TrackRender {
    pub name: String,
    pub id: usize,
    /// Presence loudness of this track — how loud it is while playing (LUFS),
    /// `-inf` if silent. The basis for the auto-level decision.
    pub lufs: f32,
    /// Auto-level gain applied to this track in the master (dB).
    pub level_db: f32,
    pub rms: f32,
    l: Vec<f32>,
    r: Vec<f32>,
}

/// A fully mastered bounce held in memory: the summed master plus every source
/// stem, ready to write to disk at the right level.
pub struct MasterRender {
    pub seconds: f64,
    /// Integrated loudness of the balanced mix before makeup (LUFS).
    pub in_lufs: f32,
    /// Master makeup gain applied on write to reach [`TARGET_LUFS`] (dB).
    pub makeup_db: f32,
    /// Digest / peak / RMS of the *raw* engine mix (pre-master) — matches
    /// `render_digest`, so the build proof is available without a second pass.
    pub raw_digest: u64,
    pub raw_peak: f32,
    pub raw_rms: f32,
    pub tracks: Vec<TrackRender>,
    l: Vec<f32>,
    r: Vec<f32>,
}

impl MasterRender {
    /// Write the mastered master mix (balanced + normalised) as a 16-bit WAV.
    pub fn write_master(&self, path: &Path) -> Result<(), String> {
        write_wav(path, &self.l, &self.r, db_to_lin(self.makeup_db))
    }

    /// Write one stem at its auto-level gain plus the master makeup, so the
    /// stems reconstruct the master at the same level.
    pub fn write_stem(&self, t: &TrackRender, path: &Path) -> Result<(), String> {
        write_wav(path, &t.l, &t.r, db_to_lin(t.level_db + self.makeup_db))
    }

    /// Digest of a stem's captured dry contribution (pre-master) — a stable
    /// proof of that stem, independent of the mastering gains applied on write.
    pub fn stem_digest(&self, t: &TrackRender) -> u64 {
        digest_f32(&t.l, &t.r)
    }
}

/// Bounce the arrangement to a mastered 16-bit stereo WAV. Returns the length
/// in seconds.
pub fn render_wav(project: &Project, path: &Path, tail_beats: f64) -> Result<f64, String> {
    let m = render_master(project, tail_beats);
    m.write_master(path)?;
    Ok(m.seconds)
}

/// Render the arrangement once, loudness-balance the tracks, and sum them back
/// into a mastered mix. Returns the whole thing in memory (see [`MasterRender`]),
/// so the caller can report the loudness figures and write coherent stems.
pub fn render_master(project: &Project, tail_beats: f64) -> MasterRender {
    let total_beats = arrangement_len(project) + tail_beats.max(0.0);

    // one engine pass separates every source track's post-fader contribution
    // and the shared effect-return wash — soloing each track instead would
    // starve the effect feedback into denormals and stall the render
    let cap = capture_render(project, total_beats);
    let seconds = cap.seconds;
    let mut tracks: Vec<TrackRender> = cap
        .sources
        .into_iter()
        .map(|s| {
            let lufs = presence_lufs(&s.l, &s.r);
            let rms = rms_of(&s.l, &s.r);
            TrackRender { name: s.name, id: s.id, lufs, level_db: 0.0, rms, l: s.l, r: s.r }
        })
        .collect();

    // auto-level: nudge each track's presence toward a window around the mix
    // median. Outliers (the too-loud bass) are pulled down; buried parts are
    // lifted; anything already in the window is left untouched. A partial
    // correction rebalances without flattening the mix's natural dynamics.
    let levels: Vec<f32> = tracks.iter().map(|t| t.lufs).filter(|v| v.is_finite()).collect();
    if let Some(med) = median(&levels) {
        for t in &mut tracks {
            if t.lufs.is_finite() {
                let target = t.lufs.clamp(med - LEVEL_BELOW, med + LEVEL_ABOVE);
                t.level_db =
                    ((target - t.lufs) * LEVEL_STRENGTH).clamp(-LEVEL_MAX_CUT, LEVEL_MAX_BOOST);
            }
        }
    }

    // rebuild the master from the balanced stems plus the (unchanged) effect
    // wash. The dry contributions carry the leveling; the shared reverb/delay
    // return rides along at its original level — a wash isn't worth a re-render.
    let n = cap.fx_l.len();
    let mut ml = cap.fx_l;
    let mut mr = cap.fx_r;
    for t in &tracks {
        let g = db_to_lin(t.level_db);
        let len = t.l.len().min(n);
        for i in 0..len {
            ml[i] += t.l[i] * g;
            mr[i] += t.r[i] * g;
        }
    }

    let in_lufs = integrated_lufs(&ml, &mr);
    let makeup_db = makeup_db(in_lufs);
    MasterRender {
        seconds,
        in_lufs,
        makeup_db,
        raw_digest: cap.raw_digest,
        raw_peak: cap.raw_peak,
        raw_rms: cap.raw_rms,
        tracks,
        l: ml,
        r: mr,
    }
}

/// A source track's captured post-fader stereo contribution to the mix.
struct SourceCap {
    name: String,
    id: usize,
    l: Vec<f32>,
    r: Vec<f32>,
}

/// Everything one offline engine pass yields: each audible source track's dry
/// contribution, the summed effect-return wash, and a digest / peak / RMS of
/// the true master output (identical to `render_digest`, so it doubles as the
/// build proof — no separate render needed).
struct Capture {
    seconds: f64,
    sources: Vec<SourceCap>,
    fx_l: Vec<f32>,
    fx_r: Vec<f32>,
    raw_digest: u64,
    raw_peak: f32,
    raw_rms: f32,
}

/// Drive the engine offline for `total_beats`, tapping per-track and effect-bus
/// signals block by block.
fn capture_render(project: &Project, total_beats: f64) -> Capture {
    let sr = SR;
    let (mut engine, mut handle) = Engine::new(sr);
    full_sync(&mut handle, project);
    handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);
    engine.enable_capture();

    let seconds = total_beats * 60.0 / project.tempo;
    let total_samples = (seconds * sr as f64) as usize;

    // source tracks the engine actually plays (honour composer solo/mute)
    let any_solo = project.tracks.iter().any(|t| t.solo);
    let src: Vec<(usize, String)> = project
        .tracks
        .iter()
        .filter(|t| t.kind != TrackKind::Effect && (t.solo || (!t.mute && !any_solo)))
        .map(|t| (t.id, t.name.clone()))
        .collect();

    let mut sources: Vec<SourceCap> = src
        .iter()
        .map(|(id, name)| SourceCap {
            name: name.clone(),
            id: *id,
            l: Vec::with_capacity(total_samples),
            r: Vec::with_capacity(total_samples),
        })
        .collect();
    let mut fx_l = Vec::with_capacity(total_samples);
    let mut fx_r = Vec::with_capacity(total_samples);

    // stream the master digest / peak / RMS instead of storing the whole mix
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;

    let mut bl = vec![0.0f32; BLOCK];
    let mut br = vec![0.0f32; BLOCK];
    let mut done = 0;
    while done < total_samples {
        let n = BLOCK.min(total_samples - done);
        engine.process(&mut bl, &mut br, n);
        for i in 0..n {
            for s in [bl[i], br[i]] {
                for b in s.to_bits().to_le_bytes() {
                    digest ^= b as u64;
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
                peak = peak.max(s.abs());
                sum_sq += (s as f64) * (s as f64);
            }
        }
        for sc in &mut sources {
            let (cl, cr) = engine.cap_source(sc.id);
            sc.l.extend_from_slice(&cl[..n]);
            sc.r.extend_from_slice(&cr[..n]);
        }
        let (el, er) = engine.cap_fx();
        fx_l.extend_from_slice(&el[..n]);
        fx_r.extend_from_slice(&er[..n]);
        done += n;
    }

    let raw_rms = (sum_sq / (total_samples.max(1) as f64 * 2.0)).sqrt() as f32;
    Capture { seconds, sources, fx_l, fx_r, raw_digest: digest, raw_peak: peak, raw_rms }
}

/// Apply `gain`, soft-clip the peaks, quantise to 16-bit and write the WAV.
fn write_wav(path: &Path, l: &[f32], r: &[f32], gain: f32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    let n = l.len().min(r.len());
    for i in 0..n {
        let sl = (soft_clip(l[i] * gain).clamp(-1.0, 1.0) * 32767.0) as i16;
        let sr = (soft_clip(r[i] * gain).clamp(-1.0, 1.0) * 32767.0) as i16;
        writer.write_sample(sl).map_err(|e| e.to_string())?;
        writer.write_sample(sr).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(())
}

/// FNV-1a 64 over the raw f32 bit stream (L, R interleaved per sample) — the
/// same walk `render_digest` performs, so a stem's proof is stable.
pub fn digest_f32(l: &[f32], r: &[f32]) -> u64 {
    let mut d = 0xcbf2_9ce4_8422_2325u64;
    let n = l.len().min(r.len());
    for i in 0..n {
        for s in [l[i], r[i]] {
            for b in s.to_bits().to_le_bytes() {
                d ^= b as u64;
                d = d.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    d
}

fn rms_of(l: &[f32], r: &[f32]) -> f32 {
    let n = l.len().min(r.len());
    if n == 0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for i in 0..n {
        sum += (l[i] as f64).powi(2) + (r[i] as f64).powi(2);
    }
    (sum / (n as f64 * 2.0)).sqrt() as f32
}

fn median(vals: &[f32]) -> Option<f32> {
    if vals.is_empty() {
        return None;
    }
    let mut v = vals.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let m = v.len() / 2;
    Some(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) * 0.5 })
}

/// Makeup gain (dB) that brings a mix measured at `lufs` to [`TARGET_LUFS`],
/// clamped to a sane range. Silence (non-finite loudness) is left untouched.
fn makeup_db(lufs: f32) -> f32 {
    if !lufs.is_finite() {
        return 0.0;
    }
    (TARGET_LUFS - lufs).clamp(MIN_MAKEUP_DB, MAX_MAKEUP_DB)
}

fn db_to_lin(db: f32) -> f32 {
    crate::dmath::powf(10.0, db / 20.0)
}

/// Transparent peak limiter: identity below a knee, then a `tanh` shoulder that
/// asymptotes to full scale, so a post-gain overshoot rounds off instead of
/// clipping hard. Leaves the bulk of the signal (|x| ≤ knee) untouched.
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        x.signum() * (KNEE + (1.0 - KNEE) * crate::dmath::tanh(over))
    }
}

/// Integrated loudness (LUFS) per ITU-R BS.1770: K-weight each channel, take
/// 400 ms mean-square blocks at 75 % overlap, then apply the −70 LUFS absolute
/// gate and the −10 LU relative gate before averaging. Returns `-inf` for
/// silence. Coefficients are the standard 48 kHz set — the bounce is always
/// 48 kHz.
fn integrated_lufs(l: &[f32], r: &[f32]) -> f32 {
    let blocks = block_powers(l, r);
    if blocks.is_empty() {
        return f32::NEG_INFINITY;
    }

    // absolute gate at −70 LUFS
    let mut sum = 0.0;
    let mut cnt = 0usize;
    for &(loud, p) in &blocks {
        if loud >= -70.0 {
            sum += p;
            cnt += 1;
        }
    }
    if cnt == 0 {
        return f32::NEG_INFINITY;
    }

    // relative gate: 10 LU below the abs-gated mean loudness
    let rel = -0.691 + 10.0 * libm::log10(sum / cnt as f64) - 10.0;
    let mut sum2 = 0.0;
    let mut cnt2 = 0usize;
    for &(loud, p) in &blocks {
        if loud >= -70.0 && loud >= rel {
            sum2 += p;
            cnt2 += 1;
        }
    }
    if cnt2 == 0 {
        return f32::NEG_INFINITY;
    }
    (-0.691 + 10.0 * libm::log10(sum2 / cnt2 as f64)) as f32
}

/// A track's *presence*: the [`PRESENCE_PCT`] percentile of its active-block
/// loudness. Silence (below the −70 LUFS gate) is ignored, so a sparse part is
/// judged by how loud it is when it actually plays — the right basis for
/// balancing a mix, where whole-song averages unfairly bury intermittent parts.
fn presence_lufs(l: &[f32], r: &[f32]) -> f32 {
    let mut ls: Vec<f64> =
        block_powers(l, r).into_iter().filter(|&(loud, _)| loud >= -70.0).map(|(loud, _)| loud).collect();
    if ls.is_empty() {
        return f32::NEG_INFINITY;
    }
    ls.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (((ls.len() - 1) as f32) * PRESENCE_PCT.clamp(0.0, 1.0)).round() as usize;
    ls[idx.min(ls.len() - 1)] as f32
}

/// K-weighted 400 ms mean-square blocks at 75 % overlap → `(loudness, power)`
/// per block. Shared by the integrated and presence measures (ITU-R BS.1770).
fn block_powers(l: &[f32], r: &[f32]) -> Vec<(f64, f64)> {
    let fl = k_weight(l);
    let fr = k_weight(r);
    let n = fl.len().min(fr.len());
    let block = (0.4 * SR as f64) as usize; // 400 ms window
    let step = (0.1 * SR as f64) as usize; // 100 ms hop → 75 % overlap
    let mut out = Vec::new();
    if n < block || block == 0 {
        return out;
    }

    // prefix sums of squared, K-weighted samples → O(1) block mean-square
    let mut pl = vec![0.0f64; n + 1];
    let mut pr = vec![0.0f64; n + 1];
    for i in 0..n {
        pl[i + 1] = pl[i] + fl[i] * fl[i];
        pr[i + 1] = pr[i] + fr[i] * fr[i];
    }

    // L and R weighted 1.0 each
    let mut start = 0;
    while start + block <= n {
        let end = start + block;
        let p = (pl[end] - pl[start] + pr[end] - pr[start]) / block as f64;
        if p > 0.0 {
            out.push((-0.691 + 10.0 * libm::log10(p), p));
        }
        start += step;
    }
    out
}

/// BS.1770 K-weighting (48 kHz): a high-shelf pre-filter feeding an RLB
/// high-pass. Returns the filtered signal in f64 for the loudness sum.
fn k_weight(sig: &[f32]) -> Vec<f64> {
    // stage 1: high-shelf (+~4 dB above ~1.5 kHz)
    let mut s1 = Biquad::new(
        1.53512485958697,
        -2.69169618940638,
        1.19839281085285,
        -1.69065929318241,
        0.73248077421585,
    );
    // stage 2: RLB high-pass (~38 Hz)
    let mut s2 = Biquad::new(1.0, -2.0, 1.0, -1.99004745483398, 0.99007225036621);
    sig.iter().map(|&x| s2.run(s1.run(x as f64))).collect()
}

/// Transposed direct-form-II biquad in f64.
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Biquad { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}
