//! `forte analyze` — the machine-readable listening report (#128). These
//! tests pin the CONTRACT an agent leans on: deterministic values, honest
//! silence maps, onsets that line up with the score, keys heard correctly,
//! and masking that flags identical spectra as identical.

use fortelang::analyze::{analyze, compare, Profile, SectionSpan};

fn project(src: &str) -> dawcore::model::Project {
    fortelang::compile_str(src).expect("song must compile")
}

#[test]
fn report_is_deterministic_and_structured() {
    let p = project(
        r#"song "D" { tempo 120bpm
  track A { instrument prisma(wave: "saw", cutoff: 0.7, sustain: 0.8, release: 0.1)
    play notes`A2:1 C3:1 E3:1 A3:1` at bars(1..2) } }"#,
    );
    let a1 = analyze(&p, &[], true);
    let a2 = analyze(&p, &[], true);
    assert_eq!(a1.to_json(), a2.to_json(), "analysis must be deterministic");
    assert!(a1.seconds > 3.9 && a1.seconds < 4.1, "two 120bpm bars ≈ 4 s, got {}", a1.seconds);
    assert!(a1.loudness.integrated_lufs < 0.0 && a1.loudness.integrated_lufs > -70.0);
    assert!(a1.loudness.crest_db > 0.0, "true peak must sit above rms");
}

#[test]
fn silence_map_sees_the_rests() {
    // one hit then 3 beats of nothing per bar — the gap is the groove
    let p = project(
        r#"song "S" { tempo 120bpm
  track A { instrument prisma(wave: "sine", cutoff: 0.5, attack: 0.001, sustain: 0.0, decay: 0.05, release: 0.05)
    play notes`C4:0.5 _:3.5` at bars(1..2) } }"#,
    );
    let a = analyze(&p, &[], false);
    assert!(
        a.structure.silence_total_pct > 40.0,
        "mostly-rest song must show silence, got {}%",
        a.structure.silence_total_pct
    );
    assert!(!a.structure.silences.is_empty(), "the silence runs must be listed");
}

#[test]
fn onsets_land_on_the_written_grid() {
    let p = project(
        r#"song "R" { tempo 120bpm
  track A { instrument prisma(wave: "saw", cutoff: 0.8, attack: 0.001, sustain: 0.0, decay: 0.08, release: 0.05)
    play notes`C4:1 C4:1 C4:1 C4:1` at bars(1..1) } }"#,
    );
    let a = analyze(&p, &[], false);
    assert_eq!(a.rhythm.score_onsets, 4);
    assert!(
        a.rhythm.audio_onsets >= 3 && a.rhythm.audio_onsets <= 6,
        "four hits should read as about four onsets, got {}",
        a.rhythm.audio_onsets
    );
    assert!(
        a.rhythm.matched_pct > 70.0,
        "detected onsets must sit on the written grid, got {}%",
        a.rhythm.matched_pct
    );
}

#[test]
fn key_estimate_hears_a_minor() {
    let p = project(
        r#"song "K" { tempo 120bpm
  key A minor
  track A { instrument prisma(wave: "sine", cutoff: 0.5, sustain: 0.8, release: 0.1)
    play notes`A2:1 C3:1 E3:1 A3:1` at bars(1..2)
    play notes`E3:1 A3:1 C4:1 E4:1` at bars(3..4) } }"#,
    );
    let a = analyze(&p, &[], false);
    assert!(
        a.tonality.agrees == Some(true) || a.tonality.relative,
        "an A-minor arpeggio must read as A minor (or its relative), got {} vs {}",
        a.tonality.estimated_key,
        a.tonality.declared_key
    );
}

#[test]
fn identical_tracks_mask_each_other_completely() {
    let p = project(
        r#"song "M" { tempo 120bpm
  track A { instrument prisma(wave: "saw", cutoff: 0.6, sustain: 0.8)
    play notes`C3:1 E3:1 G3:1 C4:1` at bars(1..1) }
  track B { instrument prisma(wave: "saw", cutoff: 0.6, sustain: 0.8)
    play notes`C3:1 E3:1 G3:1 C4:1` at bars(1..1) } }"#,
    );
    let a = analyze(&p, &[], true);
    assert_eq!(a.spectral.tracks.len(), 2, "both stems must be measured");
    let pair = &a.spectral.masking[0];
    assert!(
        pair.overlap > 0.95,
        "identical parts must show ~total spectral overlap, got {}",
        pair.overlap
    );
}

#[test]
fn unison_spread_opens_the_stereo_field() {
    // the same phrase, mono voice vs a 5-voice spread stack — the whole
    // point of #126, measured with the ears from #128
    let song = |inst: &str| {
        format!(
            r#"song "U" {{ tempo 120bpm
  track A {{ instrument {inst}
    play notes`C3:1 E3:1 G3:1 C4:1` at bars(1..2) }} }}"#
        )
    };
    let mono = fortelang::compile_str(&song(r#"prisma(wave: "saw", cutoff: 0.7, sustain: 0.8)"#))
        .unwrap();
    let wide = fortelang::compile_str(&song(
        r#"prisma(wave: "saw", cutoff: 0.7, sustain: 0.8, unison: 5, detune: 0.4, spread: 0.9)"#,
    ))
    .unwrap();
    let a_mono = analyze(&mono, &[], false);
    let a_wide = analyze(&wide, &[], false);
    assert!(
        a_mono.stereo.side_mid_db < -60.0,
        "a mono synth has no side energy, got {} dB",
        a_mono.stereo.side_mid_db
    );
    assert!(
        a_wide.stereo.side_mid_db > -20.0,
        "5-voice unison at spread 0.9 must open the field, got {} dB",
        a_wide.stereo.side_mid_db
    );
    // out-of-range voice counts are musical errors, not knob math
    let bad = fortelang::compile_str(&song(r#"prisma(unison: 9)"#));
    assert!(bad.is_err(), "unison: 9 must be rejected");
}

#[test]
fn profiles_judge_a_render_against_targets() {
    let p = project(
        r#"song "P" { tempo 120bpm
  track A { instrument prisma(wave: "saw", cutoff: 0.7, sustain: 0.8, release: 0.1)
    play notes`C3:1 E3:1 G3:1 C4:1` at bars(1..2) } }"#,
    );
    let a = analyze(&p, &[], false);
    // a profile this quiet mono phrase can never satisfy…
    let strict = Profile::from_json(
        r#"{ "name": "club", "integrated_lufs": [-9, -6], "side_mid_db": [-12, -4] }"#,
    )
    .expect("profile must parse");
    let deltas = compare(&a, &strict);
    assert_eq!(deltas.len(), 2, "only declared targets are judged");
    assert!(deltas.iter().all(|d| !d.ok), "quiet mono must miss club targets");
    let lufs = deltas.iter().find(|d| d.metric == "integrated_lufs").unwrap();
    assert!(lufs.delta < 0.0, "shortfall points DOWN toward the target, got {}", lufs.delta);
    // …and one built around what it actually measures passes
    let fitted = Profile::from_json(&format!(
        r#"{{ "name": "fit", "integrated_lufs": [{}, {}],
             "band_share_pct": {{ "mid": [0, 100] }} }}"#,
        a.loudness.integrated_lufs - 1.0,
        a.loudness.integrated_lufs + 1.0
    ))
    .unwrap();
    assert!(compare(&a, &fitted).iter().all(|d| d.ok));
}

#[test]
fn level_targets_stage_the_gain_declaratively() {
    // song-level `level -12`: the compiler measures and drives master
    let src = |level: &str| {
        format!(
            r#"song "L" {{ tempo 120bpm
  {level}
  track A {{ instrument prisma(wave: "saw", cutoff: 0.7, sustain: 0.8, release: 0.1)
    play notes`C3:1 E3:1 G3:1 C4:1` at bars(1..2) }} }}"#
        )
    };
    let p = fortelang::compile_str(&src("level -14")).expect("level song must compile");
    let a = analyze(&p, &[], false);
    assert!(
        (a.loudness.integrated_lufs - -14.0).abs() < 1.0,
        "the mix must land within 1 dB of the declared target, got {} LUFS",
        a.loudness.integrated_lufs
    );
    // a target the master clamp cannot reach is an honest error, not a
    // quietly-missed number
    assert!(
        fortelang::compile_str(&src("level -7")).is_err(),
        "an out-of-reach song target must be E-LVL-004"
    );
    // deterministic: same source, same fader math, same audio
    let p2 = fortelang::compile_str(&src("level -14")).unwrap();
    assert_eq!(
        fortelang::render_digest(&p, 1.0).f32_digest,
        fortelang::render_digest(&p2, 1.0).f32_digest,
        "level resolution must be deterministic"
    );
    // songs that never write level are untouched by the feature
    let plain = fortelang::compile_str(&src("")).unwrap();
    assert!((plain.master - 1.0).abs() < 1e-9, "no level = master untouched");
    // range and reachability are musical errors
    let bad = fortelang::compile_str(&src("level -3"));
    assert!(bad.is_err(), "level -3 must be out of range (E-LVL-001)");
    let unreachable = fortelang::compile_str(
        r#"song "U" { tempo 120bpm
  track A { instrument prisma(wave: "sine", cutoff: 0.4, sustain: 0.3)
    level -8
    play notes`C3:1 _:3` at bars(1..1) } }"#,
    );
    assert!(unreachable.is_err(), "a fader-only track cannot GAIN 10 dB (E-LVL-002)");
}

#[test]
fn mesh_instruments_get_a_stereo_field() {
    // a user device with a uni stack — the whole point of #133
    let song = |voice: &str| {
        format!(
            r#"device P : Instrument {{
  node e = adsr(a: 0.05, d: 0.3, s: 0.7, r: 0.2)
  node o = {voice}
  out gain(in: o, mod: e)
}}
song "W" {{ tempo 120bpm
  track A {{ instrument P()
    play notes`[A2 E3 A3]:2 [F2 C3 F3]:2` at bars(1..2) }} }}"#
        )
    };
    let width = |voice: &str| {
        let p = fortelang::compile_str(&song(voice)).expect("mesh song must compile");
        analyze(&p, &[], false).stereo.side_mid_db
    };
    // plain osc stays a mono point source (the bit-exact legacy path)
    assert!(
        width("osc(shape: \"saw\")") < -60.0,
        "mono mesh must have no side energy"
    );
    // a uni stack opens the field
    let wide = width("uni(shape: \"saw\", voices: 5, detune: 0.35, spread: 0.9)");
    assert!(wide > -20.0, "uni must open the field, got {wide} dB");
    // pan positions: hard-left renders more left energy than right
    let p = fortelang::compile_str(&song("pan(in: osc(shape: \"saw\"), pos: 0.02)"))
        .expect("pan song must compile");
    let (_k, s) = fortelang::render_to_sample(&p, 0.0, 60);
    let el: f64 = s.data.iter().map(|v| (*v as f64) * (*v as f64)).sum();
    let er: f64 = s
        .right
        .as_ref()
        .map(|r| r.iter().map(|v| (*v as f64) * (*v as f64)).sum())
        .unwrap_or(el);
    assert!(el > er * 10.0, "pos 0.02 must live on the left ({el} vs {er})");
    // determinism of the stereo path
    let d1 = fortelang::render_digest(
        &fortelang::compile_str(&song("uni(shape: \"saw\", voices: 5)")).unwrap(),
        2.0,
    )
    .f32_digest;
    let d2 = fortelang::render_digest(
        &fortelang::compile_str(&song("uni(shape: \"saw\", voices: 5)")).unwrap(),
        2.0,
    )
    .f32_digest;
    assert_eq!(d1, d2, "stereo grid must be deterministic");
}

#[test]
fn sections_shape_the_report() {
    let p = project(
        r#"song "T" { tempo 120bpm
  track A { instrument prisma(wave: "saw", cutoff: 0.8, attack: 0.001, sustain: 0.0, decay: 0.08, release: 0.05)
    play notes`C3:1 C3:1 C3:1 C3:1` at bars(1..1)
    play notes`C3~:4` at bars(2..2) } }"#,
    );
    let sections = vec![
        SectionSpan { name: "hits".into(), start_beat: 0.0, end_beat: 4.0 },
        SectionSpan { name: "hold".into(), start_beat: 4.0, end_beat: 8.0 },
    ];
    let a = analyze(&p, &sections, false);
    assert_eq!(a.structure.sections.len(), 2);
    assert_eq!(a.structure.sections[0].name, "hits");
    let hits = &a.rhythm.density_per_section[0];
    let hold = &a.rhythm.density_per_section[1];
    assert!(
        hits.onsets_per_second > hold.onsets_per_second,
        "four hits per two seconds must out-dense one held note ({} vs {})",
        hits.onsets_per_second,
        hold.onsets_per_second
    );
}
