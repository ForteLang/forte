//! `forte init` / `forte package add` — the project-as-package layout (#57):
//! flat vendoring, nested-packages exclusion, requires hoisting, package.lock.

use std::path::Path;
use std::process::Command;

fn forte(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_forte"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run forte");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn init_and_flat_package_add() {
    let base = std::env::temp_dir().join(format!("forte-pkg-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    // upstream dependency package
    let dep = base.join("groove-kit");
    std::fs::create_dir_all(dep.join("blocks")).unwrap();
    std::fs::write(
        dep.join("package.forte"),
        "block GrooveKit {\n  desc \"Drum grooves.\"\n  version \"0.2.0\"\n}\n",
    )
    .unwrap();

    // upstream main package: requires the dependency, and carries its own
    // vendored packages/ + .forte/ that must NOT be copied to consumers
    let main = base.join("cool-synths");
    std::fs::create_dir_all(main.join("blocks")).unwrap();
    std::fs::create_dir_all(main.join("packages").join("junk_0.0.1")).unwrap();
    std::fs::create_dir_all(main.join(".forte")).unwrap();
    std::fs::write(main.join("blocks").join("lead.forte"), "// lead blocks\n").unwrap();
    std::fs::write(
        main.join("package.forte"),
        format!(
            "block CoolSynths {{\n  desc \"Lead synth blocks.\"\n  version \"1.0.0\"\n  requires \"{}\"\n}}\n",
            dep.display()
        ),
    )
    .unwrap();

    // `forte init NAME` scaffolds a distributable project
    let (ok, out, err) = forte(&base, &["init", "my-album"]);
    assert!(ok, "init failed: {err}");
    assert!(out.contains("my-album"), "init output: {out}");
    let proj = base.join("my-album");
    for p in ["package.forte", "blocks", "songs", "packages", ".forte"] {
        assert!(proj.join(p).exists(), "init should create {p}");
    }
    let meta = std::fs::read_to_string(proj.join("package.forte")).unwrap();
    assert!(meta.contains("block MyAlbum"), "PascalCase meta block: {meta}");
    assert!(meta.contains("version \"0.1.0\""), "meta: {meta}");

    // `forte package add <local path>` vendors flat + hoists requires
    let (ok, out, err) = forte(&proj, &["package", "add", &main.display().to_string()]);
    assert!(ok, "package add failed: {err}");
    assert!(out.contains("coolsynths_1.0.0"), "add output: {out}");

    let vendored = proj.join("packages").join("coolsynths_1.0.0");
    assert!(vendored.join("package.forte").exists());
    assert!(vendored.join("blocks").join("lead.forte").exists());
    // a distributed package never brings its own packages/ or VCS state
    assert!(!vendored.join("packages").exists(), "nested packages/ must be excluded");
    assert!(!vendored.join(".forte").exists(), ".forte must be excluded");
    // requires hoisted into the SAME flat packages/
    assert!(
        proj.join("packages").join("groovekit_0.2.0").join("package.forte").exists(),
        "requires must hoist flat"
    );

    // package.lock records both, sorted by name
    let lock = std::fs::read_to_string(proj.join("package.lock")).unwrap();
    let entries: Vec<serde_json::Value> = serde_json::from_str(&lock).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["coolsynths", "groovekit"], "lock: {lock}");
    assert_eq!(entries[0]["version"], "1.0.0");

    // re-add is idempotent (skip, not duplicate)
    let (ok, out, _) = forte(&proj, &["package", "add", &main.display().to_string()]);
    assert!(ok);
    assert!(out.contains("skip"), "second add should skip: {out}");

    // `forte package list` shows name/version/desc
    let (ok, out, _) = forte(&proj, &["package", "list"]);
    assert!(ok);
    assert!(out.contains("coolsynths 1.0.0"), "list: {out}");
    assert!(out.contains("Lead synth blocks."), "list: {out}");

    // `forte package verify`: clean tree matches the lock digests
    let (ok, out, err) = forte(&proj, &["package", "verify"]);
    assert!(ok, "verify: {err}");
    assert!(out.contains("OK      : packages/coolsynths_1.0.0"), "verify: {out}");

    // tampering with a vendored file is caught
    std::fs::write(vendored.join("blocks").join("lead.forte"), "// tampered\n").unwrap();
    let (ok, out, err) = forte(&proj, &["package", "verify"]);
    assert!(!ok, "tampered verify must fail");
    assert!(out.contains("MISMATCH: packages/coolsynths_1.0.0"), "verify: {out} / {err}");

    // a vendored dir the lock doesn't know about is reported
    std::fs::create_dir_all(proj.join("packages").join("rogue_9.9.9")).unwrap();
    let (_, out, _) = forte(&proj, &["package", "verify"]);
    assert!(out.contains("unlocked"), "verify: {out}");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn search_rendering_speaks_the_add_command() {
    // a captured GitHub search response shape
    let json = r#"{"total_count":2,"items":[
      {"full_name":"someone/tb303-mods","description":"303 variants","stargazers_count":42},
      {"full_name":"other/ambient-kit","description":null,"stargazers_count":0}
    ]}"#;
    let out = fortelang::package::render_search(json).unwrap();
    assert!(out.contains("someone/tb303-mods  ★42"), "{out}");
    assert!(out.contains("forte package add github:someone/tb303-mods"), "{out}");
    assert!(out.contains("other/ambient-kit  ★0"), "{out}");

    // rate-limit / error responses surface GitHub's own words
    let err = fortelang::package::render_search(r#"{"message":"API rate limit exceeded"}"#);
    assert!(err.is_err() && err.unwrap_err().contains("rate limit"));

    // empty result set is a sentence, not silence
    let none = fortelang::package::render_search(r#"{"total_count":0,"items":[]}"#).unwrap();
    assert!(none.contains("ありません"), "{none}");
}
