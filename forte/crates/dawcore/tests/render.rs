//! Offline render tests. These exercise the full engine — scheduler, synth,
//! effects, mixer — without any audio hardware, and assert that real signal
//! comes out. They also dump a WAV so the result can be auditioned.

use dawcore::command::Command;
use dawcore::engine::Engine;
use dawcore::model::Project;
use dawcore::sync::full_sync;

#[test]
fn grid_synth_makes_sound() {
    // The demo's Bass track is a Poly Grid (NoteIn->Osc->SVF->Gain*ADSR->Out).
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    let bass = project.tracks.iter().find(|t| t.name == "Bass").unwrap();
    assert_eq!(bass.devices[0].kind, dawcore::model::DeviceKind::PolyMesh);
    assert!(bass.devices[0].grid.is_some(), "grid device has no graph");
    let bass_id = bass.id;

    full_sync(&mut handle, &project);
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);
    handle.send(Command::LaunchClip { track: bass_id, scene: 0 });

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut peak = 0.0f32;
    for _ in 0..400 {
        engine.process(&mut bl, &mut br, 512);
        peak = peak.max(handle.shared.track_peak(bass_id));
    }
    assert!(peak > 0.001, "grid synth produced no sound (peak={peak})");
}

#[test]
fn arpeggiator_chain_produces_notes() {
    // Note FX -> Instrument chain: hold one live chord through an Arpeggiator
    // in front of a Polymer; the arp must keep the track sounding over time.
    use dawcore::model::{Device, DeviceKind, Track, TrackKind};
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let mut project = Project::demo();
    let id = project.alloc_id();
    let mut t = Track::new(id, "ArpTrack", TrackKind::Instrument, [200, 100, 50]);
    // chain: Arpeggiator (note fx) BEFORE the Polymer instrument
    t.devices.insert(0, Device::new(DeviceKind::Arpeggiator));
    project.tracks.push(t);

    full_sync(&mut handle, &project);
    handle.send(Command::Play); // transport running so the arp grid advances
    // hold a chord live — never released
    for &n in &[60u8, 64, 67] {
        handle.send(Command::NoteOn { track: id, note: n, velocity: 0.9 });
    }

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    // skip the first second, then measure: a sustained chord would decay, the
    // arp keeps retriggering so the peak must persist late in the render
    for _ in 0..100 {
        engine.process(&mut bl, &mut br, 512);
    }
    let mut late_peak = 0.0f32;
    for _ in 0..300 {
        engine.process(&mut bl, &mut br, 512);
        late_peak = late_peak.max(handle.shared.track_peak(id));
    }
    assert!(late_peak > 0.001, "arpeggiator stopped sounding (peak={late_peak})");
}

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
fn automation_mutes_everything() {
    // Hold-at-zero volume automation on every track must silence the mix.
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let mut project = Project::demo();
    for t in &mut project.tracks {
        t.volume_automation = vec![dawcore::model::AutomationPoint {
            beat: 0.0,
            value: 0.0,
            hold: true,
        }];
    }
    full_sync(&mut handle, &project);
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);
    handle.send(Command::LaunchScene(0));

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut total = Vec::new();
    for _ in 0..200 {
        engine.process(&mut bl, &mut br, 512);
        total.extend_from_slice(&bl);
    }
    assert!(rms(&total) < 1e-5, "automation at 0 still audible (rms={})", rms(&total));
}

#[test]
fn sends_feed_effect_track() {
    // The demo routes Lead -> FX Return (reverb) via a post-fader send. While
    // the Lead clip plays, the FX track's peak meter must rise above zero.
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    let fx_slot = project
        .tracks
        .iter()
        .find(|t| t.kind == dawcore::model::TrackKind::Effect)
        .map(|t| t.id)
        .expect("demo has an effect track");
    let lead_slot = project.tracks.iter().find(|t| t.name == "Lead").unwrap().id;

    full_sync(&mut handle, &project);
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);
    handle.send(Command::LaunchClip { track: lead_slot, scene: 1 });

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut fx_peak = 0.0f32;
    for _ in 0..400 {
        engine.process(&mut bl, &mut br, 512);
        fx_peak = fx_peak.max(handle.shared.track_peak(fx_slot));
    }
    assert!(fx_peak > 0.0005, "effect return never received send signal (peak={fx_peak})");
}

#[test]
fn project_json_roundtrip() {
    let p = Project::demo();
    let json = p.to_json();
    assert!(!json.is_empty());
    let q = Project::from_json(&json).expect("parse back");
    assert_eq!(p.tracks.len(), q.tracks.len());
    assert_eq!(p.scenes.len(), q.scenes.len());
    let notes = |pr: &Project| -> usize {
        pr.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .filter_map(|c| c.as_ref())
            .map(|c| c.notes.len())
            .sum()
    };
    assert_eq!(notes(&p), notes(&q));
    assert_eq!(p.tracks[3].sends.len(), q.tracks[3].sends.len());
    assert_eq!(p.tracks[3].volume_automation.len(), q.tracks[3].volume_automation.len());
}

#[test]
fn bounce_writes_wav() {
    let project = Project::demo();
    let path = std::env::temp_dir().join("forte_bounce_test.wav");
    let secs = dawcore::bounce::render_wav(&project, &path, 4.0).expect("bounce");
    assert!(secs > 1.0);
    let meta = std::fs::metadata(&path).expect("wav written");
    assert!(meta.len() > 100_000, "wav suspiciously small: {} bytes", meta.len());
    // and it should decode with real audio in it
    let mut reader = hound::WavReader::open(&path).expect("readable wav");
    let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
    let energy: f64 = samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum::<f64>() / samples.len() as f64;
    assert!(energy.sqrt() > 0.001, "bounced wav is silent");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn sampler_track_makes_sound() {
    // The demo's Kick track is a Sampler; its launcher clip must produce audio.
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    let kick = project.tracks.iter().find(|t| t.name == "Kick").unwrap().id;
    // sanity: it really is a Sampler instrument
    assert_eq!(project.tracks.iter().find(|t| t.name == "Kick").unwrap().devices[0].kind,
        dawcore::model::DeviceKind::Sampler);

    full_sync(&mut handle, &project);
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);
    handle.send(Command::LaunchClip { track: kick, scene: 0 });

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut peak = 0.0f32;
    for _ in 0..400 {
        engine.process(&mut bl, &mut br, 512);
        peak = peak.max(handle.shared.track_peak(kick));
    }
    assert!(peak > 0.001, "sampler kick produced no sound (peak={peak})");
}

#[test]
fn audio_clips_play_on_timeline() {
    // The Perc (Audio) track holds audio clips; arrangement playback (no
    // launcher) should make the track's meter rise.
    let sr = 48_000.0;
    let (mut engine, mut handle) = Engine::new(sr);
    let project = Project::demo();
    let perc = project.tracks.iter().find(|t| t.name.starts_with("Perc")).unwrap().id;
    assert!(!project.tracks.iter().find(|t| t.name.starts_with("Perc")).unwrap().audio_clips.is_empty());

    full_sync(&mut handle, &project);
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);

    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let mut peak = 0.0f32;
    // first audio clip starts at beat 2 (~1s at 120bpm); render ~4s
    for _ in 0..(4 * 48_000 / 512) {
        engine.process(&mut bl, &mut br, 512);
        peak = peak.max(handle.shared.track_peak(perc));
    }
    assert!(peak > 0.001, "audio clips never played (peak={peak})");
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
