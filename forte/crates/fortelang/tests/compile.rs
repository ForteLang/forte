//! End-to-end tests: reference song compiles, renders real audio, and renders
//! reproducibly. Error paths report the documented diagnostic codes.

const REFERENCE: &str = include_str!("../../../../packages/essentials_0.6.0/songs/first-light.forte");
const REFERENCE2: &str = include_str!("../../../../packages/essentials_0.6.0/songs/slow-circles.forte");

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

const REFERENCE3: &str = include_str!("../../../../packages/essentials_0.6.0/songs/night-parade.forte");

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
    let path = format!("{}/../../../packages/essentials_0.6.0/songs/{rel}", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).expect("read song");
    let base = std::path::Path::new(&path).parent().unwrap().to_string_lossy().into_owned();
    fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base)
}

#[test]
fn user_defined_devices_compile_to_grid_graphs() {
    let p = compile_song_file("handmade.forte").expect("handmade must compile");
    let lead = p.tracks.iter().find(|t| t.name == "Lead").unwrap();
    let dev = &lead.devices[0];
    assert_eq!(dev.kind, dawcore::model::DeviceKind::PolyMesh);
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
    let path = format!("{}/../../../packages/essentials_0.6.0/songs", env!("CARGO_MANIFEST_DIR"));
    let src = r#"import { Nope } from "./devices/warm.forte"
song "S" { tempo 120bpm track A { instrument prisma() play beat`x---` at bars(1..1) } }"#;
    let err = fortelang::compile_with_loader(src, &fortelang::FsLoader, &path).err().expect("expected errors");
    assert!(err.iter().any(|d| d.code == "E-MOD-006"), "{err:?}");
    assert!(err[0].message.contains("WarmLead"), "should list available devices: {}", err[0].message);

    // unreadable path
    let src2 = r#"import { X } from "./nowhere.forte"
song "S" { tempo 120bpm track A { instrument prisma() play beat`x---` at bars(1..1) } }"#;
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
    let src5 = r#"device prisma : Instrument { out osc() } song "S" { tempo 120bpm track A { instrument mesh() play beat`x---` at bars(1..1) } }"#;
    assert!(err_codes(src5).contains(&"E-DEV-008"));
}

#[test]
fn unknown_section_and_return_are_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() send Nowhere 0.3 play beat`x---` at nowhere } }"#;
    let codes = err_codes(src);
    assert!(codes.contains(&"E-MOD-003"), "unknown section: {codes:?}");
    assert!(codes.contains(&"E-MOD-004"), "unknown return: {codes:?}");
}

#[test]
fn bad_chord_and_bad_style_are_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play prog`Hm7` at bars(1..1) } }"#;
    assert!(err_codes(src).contains(&"E-PROG-002"));
    let src2 = r#"song "X" { tempo 120bpm track A { instrument prisma() play arp(prog`Em`, style: "spiral") at bars(1..1) } }"#;
    assert!(err_codes(src2).contains(&"E-PAT-002"));
}

#[test]
fn pattern_fn_requires_prog() {
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play chords(beat`x---`) at bars(1..1) } }"#;
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
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma(cutof: 0.5) play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-DEV-002"));
}

#[test]
fn missing_instrument_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-TRACK-001"));
}

#[test]
fn out_of_range_knob_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma(cutoff: 1.5) play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-TYPE-002"));
}

#[test]
fn undefined_pattern_is_reported() {
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play nothere at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-MOD-001"));
}

#[test]
fn missing_tempo_is_reported() {
    let src = r#"song "X" { track A { instrument prisma() play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-SONG-001"));
}

#[test]
fn external_audio_is_rejected_by_design() {
    let src = r#"song "X" { tempo 120bpm track A { instrument sampler(sample: "stolen.wav") play beat`x---` at bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-DEV-003"));
}

#[test]
fn chords_and_fraction_durations_parse() {
    let src = r#"song "X" { tempo 100bpm track A { instrument prisma() play notes`[C4 E4 G4]:1/2 _:1/2 D4:1` at bars(1..1) } }"#;
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
            instrument prisma(wave: "saw")
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
            instrument prisma(wave: "saw", cutoff: 0.4)
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
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play beat`x---` at bars(1..2) automate pan from 0.0 to 1.0 over bars(1..2) } }"#;
    assert!(err_codes(src).contains(&"E-AUTO-001"));
    // unknown modulate parameter lists what exists
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play beat`x---` at bars(1..2) modulate cutof with lfo(rate: 0.3, amount: 0.4) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-001"));
    // raw grid instruments expose no named params
    let src = r#"song "X" { tempo 120bpm track A { instrument mesh() play beat`x---` at bars(1..2) modulate cutoff with lfo(rate: 0.3, amount: 0.4) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-001"));
    // amount is required
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play beat`x---` at bars(1..2) modulate cutoff with lfo(rate: 0.3) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-003"));
    // modulator kinds are lfo / steps / random / adsr, or a body-level
    // `let` shared modulator — anything else lists what exists
    let src = r#"song "X" { tempo 120bpm track A { instrument prisma() play beat`x---` at bars(1..2) modulate cutoff with wobble(amount: 0.4) } }"#;
    assert!(err_codes(src).contains(&"E-LFO-005"));
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
    instrument prisma(wave: "tri")
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
    assert_eq!(devices[1].kind, dawcore::model::DeviceKind::MeshFx);
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
    let src = FX_SONG.replace("instrument prisma(wave: \"tri\")", "instrument Fuzz()");
    assert!(codes(&src).contains(&"E-DEV-009"), "{src}");
    let src = r#"device L : Instrument { node o = osc() out gain(in: o) }
song "X" { tempo 100bpm track A { instrument prisma() insert L() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-DEV-009"));
    // note.* has no meaning inside an Effect; audio.in none inside an Instrument
    let src = r#"device E : Effect { node g = gain(in: note.freq) out g }
song "X" { tempo 100bpm track A { instrument prisma() insert E() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-GRID-003"));
    let src = r#"device I : Instrument { node g = gain(in: audio.in) out g }
song "X" { tempo 100bpm track A { instrument I() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-GRID-003"));
    // adsr inside an Effect must state its gate explicitly
    let src = r#"device E : Effect { node env = adsr() out gain(in: audio.in, mod: env) }
song "X" { tempo 100bpm track A { instrument prisma() insert E() play beat`x---` at bars(1..1) } }"#;
    assert!(codes(src).contains(&"E-GRID-001"));
}

#[test]
fn stems_are_isolated_deterministic_and_keep_sends() {
    let p = fortelang::compile_str(include_str!("../../../../packages/essentials_0.6.0/songs/night-parade.forte")).unwrap();
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

#[test]
fn master_gain_scales_the_mix_and_validates_range() {
    let song = |header: &str| {
        format!(
            "song \"M\" {{ tempo 120bpm {header}\n  track T {{ instrument prisma(wave: \"sine\", cutoff: 0.4)\n    play notes`A1:1 _:1 A1:1 _:1` at bars(1..1) }} }}"
        )
    };
    let quiet = fortelang::compile_str(&song("")).unwrap();
    let loud = fortelang::compile_str(&song("master 2.0")).unwrap();
    assert_eq!(quiet.master, 1.0);
    assert_eq!(loud.master, 2.0);
    let a = fortelang::render_digest(&quiet, 2.0);
    let b = fortelang::render_digest(&loud, 2.0);
    // +6 dB pre-limiter: doubled RMS on a signal far below saturation
    assert!(
        (b.rms / a.rms - 2.0).abs() < 0.05,
        "master 2.0 must double the level (rms {} → {})",
        a.rms,
        b.rms
    );
    assert!(b.peak <= 1.0, "the master limiter still bounds output (peak {})", b.peak);
    // default is the identity — omitting master must not change a single bit
    let again = fortelang::render_digest(&quiet, 2.0);
    assert_eq!(a.f32_digest, again.f32_digest);
    // out-of-range values are rejected, not clamped silently
    assert!(err_codes(&song("master 9.0")).contains(&"E-SONG-005"));
    assert!(err_codes(&song("master 0.0")).contains(&"E-SONG-005"));
}

#[test]
fn groove_vocabulary_polymeter_euclid_ratchet_humanize() {
    let src = r#"song "G" { tempo 120bpm
      track Poly { instrument prisma(wave: "sine")
        play cycle(beat`x--`, span: 1.5) at bars(1..2) }
      track Euclid { instrument prisma(wave: "square")
        play beat`euclid(3, 8)` at bars(1..1) }
      track Ratchet { instrument prisma(wave: "tri")
        play beat`x*3 - - x*2` at bars(1..1) }
    }"#;
    let p = fortelang::compile_str(src).expect("groove song must compile");

    // polymeter: the clip cycles at its own 1.5-beat period, not the bar
    let poly = &p.tracks.iter().find(|t| t.name == "Poly").unwrap().arranger[0];
    assert_eq!(poly.clip.length, 1.5);
    assert_eq!(poly.duration, 8.0, "placed over 2 bars of 4/4");

    // euclid(3,8) = x--x--x- : hits on steps 0, 3, 6 of 8 over 4 beats
    let euc = &p.tracks.iter().find(|t| t.name == "Euclid").unwrap().arranger[0].clip;
    let starts: Vec<f64> = euc.notes.iter().map(|n| n.start).collect();
    assert_eq!(starts, vec![0.0, 1.5, 3.0]);

    // ratchet: x*3 subdivides step one into 3 retrigs, velocity decaying
    let rat = &p.tracks.iter().find(|t| t.name == "Ratchet").unwrap().arranger[0].clip;
    assert_eq!(rat.notes.len(), 5);
    assert!((rat.notes[1].start - 1.0 / 3.0).abs() < 1e-9);
    assert!(rat.notes[0].velocity > rat.notes[1].velocity);
    assert!(rat.notes[1].velocity > rat.notes[2].velocity);

    // humanize: seeded and deterministic — same seed same bits, seeds differ
    let hsong = |seed: u32| {
        format!(
            r#"song "H" {{ tempo 120bpm
              track T {{ instrument prisma(wave: "saw")
                play humanize(beat`x-x- x-x-`, time: 0.03, vel: 12, seed: {seed}) at bars(1..1) }} }}"#
        )
    };
    let a = fortelang::render_digest(&fortelang::compile_str(&hsong(7)).unwrap(), 2.0);
    let b = fortelang::render_digest(&fortelang::compile_str(&hsong(7)).unwrap(), 2.0);
    let c = fortelang::render_digest(&fortelang::compile_str(&hsong(8)).unwrap(), 2.0);
    assert_eq!(a.f32_digest, b.f32_digest, "same seed must be bit-identical");
    assert_ne!(a.f32_digest, c.f32_digest, "different seeds must differ");

    // error paths carry the documented codes
    let wrap = |body: &str| format!(r#"song "E" {{ tempo 120bpm track T {{ instrument prisma() {body} }} }}"#);
    assert!(err_codes(&wrap("play beat`euclid(0, 8)` at bars(1..1)")).contains(&"E-BEAT-003"));
    assert!(err_codes(&wrap("play beat`x*20 -` at bars(1..1)")).contains(&"E-BEAT-004"));
    assert!(err_codes(&wrap("play cycle(beat`x--`) at bars(1..1)")).contains(&"E-PAT-004"));
    assert!(err_codes(&wrap("play cycle(prog`Am | F`, span: 2) at bars(1..1)")).contains(&"E-PAT-001"));
}

#[test]
fn glitch_effects_render_deterministically_and_audibly() {
    let song = |fx: &str| {
        format!(
            r#"song "GX" {{ tempo 120bpm
              track T {{ instrument prisma(wave: "saw", cutoff: 0.5)
                {fx}
                play beat`x-x- x-x-` at bars(1..1) }} }}"#
        )
    };
    let dry = fortelang::render_digest(&fortelang::compile_str(&song("")).unwrap(), 2.0);
    for fx in [
        "insert crush(bits: 0.7, rate: 0.5, mix: 1.0)",
        "insert stutter(beats: 0.25, mix: 0.8)",
        "insert gate(depth: 0.9, beats: 0.25, duty: 0.4)",
    ] {
        let p = fortelang::compile_str(&song(fx)).unwrap_or_else(|e| panic!("{fx}: {e:?}"));
        let a = fortelang::render_digest(&p, 2.0);
        let b = fortelang::render_digest(&p, 2.0);
        assert_eq!(a.f32_digest, b.f32_digest, "{fx} must render bit-identically");
        assert_ne!(a.f32_digest, dry.f32_digest, "{fx} must change the sound");
        assert!(a.rms > 0.003, "{fx} must stay audible (rms {})", a.rms);
        assert!(a.peak <= 1.0, "{fx} stays inside the limiter (peak {})", a.peak);
    }
    // unknown knobs are rejected, not ignored
    assert!(!err_codes(&song("insert crush(foo: 0.5)")).is_empty());
}

#[test]
fn bounce_to_sample_wraps_instruments_deterministically() {
    let src = r#"song "B" { tempo 120bpm
      sample Sub = bounce(prisma(wave: "sine", cutoff: 0.5, sustain: 0.8), note: C2, beats: 1)
      track Bass { instrument sampler(sample: Sub, decay: 0.6)
        play notes`C1:1 C2:0.5 G1:0.5 C2:1` at bars(1..1) } }"#;
    let p = fortelang::compile_str(src).expect("bounce song must compile");
    let a = fortelang::render_digest(&p, 2.0);
    // the bounce is part of compilation: recompile end-to-end and compare
    let p2 = fortelang::compile_str(src).unwrap();
    let b = fortelang::render_digest(&p2, 2.0);
    assert_eq!(a.f32_digest, b.f32_digest, "bounce assets must be bit-stable");
    assert!(a.rms > 0.0005, "sampler-wrapped instrument must sound (rms {})", a.rms);

    // repitching audio is not the same sound as playing the source directly
    let direct = r#"song "D" { tempo 120bpm
      track Bass { instrument prisma(wave: "sine", cutoff: 0.5, sustain: 0.8)
        play notes`C1:1 C2:0.5 G1:0.5 C2:1` at bars(1..1) } }"#;
    let d = fortelang::render_digest(&fortelang::compile_str(direct).unwrap(), 2.0);
    assert_ne!(a.f32_digest, d.f32_digest, "audio-domain repitch must differ from synthesis");

    // error paths
    let bad = |body: &str| {
        format!(r#"song "E" {{ tempo 120bpm {body} }}"#)
    };
    assert!(err_codes(&bad(
        "sample S = bounce(prisma(), note: C2, beats: 99)\n track T { instrument sampler(sample: S) play beat`x` at bars(1..1) }"
    ))
    .contains(&"E-SMP-001"));
    assert!(err_codes(&bad(
        "track T { instrument sampler(sample: NoSuch) play beat`x` at bars(1..1) }"
    ))
    .contains(&"E-SMP-002"));
}

#[test]
fn sampler_glide_and_slices_are_audio_domain_tools() {
    // glide: tied notes slide the running voice instead of retriggering
    let song = |glide: &str| {
        format!(
            r#"song "G" {{ tempo 120bpm
              sample Sub = bounce(prisma(wave: "sine", cutoff: 0.4, sustain: 0.9), note: C2, beats: 2)
              track B {{ instrument sampler(sample: Sub{glide}, sustain: 0.9, loop: "on")
                play notes`C1~:1 Eb1~:1 C1:1` at bars(1..1) }} }}"#
        )
    };
    let with = fortelang::render_digest(&fortelang::compile_str(&song(", glide: 0.2")).unwrap(), 2.0);
    let without = fortelang::render_digest(&fortelang::compile_str(&song("")).unwrap(), 2.0);
    assert_ne!(with.f32_digest, without.f32_digest, "glide must change the sound");
    assert!(with.rms > 0.0005, "glide render must sound (rms {})", with.rms);
    let again = fortelang::render_digest(&fortelang::compile_str(&song(", glide: 0.2")).unwrap(), 2.0);
    assert_eq!(with.f32_digest, again.f32_digest, "glide must be deterministic");

    // slices: notes pick chunks at original speed; different notes = different cuts
    let chop = |pat: &str| {
        format!(
            r#"song "S" {{ tempo 120bpm
              sample L = bounce(prisma(wave: "saw", cutoff: 0.6, decay: 0.15), note: C3, beats: 2)
              track C {{ instrument sampler(sample: L, slices: 8, decay: 0.2, sustain: 0.0)
                play notes`{pat}` at bars(1..1) }} }}"#
        )
    };
    let a = fortelang::render_digest(&fortelang::compile_str(&chop("C3:0.5 G3:0.5 D#3:0.5 A3:0.5")).unwrap(), 2.0);
    let b = fortelang::render_digest(&fortelang::compile_str(&chop("C3:0.5 C3:0.5 C3:0.5 C3:0.5")).unwrap(), 2.0);
    assert_ne!(a.f32_digest, b.f32_digest, "different slices must sound different");
    assert!(a.rms > 0.0005, "sliced render must sound (rms {})", a.rms);
    // bad slice counts are rejected
    assert!(!err_codes(&chop("C3:0.5").replace("slices: 8", "slices: 99")).is_empty());
}

#[test]
fn pedal_effects_render_deterministically_and_change_the_sound() {
    let song = |fx: &str| {
        format!(
            r#"song "FX" {{ tempo 120bpm
              track T {{ instrument prisma(wave: "saw", cutoff: 0.5)
                {fx}
                play beat`x-x- x-x-` at bars(1..1) }} }}"#
        )
    };
    let dry = fortelang::render_digest(&fortelang::compile_str(&song("")).unwrap(), 2.0);
    for fx in [
        r#"insert saturate(mode: "tape", drive: 0.6)"#,
        r#"insert saturate(mode: "tube", drive: 0.5)"#,
        r#"insert saturate(mode: "fuzz", drive: 0.7, tone: 0.4)"#,
        "insert transient(attack: 0.9, sustain: 0.2)",
        "insert parcomp(amount: 0.6, drive: 0.7, color: 0.5)",
        "insert exciter(amount: 0.6)",
        "insert ringmod(freq: 0.5, mix: 0.7)",
        "insert tapestop(amount: 0.4)",
    ] {
        let p = fortelang::compile_str(&song(fx)).unwrap_or_else(|e| panic!("{fx}: {e:?}"));
        let a = fortelang::render_digest(&p, 2.0);
        let b = fortelang::render_digest(&p, 2.0);
        assert_eq!(a.f32_digest, b.f32_digest, "{fx} must render bit-identically");
        assert_ne!(a.f32_digest, dry.f32_digest, "{fx} must change the sound");
        assert!(a.rms > 0.002, "{fx} must stay audible (rms {})", a.rms);
        assert!(a.peak <= 1.0, "{fx} bounded by the limiter (peak {})", a.peak);
    }
    // tapestop at 0 must be bit-exact bypass
    let z = fortelang::render_digest(
        &fortelang::compile_str(&song("insert tapestop(amount: 0.0)")).unwrap(),
        2.0,
    );
    assert_eq!(z.f32_digest, dry.f32_digest, "tapestop amount 0 must be a true bypass");
    // the loudness convention chain compiles: eq -> saturate -> comp
    let chain = song(
        r#"insert eq(low: 0.6, mid: 0.45, high: 0.6)
           insert saturate(mode: "tape", drive: 0.5)
           insert comp(thresh: 0.4, ratio: 0.7, makeup: 0.3)"#,
    );
    assert!(fortelang::compile_str(&chain).is_ok(), "the eq->saturate->comp chain must compile");
}

#[test]
fn wrapped_instrument_blocks_carry_their_own_samples() {
    // a block library declares the bounce inside the block; a song inherits
    // the block and adds plays — the shipping form of wrapped instruments
    let src = r#"song "W" { tempo 140bpm
      block Wrapped {
        sample Src = bounce(prisma(wave: "sine", cutoff: 0.4, sustain: 0.9), note: C1, beats: 2)
        track Sub {
          instrument sampler(sample: Src, glide: 0.1, sustain: 0.9, loop: "on")
          insert saturate(mode: "tape", drive: 0.4)
          insert comp(thresh: 0.45, ratio: 0.6, makeup: 0.2)
        }
      }
      block Line : Wrapped {
        track Sub { play notes`C1~:1 Eb1~:0.5 C1:1.5` at bars(1..1) }
      }
      play Line at bars(1..1)
    }"#;
    let p = fortelang::compile_str(src).expect("block-scoped sample must compile");
    let a = fortelang::render_digest(&p, 2.0);
    let b = fortelang::render_digest(&fortelang::compile_str(src).unwrap(), 2.0);
    assert_eq!(a.f32_digest, b.f32_digest, "block-scoped bounce must be bit-stable");
    assert!(a.rms > 0.001, "wrapped instrument must sound (rms {})", a.rms);
}

#[test]
fn sidechain_duck_carves_the_source_hits() {
    // a sustained pad ducked by the kick should dip hard right after each
    // kick and recover between them — the glitch groove engine
    let song = |duck: &str| {
        format!(
            r#"song "D" {{ tempo 128bpm
              track Kick {{ instrument sampler(sample: "Kick", decay: 0.3, sustain: 0.0)
                volume 0.0
                play beat`x--- x--- x--- x---` at bars(1..1) }}
              track Pad {{ instrument prisma(wave: "saw", cutoff: 0.4, sustain: 0.9)
                {duck}
                play notes`C3:4` at bars(1..1) }} }}"#
        )
    };
    let ducked = fortelang::compile_str(&song("insert duck(from: Kick, amount: 0.9, release: 0.3)")).unwrap();
    let plain = fortelang::compile_str(&song("")).unwrap();
    let a = fortelang::render_digest(&ducked, 1.0);
    let b = fortelang::render_digest(&plain, 1.0);
    assert_ne!(a.f32_digest, b.f32_digest, "the duck must reshape the sound");
    // the duck lowers overall energy (it removes signal between hits)
    assert!(a.rms < b.rms, "ducked rms {} must be below plain {}", a.rms, b.rms);
    // deterministic
    let again = fortelang::render_digest(&fortelang::compile_str(&song("insert duck(from: Kick, amount: 0.9, release: 0.3)")).unwrap(), 1.0);
    assert_eq!(a.f32_digest, again.f32_digest, "duck must be bit-stable");

    // errors: missing from, unknown source
    let wrap = |body: &str| format!(r#"song "E" {{ tempo 120bpm track T {{ instrument prisma() {body} play beat`x` at bars(1..1) }} }}"#);
    assert!(fortelang::compile_str(&wrap("insert duck(amount: 0.9)")).is_err());
    assert!(fortelang::compile_str(&wrap("insert duck(from: Nope)")).is_err());
}

#[test]
fn sampler_pitch_automation_bends_held_audio_and_stays_bitexact_when_constant() {
    let song = |auto: &str| {
        format!(
            r#"song "P" {{ tempo 120bpm
              sample Tone = bounce(prisma(wave: "saw", cutoff: 0.5, sustain: 0.9), note: C3, beats: 2)
              track S {{ instrument sampler(sample: Tone, sustain: 0.9, loop: "on")
                {auto}
                play notes`C2:4` at bars(1..1) }} }}"#
        )
    };
    // automating pitch bends the running voice — differs from constant pitch
    let swept = fortelang::render_digest(&fortelang::compile_str(&song("automate pitch from 0.5 to 0.75 over bars(1..1)")).unwrap(), 1.0);
    let flat = fortelang::render_digest(&fortelang::compile_str(&song("")).unwrap(), 1.0);
    assert_ne!(swept.f32_digest, flat.f32_digest, "a pitch sweep must bend the sound");
    // and a CONSTANT pitch is bit-identical to no automation at all
    let const_pitch = fortelang::render_digest(&fortelang::compile_str(&song("automate pitch from 0.5 to 0.5 over bars(1..1)")).unwrap(), 1.0);
    assert_eq!(const_pitch.f32_digest, flat.f32_digest, "constant pitch must be a true no-op");

    // duck params are automatable (attack/release/amount)
    let dsong = |auto: &str| format!(
        r#"song "D" {{ tempo 128bpm
          track Kick {{ instrument sampler(sample: "Kick", sustain: 0.0) volume 0.0
            play beat`x--- x--- x--- x---` at bars(1..1) }}
          track Pad {{ instrument prisma(wave: "saw", sustain: 0.9)
            insert duck(from: Kick, amount: 0.9, release: 0.1)
            {auto}
            play notes`C3:4` at bars(1..1) }} }}"#);
    let a = fortelang::render_digest(&fortelang::compile_str(&dsong("automate duck.release from 0.05 to 0.9 over bars(1..1)")).unwrap(), 1.0);
    let b = fortelang::render_digest(&fortelang::compile_str(&dsong("")).unwrap(), 1.0);
    assert_ne!(a.f32_digest, b.f32_digest, "automating the duck release must change the carve");
}

#[test]
fn a_song_bounces_its_own_mix_of_placed_blocks() {
    // the mix-chop workflow: a block PLACES other blocks (which declare
    // their own internal bounce samples), the song bounces that whole mix
    // to one record and chops it — so a rest silences everything at once.
    // This requires machine-internal sample_lets to resolve BEFORE the
    // song-level bounce (collection order: blocks first, root last).
    let src = r#"
      block Machine {
        block MachineSrc {
          track K { instrument prisma(wave: "sine", cutoff: 0.5, sustain: 0.8)
            play notes`C2:1 _:3` at bars(1..1) }
        }
        sample MachineS = bounce(MachineSrc, note: C2, beats: 4)
        track Hit { instrument sampler(sample: MachineS, end: 0.667, sustain: 0.0, decay: 0.5)
          play beat`x--- x--- x--- x---` at bars(1..1) }
      }
      song "MixChop" { tempo 120bpm
        block Mix { play Machine at bars(1..1) }
        sample MixS = bounce(Mix, note: C3, beats: 4)
        track Cut { instrument sampler(sample: MixS, slices: 4, end: 0.667, choke: "on", sustain: 1.0, release: 0.05)
          play notes`C3~:1 _:1 D3~:0.5 _:1.5` at bars(1..1) }
      }"#;
    let p = fortelang::compile_str(src).expect("mix bounce of placed blocks must compile");
    let a = fortelang::render_digest(&p, 2.0);
    assert!(a.rms > 0.0005, "the chopped mix must sound (rms {})", a.rms);
    let b = fortelang::render_digest(&fortelang::compile_str(src).unwrap(), 2.0);
    assert_eq!(a.f32_digest, b.f32_digest, "mix bounce must be bit-stable");
}

#[test]
fn dig_samples_another_song_as_a_record() {
    // crate digging: a song renders ANOTHER SONG FILE to a sample and chops
    // it — key-fit with `semis`, window with `skip`/`beats`, and `end`
    // defaults to the musical edge (no tail-fraction magic).
    let dir = std::env::temp_dir().join(format!("forte-dig-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("record.forte"),
        r#"song "Record" { tempo 120bpm
  track A { instrument prisma(wave: "sine", cutoff: 0.5, sustain: 0.8)
    play notes`C2:1 E2:1 G2:1 C3:1` at bars(1..2) } }"#,
    )
    .unwrap();
    let song = r#"song "Digger" { tempo 100bpm
  sample Rec = dig("./record.forte", beats: 4, skip: 2)
  track Cut { instrument sampler(sample: Rec, slices: 4, choke: "on", sustain: 1.0, release: 0.05, semis: -5)
    play notes`C3~:1 _:1 D3~:0.5 _:1.5` at bars(1..1) } }"#;
    let dirs = dir.to_str().unwrap();
    let p = fortelang::compile_with_loader(song, &fortelang::FsLoader, dirs)
        .expect("dig song must compile");
    let a = fortelang::render_digest(&p, 2.0);
    assert!(a.rms > 0.0005, "the dug record must sound (rms {})", a.rms);
    let b = fortelang::render_digest(
        &fortelang::compile_with_loader(song, &fortelang::FsLoader, dirs).unwrap(),
        2.0,
    );
    assert_eq!(a.f32_digest, b.f32_digest, "dig must be bit-stable");

    // auto `end` = beats/(beats+2): the sampler's region ends at the
    // musical edge without the user computing tail fractions
    let cut = p.tracks.iter().find(|t| t.name == "Cut").expect("Cut track");
    let end = cut.devices[0].params[7];
    assert!((end - 4.0 / 6.0).abs() < 1e-6, "auto end must be beats/(beats+2), got {end}");
    // and semis: -5 lands on the pitch slot as 0.5 - 5/48
    let pitch = cut.devices[0].params[5];
    assert!((pitch - (0.5 - 5.0 / 48.0)).abs() < 1e-6, "semis must map to the pitch slot, got {pitch}");

    // a dig cycle reports E-DIG-002 instead of recursing forever
    std::fs::write(
        dir.join("a.forte"),
        r#"song "A" { tempo 120bpm
  sample R = dig("./b.forte", beats: 4)
  track T { instrument sampler(sample: R) play beat`x--- ---- ---- ----` at bars(1..1) } }"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("b.forte"),
        r#"song "B" { tempo 120bpm
  sample R = dig("./a.forte", beats: 4)
  track T { instrument sampler(sample: R) play beat`x--- ---- ---- ----` at bars(1..1) } }"#,
    )
    .unwrap();
    let cyc = std::fs::read_to_string(dir.join("a.forte")).unwrap();
    let errs = fortelang::check_with_loader(&cyc, &fortelang::FsLoader, dirs)
        .err()
        .expect("dig cycle must be an error");
    assert!(errs.iter().any(|d| d.code == "E-DIG-002" || d.code == "E-DIG-003"),
        "cycle must surface E-DIG-002/003, got {:?}",
        errs.iter().map(|d| d.code).collect::<Vec<_>>());
}

#[test]
fn stereo_survives_bounce_and_dig() {
    // the pressing keeps the field: a source whose stereo comes from its
    // effect returns must come out of bounce/dig with L != R, and a sampler
    // PLAYING that record must keep the difference in its own output.
    let dir = std::env::temp_dir().join(format!("forte-stereo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("wide.forte"),
        r#"song "Wide" { tempo 120bpm
  track A { instrument prisma(wave: "saw", cutoff: 0.6, sustain: 0.8)
    insert chorus(rate: 0.4, depth: 0.7, mix: 0.6)
    insert reverb(size: 0.7, decay: 0.6, mix: 0.4)
    play notes`C3:1 E3:1 G3:1 C4:1` at bars(1..1) } }"#,
    )
    .unwrap();
    let dirs = dir.to_str().unwrap();
    let src = std::fs::read_to_string(dir.join("wide.forte")).unwrap();
    let record = fortelang::compile_with_loader(&src, &fortelang::FsLoader, dirs).unwrap();
    let (_k, pressed) = fortelang::render_to_sample(&record, 2.0, 48);
    let r = pressed.right.as_ref().expect("bounce must keep both channels");
    let differs = pressed.data.iter().zip(r.iter()).filter(|(a, b)| a != b).count();
    assert!(differs > 1000, "the pressed record must be stereo ({differs} differing samples)");

    // and through a sampler: dig the record, play it, render the digger
    let digger = r#"song "Digger" { tempo 120bpm
  sample Rec = dig("./wide.forte", beats: 4)
  track Cut { instrument sampler(sample: Rec, sustain: 1.0, release: 0.1)
    play notes`C3~:4` at bars(1..1) } }"#;
    let p = fortelang::compile_with_loader(digger, &fortelang::FsLoader, dirs).unwrap();
    let (_k2, out) = fortelang::render_to_sample(&p, 2.0, 48);
    let r2 = out.right.as_ref().unwrap();
    let d2 = out.data.iter().zip(r2.iter()).filter(|(a, b)| a != b).count();
    assert!(d2 > 1000, "sampler playback must keep the field ({d2} differing samples)");
}

#[test]
fn master_chain_and_limiter_glue_the_mix() {
    // song-level inserts are the MASTER BUS: a limiter on the 2-bus caps
    // the summed peak; removing the chain is a bit-different (i.e. real) change
    let song = |chain: &str| {
        format!(
            r#"song "M" {{ tempo 120bpm
      master 2.5
      {chain}
      track A {{ instrument prisma(wave: "saw", cutoff: 0.7, sustain: 0.9)
        play notes`[C2 G2 C3]:1 [C2 G2 C3]:1 [C2 G2 C3]:1 [C2 G2 C3]:1` at bars(1..1) }} }}"#
        )
    };
    let plain = fortelang::compile_str(&song("")).unwrap();
    let glued = fortelang::compile_str(&song(r#"insert comp(thresh: 0.4, ratio: 0.7, makeup: 0.1)
      insert limiter(ceiling: 0.5, release: 0.3)"#)).unwrap();
    assert_eq!(glued.master_inserts.len(), 2, "song-level inserts land on the master bus");
    let a = fortelang::render_digest(&plain, 1.0);
    let b = fortelang::render_digest(&glued, 1.0);
    assert_ne!(a.f32_digest, b.f32_digest, "the master chain must change the sound");
    assert!(b.peak <= 0.52, "the limiter must cap the bus (peak {})", b.peak);
    assert!(a.peak > 0.6, "the unglued mix should exceed the ceiling (peak {})", a.peak);
}

#[test]
fn warp_bars_and_semis_automation_speak_music() {
    // dig(bars: 2..2) windows by the SOURCE's bars; warp: "on" time-stretches
    // the record to the SONG's tempo; automate semis rides pitch in semitones
    let dir = std::env::temp_dir().join(format!("forte-warp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rec.forte"),
        r#"song "Rec" { tempo 120bpm
  track A { instrument prisma(wave: "sine", cutoff: 0.5, sustain: 0.8)
    play notes`C2:1 E2:1 G2:1 C3:1` at bars(1..2) } }"#,
    )
    .unwrap();
    let digger = r#"song "W" { tempo 90bpm
  sample Rec = dig("./rec.forte", bars: 2..2)
  track Cut { instrument sampler(sample: Rec, warp: "on", sustain: 1.0, release: 0.1)
    automate semis from 0 to -5 over bars(1..2)
    play notes`C3~:4` at bars(1..1) } }"#;
    let p = fortelang::compile_with_loader(digger, &fortelang::FsLoader, dir.to_str().unwrap())
        .expect("warp/bars/semis song must compile");
    let cut = p.tracks.iter().find(|t| t.name == "Cut").unwrap();
    // bars: 2..2 of a 4/4 source = a 4-beat window -> auto end = 4/6
    let end = cut.devices[0].params[7];
    assert!((end - 4.0 / 6.0).abs() < 1e-6, "bars window must drive auto end, got {end}");
    // warp: stretch = 0.5 * song/record = 0.5 * 90/120 = 0.375
    let stretch = cut.devices[0].params[14];
    assert!((stretch - 0.375).abs() < 1e-6, "warp must tempo-sync the record, got {stretch}");
    // automate semis 0 -> -5 lands on the pitch slot as 0.5 -> 0.5 - 5/48
    let lane = cut
        .param_automation
        .iter()
        .find(|pa| pa.device == 0 && pa.param == 5)
        .expect("semis automation must target the pitch slot");
    assert!((lane.points[0].value - 0.5).abs() < 1e-6);
    assert!((lane.points[1].value - (0.5 - 5.0 / 48.0)).abs() < 1e-6);
}

#[test]
fn dig_windows_by_source_section_name() {
    // dig(section: "drop") grabs the source's own named bars — rearranging
    // the record moves the window with it
    let dir = std::env::temp_dir().join(format!("forte-digsec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rec.forte"),
        r#"song "Rec" { tempo 120bpm
  section drop = bars(2..2)
  track A { instrument prisma(wave: "sine", cutoff: 0.5, sustain: 0.8)
    play notes`C2:1 E2:1 G2:1 C3:1` at bars(1..2) } }"#,
    )
    .unwrap();
    let dirs = dir.to_str().unwrap();
    let digger = r#"song "S" { tempo 120bpm
  sample Rec = dig("./rec.forte", section: "drop")
  track Cut { instrument sampler(sample: Rec, sustain: 1.0, release: 0.1)
    play notes`C3~:4` at bars(1..1) } }"#;
    let p = fortelang::compile_with_loader(digger, &fortelang::FsLoader, dirs).unwrap();
    let cut = p.tracks.iter().find(|t| t.name == "Cut").unwrap();
    // section drop = one 4/4 bar = 4 beats -> auto end 4/6
    let end = cut.devices[0].params[7];
    assert!((end - 4.0 / 6.0).abs() < 1e-6, "section window must drive auto end, got {end}");
    // unknown section names are E-DIG-005 with the available list
    let bad = digger.replace("\"drop\"", "\"nosuch\"");
    let errs = fortelang::check_with_loader(&bad, &fortelang::FsLoader, dirs).err().unwrap();
    assert!(errs.iter().any(|d| d.code == "E-DIG-005"));
}

#[test]
fn space_reverb_characters_and_decay_are_real() {
    // the new-generation reverb: three characters, decay that actually
    // scales the tail, deterministic to the bit
    let song = |args: &str| {
        format!(
            r#"song "V" {{ tempo 120bpm
      track A {{ instrument prisma(wave: "saw", cutoff: 0.6, sustain: 0.8)
        insert space({args})
        play notes`C3:0.5 _:3.5` at bars(1..1) }} }}"#
        )
    };
    let render = |args: &str| {
        let p = fortelang::compile_str(&song(args)).expect("space song must compile");
        fortelang::render_digest(&p, 6.0)
    };
    let room = render(r#"type: "room", decay: 0.5, mix: 0.5"#);
    let hall = render(r#"type: "hall", decay: 0.5, mix: 0.5"#);
    let hall2 = render(r#"type: "hall", decay: 0.5, mix: 0.5"#);
    assert_eq!(hall.f32_digest, hall2.f32_digest, "space must be deterministic");
    assert_ne!(room.f32_digest, hall.f32_digest, "characters must differ");
    // decay scales the audible tail: long-decay render carries much more
    // energy than short-decay once the dry note is gone
    let short = render(r#"type: "hall", decay: 0.1, mix: 0.5"#);
    let long = render(r#"type: "hall", decay: 0.95, mix: 0.5"#);
    assert!(long.rms > short.rms * 1.05, "decay must lengthen the tail (short {} long {})", short.rms, long.rms);
}

#[test]
fn vcf_is_an_analog_character_filter() {
    // one acid voice, permuted through the vcf's promises. The envelope
    // gates the INPUT (a 0.2 s ping) and the filter output goes straight
    // out while the note holds — so a self-oscillating filter is free to
    // keep singing after its excitation dies.
    let song = |vcf: &str| {
        format!(
            r#"device A : Instrument {{
  node e = adsr(a: 0.005, d: 0.2, s: 0.0, r: 0.05)
  node src = osc(shape: "saw")
  node o = gain(in: src, mod: e)
  node f = {vcf}
  out f
}}
song "V" {{ tempo 120bpm
  track T {{ instrument A()
    play notes`[C2 E2 G2]~:4` at bars(1..1) }} }}"#
        )
    };
    let dig = |vcf: &str| {
        let p = fortelang::compile_str(&song(vcf)).expect("vcf song must compile");
        fortelang::render_digest(&p, 4.0)
    };
    let base = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.3)"#);
    let base2 = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.3)"#);
    assert_eq!(base.f32_digest, base2.f32_digest, "vcf must be deterministic");
    // ladder and svf are different filters
    let svf_mode = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.3, mode: "svf")"#);
    assert_ne!(base.f32_digest, svf_mode.f32_digest, "modes must differ");
    // self-oscillation: at the top of the reso range the filter keeps
    // singing long after the 0.5-beat ping is gone
    let singing = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.98)"#);
    assert!(
        singing.rms > base.rms * 1.5,
        "high reso must self-oscillate into the tail (rms {} vs {})",
        singing.rms,
        base.rms
    );
    // keytracking and per-voice drift both change the audio
    let tracked = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.3, track: 1.0)"#);
    assert_ne!(base.f32_digest, tracked.f32_digest, "keytracking must engage");
    let drifted = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.3, drift: 0.8)"#);
    assert_ne!(base.f32_digest, drifted.f32_digest, "per-voice drift must engage");
    let drifted2 = dig(r#"vcf(in: o, cutoff: 0.5, reso: 0.3, drift: 0.8)"#);
    assert_eq!(drifted.f32_digest, drifted2.f32_digest, "drift is deterministic");
}

#[test]
fn nonlinear_effects_take_an_os_switch() {
    // `os: "off"/"2x"/"4x"` on the four waveshaping inserts; off is the
    // default and stays on the legacy bit-exact path
    let song = |inserts: &str| {
        format!(
            r#"song "O" {{ tempo 120bpm
      track A {{ instrument prisma(wave: "saw", cutoff: 0.9, sustain: 0.8)
        {inserts}
        play notes`C5:2 _:2` at bars(1..1) }} }}"#
        )
    };
    let p = fortelang::compile_str(&song(
        r#"insert saturate(mode: "fuzz", drive: 0.8, os: "4x")
        insert crush(bits: 0.6, os: "2x")
        insert parcomp(amount: 0.5, os: "4x")
        insert drive(amount: 0.4, os: "2x")"#,
    ))
    .expect("os switch must compile");
    let chain = &p.tracks[0].devices;
    // switch choices land as their raw index: off/2x/4x → 0/1/2
    assert_eq!(chain[1].params[4], 2.0, "saturate os 4x");
    assert_eq!(chain[2].params[3], 1.0, "crush os 2x");
    assert_eq!(chain[3].params[3], 2.0, "parcomp os 4x");
    assert_eq!(chain[4].params[1], 1.0, "drive os 2x");
    // "off" renders bit-identically to leaving os out entirely…
    let plain = fortelang::compile_str(&song(r#"insert saturate(mode: "fuzz", drive: 0.9)"#)).unwrap();
    let off = fortelang::compile_str(&song(r#"insert saturate(mode: "fuzz", drive: 0.9, os: "off")"#)).unwrap();
    let d_plain = fortelang::render_digest(&plain, 3.0);
    let d_off = fortelang::render_digest(&off, 3.0);
    assert_eq!(d_plain.f32_digest, d_off.f32_digest, "os off must be the legacy path");
    // …while 4x actually changes the audio (the aliasing is gone)
    let hq = fortelang::compile_str(&song(r#"insert saturate(mode: "fuzz", drive: 0.9, os: "4x")"#)).unwrap();
    let d_hq = fortelang::render_digest(&hq, 3.0);
    assert_ne!(d_plain.f32_digest, d_hq.f32_digest, "os 4x must engage");
}
