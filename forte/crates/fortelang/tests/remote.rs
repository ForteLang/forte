//! `forte remote add` / `forte push` / `forte pull` — distribution IS GitHub
//! (issue #52). A local bare repo stands in for github.com.

use std::path::Path;
use std::process::Command;

fn forte(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_forte"))
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run forte");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn remote_add_push_pull_roundtrip() {
    let base = std::env::temp_dir().join(format!("forte-remote-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    // "GitHub": a bare repository
    let origin = base.join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "."]);

    // author project, scaffolded by forte init
    let (ok, _, err) = forte(&base, &["init", "my-album"]);
    assert!(ok, "init: {err}");
    let proj = base.join("my-album");
    std::fs::write(
        proj.join("blocks").join("riff.forte"),
        "block Riff {\n  desc \"One riff.\"\n  track A {\n    instrument prisma()\n    play notes`C4:1` at bars(1..1)\n  }\n}\n",
    )
    .unwrap();

    // no remote yet → push refuses with guidance
    let (ok, _, err) = forte(&proj, &["push"]);
    assert!(!ok, "push without remote must fail");
    assert!(err.contains("remote"), "guidance: {err}");

    let (ok, out, err) = forte(&proj, &["remote", "add", &origin.display().to_string()]);
    assert!(ok, "remote add: {err}");
    assert!(out.contains("origin ="), "remote add output: {out}");

    let (ok, out, err) = forte(&proj, &["push", "-m", "first release"]);
    assert!(ok, "push: {err}");
    assert!(out.contains("pushed"), "push output: {out}");

    // a consumer clones the published project and finds everything
    let clone = base.join("listener");
    git(&base, &["clone", &origin.display().to_string(), "listener"]);
    assert!(clone.join("package.forte").exists(), "meta travels");
    assert!(clone.join("blocks").join("riff.forte").exists(), "blocks travel");
    assert_eq!(git(&clone, &["log", "-1", "--format=%s"]), "first release");

    // upstream gains a commit; forte pull brings it in
    std::fs::write(clone.join("blocks").join("extra.forte"), "// more\n").unwrap();
    git(&clone, &["add", "-A"]);
    git(&clone, &["commit", "-m", "extra"]);
    git(&clone, &["push", "origin", "HEAD"]);

    let (ok, out, err) = forte(&proj, &["pull"]);
    assert!(ok, "pull: {err}");
    assert!(out.contains("pulled"), "pull output: {out}");
    assert!(proj.join("blocks").join("extra.forte").exists(), "pull brings new files");

    // pushing with no changes is a no-op commit-wise but still succeeds
    let (ok, _, err) = forte(&proj, &["push"]);
    assert!(ok, "idempotent push: {err}");

    let _ = std::fs::remove_dir_all(&base);
}
