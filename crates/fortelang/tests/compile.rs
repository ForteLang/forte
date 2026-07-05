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

// ---------------------------------------------------------------------------
// automate / modulate
// ---------------------------------------------------------------------------

/// Render the arrangement (no tail) and return the RMS of each half — enough
/// to hear a volume ramp without inspecting samples by hand.
fn half_rms(p: &dawcore::model::Project) -> (f64, f64) {
    let sr = 48_000.0f32;
    let (mut engine, mut handle) = dawcore::engine::Engine::new(sr);
    dawcore::sync::full_sync(&mut handle, p);
    handle.send(dawcore::command::Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(dawcore::command::Command::SetLaunchQuant(0.0));
    handle.send(dawcore::command::Command::Play);
    let beats = dawcore::bounce::arrangement_len(p);
    let total = ((beats * 60.0 / p.tempo) * sr as f64) as usize;
    let mut bl = vec![0.0f32; 512];
    let mut br = vec![0.0f32; 512];
    let (mut sq, mut done) = ([0.0f64; 2], 0usize);
    while done < total {
        let n = 512.min(total - done);
        engine.process(&mut bl, &mut br, n);
        for i in 0..n {
            let half = if done + i < total / 2 { 0 } else { 1 };
            sq[half] += (bl[i] as f64).powi(2) + (br[i] as f64).powi(2);
        }
        done += n;
    }
    let half_n = (total as f64).max(1.0); // both halves ~equal length
    ((sq[0] / half_n).sqrt(), (sq[1] / half_n).sqrt())
}

#[test]
fn automate_volume_ramps_audibly() {
    let src = r#"song "X" {
        tempo 120bpm
        section all = bars(1..8)
        track A {
            instrument polymer(wave: "saw")
            play notes`C3:4` at all
            automate volume from 0.05 to 0.9 over all
        }
    }"#;
    let p = fortelang::compile_str(src).unwrap();
    let lane = &p.tracks[0].volume_automation;
    assert_eq!(lane.len(), 2);
    assert_eq!((lane[0].beat, lane[0].value), (0.0, 0.05));
    assert_eq!((lane[1].beat, lane[1].value), (32.0, 0.9));
    let (first, second) = half_rms(&p);
    assert!(
        second > first * 2.0,
        "ramp 0.05 → 0.9 must get audibly louder (rms {first:.4} → {second:.4})"
    );
}

#[test]
fn modulate_routes_an_lfo_at_the_instrument() {
    let with = r#"song "X" {
        tempo 120bpm
        track A {
            instrument polymer(wave: "saw", cutoff: 0.4)
            play notes`C3:4` at bars(1..4)
            modulate cutoff with lfo(rate: 0.4, amount: 0.5, shape: "tri")
        }
    }"#;
    let p = fortelang::compile_str(with).unwrap();
    let mods = &p.tracks[0].devices[0].modulators;
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].kind, dawcore::model::ModKind::Lfo);
    assert_eq!(mods[0].shape, 1); // tri
    assert_eq!(mods[0].rate, 0.4);
    assert_eq!(mods[0].routes.len(), 1);
    assert_eq!(mods[0].routes[0].param, 1); // Polymer "Cutoff"
    assert_eq!(mods[0].routes[0].amount, 0.5);
    // the LFO must change the rendered audio
    let without = with.replace("modulate cutoff with lfo(rate: 0.4, amount: 0.5, shape: \"tri\")", "");
    let q = fortelang::compile_str(&without).unwrap();
    assert_ne!(
        fortelang::render_digest(&p, 2.0).f32_digest,
        fortelang::render_digest(&q, 2.0).f32_digest,
        "modulate must be audible in the build digest"
    );
}

#[test]
fn automate_and_modulate_errors_are_reported() {
    // an unknown automate target lists what exists
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play beat`x---` at bars(1..2) automate pan from 0.0 to 1.0 over bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-AUTO-001"));
    // unknown modulate parameter lists what exists
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play beat`x---` at bars(1..2) modulate cutof with lfo(rate: 0.3, amount: 0.4) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-001"));
    // raw grid instruments expose no named params
    let src = r#"song "X" { tempo 120bpm track A { instrument grid() play beat`x---` at bars(1..2) modulate cutoff with lfo(rate: 0.3, amount: 0.4) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-001"));
    // amount is required
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play beat`x---` at bars(1..2) modulate cutoff with lfo(rate: 0.3) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-003"));
    // modulator kinds are lfo / steps / random — anything else is a parse error
    let src = r#"song "X" { tempo 120bpm track A { instrument polymer() play beat`x---` at bars(1..2) modulate cutoff with wobble(amount: 0.4) } }"#;
    assert!(err_codes(src).contains(&"E-PARSE-021"));
}

// ---------------------------------------------------------------------------
// noise / shaper: sound design primitives
// ---------------------------------------------------------------------------

const NOISE_SONG: &str = r#"device Snare : Instrument {
  node env = adsr(a: 0.001, d: 0.12, s: 0.0, r: 0.08)
  node n   = noise()
  node f   = svf(in: n, cutoff: 0.75, reso: 0.3)
  out gain(in: f, mod: env, level: 0.9)
}
device FoldLead : Instrument {
  node env = adsr(a: 0.01, d: 0.3, s: 0.5, r: 0.2)
  node o   = osc(shape: "sine")
  node sh  = shaper(in: o, drive: 0.6, mode: "fold")
  out gain(in: sh, mod: env, level: 0.8)
}
song "SoundDesign" {
  tempo 120bpm
  track Drums { instrument Snare() play beat`x-x- x-x-` at bars(1..2) }
  track Lead  { instrument FoldLead() play notes`C3:1 G3:1` at bars(1..2) }
}"#;

#[test]
fn noise_and_shaper_render_deterministically() {
    let p = fortelang::compile_str(NOISE_SONG).expect("noise/shaper song must compile");
    let a = fortelang::render_digest(&p, 2.0);
    let b = fortelang::render_digest(&p, 2.0);
    assert!(a.rms > 0.005, "noise snare + folded lead must sound (rms {})", a.rms);
    assert_eq!(a.f32_digest, b.f32_digest, "noise must be deterministic (reseeded per note)");

    // the shaper mode is audible in the build digest
    let tanh = NOISE_SONG.replace("mode: \"fold\"", "mode: \"tanh\"");
    let p2 = fortelang::compile_str(&tanh).unwrap();
    assert_ne!(
        a.f32_digest,
        fortelang::render_digest(&p2, 2.0).f32_digest,
        "fold and tanh must sound different"
    );
}

#[test]
fn unknown_primitive_lists_the_new_ones() {
    let src = r#"device X : Instrument { node a = warp() out gain(in: a) }
song "Y" { tempo 120bpm track A { instrument X() play beat`x---` at bars(1..1) } }"#;
    let err = match fortelang::compile_str(src) {
        Err(ds) => ds.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n"),
        Ok(_) => String::new(),
    };
    assert!(err.contains("noise") && err.contains("shaper"), "{err}");
}

#[test]
fn osc_pitch_mod_makes_kick_drops() {
    let with_drop = r#"device K : Instrument {
  node env  = adsr(a: 0.001, d: 0.18, s: 0.0, r: 0.1)
  node penv = adsr(a: 0.001, d: 0.05, s: 0.0, r: 0.03)
  node o    = osc(shape: "sine", mod: gain(in: penv, level: 0.4))
  out gain(in: o, mod: env, level: 1.0)
}
song "X" { tempo 120bpm track A { instrument K() play beat`x---` at bars(1..1) } }"#;
    let p = fortelang::compile_str(with_drop).unwrap();
    let dropped = fortelang::render_digest(&p, 2.0);
    assert!(dropped.rms > 0.005, "kick must sound (rms {})", dropped.rms);
    // the pitch envelope is audible: same patch without the mod differs
    let flat = with_drop.replace(", mod: gain(in: penv, level: 0.4)", "");
    let p2 = fortelang::compile_str(&flat).unwrap();
    assert_ne!(dropped.f32_digest, fortelang::render_digest(&p2, 2.0).f32_digest);
}

// ---------------------------------------------------------------------------
// device … : Effect — user-defined audio effects
// ---------------------------------------------------------------------------

const FX_SONG: &str = r#"device Fuzz : Effect {
  param amount = 0.6 in 0.0..1.0
  node crushed = shaper(in: audio.in, drive: amount, mode: "fold")
  node dry     = gain(in: audio.in, level: 0.3)
  out mix(a: crushed, b: dry)
}
device Tremolo : Effect {
  param speed = 0.45
  node wob = lfo(rate: speed, shape: "sine")
  out gain(in: audio.in, mod: gain(in: wob, level: 0.5))
}
song "FX" {
  tempo 110bpm
  track Keys {
    instrument polymer(wave: "tri")
    insert Fuzz(amount: 0.7)
    insert Tremolo(speed: 0.5)
    play notes`C3:1 E3:1 G3:1 _:1` at bars(1..2)
  }
}"#;

#[test]
fn user_defined_effects_process_audio() {
    let p = fortelang::compile_str(FX_SONG).expect("effect devices must compile");
    let devices = &p.tracks[0].devices;
    assert_eq!(devices.len(), 3); // polymer + Fuzz + Tremolo
    assert_eq!(devices[1].kind, dawcore::model::DeviceKind::GridFx);
    assert!(devices[1].grid.is_some());

    let wet = fortelang::render_digest(&p, 2.0);
    assert!(wet.rms > 0.005, "fx chain must pass signal (rms {})", wet.rms);
    assert_eq!(wet.f32_digest, fortelang::render_digest(&p, 2.0).f32_digest);

    let dry_src = FX_SONG
        .replace("insert Fuzz(amount: 0.7)", "")
        .replace("insert Tremolo(speed: 0.5)", "");
    let dry = fortelang::render_digest(&fortelang::compile_str(&dry_src).unwrap(), 2.0);
    assert_ne!(wet.f32_digest, dry.f32_digest, "effects must be audible");
}

#[test]
fn effect_device_errors_are_reported() {
    let codes = |src: &str| -> Vec<&'static str> {
        match fortelang::compile_str(src) {
            Ok(_) => Vec::new(),
            Err(ds) => ds.into_iter().map(|d| d.code).collect(),
        }
    };
    // an Effect cannot be an instrument, an Instrument cannot be an insert
    let src = FX_SONG.replace("instrument polymer(wave: \"tri\")", "instrument Fuzz()");
    assert!(codes(&src).contains(&"E-DEV-009"), "{src}");
    let src = r#"device L : Instrument { node o = osc() out gain(in: o) }
song "X" { tempo 100bpm track A { instrument polymer() insert L() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-DEV-009"));
    // note.* has no meaning inside an Effect; audio.in none inside an Instrument
    let src = r#"device E : Effect { node g = gain(in: note.freq) out g }
song "X" { tempo 100bpm track A { instrument polymer() insert E() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-GRID-003"));
    let src = r#"device I : Instrument { node g = gain(in: audio.in) out g }
song "X" { tempo 100bpm track A { instrument I() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-GRID-003"));
    // adsr inside an Effect must state its gate explicitly
    let src = r#"device E : Effect { node env = adsr() out gain(in: audio.in, mod: env) }
song "X" { tempo 100bpm track A { instrument polymer() insert E() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-GRID-001"));
}

#[test]
fn stems_are_isolated_deterministic_and_keep_sends() {
    let p = fortelang::compile_str(include_str!("../../../songs/night-parade.forte")).unwrap();
    let mix = fortelang::render_digest(&p, 2.0);
    let mut seen = std::collections::HashSet::new();
    for t in p.tracks.iter().filter(|t| t.kind != dawcore::model::TrackKind::Effect) {
        let solo = fortelang::solo_project(&p, t.id);
        // returns stay audible so a stem keeps its reverb sends
        assert!(solo.tracks.iter().all(|x| {
            x.solo == (x.id == t.id || x.kind == dawcore::model::TrackKind::Effect)
        }));
        let a = fortelang::render_digest(&solo, 2.0);
        let b = fortelang::render_digest(&solo, 2.0);
        assert_eq!(a.f32_digest, b.f32_digest, "stem {} must be deterministic", t.name);
        assert_ne!(a.f32_digest, mix.f32_digest, "stem {} must differ from the mix", t.name);
        assert!(seen.insert(a.f32_digest), "stem {} must differ from other stems", t.name);
        assert!(a.rms > 0.0005, "stem {} must be audible (rms {})", t.name, a.rms);
    }
}
