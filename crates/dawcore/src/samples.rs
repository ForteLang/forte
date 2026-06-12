//! Built-in sample content. Drum one-shots are synthesised procedurally so the
//! app ships with audible samples and no binary assets; WAV files can also be
//! loaded from disk into the same shared-buffer form.

use std::path::Path;
use std::sync::Arc;

use crate::dsp::sampler::Sample;

const SR: f32 = 44_100.0;

fn env(t: f32, decay: f32) -> f32 {
    (-t / decay).exp()
}

/// A punchy kick: pitch-swept sine with a fast amplitude decay.
pub fn kick() -> Arc<Sample> {
    let len = (SR * 0.5) as usize;
    let mut d = vec![0.0f32; len];
    for (i, s) in d.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let freq = 120.0 * env(t, 0.06) + 45.0;
        let phase = std::f32::consts::TAU * freq * t;
        *s = phase.sin() * env(t, 0.18) * 0.9;
    }
    Arc::new(Sample::one_shot(d.into(), SR, 36))
}

/// Snare: tone body plus filtered noise.
pub fn snare() -> Arc<Sample> {
    let len = (SR * 0.3) as usize;
    let mut d = vec![0.0f32; len];
    let mut rng = 0x1234_5678u32;
    let mut lp = 0.0f32;
    for (i, s) in d.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let tone = (std::f32::consts::TAU * 190.0 * t).sin() * env(t, 0.08) * 0.5;
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let white = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        lp += (white - lp) * 0.5;
        let noise = lp * env(t, 0.12) * 0.6;
        *s = (tone + noise) * 0.8;
    }
    Arc::new(Sample::one_shot(d.into(), SR, 38))
}

/// Closed hi-hat: bright, very short filtered noise.
pub fn hat() -> Arc<Sample> {
    let len = (SR * 0.12) as usize;
    let mut d = vec![0.0f32; len];
    let mut rng = 0x9E37_79B9u32;
    let mut hp = 0.0f32;
    let mut prev = 0.0f32;
    for (i, s) in d.iter_mut().enumerate() {
        let t = i as f32 / SR;
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let white = (rng as f32 / u32::MAX as f32) * 2.0 - 1.0;
        hp = white - prev + 0.92 * hp; // crude high-pass
        prev = white;
        *s = hp * env(t, 0.03) * 0.5;
    }
    Arc::new(Sample::one_shot(d.into(), SR, 42))
}

/// Load a mono (or down-mixed) sample from a WAV file on disk.
pub fn load_wav(path: &Path, root: u8) -> Result<Arc<Sample>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.unwrap_or(0.0))
            .collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap_or(0) as f32 / max)
                .collect()
        }
    };
    // down-mix to mono
    let mono: Vec<f32> = if ch <= 1 {
        raw
    } else {
        raw.chunks(ch)
            .map(|c| c.iter().sum::<f32>() / ch as f32)
            .collect()
    };
    if mono.is_empty() {
        return Err("empty sample".into());
    }
    Ok(Arc::new(Sample::one_shot(mono.into(), spec.sample_rate as f32, root)))
}

/// Built-in sample identifiers selectable from the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinSample {
    Kick,
    Snare,
    Hat,
}

impl BuiltinSample {
    pub fn label(self) -> &'static str {
        match self {
            BuiltinSample::Kick => "Kick",
            BuiltinSample::Snare => "Snare",
            BuiltinSample::Hat => "Hat",
        }
    }
    pub fn build(self) -> Arc<Sample> {
        match self {
            BuiltinSample::Kick => kick(),
            BuiltinSample::Snare => snare(),
            BuiltinSample::Hat => hat(),
        }
    }
}
