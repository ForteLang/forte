//! `forte package update` — pip's ergonomics on fork semantics (#5):
//! pristine copies are replaced, locally-modified copies get a THREE-WAY
//! merge against the commit recorded at add time, conflicts abort cleanly,
//! and every update prints a semantic (musical) diff.

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

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

const RIFF_V1: &str = "block Riff {\n  track A {\n    instrument prisma(wave: \"saw\")\n    play beat`x---` at bars(1..1)\n  }\n}\n";

#[test]
fn update_replaces_merges_and_conflicts() {
    let base = std::env::temp_dir().join(format!("forte-upd-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    // upstream: a git-hosted package at v0.1.0
    let up = base.join("kit");
    std::fs::create_dir_all(up.join("blocks")).unwrap();
    std::fs::write(up.join("package.forte"), "block Kit {\n  desc \"Riffs.\"\n  version \"0.1.0\"\n}\n").unwrap();
    std::fs::write(up.join("blocks/riff.forte"), RIFF_V1).unwrap();
    git(&up, &["init", "-q"]);
    git(&up, &["add", "-A"]);
    git(&up, &["commit", "-q", "-m", "v0.1.0"]);
    let src = format!("file://{}", up.display());

    // consumer vendors it
    let proj = base.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let (ok, out, err) = forte(&proj, &["package", "add", &src]);
    assert!(ok, "{out}\n{err}");
    assert!(proj.join("packages/kit_0.1.0/blocks/riff.forte").is_file());

    // upstream moves to 0.2.0 (pattern change)
    std::fs::write(up.join("package.forte"), "block Kit {\n  desc \"Riffs.\"\n  version \"0.2.0\"\n}\n").unwrap();
    std::fs::write(up.join("blocks/riff.forte"), RIFF_V1.replace("x---", "x-x-")).unwrap();
    git(&up, &["add", "-A"]);
    git(&up, &["commit", "-q", "-m", "v0.2.0"]);

    // pristine update: straight replacement + lock moves to 0.2.0
    let (ok, out, err) = forte(&proj, &["package", "update", "kit"]);
    assert!(ok, "{out}\n{err}");
    assert!(out.contains("聴けるレビュー"), "semantic diff must be shown:\n{out}");
    assert!(proj.join("packages/kit_0.2.0/blocks/riff.forte").is_file());
    assert!(!proj.join("packages/kit_0.1.0").exists(), "old dir must be gone");
    let lock = std::fs::read_to_string(proj.join("package.lock")).unwrap();
    assert!(lock.contains("\"0.2.0\""), "{lock}");
    let (ok, out, err) = forte(&proj, &["package", "verify"]);
    assert!(ok, "verify after update: {out}\n{err}");

    // the consumer forks the riff locally (adds a track — separate region)
    let vendored = proj.join("packages/kit_0.2.0/blocks/riff.forte");
    let local = std::fs::read_to_string(&vendored).unwrap().replace(
        "play beat`x-x-` at bars(1..1)\n  }",
        "play beat`x-x-` at bars(1..1)\n    volume 0.8\n  }",
    );
    std::fs::write(&vendored, &local).unwrap();

    // upstream moves to 0.3.0 touching a DIFFERENT file
    std::fs::write(up.join("package.forte"), "block Kit {\n  desc \"More riffs.\"\n  version \"0.3.0\"\n}\n").unwrap();
    git(&up, &["add", "-A"]);
    git(&up, &["commit", "-q", "-m", "v0.3.0"]);

    // dirty update: three-way merge keeps the local edit AND takes upstream
    let (ok, out, err) = forte(&proj, &["package", "update", "kit"]);
    assert!(ok, "{out}\n{err}");
    assert!(out.contains("3方マージ"), "{out}");
    let merged = std::fs::read_to_string(proj.join("packages/kit_0.3.0/blocks/riff.forte")).unwrap();
    assert!(merged.contains("volume 0.8"), "local edit must survive:\n{merged}");
    let meta = std::fs::read_to_string(proj.join("packages/kit_0.3.0/package.forte")).unwrap();
    assert!(meta.contains("More riffs."), "upstream change must arrive:\n{meta}");

    // conflict: both sides edit the same line differently → clean abort
    let vendored = proj.join("packages/kit_0.3.0/blocks/riff.forte");
    std::fs::write(&vendored, merged.replace("x-x-", "X-x-")).unwrap();
    std::fs::write(up.join("blocks/riff.forte"), RIFF_V1.replace("x---", "xxxx")).unwrap();
    std::fs::write(up.join("package.forte"), "block Kit {\n  desc \"More riffs.\"\n  version \"0.4.0\"\n}\n").unwrap();
    git(&up, &["add", "-A"]);
    git(&up, &["commit", "-q", "-m", "v0.4.0"]);
    let (ok, out, err) = forte(&proj, &["package", "update", "kit"]);
    assert!(!ok, "conflicting update must fail:\n{out}");
    assert!(err.contains("衝突") || out.contains("衝突"), "{out}\n{err}");
    assert!(proj.join("packages/kit_0.3.0").exists(), "no half-updated state");

    let _ = std::fs::remove_dir_all(&base);
}
