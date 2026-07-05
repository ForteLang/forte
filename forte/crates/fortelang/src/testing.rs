//! `forte test` — regression tests for songs and libraries, in sound.
//!
//! Because builds are deterministic, "the music didn't change" is a testable
//! fact: every renderable file (a song, or a block library whose last block
//! is the root) gets a build digest, locked in a `forte-test.lock` next to
//! the tested root. A later run fails if any digest moved.
//!
//!   forte test songs/            # compare against songs/forte-test.lock
//!   forte test songs/ --update   # record the current digests as expected
//!
//! Compile-error expectations: a file whose header carries
//! `// expect-error: E-XXX-000` MUST fail to compile with that code — this is
//! how error paths (bad params, broken imports) stay tested as the language
//! evolves. Device libraries are compile-checked (they render nothing).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{check_with_loader, render_digest, Checked, FsLoader};

const LOCK: &str = "forte-test.lock";
const TAIL_BEATS: f64 = 8.0; // identical to `forte build` — digests line up

struct Outcome {
    ok: usize,
    new: usize,
    fail: usize,
}

/// Run tests over the given paths (files or directories; directories are
/// walked recursively). Returns a process exit code.
pub fn run(paths: &[String], update: bool) -> i32 {
    let paths: Vec<String> = if paths.is_empty() { vec![".".into()] } else { paths.to_vec() };
    let mut total = Outcome { ok: 0, new: 0, fail: 0 };
    for arg in &paths {
        let p = Path::new(arg);
        let (root, files) = if p.is_dir() {
            (p.to_path_buf(), collect(p))
        } else if p.is_file() {
            (p.parent().unwrap_or(Path::new(".")).to_path_buf(), vec![p.to_path_buf()])
        } else {
            eprintln!("test: '{arg}' が見つかりません");
            total.fail += 1;
            continue;
        };
        run_root(&root, &files, update, &mut total);
    }
    println!(
        "結果: {} ok, {} new, {} fail{}",
        total.ok,
        total.new,
        total.fail,
        if total.new > 0 && !update { "(new は --update で forte-test.lock に記録)" } else { "" }
    );
    if total.fail > 0 {
        1
    } else {
        0
    }
}

fn run_root(root: &Path, files: &[PathBuf], update: bool, total: &mut Outcome) {
    println!("== forte test: {} ==", root.display());
    let lock_path = root.join(LOCK);
    let mut lock: BTreeMap<String, String> = std::fs::read_to_string(&lock_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut changed = false;

    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                println!("   FAIL  {rel} — 読めません: {e}");
                total.fail += 1;
                continue;
            }
        };
        let base = f.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();

        // error-expectation tests: the file must fail with the stated code
        if let Some(code) = expect_error(&src) {
            match check_with_loader(&src, &FsLoader, &base) {
                Err(diags) if diags.iter().any(|d| d.code == code) => {
                    println!("   ok    {rel}({code} を期待どおり検出)");
                    total.ok += 1;
                }
                Err(diags) => {
                    let got: Vec<&str> = diags.iter().map(|d| d.code).collect();
                    println!("   FAIL  {rel} — {code} を期待しましたが {} でした", got.join(", "));
                    total.fail += 1;
                }
                Ok(_) => {
                    println!("   FAIL  {rel} — {code} を期待しましたがコンパイルが通りました");
                    total.fail += 1;
                }
            }
            continue;
        }

        // normal files: compile; renderable roots get a digest lock
        let project = match check_with_loader(&src, &FsLoader, &base) {
            Ok(Checked::Song(p)) => Some(p),
            Ok(Checked::BlockLibrary { root, .. }) => Some(*root),
            Ok(Checked::DeviceLibrary { devices }) => {
                println!("   ok    {rel}(device library, {devices} devices)");
                total.ok += 1;
                None
            }
            Err(diags) => {
                println!("   FAIL  {rel} — コンパイルエラー {} 件({})", diags.len(), diags[0]);
                total.fail += 1;
                None
            }
        };
        let Some(project) = project else { continue };
        let digest = format!("{:016x}", render_digest(&project, TAIL_BEATS).f32_digest);
        match lock.get(&rel) {
            Some(want) if *want == digest => {
                println!("   ok    {rel}  {digest}");
                total.ok += 1;
            }
            Some(want) => {
                if update {
                    println!("   UPD   {rel}  {want} → {digest}");
                    lock.insert(rel, digest);
                    changed = true;
                    total.ok += 1;
                } else {
                    println!("   FAIL  {rel} — 音が変わっています: 期待 {want}, 実際 {digest}");
                    total.fail += 1;
                }
            }
            None => {
                if update {
                    println!("   NEW   {rel}  {digest}(記録しました)");
                    lock.insert(rel, digest);
                    changed = true;
                    total.ok += 1;
                } else {
                    println!("   NEW   {rel}  {digest}");
                    total.new += 1;
                }
            }
        }
    }

    if changed {
        match serde_json::to_string_pretty(&lock) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&lock_path, json + "\n") {
                    eprintln!("test: {} を書けません: {e}", lock_path.display());
                    total.fail += 1;
                } else {
                    println!("   lock: {} を更新しました", lock_path.display());
                }
            }
            Err(e) => eprintln!("test: lock の生成に失敗: {e}"),
        }
    }
}

/// `// expect-error: E-XXX-000` in the file header (first 20 lines).
fn expect_error(src: &str) -> Option<String> {
    for line in src.lines().take(20) {
        if let Some(rest) = line.trim().strip_prefix("// expect-error:") {
            let code = rest.trim();
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }
    None
}

/// Recursively collect .forte files, deterministic order, skipping hidden
/// dirs, build output and vendored node_modules.
fn collect(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if p.is_dir() {
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    stack.push(p);
                }
            } else if name.ends_with(".forte") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}
