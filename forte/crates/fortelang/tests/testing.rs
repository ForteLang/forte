//! `forte test` — the digest-locked regression runner. Determinism makes
//! "the music didn't change" a testable fact: this exercises the whole
//! lifecycle (new → --update → locked ok → sound change fails), the
//! expect-error directive, and compile-failure reporting.

use std::fs;

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("forte-testcmd-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

const TINY: &str = r#"song "Tiny" {
  tempo 120bpm
  key A minor
  track A {
    instrument prisma(wave: "saw", cutoff: 0.4)
    play notes`A2:1 C3:1 E3:1 A2:1` at bars(1..1)
  }
}"#;

#[test]
fn digest_lock_lifecycle() {
    let dir = scratch("lifecycle");
    fs::write(dir.join("tiny.forte"), TINY).unwrap();
    let path = vec![dir.to_string_lossy().into_owned()];

    // no lock yet: the song is NEW, which is not a failure
    assert_eq!(fortelang::testing::run(&path, false), 0);
    assert!(!dir.join("forte-test.lock").is_file(), "no lock without --update");

    // --update records the digest
    assert_eq!(fortelang::testing::run(&path, true), 0);
    let lock = fs::read_to_string(dir.join("forte-test.lock")).unwrap();
    assert!(lock.contains("tiny.forte"), "{lock}");

    // locked and unchanged: ok
    assert_eq!(fortelang::testing::run(&path, false), 0);

    // the sound changes → the run fails
    fs::write(dir.join("tiny.forte"), TINY.replace("cutoff: 0.4", "cutoff: 0.8")).unwrap();
    assert_eq!(fortelang::testing::run(&path, false), 1);

    // --update accepts the new sound, then it passes again
    assert_eq!(fortelang::testing::run(&path, true), 0);
    assert_eq!(fortelang::testing::run(&path, false), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn expect_error_directive_and_compile_failures() {
    let dir = scratch("expect");
    // a file that must fail with a specific code — passes
    fs::write(
        dir.join("bad-param.forte"),
        "// expect-error: E-DEV-002\nsong \"B\" { tempo 120bpm track A { instrument prisma(cutof: 0.4) play beat`x---` at bars(1..1) } }",
    )
    .unwrap();
    let path = vec![dir.to_string_lossy().into_owned()];
    assert_eq!(fortelang::testing::run(&path, false), 0);

    // expecting the wrong code fails the run
    fs::write(
        dir.join("wrong-code.forte"),
        "// expect-error: E-TIME-003\nsong \"W\" { tempo 120bpm track A { instrument prisma(cutof: 0.4) play beat`x---` at bars(1..1) } }",
    )
    .unwrap();
    assert_eq!(fortelang::testing::run(&path, false), 1);
    fs::remove_file(dir.join("wrong-code.forte")).unwrap();

    // a plain compile error (no directive) fails the run
    fs::write(dir.join("broken.forte"), "song \"X\" { tempo 120bpm track A { play beat`x---` at bars(1..1) } }")
        .unwrap();
    assert_eq!(fortelang::testing::run(&path, false), 1);
    let _ = fs::remove_dir_all(&dir);
}
