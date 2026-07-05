//! REPL session semantics (multi-track layering, transactional mutations,
//! undo) + a full pipe-driven run of the real binary.

use std::io::Write;
use std::process::{Command, Stdio};

use fortelang::repl::{Action, Session};

fn eval(s: &mut Session, undo: &mut Vec<Session>, input: &str) -> Action {
    s.eval(input, undo)
}

#[test]
fn layering_builds_a_multitrack_song() {
    let mut s = Session::default();
    let mut undo = Vec::new();
    assert!(matches!(eval(&mut s, &mut undo, ":tempo 140"), Action::Play(_)));
    assert!(matches!(eval(&mut s, &mut undo, "beat`x--- x-x-`"), Action::Play(_)));
    assert!(matches!(eval(&mut s, &mut undo, ":track Bass"), Action::Msg(_)));
    assert!(matches!(eval(&mut s, &mut undo, ":inst prisma(wave: \"saw\", sub: 0.8)"), Action::Play(_)));
    assert!(matches!(eval(&mut s, &mut undo, "notes`C2:1 G2:1`"), Action::Play(_)));
    assert!(matches!(eval(&mut s, &mut undo, ":vol 0.7"), Action::Play(_)));

    let p = fortelang::compile_str(&s.source()).expect("layered session must compile");
    assert_eq!(p.tracks.len(), 2, "Main + Bass");
    assert_eq!(p.tempo, 140.0);
    let bass = p.tracks.iter().find(|t| t.name == "Bass").unwrap();
    assert_eq!(bass.volume, 0.7);
}

#[test]
fn mutations_are_transactional_and_undoable() {
    let mut s = Session::default();
    let mut undo = Vec::new();
    eval(&mut s, &mut undo, "beat`x---`");

    // a bad instrument never lands in the session
    let before = s.tracks[0].instrument.clone();
    assert!(matches!(eval(&mut s, &mut undo, ":inst nosuchsynth()"), Action::Msg(_)));
    assert_eq!(s.tracks[0].instrument, before);

    // undo rewinds the pattern
    eval(&mut s, &mut undo, "beat`xxxx`");
    assert_eq!(s.tracks[0].pattern.as_deref(), Some("beat`xxxx`"));
    assert!(matches!(eval(&mut s, &mut undo, ":undo"), Action::Play(_)));
    assert_eq!(s.tracks[0].pattern.as_deref(), Some("beat`x---`"));

    // :drop removes a track, and is itself undoable
    eval(&mut s, &mut undo, ":track Lead");
    eval(&mut s, &mut undo, "notes`C5:1`");
    assert_eq!(s.tracks.len(), 2);
    eval(&mut s, &mut undo, ":drop Lead");
    assert_eq!(s.tracks.len(), 1);
    eval(&mut s, &mut undo, ":undo");
    assert_eq!(s.tracks.len(), 2, "undo restores the dropped track");
}

#[test]
fn repl_binary_end_to_end() {
    let out = std::env::temp_dir().join(format!("forte-repl-jam-{}.forte", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let script = format!(
        ":tempo 132\nbeat`x-x- x--x`\n:track Bloopy\ndevice Bloop : Instrument {{\n  node o = osc(shape: \"square\")\n  out gain(in: o, mod: adsr())\n}}\n:inst Bloop()\nnotes`C4:1/2 G4:1/2`\n:tracks\n:save {}\n:quit\n",
        out.display()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_forte"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn repl");
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("♪ playing"), "{stdout}");
    assert!(stdout.contains("2 tracks"), "layered playback: {stdout}");
    assert!(stdout.contains("saved:"), "{stdout}");

    // the jam is a real two-track song with the user's device
    let saved = std::fs::read_to_string(&out).unwrap();
    let p = fortelang::compile_str(&saved).expect("saved jam must compile");
    assert_eq!(p.tracks.len(), 2);
    assert!(saved.contains("device Bloop"));
    assert!(saved.contains("track Bloopy"));
}
