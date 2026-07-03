//! REPL session semantics + a full pipe-driven run of the real binary.

use std::io::Write;
use std::process::{Command, Stdio};

use fortelang::repl::{Action, Session};

#[test]
fn session_accumulates_and_renders_compilable_source() {
    let mut s = Session::default();
    assert!(matches!(s.eval(":tempo 140"), Action::Play(_)));
    assert!(matches!(s.eval("let theme = prog`Am | F | C | G`"), Action::Msg(_)));
    assert!(matches!(s.eval(":fx reverb(mix: 0.3)"), Action::Play(_)));
    let Action::Play(src) = s.eval("arp(theme, rate: 0.25, style: \"up\")") else {
        panic!("pattern must play");
    };
    let p = fortelang::compile_str(&src).expect("repl source must compile");
    assert_eq!(p.tempo, 140.0);

    // a bad instrument rolls back instead of poisoning the session
    let before = s.instrument.clone();
    assert!(matches!(s.eval(":inst nosuchsynth()"), Action::Msg(_)));
    assert_eq!(s.instrument, before);
    assert!(matches!(s.eval("theme"), Action::Play(_)), "session still usable");
}

#[test]
fn repl_binary_end_to_end() {
    let out = std::env::temp_dir().join(format!("forte-repl-jam-{}.forte", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let script = format!(
        ":tempo 132\nbeat`x-x- x--x`\ndevice Bloop : Instrument {{\n  node o = osc(shape: \"square\")\n  out gain(in: o, mod: adsr())\n}}\n:inst Bloop()\nnotes`C4:1/2 G4:1/2`\n:save {}\n:quit\n",
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
    assert!(stdout.contains("saved:"), "{stdout}");

    // the jam is a real song
    let saved = std::fs::read_to_string(&out).unwrap();
    let p = fortelang::compile_str(&saved).expect("saved jam must compile");
    assert_eq!(p.tempo, 132.0);
    assert!(saved.contains("device Bloop"));
}
