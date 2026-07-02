//! `fortelang` — the Forte v0 language slice: parse `.forte` sources, check
//! them, and compile to a `dawcore` project that renders deterministically
//! (07-determinism-spike.md) on native and wasm from the same source.

pub mod ast;
#[cfg(not(target_family = "wasm"))]
pub mod audio;
pub mod compile;
pub mod diag;
pub mod lexer;
pub mod music;
pub mod parser;

use dawcore::command::Command;
use dawcore::engine::Engine;
use dawcore::model::Project;
use dawcore::sync::full_sync;
use diag::Diag;

/// Parse + compile a `.forte` source into an engine project.
pub fn compile_str(src: &str) -> Result<Project, Vec<Diag>> {
    let ast = parser::parse(src)?;
    compile::compile(&ast)
}

pub struct RenderInfo {
    pub f32_digest: u64,
    pub frames: usize,
    pub seconds: f64,
    pub peak: f32,
    pub rms: f64,
}

/// Render the arrangement offline (same engine as playback) and digest the
/// exact f32 bit stream — the build proof recorded in build.manifest.json
/// (SRS-BLD-001). FNV-1a 64 stands in for SHA-256 in the v0 slice.
pub fn render_digest(project: &Project, tail_beats: f64) -> RenderInfo {
    const BLOCK: usize = 512;
    let sr = 48_000.0f32;
    let (mut engine, mut handle) = Engine::new(sr);
    full_sync(&mut handle, project);
    handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);

    let total_beats = dawcore::bounce::arrangement_len(project) + tail_beats.max(0.0);
    let seconds = total_beats * 60.0 / project.tempo;
    let total_samples = (seconds * sr as f64) as usize;

    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut update = |bytes: &[u8]| {
        for &b in bytes {
            digest ^= b as u64;
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };

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
                update(&s.to_bits().to_le_bytes());
                peak = peak.max(s.abs());
                sum_sq += (s as f64) * (s as f64);
            }
        }
        done += n;
    }
    RenderInfo {
        f32_digest: digest,
        frames: total_samples,
        seconds,
        peak,
        rms: (sum_sq / (total_samples.max(1) as f64 * 2.0)).sqrt(),
    }
}

/// FNV-1a 64 of arbitrary bytes (used for the source hash in the manifest).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
