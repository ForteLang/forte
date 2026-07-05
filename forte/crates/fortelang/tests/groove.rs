//! Groove foundation: swing and ghost notes must audibly change the render,
//! deterministically, and leave straight songs bit-identical.

use fortelang::{compile_with_loader, render_digest, FsLoader};

fn digest(header: &str, pattern: &str) -> String {
    let src = format!(
        r#"song "G" {{
  tempo 120bpm
  {header}
  key C minor
  track D {{
    instrument sampler(sample: "Hat")
    play beat`{pattern}` at bars(1..2)
  }}
}}"#
    );
    let p = compile_with_loader(&src, &FsLoader, ".").expect("groove song");
    let info = render_digest(&p, 4.0);
    format!("{:016x}", info.f32_digest)
}

#[test]
fn swing_delays_the_offbeats() {
    let straight = digest("", "xxxx xxxx xxxx xxxx");
    let explicit = digest("swing 0.5", "xxxx xxxx xxxx xxxx");
    assert_eq!(straight, explicit, "swing 0.5 must be exactly straight");

    let shuffled = digest("swing 0.66", "xxxx xxxx xxxx xxxx");
    assert_ne!(straight, shuffled, "swing 0.66 must move the off-16ths");
    assert_eq!(
        shuffled,
        digest("swing 0.66", "xxxx xxxx xxxx xxxx"),
        "swing must render deterministically"
    );

    // notes only on downbeats are untouched by swing
    let down_straight = digest("", "x--- x--- x--- x---");
    let down_swung = digest("swing 0.7", "x--- x--- x--- x---");
    assert_eq!(down_straight, down_swung, "swing must not move downbeats");
}

#[test]
fn swing_range_is_validated() {
    let src = r#"song "G" {
  tempo 120bpm
  swing 0.95
  key C minor
  track D { instrument sampler(sample: "Hat") play beat`x-x-` at bars(1..1) }
}"#;
    let err = compile_with_loader(src, &FsLoader, ".").err().expect("must reject");
    assert!(err.iter().any(|d| d.code == "E-TIME-004"), "{err:?}");
}

#[test]
fn ghost_notes_are_quieter() {
    let loud = {
        let src = r#"song "G" {
  tempo 120bpm
  key C minor
  track D { instrument sampler(sample: "Snare") play beat`x---` at bars(1..1) }
}"#;
        let p = compile_with_loader(src, &FsLoader, ".").unwrap();
        render_digest(&p, 2.0)
    };
    let ghost = {
        let src = r#"song "G" {
  tempo 120bpm
  key C minor
  track D { instrument sampler(sample: "Snare") play beat`.---` at bars(1..1) }
}"#;
        let p = compile_with_loader(src, &FsLoader, ".").unwrap();
        render_digest(&p, 2.0)
    };
    assert!(
        ghost.peak < loud.peak * 0.8,
        "ghost must be noticeably quieter: ghost {} vs x {}",
        ghost.peak,
        loud.peak
    );
    assert_ne!(ghost.f32_digest, loud.f32_digest);
}

#[test]
fn tied_notes_glide_on_mono_devices() {
    let song = |line: &str| {
        format!(
            r#"device MonoGlide : Instrument {{
  param glide = 0.08 in 0.0..0.5
  node env = adsr(a: 0.003, d: 0.3, s: 0.4, r: 0.05)
  node o   = osc(shape: "saw")
  node f   = svf(in: o, cutoff: 0.4, reso: 0.6)
  out gain(in: f, mod: env, level: 0.8)
}}
song "S" {{
  tempo 120bpm
  key C minor
  track A {{
    instrument MonoGlide()
    play notes`{line}` at bars(1..1)
  }}
}}"#
        )
    };
    let render = |src: &str| {
        let p = compile_with_loader(src, &FsLoader, ".").expect("mono song");
        format!("{:016x}", render_digest(&p, 2.0).f32_digest)
    };
    let tied = render(&song("C2~:1 C3:1 G2:2"));
    let plain = render(&song("C2:1 C3:1 G2:2"));
    assert_ne!(tied, plain, "the tie must audibly slide instead of retrigger");
    assert_eq!(tied, render(&song("C2~:1 C3:1 G2:2")), "slides must be deterministic");
}
