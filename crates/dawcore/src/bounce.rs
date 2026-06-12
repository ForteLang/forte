//! Offline rendering: bounce the project's Arranger Timeline to a WAV file.
//! Reuses the exact real-time engine, just driven faster than real time.

use std::path::Path;

use crate::command::Command;
use crate::engine::Engine;
use crate::model::Project;
use crate::sync::full_sync;

const BLOCK: usize = 512;

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

/// Render the arrangement (plus a reverb/delay tail) to a 16-bit stereo WAV.
/// Returns the rendered length in seconds.
pub fn render_wav(project: &Project, path: &Path, tail_beats: f64) -> Result<f64, String> {
    let sr = 48_000.0f32;
    let (mut engine, mut handle) = Engine::new(sr);

    full_sync(&mut handle, project);
    handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);

    let total_beats = arrangement_len(project) + tail_beats.max(0.0);
    let seconds = total_beats * 60.0 / project.tempo;
    let total_samples = (seconds * sr as f64) as usize;

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sr as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;

    let mut bl = vec![0.0f32; BLOCK];
    let mut br = vec![0.0f32; BLOCK];
    let mut done = 0;
    while done < total_samples {
        let n = BLOCK.min(total_samples - done);
        engine.process(&mut bl, &mut br, n);
        for i in 0..n {
            let l = (bl[i].clamp(-1.0, 1.0) * 32767.0) as i16;
            let r = (br[i].clamp(-1.0, 1.0) * 32767.0) as i16;
            writer.write_sample(l).map_err(|e| e.to_string())?;
            writer.write_sample(r).map_err(|e| e.to_string())?;
        }
        done += n;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(seconds)
}
