//! The standard instrument library (lib/std) and sampler v2 sound design:
//! every shipped instrument validates and renders deterministically, and one
//! recorded take becomes many instruments via start/end/loop/reverse.

use fortelang::{check_with_loader, compile_with_loader, render_digest, Checked, FsLoader};

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[test]
fn every_std_library_validates() {
    let expected = [
        ("drums", 22),
        ("percussion", 12),
        ("bass", 13),
        ("keys", 15),
        ("pads", 10),
        ("leads", 13),
        ("synths", 12),
        ("fx", 6),
    ];
    let mut total = 0;
    for (name, count) in expected {
        let path = repo_root().join(format!("lib/std/{name}.forte"));
        let src = std::fs::read_to_string(&path).unwrap();
        let base = path.parent().unwrap().to_str().unwrap().to_string();
        match check_with_loader(&src, &FsLoader, &base) {
            Ok(Checked::DeviceLibrary { devices }) => {
                assert_eq!(devices, count, "lib/std/{name}.forte device count");
                total += devices;
            }
            other => panic!(
                "lib/std/{name}.forte must be a device library: {:?}",
                other.err().map(|d| d.iter().map(|d| d.message.clone()).collect::<Vec<_>>())
            ),
        }
    }
    assert_eq!(total, 103, "the standard library ships 103 instruments");
}

#[test]
fn std_tour_renders_the_pinned_digest() {
    // the demo song plays ten std instruments at once; its digest is a
    // determinism gate for the whole library (same contract as the songs in
    // scripts/determinism_test.sh)
    let path = repo_root().join("songs/std-tour.forte");
    let src = std::fs::read_to_string(&path).unwrap();
    let p = compile_with_loader(&src, &FsLoader, path.parent().unwrap().to_str().unwrap())
        .expect("std-tour must compile");
    let info = render_digest(&p, 8.0);
    assert_eq!(format!("{:016x}", info.f32_digest), "b88ecb2e2c7c1c5b");
    assert!(info.peak > 0.05, "the tour must actually make sound");
}

// ---------------------------------------------------------------------------
// sampler v2: one take, many instruments
// ---------------------------------------------------------------------------

fn take_song(dir: &std::path::Path, sampler_args: &str) -> String {
    // an asymmetric take (decaying ramp) so reverse audibly differs
    let tone: Vec<f32> = (0..9_600)
        .map(|i| {
            let t = i as f32 / 9_600.0;
            (i as f32 * 0.08).sin() * 0.4 * (1.0 - t)
        })
        .collect();
    let prov = serde_json::json!({
        "device_class": "microphone", "recorded_at": "2026-07-04T00:00:00Z",
        "by": "user:test", "session": "s1", "sig": "ed25519:stub",
    });
    std::fs::write(dir.join("take.frec"), fortelang::frec::encode(48_000, 1, &tone, &prov))
        .unwrap();
    format!(
        r#"import voice from "./take.frec"
song "S" {{
  tempo 120bpm
  track A {{
    instrument sampler(take: voice{sampler_args})
    play notes`C4:1 G3:1 C4:2` at bars(1..2)
  }}
}}"#
    )
}

#[test]
fn sampler_trim_loop_reverse_shape_the_sound_deterministically() {
    let dir = std::env::temp_dir().join(format!("forte-sampler2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let base = dir.to_str().unwrap();

    let variants = [
        ("plain", ""),
        ("trim", ", start: 0.25, end: 0.6"),
        ("loop", ", end: 0.1, loop: \"on\""),
        ("reverse", ", reverse: \"on\""),
    ];
    let mut digests = Vec::new();
    for (name, args) in variants {
        let src = take_song(&dir, args);
        let render = |src: &str| {
            let p = compile_with_loader(src, &FsLoader, base).expect(name);
            format!("{:016x}", render_digest(&p, 4.0).f32_digest)
        };
        let d1 = render(&src);
        let d2 = render(&src);
        assert_eq!(d1, d2, "{name} must render deterministically");
        digests.push((name, d1));
    }
    for i in 0..digests.len() {
        for j in i + 1..digests.len() {
            assert_ne!(
                digests[i].1, digests[j].1,
                "{} and {} must sound different",
                digests[i].0, digests[j].0
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
