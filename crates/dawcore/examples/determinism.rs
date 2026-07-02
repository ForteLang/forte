//! Determinism spike (Forte Phase 0.4 / roadmap 06): render the demo project
//! offline on different compilation targets and compare digests of the raw
//! sample stream. Runs on native and wasm32-wasip1 with no arguments.
//!
//!   cargo run --release -p dawcore --example determinism [-- dump.f32]
//!
//! Prints an FNV-1a 64 digest over (a) the exact f32 bit patterns and (b) the
//! i16-quantised stream (what a WAV bounce would contain), plus peak/RMS
//! stats. If a path argument is given, the interleaved f32 LE stream is also
//! written there so mismatching targets can be diffed sample by sample.

use dawcore::command::Command;
use dawcore::engine::Engine;
use dawcore::model::Project;
use dawcore::sync::full_sync;

const BLOCK: usize = 512;
const TAIL_BEATS: f64 = 8.0;

struct Fnv1a64(u64);

impl Fnv1a64 {
    fn new() -> Self {
        Fnv1a64(0xcbf2_9ce4_8422_2325)
    }
    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn main() {
    let dump = std::env::args().nth(1);

    let project = Project::demo();
    let sr = 48_000.0f32;
    let (mut engine, mut handle) = Engine::new(sr);
    full_sync(&mut handle, &project);
    handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);

    let total_beats = dawcore::bounce::arrangement_len(&project) + TAIL_BEATS;
    let seconds = total_beats * 60.0 / project.tempo;
    let total_samples = (seconds * sr as f64) as usize;

    let mut f32_hash = Fnv1a64::new();
    let mut i16_hash = Fnv1a64::new();
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    let mut raw: Vec<u8> = Vec::new();

    let mut bl = vec![0.0f32; BLOCK];
    let mut br = vec![0.0f32; BLOCK];
    let mut done = 0;
    while done < total_samples {
        let n = BLOCK.min(total_samples - done);
        engine.process(&mut bl, &mut br, n);
        for i in 0..n {
            for s in [bl[i], br[i]] {
                f32_hash.update(&s.to_bits().to_le_bytes());
                let q = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                i16_hash.update(&q.to_le_bytes());
                peak = peak.max(s.abs());
                sum_sq += (s as f64) * (s as f64);
                if dump.is_some() {
                    raw.extend_from_slice(&s.to_le_bytes());
                }
            }
        }
        done += n;
    }

    let rms = (sum_sq / (total_samples as f64 * 2.0)).sqrt();
    println!("target      : {}", std::env::consts::ARCH);
    println!("samples     : {total_samples} frames @ {sr} Hz ({seconds:.2}s)");
    println!("f32 digest  : {:016x}", f32_hash.0);
    println!("i16 digest  : {:016x}", i16_hash.0);
    println!("peak        : {peak:.6}");
    println!("rms         : {rms:.6}");

    if let Some(path) = dump {
        std::fs::write(&path, &raw).expect("write dump");
        println!("dumped      : {path} ({} bytes)", raw.len());
    }
}
