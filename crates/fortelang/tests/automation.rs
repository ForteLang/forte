//! Bitwig-style modulation & parameter automation on device-DSL instruments:
//! `automate <param>` ramps any declared `param` over bars/sections, and
//! `modulate <param> with lfo(...) / steps(...) / random(...)` plugs
//! modulators into the same params. Everything must stay deterministic.

use fortelang::{compile_str, render_digest};

/// An acid-style mono device whose cutoff is a declared param — the compiler
/// must expose it for automate/modulate through the grid param binds.
fn acid_song(body: &str) -> String {
    format!(
        r#"device Acid : Instrument {{
  param cutoff = 0.3 in 0.0..1.0
  param reso   = 0.7 in 0.0..1.0
  param glide  = 0.06 in 0.0..0.5
  node env = adsr(a: 0.002, d: 0.18, s: 0.1, r: 0.05)
  node o   = osc(shape: "saw")
  node f   = svf(in: o, cutoff: cutoff, reso: reso, mod: gain(in: env, level: 0.3))
  out gain(in: f, mod: env, level: 0.85)
}}
song "A" {{
  tempo 130bpm
  key A minor
  track Acid {{
    instrument Acid()
    play notes`A1:0.25 A1:0.25 C2:0.25 A1:0.25 E2:0.25 A1:0.25 G2~:0.25 A2:0.25` at bars(1..4)
    {body}
  }}
}}"#
    )
}

fn digest(src: &str) -> String {
    let p = compile_str(src).expect("song must compile");
    format!("{:016x}", render_digest(&p, 6.0).f32_digest)
}

#[test]
fn automate_sweeps_a_device_param() {
    let plain = digest(&acid_song(""));
    let swept = acid_song("automate cutoff from 0.1 to 0.9 over bars(1..4)");
    let d1 = digest(&swept);
    assert_ne!(plain, d1, "the cutoff sweep must be audible in the digest");
    assert_eq!(d1, digest(&swept), "automation must render deterministically");
    // param names resolve case-insensitively
    let upper = acid_song("automate Cutoff from 0.1 to 0.9 over bars(1..4)");
    assert_eq!(d1, digest(&upper));
}

#[test]
fn automate_follows_sections() {
    let src = r#"device Acid : Instrument {
  param cutoff = 0.3 in 0.0..1.0
  node env = adsr(a: 0.002, d: 0.18, s: 0.1, r: 0.05)
  node o   = osc(shape: "saw")
  node f   = svf(in: o, cutoff: cutoff, reso: 0.7)
  out gain(in: f, mod: env, level: 0.85)
}
song "S" {
  tempo 130bpm
  key A minor
  section Intro = bars(1..2)
  section Drop  = bars(3..4)
  track Acid {
    instrument Acid()
    play notes`A1:0.5 C2:0.5 E2:0.5 A1:0.5` at bars(1..4)
    automate cutoff from 0.15 to 0.8 over Drop
  }
}"#;
    let with = digest(src);
    let without = digest(&src.replace("automate cutoff from 0.15 to 0.8 over Drop\n", ""));
    assert_ne!(with, without, "a section-scoped sweep must change the render");
    assert_eq!(with, digest(src), "section automation must be deterministic");
}

#[test]
fn ramps_on_the_same_param_merge_into_one_lane() {
    // two consecutive sweeps: the first must still be audible — if each
    // `automate` became its own lane, the second would cover the whole
    // timeline and erase the first
    let both = acid_song(
        "automate cutoff from 0.1 to 0.5 over bars(1..2)\n    automate cutoff from 0.5 to 0.9 over bars(3..4)",
    );
    let second_only = acid_song("automate cutoff from 0.5 to 0.9 over bars(3..4)");
    let p = compile_str(&both).unwrap();
    assert_eq!(p.tracks[0].param_automation.len(), 1, "same param → one merged lane");
    assert_eq!(p.tracks[0].param_automation[0].points.len(), 4);
    assert_ne!(digest(&both), digest(&second_only), "the first ramp must still be heard");
}

#[test]
fn modulators_plug_into_device_params() {
    let plain = digest(&acid_song(""));

    let lfo = acid_song(r#"modulate cutoff with lfo(rate: 0.5, amount: 0.4, shape: "tri")"#);
    let steps =
        acid_song(r#"modulate cutoff with steps(seq: "0.1 0.6 0.3 0.9", every: "1/16", amount: 0.5)"#);
    let random = acid_song("modulate cutoff with random(rate: 0.4, amount: 0.4, smooth: 0.5)");

    for (name, src) in [("lfo", &lfo), ("steps", &steps), ("random", &random)] {
        let d1 = digest(src);
        assert_ne!(plain, d1, "{name} modulator must be audible");
        assert_eq!(d1, digest(src), "{name} modulator must render deterministically");
    }
    // the three modulator kinds are genuinely different circuits
    assert_ne!(digest(&lfo), digest(&steps));
    assert_ne!(digest(&steps), digest(&random));
}

#[test]
fn step_sequences_are_tempo_synced() {
    // same seq at a different `every` (or a different seq) must move the sound
    let a = acid_song(r#"modulate cutoff with steps(seq: "0.1 0.9", every: "1/16", amount: 0.6)"#);
    let b = acid_song(r#"modulate cutoff with steps(seq: "0.1 0.9", every: "1/8", amount: 0.6)"#);
    let c = acid_song(r#"modulate cutoff with steps(seq: "0.9 0.1", every: "1/16", amount: 0.6)"#);
    let (da, db, dc) = (digest(&a), digest(&b), digest(&c));
    assert_ne!(da, db, "every: 1/16 vs 1/8 must differ");
    assert_ne!(da, dc, "the step order must matter");
}

#[test]
fn modulators_stack_on_automation() {
    // an automation ramp and an LFO on the same param compose (ramp = base,
    // modulator rides on top), and the combination stays deterministic
    let combined = acid_song(
        r#"automate cutoff from 0.2 to 0.7 over bars(1..4)
    modulate cutoff with lfo(rate: 0.6, amount: 0.25, shape: "sine")"#,
    );
    let ramp_only = acid_song("automate cutoff from 0.2 to 0.7 over bars(1..4)");
    let d = digest(&combined);
    assert_ne!(d, digest(&ramp_only), "the LFO must ride on top of the ramp");
    assert_eq!(d, digest(&combined), "stacked modulation must be deterministic");
}

#[test]
fn builtin_instruments_keep_working() {
    // the generalized path must still serve polymer's parameter table
    let src = r#"song "P" {
  tempo 120bpm
  key C minor
  track A {
    instrument polymer(wave: "saw", cutoff: 0.4)
    play notes`C3:1 G3:1 C3:1 G3:1` at bars(1..2)
    automate cutoff from 0.2 to 0.9 over bars(1..2)
    modulate cutoff with steps(seq: "0.2 0.8", every: "1/8", amount: 0.3)
  }
}"#;
    let with = digest(src);
    let without = digest(
        &src.replace("automate cutoff from 0.2 to 0.9 over bars(1..2)", "")
            .replace(r#"modulate cutoff with steps(seq: "0.2 0.8", every: "1/8", amount: 0.3)"#, ""),
    );
    assert_ne!(with, without);
    assert_eq!(with, digest(src));
}
