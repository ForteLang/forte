//! The soundnote: recorded takes inside the device DSL (`take` slots +
//! `sample()` nodes) and the `kit()` builtin (pitch → take drum kits).
//! One recording, processed like any other signal — filters, shapers,
//! envelopes — all deterministic.

use fortelang::{check_with_loader, compile_with_loader, render_digest, Checked, FsLoader};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("forte-soundnote-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn prov() -> serde_json::Value {
    serde_json::json!({
        "device_class": "microphone", "recorded_at": "2026-07-04T00:00:00Z",
        "by": "user:test", "session": "s1", "sig": "ed25519:stub",
    })
}

/// An asymmetric tone (decaying ramp) so reverse/trim audibly differ.
fn write_take(path: &std::path::Path, seed: f32) {
    let tone: Vec<f32> = (0..9_600)
        .map(|i| {
            let t = i as f32 / 9_600.0;
            (i as f32 * seed).sin() * 0.4 * (1.0 - t)
        })
        .collect();
    std::fs::write(path, fortelang::frec::encode(48_000, 1, &tone, &prov())).unwrap();
}

fn digest_of(src: &str, base: &str) -> String {
    let p = compile_with_loader(src, &FsLoader, base).expect("song must compile");
    format!("{:016x}", render_digest(&p, 4.0).f32_digest)
}

#[test]
fn sample_node_makes_takes_a_graph_source() {
    let dir = scratch("node");
    write_take(&dir.join("voice.frec"), 0.08);
    let base = dir.to_str().unwrap();

    let song = |node_args: &str| {
        format!(
            r#"import myVoice from "./voice.frec"

device VoxKeys : Instrument {{
  take voice
  param cutoff = 0.55 in 0.0..1.0

  node s   = sample(take: voice{node_args})
  node f   = svf(in: s, cutoff: cutoff, reso: 0.25)
  node env = adsr(a: 0.005, d: 0.3, s: 0.6, r: 0.2)
  out gain(in: f, mod: env, level: 0.9)
}}

song "Vox" {{
  tempo 120bpm
  track A {{
    instrument VoxKeys(voice: myVoice, cutoff: 0.6)
    play notes`C4:1 G3:1 C4:2` at bars(1..2)
  }}
}}"#
        )
    };

    let variants =
        [("plain", ""), ("looped", ", end: 0.1, loop: \"on\""), ("reversed", ", reverse: \"on\"")];
    let mut digests = Vec::new();
    for (name, args) in variants {
        let src = song(args);
        let d1 = digest_of(&src, base);
        let d2 = digest_of(&src, base);
        assert_eq!(d1, d2, "{name} must render deterministically");
        let p = compile_with_loader(&src, &FsLoader, base).unwrap();
        assert!(render_digest(&p, 4.0).peak > 0.01, "{name} must make sound");
        digests.push((name, d1));
    }
    for i in 0..digests.len() {
        for j in i + 1..digests.len() {
            assert_ne!(digests[i].1, digests[j].1, "{} vs {}", digests[i].0, digests[j].0);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kit_maps_pitches_to_takes() {
    let dir = scratch("kit");
    write_take(&dir.join("kick.frec"), 0.02); // low thump
    write_take(&dir.join("snare.frec"), 0.3); // brighter burst
    let base = dir.to_str().unwrap();

    let song = |map: &str| {
        format!(
            r#"import kick from "./kick.frec"
import snare from "./snare.frec"

song "Kit" {{
  tempo 120bpm
  track Drums {{
    instrument kit({map}, gain: 0.9)
    play notes`C2:1/2 D2:1/2 C2:1/2 D2:1/2` at bars(1..2)
  }}
}}"#
        )
    };

    let a = digest_of(&song("C2: kick, D2: snare"), base);
    let a2 = digest_of(&song("C2: kick, D2: snare"), base);
    let swapped = digest_of(&song("C2: snare, D2: kick"), base);
    assert_eq!(a, a2, "a kit renders deterministically");
    assert_ne!(a, swapped, "which pad holds which take is audible");

    // beat literals trigger the lowest pad (C2 = 36 here)
    let beat_song = r#"import kick from "./kick.frec"
song "Beat" {
  tempo 120bpm
  track Drums {
    instrument kit(C2: kick)
    play beat`x-x- x-x-` at bars(1..1)
  }
}"#;
    let p = compile_with_loader(beat_song, &FsLoader, base).unwrap();
    assert!(render_digest(&p, 4.0).peak > 0.01, "beat patterns hit the lowest pad");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn soundnote_errors_speak_music() {
    let dir = scratch("errors");
    write_take(&dir.join("voice.frec"), 0.08);
    let base = dir.to_str().unwrap();

    // a sample node needs its take bound at the call site
    let unbound = r#"import myVoice from "./voice.frec"
device V : Instrument {
  take voice
  out sample(take: voice)
}
song "S" {
  tempo 120bpm
  track A { instrument V() play beat`x---` at bars(1..1) }
}"#;
    let Err(err) = compile_with_loader(unbound, &FsLoader, base) else {
        panic!("an unbound take must be rejected");
    };
    assert!(
        err.iter().any(|d| d.message.contains("take 'voice' を渡してください")),
        "unbound take: {:?}",
        err.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // sample is an instrument-only primitive
    let in_effect = r#"device Fx : Effect {
  take voice
  out sample(take: voice)
}"#;
    let Err(err) = check_with_loader(in_effect, &FsLoader, base) else {
        panic!("sample in an Effect must be rejected");
    };
    assert!(
        err.iter().any(|d| d.message.contains("Effect では sample は使えません")),
        "sample in effect: {:?}",
        err.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // a library that declares takes validates standalone (slots stay unbound)
    let lib = r#"device V : Instrument {
  take voice
  node env = adsr(a: 0.005, d: 0.3, s: 0.5, r: 0.2)
  out gain(in: sample(take: voice), mod: env, level: 0.9)
}"#;
    match check_with_loader(lib, &FsLoader, base) {
        Ok(Checked::DeviceLibrary { devices }) => assert_eq!(devices, 1),
        other => panic!(
            "take-library must validate: {:?}",
            other.err().map(|d| d.iter().map(|d| d.message.clone()).collect::<Vec<_>>())
        ),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
