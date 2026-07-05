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
    instrument prisma(wave: "saw", cutoff: 0.5)
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
    instrument prisma(wave: "tri")
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
    instrument prisma(wave: "saw")
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
    instrument prisma(wave: "saw")
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

// ---------------------------------------------------------------------------
// inheritance: block Child : Parent { … } overrides like a class
// ---------------------------------------------------------------------------

const PARENT: &str = r#"block Line {
  key A minor
  track Lead {
    instrument prisma(wave: "saw", cutoff: 0.5)
    insert delay(time: 0.3, fdbk: 0.3, mix: 0.2)
    play notes`A2:1 C3:1 E3:1 A3:1` at bars(1..1)
  }
}"#;

#[test]
fn inheritance_overrides_instruments_and_effect_params() {
    // swap the instrument
    let swapped = format!(
        "{PARENT}\nblock Dark : Line {{ track Lead {{ instrument prisma(wave: \"square\", cutoff: 0.2) }} }}\nsong \"S\" {{ tempo 120bpm key A minor play Dark at bars(1..1) }}"
    );
    let base = format!("{PARENT}\nsong \"S\" {{ tempo 120bpm key A minor play Line at bars(1..1) }}");
    assert_ne!(digest(&base), digest(&swapped), "the child's instrument must replace the parent's");

    // change only an insert's params (same insert name → params replaced)
    let wetter = format!(
        "{PARENT}\nblock Wet : Line {{ track Lead {{ insert delay(time: 0.3, fdbk: 0.5, mix: 0.5) }} }}\nsong \"S\" {{ tempo 120bpm key A minor play Wet at bars(1..1) }}"
    );
    let p = compile_str(&wetter).unwrap();
    let lead = p.tracks.iter().find(|t| t.name == "Wet.Lead").unwrap();
    assert_eq!(lead.devices.len(), 2, "same-name insert must replace, not stack");
    assert_ne!(digest(&base), digest(&wetter));

    // add a new effect (different insert name → appended)
    let verbed = format!(
        "{PARENT}\nblock Verb : Line {{ track Lead {{ insert reverb(size: 0.7, mix: 0.4) }} }}\nsong \"S\" {{ tempo 120bpm key A minor play Verb at bars(1..1) }}"
    );
    let p = compile_str(&verbed).unwrap();
    let lead = p.tracks.iter().find(|t| t.name == "Verb.Lead").unwrap();
    assert_eq!(lead.devices.len(), 3, "a new insert must append after the parent's");
}

#[test]
fn inheritance_replaces_patterns_and_chains() {
    // a child with plays replaces the parent's pattern; chains resolve A:B:C
    let src = format!(
        "{PARENT}\nblock Var : Line {{ track Lead {{ play notes`E3:1 A3:1 C4:1 E4:1` at bars(1..1) }} }}\nblock Loud : Var {{ track Lead {{ volume 1.0 }} }}\nsong \"S\" {{ tempo 120bpm key A minor play Loud at bars(1..1) }}"
    );
    let p = compile_str(&src).unwrap();
    let lead = p.tracks.iter().find(|t| t.name == "Loud.Lead").unwrap();
    assert_eq!(lead.arranger[0].clip.notes[0].pitch, 52, "Var's E3 pattern must win");
    assert_eq!(lead.volume, 1.0, "Loud's volume must win");
    assert_eq!(digest(&src), digest(&src));
}

#[test]
fn inheritance_errors_are_reported() {
    let unknown = r#"block A : Nope { track T { instrument prisma() play beat`x---` at bars(1..1) } }"#;
    let errs = compile_str(unknown).err().expect("unknown parent must fail");
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-005"), "{errs:?}");

    let cycle = r#"block A : B { track T { instrument prisma() play beat`x---` at bars(1..1) } }
block B : A { track U { instrument prisma() play beat`x---` at bars(1..1) } }"#;
    let errs = compile_str(cycle).err().expect("inheritance cycle must fail");
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-006"), "{errs:?}");
}

#[test]
fn block_errors_speak_the_language() {
    let unknown = r#"song "S" { tempo 120bpm track T { instrument prisma() play beat`x---` at bars(1..1) } play Nope at bars(1..1) }"#;
    let errs = compile_str(unknown).err().expect("unknown block must fail");
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-001"), "{errs:?}");

    let cycle = r#"block A { play B at bars(1..1) }
block B { play A at bars(1..1) }
song "S" { tempo 120bpm track T { instrument prisma() play beat`x---` at bars(1..1) } play A at bars(1..1) }"#;
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

// ---------------------------------------------------------------------------
// metadata: desc and tags
// ---------------------------------------------------------------------------

#[test]
fn desc_and_tags_ride_the_root_block() {
    let src = r#"block Meta {
  desc "A tiny demo."
  tags "demo, test, meta"
  track T { instrument prisma() play notes`C3:1` at bars(1..1) }
}"#;
    let p = compile_str(src).unwrap();
    assert_eq!(p.name, "Meta");
    assert_eq!(p.desc, "A tiny demo.");
    assert_eq!(p.tags, vec!["demo", "test", "meta"]);

    // inheritance: the child's desc wins when present
    let child = format!(
        "{src}\nblock Var : Meta {{ desc \"The variant.\" }}\nsong \"S\" {{ tempo 120bpm play Var at bars(1..1) }}"
    );
    let p = compile_str(&child).unwrap();
    assert_eq!(p.desc, "", "the song root has no desc of its own");

    // a described, empty root block is valid package metadata
    let meta_only = r#"block Pkg { desc "A package." tags "pkg" }"#;
    compile_str(meta_only).expect("desc-only blocks are valid (package.forte)");
    // …but an undescribed empty root is still an error
    assert!(compile_str(r#"block Nope { tempo 120bpm }"#).is_err());
}

// ---------------------------------------------------------------------------
// external control of a placed instance (issue #43 第三弾)
// ---------------------------------------------------------------------------

#[test]
fn placement_volume_scales_the_instance_for_its_span_only() {
    let src = format!(
        "{RIFF}\nsong \"S\" {{\n  tempo 120bpm\n  key A minor\n  track Bass {{ instrument prisma() volume 0.9 play notes`A1:1` at bars(1..8) }}\n  play Riff at bars(1..4)\n  play Riff(volume: 0.5) at bars(5..8)\n}}"
    );
    let p = compile_str(&src).unwrap();
    let lead = p.tracks.iter().find(|t| t.name == "Riff.Lead").unwrap();
    let fader = lead.volume;
    // span 5..8 = beats 16..32: scaled step in, fader restored after
    let at = |beat: f64| {
        lead.volume_automation
            .iter()
            .filter(|pt| (pt.beat - beat).abs() < 1e-9)
            .collect::<Vec<_>>()
    };
    assert!(at(0.0).iter().any(|pt| (pt.value - fader).abs() < 1e-6), "guard pins the fader at 0");
    assert!(
        at(16.0).iter().any(|pt| (pt.value - 0.5 * fader).abs() < 1e-6),
        "scaled at the span start: {:?}",
        lead.volume_automation.iter().map(|p| (p.beat, p.value)).collect::<Vec<_>>()
    );
    assert!(at(32.0).iter().any(|pt| (pt.value - fader).abs() < 1e-6), "restored at the span end");
    // the un-scaled sibling track outside the block is untouched
    let bass = p.tracks.iter().find(|t| t.name == "Bass").unwrap();
    assert!(bass.volume_automation.is_empty());
    // range check
    let bad = format!("{RIFF}\nsong \"S\" {{ tempo 120bpm play Riff(volume: 1.5) at bars(1..2) }}");
    let errs = match fortelang::compile_str(&bad) {
        Err(e) => e,
        Ok(_) => panic!("volume 1.5 must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-TYPE-002"), "{errs:?}");
}

#[test]
fn placement_automation_fades_an_instance_in() {
    let src = format!(
        "{RIFF}\nsong \"S\" {{\n  tempo 120bpm\n  key A minor\n  section intro = bars(1..4)\n  play Riff at bars(1..8)\n  automate Riff.volume from 0 to 1 over intro\n}}"
    );
    let p = compile_str(&src).unwrap();
    for name in ["Riff.Lead", "Riff.Drums"] {
        let t = p.tracks.iter().find(|t| t.name == name).unwrap();
        let fader = t.volume;
        let ramp_start = t
            .volume_automation
            .iter()
            .find(|pt| pt.beat.abs() < 1e-9 && pt.value.abs() < 1e-6)
            .unwrap_or_else(|| panic!("{name} must start the fade at 0: {:?}",
                t.volume_automation.iter().map(|p| (p.beat, p.value)).collect::<Vec<_>>()));
        assert!(!ramp_start.hold, "the fade ramps (lerp), not steps");
        assert!(
            t.volume_automation
                .iter()
                .any(|pt| (pt.beat - 16.0).abs() < 1e-9 && (pt.value - fader).abs() < 1e-6),
            "{name} reaches its fader at bar 5"
        );
    }

    // errors speak the language: unknown instance, non-volume target
    let bad = format!(
        "{RIFF}\nsong \"S\" {{ tempo 120bpm play Riff at bars(1..2) automate Ghost.volume from 0 to 1 over bars(1..2) }}"
    );
    let errs = match fortelang::compile_str(&bad) {
        Err(e) => e,
        Ok(_) => panic!("unknown instance must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-AUTO-002" && d.message.contains("Riff")), "{errs:?}");
    let bad2 = format!(
        "{RIFF}\nsong \"S\" {{ tempo 120bpm play Riff at bars(1..2) automate Riff.cutoff from 0 to 1 over bars(1..2) }}"
    );
    let errs = match fortelang::compile_str(&bad2) {
        Err(e) => e,
        Ok(_) => panic!("non-volume target must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-AUTO-002" && d.message.contains("volume")), "{errs:?}");
}

#[test]
fn block_params_wire_placements_to_instrument_knobs() {
    const KNOB: &str = r#"block Knob {
  param cutoff = 0.5 in 0..1
  track Lead {
    instrument prisma(wave: "saw", cutoff: cutoff)
    play notes`A2:1` at bars(1..1)
  }
}"#;
    // default value vs an override must change the sound (different digest)
    let base = format!("{KNOB}\nsong \"S\" {{ tempo 120bpm play Knob at bars(1..2) }}");
    let hot = format!("{KNOB}\nsong \"S\" {{ tempo 120bpm play Knob(cutoff: 0.9) at bars(1..2) }}");
    assert_ne!(digest(&base), digest(&hot), "the param must reach the instrument");
    // explicit default = the same sound
    let explicit = format!("{KNOB}\nsong \"S\" {{ tempo 120bpm play Knob(cutoff: 0.5) at bars(1..2) }}");
    assert_eq!(digest(&base), digest(&explicit));

    // unknown param name → the declared ones are listed
    let bad = format!("{KNOB}\nsong \"S\" {{ tempo 120bpm play Knob(reso: 0.9) at bars(1..2) }}");
    let errs = match fortelang::compile_str(&bad) {
        Err(e) => e,
        Ok(_) => panic!("unknown param must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-005" && d.message.contains("cutoff")), "{errs:?}");

    // out of the declared range
    let oob = format!("{KNOB}\nsong \"S\" {{ tempo 120bpm play Knob(cutoff: 1.5) at bars(1..2) }}");
    let errs = match fortelang::compile_str(&oob) {
        Err(e) => e,
        Ok(_) => panic!("out-of-range param must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-TYPE-002"), "{errs:?}");

    // same block, different knob values → tracks are shared, so refuse and
    // point at inheritance
    let conflict = format!(
        "{KNOB}\nsong \"S\" {{ tempo 120bpm play Knob(cutoff: 0.2) at bars(1..2) play Knob(cutoff: 0.9) at bars(3..4) }}"
    );
    let errs = match fortelang::compile_str(&conflict) {
        Err(e) => e,
        Ok(_) => panic!("conflicting param values must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-005" && d.message.contains("継承")), "{errs:?}");

    // inheritance can override the DEFAULT instead
    let inherited = format!(
        "{KNOB}\nblock Dark : Knob {{ param cutoff = 0.1 in 0..1 }}\nsong \"S\" {{ tempo 120bpm play Dark at bars(1..2) }}"
    );
    let dark = digest(&inherited);
    assert_ne!(dark, digest(&base), "the inherited default must change the sound");
}

const SIXTEENTHS: &str = r#"block Line {
  key A minor
  track Lead {
    instrument prisma(wave: "saw", cutoff: 0.5)
    play notes`A2:0.25 C3:0.25 E3:0.25 A3:0.25` at bars(1..1)
  }
}"#;

#[test]
fn placement_swing_and_stretch_shape_one_instance() {
    // swing shifts off-beat 16ths: the same block straight and shuffled
    // must sound different, and the sibling placement stays straight
    let swung = format!(
        "{SIXTEENTHS}\nsong \"S\" {{\n  tempo 120bpm\n  key A minor\n  play Line at bars(1..2)\n  play Line(swing: 0.66) at bars(3..4)\n}}"
    );
    let straight = format!(
        "{SIXTEENTHS}\nsong \"S\" {{\n  tempo 120bpm\n  key A minor\n  play Line at bars(1..2)\n  play Line at bars(3..4)\n}}"
    );
    assert_ne!(digest(&swung), digest(&straight), "local swing must reach the notes");
    // out of range
    let bad = format!("{RIFF}\nsong \"S\" {{ tempo 120bpm play Riff(swing: 0.9) at bars(1..2) }}");
    let errs = match fortelang::compile_str(&bad) {
        Err(e) => e,
        Ok(_) => panic!("swing 0.9 must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-TYPE-002"), "{errs:?}");

    // stretch: 2 — the one-bar riff fills two bars, notes land twice as late
    let stretched = format!(
        "{RIFF}\nsong \"S\" {{\n  tempo 120bpm\n  key A minor\n  play Riff(stretch: 2) at bars(1..2)\n}}"
    );
    let p = compile_str(&stretched).unwrap();
    let lead = p.tracks.iter().find(|t| t.name == "Riff.Lead").unwrap();
    let clip = &lead.arranger[0];
    assert!((clip.clip.length - 8.0).abs() < 1e-9, "clip length doubles: {}", clip.clip.length);
    // A2:1 C3:1 E3:1 A3:1 → the second note starts at beat 2 after stretch
    assert!(
        clip.clip.notes.iter().any(|n| (n.start - 2.0).abs() < 1e-9),
        "note starts double: {:?}",
        clip.clip.notes.iter().map(|n| n.start).collect::<Vec<_>>()
    );
    // stretch out of range
    let bad = format!("{RIFF}\nsong \"S\" {{ tempo 120bpm play Riff(stretch: 8) at bars(1..2) }}");
    let errs = match fortelang::compile_str(&bad) {
        Err(e) => e,
        Ok(_) => panic!("stretch 8 must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-TYPE-002"), "{errs:?}");
}

#[test]
fn as_alias_shares_one_lane_across_variants() {
    const FAM: &str = r#"block Groove {
  track Drums { instrument sampler(sample: "Kick") play beat`x---` at bars(1..1) }
}
block GrooveBusy : Groove {
  track Drums { play beat`x-x- x-xx` at bars(1..1) }
}"#;
    // variants share ONE lane via the alias — a single Drums track with
    // clips from both placements
    let src = format!(
        "{FAM}\nsong \"S\" {{\n  tempo 120bpm\n  play Groove as Drums at bars(1..4)\n  play GrooveBusy as Drums at bars(5..8)\n}}"
    );
    let p = compile_str(&src).unwrap();
    let lanes: Vec<&str> =
        p.tracks.iter().map(|t| t.name.as_str()).filter(|n| n.contains("Drums")).collect();
    assert_eq!(lanes, ["Drums.Drums"], "one shared lane: {lanes:?}");
    let t = p.tracks.iter().find(|t| t.name == "Drums.Drums").unwrap();
    let has_early = t.arranger.iter().any(|c| c.start < 8.0);
    let has_late = t.arranger.iter().any(|c| c.start >= 16.0);
    assert!(has_early && has_late, "clips from both variants share the lane");

    // without the alias the variants stack as separate lanes
    let plain = format!(
        "{FAM}\nsong \"S\" {{\n  tempo 120bpm\n  play Groove at bars(1..4)\n  play GrooveBusy at bars(5..8)\n}}"
    );
    let p = compile_str(&plain).unwrap();
    assert!(p.tracks.iter().any(|t| t.name == "Groove.Drums"));
    assert!(p.tracks.iter().any(|t| t.name == "GrooveBusy.Drums"));

    // structure mismatch under one alias is refused with guidance
    let clash = format!(
        "{FAM}\nblock GrooveFx : Groove {{ track Drums {{ insert drive(drive: 0.4) }} }}\nsong \"S\" {{\n  tempo 120bpm\n  play Groove as Drums at bars(1..4)\n  play GrooveFx as Drums at bars(5..8)\n}}"
    );
    let errs = match fortelang::compile_str(&clash) {
        Err(e) => e,
        Ok(_) => panic!("structure mismatch must be rejected"),
    };
    assert!(errs.iter().any(|d| d.code == "E-BLOCK-007"), "{errs:?}");

    // placement automation talks to the ALIAS
    let fade = format!(
        "{FAM}\nsong \"S\" {{\n  tempo 120bpm\n  play Groove as Drums at bars(1..4)\n  automate Drums.volume from 0 to 1 over bars(1..2)\n}}"
    );
    let p = compile_str(&fade).unwrap();
    let t = p.tracks.iter().find(|t| t.name == "Drums.Drums").unwrap();
    assert!(!t.volume_automation.is_empty(), "alias-targeted fade lands on the lane");
}
