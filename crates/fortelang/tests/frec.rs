//! Recorded-audio assets: only provenance-stamped .frec files can enter a
//! song (SYS-REC-001/002), and placed takes actually sound in the render.

use fortelang::{compile_with_loader, frec, FsLoader};

fn temp_dir(tag: &str) -> String {
    let d = std::env::temp_dir().join(format!("forte-frec-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn provenance() -> serde_json::Value {
    serde_json::json!({
        "device_class": "microphone",
        "recorded_at": "2026-07-03T00:00:00Z",
        "by": "user:test",
        "session": "sess-1",
        "sig": "ed25519:stub",
    })
}

/// 1 second of 440 Hz sine at 48 kHz mono.
fn tone() -> Vec<f32> {
    (0..48_000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5).collect()
}

const SONG: &str = r#"import take from "./take.frec"
song "Rec" {
  tempo 120bpm
  track Kick { instrument sampler(sample: "Kick") play beat`x---` at bars(1..2) }
  track Voice { audio take at bars(1..2) }
}"#;

#[test]
fn recorded_take_plays_in_the_song() {
    let dir = temp_dir("play");
    std::fs::write(
        format!("{dir}/take.frec"),
        frec::encode(48_000, 1, &tone(), &provenance()),
    )
    .unwrap();
    let p = compile_with_loader(SONG, &FsLoader, &dir).expect("song with take must compile");
    let voice = p.tracks.iter().find(|t| t.name == "Voice").unwrap();
    assert_eq!(voice.audio_clips.len(), 1);
    assert!(voice.devices.is_empty(), "pure audio track needs no instrument");

    // the take must be audible: compare against the same song without it
    let with = fortelang::render_digest(&p, 2.0);
    let no_voice = SONG.replace("track Voice { audio take at bars(1..2) }", "");
    let p2 = compile_with_loader(&no_voice, &FsLoader, &dir).unwrap();
    let without = fortelang::render_digest(&p2, 2.0);
    assert!(with.rms > without.rms + 0.01, "take must add energy: {} vs {}", with.rms, without.rms);
}

#[test]
fn audio_without_provenance_is_rejected() {
    let dir = temp_dir("noprov");
    // structurally valid frec but with an empty provenance block
    std::fs::write(
        format!("{dir}/take.frec"),
        frec::encode(48_000, 1, &tone(), &serde_json::json!({})),
    )
    .unwrap();
    let err = compile_with_loader(SONG, &FsLoader, &dir).err().expect("must fail");
    assert!(err.iter().any(|d| d.code == "E-PROV-001"), "{err:?}");

    // raw PCM / random bytes are not assets at all
    std::fs::write(format!("{dir}/take.frec"), vec![0u8; 1024]).unwrap();
    let err = compile_with_loader(SONG, &FsLoader, &dir).err().expect("must fail");
    assert!(err.iter().any(|d| d.code == "E-PROV-001"), "{err:?}");
}

#[test]
fn missing_asset_and_unknown_name_are_reported() {
    let dir = temp_dir("missing");
    let err = compile_with_loader(SONG, &FsLoader, &dir).err().expect("must fail");
    assert!(err.iter().any(|d| d.code == "E-MOD-005"), "{err:?}");

    std::fs::write(
        format!("{dir}/take.frec"),
        frec::encode(48_000, 1, &tone(), &provenance()),
    )
    .unwrap();
    let wrong = SONG.replace("audio take at", "audio nope at");
    let err = compile_with_loader(&wrong, &FsLoader, &dir).err().expect("must fail");
    assert!(err.iter().any(|d| d.code == "E-PROV-003"), "{err:?}");
}

#[test]
fn stereo_takes_are_mono_mixed_and_seconds_computed() {
    let dir = temp_dir("stereo");
    let mono = tone();
    let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();
    std::fs::write(
        format!("{dir}/take.frec"),
        frec::encode(48_000, 2, &stereo, &provenance()),
    )
    .unwrap();
    compile_with_loader(SONG, &FsLoader, &dir).expect("stereo take must compile");
}
