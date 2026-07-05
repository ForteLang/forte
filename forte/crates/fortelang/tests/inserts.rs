//! Builtin insert effects: each one must (1) change the sound versus a dry
//! render, and (2) render bit-identically across runs — the same determinism
//! contract as everything else in the engine.

use fortelang::{compile_with_loader, render_digest, FsLoader};

fn digest(insert: &str) -> String {
    let src = format!(
        r#"song "S" {{
  tempo 120bpm
  key C minor
  track A {{
    instrument prisma(wave: "saw", cutoff: 0.6)
    pan 0.3
    {insert}
    play notes`C3:0.5 G3:0.5 [C3 Eb3 G3]:1 C4:0.5 G2:0.5 C3:1` at bars(1..2)
  }}
}}"#
    );
    let p = compile_with_loader(&src, &FsLoader, ".").expect(insert);
    format!("{:016x}", render_digest(&p, 4.0).f32_digest)
}

#[test]
fn every_new_insert_changes_the_sound_and_stays_deterministic() {
    let dry = digest("");
    let inserts = [
        "insert comp(thresh: 0.25, ratio: 0.7, attack: 0.05, release: 0.3, makeup: 0.4)",
        "insert chorus(rate: 0.35, depth: 0.6, mix: 0.6)",
        "insert pump(amount: 0.8, beats: 1.0)",
        // inserts run pre-pan, so width needs a stereo source: chorus first
        "insert chorus(rate: 0.35, depth: 0.6, mix: 0.6)\n    insert width(amount: 1.0)",
    ];
    let mut prev = dry;
    for insert in inserts {
        let d1 = digest(insert);
        let d2 = digest(insert);
        assert_eq!(d1, d2, "{insert} must render deterministically");
        assert_ne!(d1, prev, "{insert} must audibly change the render");
        prev = d1;
    }
}

#[test]
fn pump_period_follows_tempo() {
    // the same pump settings at different tempos must duck at different rates
    let at = |bpm: u32| {
        let src = format!(
            r#"song "S" {{
  tempo {bpm}bpm
  key C minor
  track A {{
    instrument prisma(wave: "saw")
    insert pump(amount: 0.9)
    play notes`C3:8` at bars(1..2)
  }}
}}"#
        );
        let p = compile_with_loader(&src, &FsLoader, ".").expect("pump song");
        // fixed 2s window so only the duck rate differs, not the note length
        format!("{:016x}", render_digest(&p, 2.0).f32_digest)
    };
    assert_ne!(at(100), at(140), "pump must be tempo-synced");
}
