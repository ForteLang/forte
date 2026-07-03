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

const REFERENCE3: &str = include_str!("../../../songs/night-parade.forte");

#[test]
fn third_reference_song_uses_prog_sections_and_sends() {
    let p = fortelang::compile_str(REFERENCE3).expect("night-parade must compile");
    // 5 tracks + 1 return
    assert_eq!(p.tracks.len(), 6);
    let space = p.tracks.iter().find(|t| t.name == "Space").unwrap();
    assert_eq!(space.kind, dawcore::model::TrackKind::Effect);
    // sends resolved to the return's id
    let keys = p.tracks.iter().find(|t| t.name == "Keys").unwrap();
    assert_eq!(keys.sends, vec![(space.id, 0.35)]);
    // section placement: hats start at bar 5 (beat 16)
    let hats = p.tracks.iter().find(|t| t.name == "Hats").unwrap();
    assert_eq!(hats.arranger[0].start, 16.0);
    // prog: Em block chord = E3/G3/B3 (52/55/59)
    let keys_clip = &keys.arranger[0].clip;
    let mut first: Vec<u8> =
        keys_clip.notes.iter().filter(|n| n.start == 0.0).map(|n| n.pitch).collect();
    first.sort_unstable();
    assert_eq!(first, vec![52, 55, 59]);
    // arp fills bar at rate 0.25: 16 notes per bar
    let arp = p.tracks.iter().find(|t| t.name == "Arp").unwrap();
    let arp_clip = &arp.arranger[0].clip;
    assert_eq!(arp_clip.notes.iter().filter(|n| n.start < 4.0).count(), 16);
    // renders with signal
    let info = fortelang::render_digest(&p, 4.0);
    assert!(info.rms > 0.01, "render must contain signal (rms {})", info.rms);
}

fn compile_song_file(rel: &str) -> Result<dawcore::model::Project, Vec<fortelang::diag::Diag>> {
    let path = format!("{}/../../songs/{rel}", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).expect("read song");
    let base = std::path::Path::new(&path).parent().unwrap().to_string_lossy().into_owned();
    fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base)
}

#[test]
fn user_defined_devices_compile_to_grid_graphs() {
    let p = compile_song_file("handmade.forte").expect("handmade must compile");
    let lead = p.tracks.iter().find(|t| t.name == "Lead").unwrap();
    let dev = &lead.devices[0];
    assert_eq!(dev.kind, dawcore::model::DeviceKind::PolyGrid);
    let graph = dev.grid.as_ref().unwrap();
    // NoteIn + osc + adsr + lfo + svf + gain + Out = 7 modules
    assert_eq!(graph.modules.len(), 7);
    assert_eq!(graph.modules[0].kind, dawcore::model::GridModuleKind::NoteIn);
    assert_eq!(graph.modules.last().unwrap().kind, dawcore::model::GridModuleKind::Out);
    // instantiation arg cutoff: 0.7 overrode the 0.6 default on the svf node
    let svf = graph
        .modules
        .iter()
        .find(|m| m.kind == dawcore::model::GridModuleKind::Filter)
        .unwrap();
    assert_eq!(svf.params[0], 0.7);
    // and it makes sound
    let info = fortelang::render_digest(&p, 4.0);
    assert!(info.rms > 0.01, "custom devices must produce signal (rms {})", info.rms);
}

#[test]
fn import_resolution_and_errors() {
    // a missing name in an existing library
    let path = format!("{}/../../songs", env!("CARGO_MANIFEST_DIR"));
    let src = r#"import { Nope } from "./devices/warm.forte"
song "S" { tempo 120bpm track A { instrument polymer() play beat`x---` at bars(1..1) } }"#;
    let err = fortelang::compile_with_loader(src, &fortelang::FsLoader, &path).err().expect("expected errors");
    assert!(err.iter().any(|d| d.code == "E-MOD-006"), "{err:?}");
    assert!(err[0].message.contains("WarmLead"), "should list available devices: {}", err[0].message);

    // unreadable path
    let src2 = r#"import { X } from "./nowhere.forte"
song "S" { tempo 120bpm track A { instrument polymer() play beat`x---` at bars(1..1) } }"#;
    let err2 = fortelang::compile_with_loader(src2, &fortelang::FsLoader, &path).err().expect("expected errors");
    assert!(err2.iter().any(|d| d.code == "E-MOD-005"), "{err2:?}");

    // browser environment (NoLoader): imports are a polite error
    let err3 = fortelang::compile_str(src2).err().expect("expected errors");
    assert!(err3.iter().any(|d| d.code == "E-MOD-005"));

    // a device library validates standalone
    let lib = std::fs::read_to_string(format!("{path}/devices/warm.forte")).unwrap();
    match fortelang::check_with_loader(&lib, &fortelang::FsLoader, &path).unwrap() {
        fortelang::Checked::DeviceLibrary { devices } => assert_eq!(devices, 2),
        _ => panic!("expected device library"),
    }
}

#[test]
fn device_errors_are_reported() {
    // unknown primitive
    let src = r#"device X : Instrument { out warp(in: osc()) } song "S" { tempo 120bpm track A { instrument X() play beat`x---` at bars(1..1) } }"#;
    assert!(err_codes(src).contains(&"E-GRID-004"), "{:?}", err_codes(src));
    // missing required input
    let src2 = r#"device X : Instrument { out svf(cutoff: 0.5) } song "S" { tempo 120bpm track A { instrument X() play beat`x---` at bars(1..1) } }"#;
    assert!(err_codes(src2).contains(&"E-GRID-001"));
    // forward reference
    let src3 = r#"device X : Instrument { node a = gain(in: b) node b = osc() out a } song "S" { tempo 120bpm track A { instrument X() play beat`x---` at bars(1..1) } }"#;
    assert!(err_codes(src3).contains(&"E-GRID-002"));
    // instantiation out of declared range
    let src4 = r#"device X : Instrument { param c = 0.5 in 0.0..1.0 out gain(in: osc(), level: c) } song "S" { tempo 120bpm track A { instrument X(c: 2.0) play beat`x---` at bars(1..1) } }"#;
    assert!(err_codes(src4).contains(&"E-TYPE-002"));
    // builtin shadowing
    let src5 = r#"device polymer : Instrument { out osc() } song "S" { tempo 120bpm track A { instrument grid() play beat`x---` at bars(1..1) } }"#;
    assert!(err_codes(src5).contains(&"E-DEV-008"));
}

#[test]
fn unknown_section_and_return_are_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() send Nowhere 0.3 play beat`x---` at nowhere } }"#;
    let codes = err_codes(src);
    assert!(codes.contains(&"E-MOD-003"), "unknown section: {codes:?}");
    assert!(codes.contains(&"E-MOD-004"), "unknown return: {codes:?}");
}

#[test]
fn bad_chord_and_bad_style_are_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play prog`Hm7` at bars(1..1) } }"#;
    assert!(err_codes(src).contains(&"E-PROG-002"));
    let src2 = r#"song "X" { tempo 120bpm track A { instrument polymer() play arp(prog`Em`, style: "spiral") at bars(1..1) } }"#;
    assert!(err_codes(src2).contains(&"E-PAT-002"));
}

#[test]
fn pattern_fn_requires_prog() {
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play chords(beat`x---`) at bars(1..1) } }"#;
    assert!(err_codes(src).contains(&"E-PAT-001"));
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
