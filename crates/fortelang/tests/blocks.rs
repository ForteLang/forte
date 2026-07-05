//! Blocks: the universal composition unit. A block is a self-contained piece
//! of music; a song is just the outermost block. Placements nest, the upper
//! block's settings win, keys transpose melodic content (never beat pads),
//! windows select bars, and content loops across longer placements.

use fortelang::{compile_str, render_digest};

fn digest(src: &str) -> String {
    let p = compile_str(src).expect("must compile");
    format!("{:016x}", render_digest(&p, 4.0).f32_digest)
}

const RIFF: &str = r#"block Riff {
  key A minor
  track Lead {
    instrument polymer(wave: "saw", cutoff: 0.5)
    play notes`A2:1 C3:1 E3:1 A3:1` at bars(1..1)
  }
  track Drums {
    instrument sampler(sample: "Kick")
    play beat`x--- x---` at bars(1..1)
  }
}"#;

#[test]
fn a_block_file_builds_with_the_last_block_as_root() {
    let p = compile_str(RIFF).expect("a lone block must build");
    assert_eq!(p.tracks.len(), 2);
    assert_eq!(p.tempo, 120.0, "a root block without tempo gets the default");
    let d = digest(RIFF);
    assert_eq!(d, digest(RIFF), "block root must render deterministically");
}

#[test]
fn songs_place_blocks_and_loop_them() {
    let song = format!(
        r#"{RIFF}
song "S" {{
  tempo 120bpm
  key A minor
  play Riff at bars(1..2)
}}"#
    );
    let p = compile_str(&song).unwrap();
    // the block's tracks appear, name-spaced
    let names: Vec<&str> = p.tracks.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Riff.Lead"), "{names:?}");
    assert!(names.contains(&"Riff.Drums"), "{names:?}");
    // a 1-bar block across 2 bars loops: the Lead track carries 2 clips
    let lead = p.tracks.iter().find(|t| t.name == "Riff.Lead").unwrap();
    assert_eq!(lead.arranger.len(), 2, "1-bar block over 2 bars must loop");
    assert_eq!(lead.arranger[1].start, 4.0);
    assert_eq!(digest(&song), digest(&song));
}

#[test]
fn placement_keys_transpose_melody_but_not_beats() {
    let base = format!(
        r#"{RIFF}
song "S" {{
  tempo 120bpm
  key A minor
  play Riff at bars(1..1)
}}"#
    );
    let up = base.replace("play Riff at", r#"play Riff(key: "C minor") at"#);
    let p_base = compile_str(&base).unwrap();
    let p_up = compile_str(&up).unwrap();
    let lead_pitch = |p: &dawcore::model::Project| {
        p.tracks.iter().find(|t| t.name == "Riff.Lead").unwrap().arranger[0].clip.notes[0].pitch
    };
    let drum_pitch = |p: &dawcore::model::Project| {
        p.tracks.iter().find(|t| t.name == "Riff.Drums").unwrap().arranger[0].clip.notes[0].pitch
    };
    assert_eq!(lead_pitch(&p_up), lead_pitch(&p_base) + 3, "A→C = +3 semitones");
    assert_eq!(drum_pitch(&p_up), drum_pitch(&p_base), "beat pads must not transpose");
    assert_ne!(digest(&base), digest(&up));
}

#[test]
fn the_upper_blocks_settings_win() {
    // the block's own tempo/key are overridden by the root above it
    let inner_says_90 = r#"block Part {
  tempo 90bpm
  key A minor
  track L {
    instrument polymer(wave: "tri")
    play notes`A2:1 E3:1` at bars(1..1)
  }
}
song "S" {
  tempo 120bpm
  key A minor
  play Part at bars(1..1)
}"#
    .to_string();
    let inner_silent = inner_says_90.replace("  tempo 90bpm\n", "");
    assert_eq!(
        digest(&inner_says_90),
        digest(&inner_silent),
        "a placed block's own tempo must be ignored (the block above wins)"
    );
}

#[test]
fn windows_select_bars_inside_a_block() {
    let two_bars = r#"block Two {
  key C minor
  track L {
    instrument polymer(wave: "saw")
    play notes`C3:4` at bars(1..1)
    play notes`G3:4` at bars(2..2)
  }
}"#;
    let full = format!("{two_bars}\nsong \"S\" {{ tempo 120bpm key C minor play Two at bars(1..2) }}");
    let second_only =
        format!("{two_bars}\nsong \"S\" {{ tempo 120bpm key C minor play Two(from: 2, to: 2) at bars(1..1) }}");
    let p = compile_str(&second_only).unwrap();
    let l = p.tracks.iter().find(|t| t.name == "Two.L").unwrap();
    assert_eq!(l.arranger.len(), 1, "the window must keep only bar 2");
    assert_eq!(l.arranger[0].start, 0.0, "windowed content rebases to the placement");
    assert_eq!(l.arranger[0].clip.notes[0].pitch, 55, "G3 = the second bar's note");
    assert_ne!(digest(&full), digest(&second_only));
}

#[test]
fn blocks_nest_and_transpose_accumulates_to_the_effective_key() {
    let src = r#"block Cell {
  key A minor
  track L {
    instrument polymer(wave: "saw")
    play notes`A2:1` at bars(1..1)
  }
}
block Phrase {
  key A minor
  play Cell at bars(1..1)
  play Cell(key: "D minor") at bars(2..2)
}
song "S" {
  tempo 120bpm
  key A minor
  play Phrase at bars(1..2)
}"#;
    let p = compile_str(src).unwrap();
    let l = p.tracks.iter().find(|t| t.name == "Phrase.Cell.L").unwrap();
    assert_eq!(l.arranger.len(), 2);
    let mut pitches: Vec<u8> =
        l.arranger.iter().map(|c| c.clip.notes[0].pitch).collect();
    pitches.sort();
    assert_eq!(pitches, vec![45, 50], "A2 plain + A2→D3 (+5) via the nested key override");
    assert_eq!(digest(src), digest(src));
}

#[test]
fn block_errors_speak_the_language() {
    let unknown = r#"song "S" { tempo 120bpm track T { instrument polymer() play beat`x---` at bars(1..1) } play Nope at bars(1..1) }"#;
    let errs = compile_str(unknown).err().expect("unknown block must fail");
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-001"), "{errs:?}");

    let cycle = r#"block A { play B at bars(1..1) }
block B { play A at bars(1..1) }
song "S" { tempo 120bpm track T { instrument polymer() play beat`x---` at bars(1..1) } play A at bars(1..1) }"#;
    let errs = compile_str(cycle).err().expect("cycles must fail");
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-002"), "{errs:?}");
}

#[test]
fn blocks_import_across_files() {
    let dir = std::env::temp_dir().join(format!("forte-blocks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("riff.forte"),
        r#"device Blip : Instrument {
  node env = adsr(a: 0.005, d: 0.2, s: 0.0, r: 0.05)
  node o = osc(shape: "square")
  out gain(in: o, mod: env, level: 0.7)
}
block Hook {
  key A minor
  track B { instrument Blip() play notes`A3:0.5 E4:0.5 A4:1` at bars(1..1) }
}"#,
    )
    .unwrap();
    let song = r#"import { Hook } from "./riff.forte"
song "S" {
  tempo 120bpm
  key C minor
  play Hook at bars(1..2)
}"#;
    let p = fortelang::compile_with_loader(song, &fortelang::FsLoader, dir.to_str().unwrap())
        .expect("imported block must compile (with its home devices)");
    let b = p.tracks.iter().find(|t| t.name == "Hook.B").unwrap();
    // A minor content placed under a C minor root transposes +3
    assert_eq!(b.arranger[0].clip.notes[0].pitch, 57 + 3);
    let _ = std::fs::remove_dir_all(&dir);
}
