//! Offline render tests. These exercise the full engine — scheduler, synth,
//! effects, mixer — without any audio hardware, and assert that real signal
//! comes out. They also dump a WAV so the result can be auditioned.

use dawcore::command::Command;
use dawcore::engine::Engine;
use dawcore::model::Project;
use dawcore::sync::full_sync;

fn render_project(seconds: f32, sr: f32) -> (Vec<f32>, Vec<f32>) {
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();

    full_sync(&mut handle, &project);
    handle.send(Command::SetTempo(project.tempo));
    handle.send(Command::Play);
    // launch scene 0 on every track that has a clip there
    handle.send(Command::LaunchScene(0));

    let total = (seconds * sr) as usize;
    let block = 512;
    let mut out_l = Vec::with_capacity(total);
    let mut out_r = Vec::with_capacity(total);
    let mut bl = vec![0.0f32; block];
    let mut br = vec![0.0f32; block];

    let mut done = 0;
    while done < total {
        let n = block.min(total - done);
        engine.process(&mut bl, &mut br, n);
        out_l.extend_from_slice(&bl[..n]);
        out_r.extend_from_slice(&br[..n]);
        done += n;
    }
    (out_l, out_r)
}

fn rms(buf: &[f32]) -> f32 {
    if buf.is_empty() {
        return 0.0;
    }
    (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt()
}

#[test]
fn engine_produces_audio() {
    let sr = 48_000.0;
    let (l, r) = render_project(4.0, sr);

    let rl = rms(&l);
    let rr = rms(&r);
    println!("RMS L={rl:.4} R={rr:.4}");

    assert!(rl > 0.001, "left channel is silent (rms={rl})");
    assert!(rr > 0.001, "right channel is silent (rms={rr})");

    // no NaNs / infinities escaped the DSP
    assert!(l.iter().all(|x| x.is_finite()), "non-finite sample in L");
    assert!(r.iter().all(|x| x.is_finite()), "non-finite sample in R");

    // peak should be controlled by the master limiter
    let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak <= 1.0001, "master limiter let peak through: {peak}");

    // write an audible artifact for manual checking
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sr as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Ok(mut w) = hound::WavWriter::create("render_test.wav", spec) {
        for (a, b) in l.iter().zip(r.iter()) {
            let _ = w.write_sample((a.clamp(-1.0, 1.0) * 32767.0) as i16);
            let _ = w.write_sample((b.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
        let _ = w.finalize();
    }
}

#[test]
fn arranger_plays_without_launcher() {
    // No scenes launched: the engine should play the Arranger Timeline clips.
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    full_sync(&mut handle, &project);
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::SetLoop { enabled: true, start: 0.0, end: 32.0 });
    handle.send(Command::Play);

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut total = Vec::new();
    // render ~4 seconds of arrangement
    for _ in 0..(4 * 48_000 / 512) {
        engine.process(&mut bl, &mut br, 512);
        total.extend_from_slice(&bl);
    }
    assert!(rms(&total) > 0.001, "arranger produced no sound (rms={})", rms(&total));
    assert!(total.iter().all(|x| x.is_finite()));
}

#[test]
fn loop_wraps_playhead() {
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    full_sync(&mut handle, &project);
    handle.send(Command::SetLoop { enabled: true, start: 0.0, end: 4.0 });
    handle.send(Command::Play);

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    // render well past the 4-beat loop; position must stay within [0,4)
    let mut max_pos = 0.0f64;
    for _ in 0..2000 {
        engine.process(&mut bl, &mut br, 512);
        max_pos = max_pos.max(handle.shared.position_beats());
    }
    assert!(max_pos < 4.0, "playhead escaped the loop region: {max_pos}");
}

#[test]
fn silent_when_stopped() {
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    full_sync(&mut handle, &project);
    // never sent Play

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    for _ in 0..50 {
        engine.process(&mut bl, &mut br, 512);
    }
    assert!(rms(&bl) < 1e-6, "engine made noise while stopped");
}

#[test]
fn live_notes_sound() {
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    full_sync(&mut handle, &project);

    let track0 = project.tracks[0].id;
    handle.send(Command::NoteOn { track: track0, note: 64, velocity: 1.0 });

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut total = Vec::new();
    for _ in 0..20 {
        engine.process(&mut bl, &mut br, 512);
        total.extend_from_slice(&bl);
    }
    assert!(rms(&total) > 0.001, "live note produced no sound");
}
