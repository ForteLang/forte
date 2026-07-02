//! End-to-end tests: reference song compiles, renders real audio, and renders
//! reproducibly. Error paths report the documented diagnostic codes.

const REFERENCE: &str = include_str!("../../../songs/first-light.forte");
const REFERENCE2: &str = include_str!("../../../songs/slow-circles.forte");

#[test]
fn reference_song_compiles() {
    let p = fortelang::compile_str(REFERENCE).expect("reference song must compile");
    assert_eq!(p.tracks.len(), 6);
    assert_eq!(p.tempo, 96.0);
    assert_eq!(p.time_sig, (4, 4));
    // 16 bars of 4 beats
    assert_eq!(dawcore::bounce::arrangement_len(&p), 64.0);
    // Bass has instrument + drive
    let bass = p.tracks.iter().find(|t| t.name == "Bass").unwrap();
    assert_eq!(bass.devices.len(), 2);
}

#[test]
fn reference_song_renders_deterministically_and_audibly() {
    let p = fortelang::compile_str(REFERENCE).unwrap();
    let a = fortelang::render_digest(&p, 8.0);
    let b = fortelang::render_digest(&p, 8.0);
    assert_eq!(a.f32_digest, b.f32_digest, "same project must render bit-identically");
    assert!(a.rms > 0.01, "render must contain real signal (rms {})", a.rms);
    assert!(a.peak <= 1.0, "master limiter must bound output (peak {})", a.peak);
}

#[test]
fn second_reference_song_compiles_in_six_eight() {
    let p = fortelang::compile_str(REFERENCE2).expect("slow-circles must compile");
    assert_eq!(p.tracks.len(), 4);
    assert_eq!(p.time_sig, (6, 8));
    // 6/8 -> 3 engine beats per bar; 12 bars = 36 beats
    assert_eq!(dawcore::bounce::arrangement_len(&p), 36.0);
    let info = fortelang::render_digest(&p, 4.0);
    assert!(info.rms > 0.005, "6/8 render must contain signal (rms {})", info.rms);
}

fn err_codes(src: &str) -> Vec<&'static str> {
    match fortelang::compile_str(src) {
        Ok(_) => Vec::new(),
        Err(ds) => ds.into_iter().map(|d| d.code).collect(),
    }
}

#[test]
fn unknown_param_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer(cutof: 0.5) play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-DEV-002"));
}

#[test]
fn missing_instrument_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-TRACK-001"));
}

#[test]
fn out_of_range_knob_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer(cutoff: 1.5) play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-TYPE-002"));
}

#[test]
fn undefined_pattern_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play nothere at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-MOD-001"));
}

#[test]
fn missing_tempo_is_reported() {
    let src = r#"song "X" { track A { instrument polymer() play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-SONG-001"));
}

#[test]
fn external_audio_is_rejected_by_design() {
    let src = r#"song "X" { tempo 120bpm track A { instrument sampler(sample: "stolen.wav") play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-DEV-003"));
}

#[test]
fn chords_and_fraction_durations_parse() {
    let src = r#"song "X" { tempo 100bpm track A { instrument polymer() play notes`[C4 E4 G4]:1/2 _:1/2 D4:1` at bars(1..1) } }"#;
    let p = fortelang::compile_str(src).unwrap();
    let clip = &p.tracks[0].arranger[0].clip;
    assert_eq!(clip.notes.len(), 4); // 3 chord tones + 1 melody note (rest is silent)
    assert_eq!(clip.length, 2.0);
    assert_eq!(clip.notes[3].start, 1.0); // after chord (0.5) + rest (0.5)
}
