//! `forte package` — the acquisition side of the ecosystem (issue #52/#57).
//!
//! A project made by `forte init` IS a distributable package; consumers pull
//! it with `forte package add <src>` and it lands in the project's flat
//! `packages/` directory. Dependencies declared with `requires "…"` are
//! resolved recursively into the SAME flat directory (npm-style hoisting), so
//! nested `packages/` never exist: a distributed package's own `packages/`
//! and `.forte/` are excluded when it is copied.

use std::path::{Path, PathBuf};

/// `github:owner/repo[@ref]` → (clone URL, optional ref). Anything else is a
/// git URL or a local path, passed through.
fn resolve_src(src: &str) -> (String, Option<String>) {
    let (base, git_ref) = match src.rsplit_once('@') {
        // don't split scp-style URLs (git@github.com:…) on their first '@'
        Some((b, r)) if !b.is_empty() && !r.contains('/') && !r.contains(':') => {
            (b.to_string(), Some(r.to_string()))
        }
        _ => (src.to_string(), None),
    };
    if let Some(rest) = base.strip_prefix("github:") {
        (format!("https://github.com/{rest}.git"), git_ref)
    } else {
        (base, git_ref)
    }
}

/// Copy a package tree, excluding what must never nest: the package's own
/// vendored dependencies, VCS state, and git internals.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        if name_s == "packages" || name_s == ".forte" || name_s == ".git" || name_s == "target" {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// Read a package's identity from its root package.forte (or any single
/// top-level meta block): (name, version, requires).
fn read_meta(dir: &Path) -> Result<(String, String, Vec<String>), String> {
    let meta_path = dir.join("package.forte");
    let src = std::fs::read_to_string(&meta_path)
        .map_err(|_| format!("{} に package.forte がありません(package の必須メタ)", dir.display()))?;
    let ast = crate::parser::parse(&src)
        .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))?;
    let root = ast
        .blocks
        .last()
        .ok_or("package.forte に meta block がありません(block Name { desc … version … })")?;
    let name = root.name.to_ascii_lowercase();
    let version = root.body.version.clone().unwrap_or_else(|| "0.0.0".into());
    Ok((name, version, root.body.requires.clone()))
}

/// One resolved dependency, recorded in package.lock for reproducibility.
#[derive(serde::Serialize, serde::Deserialize)]
struct LockEntry {
    name: String,
    version: String,
    source: String,
    commit: String,
    /// FNV-1a 64 over the vendored tree (sorted rel-path + bytes), the same
    /// hash family as the build digest. Lets `forte package verify` prove a
    /// vendored package is exactly what was fetched.
    #[serde(default)]
    digest: String,
}

/// Content digest of a vendored package directory: every file's relative
/// path and bytes, in sorted order, through FNV-1a 64.
fn tree_digest(dir: &Path) -> Result<String, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let mut entries: Vec<_> =
            std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out)?;
            } else {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, &mut files)?;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut update = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for f in files {
        let rel = f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().replace('\\', "/");
        update(rel.as_bytes());
        update(&[0]);
        update(&std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?);
        update(&[0]);
    }
    Ok(format!("{h:016x}"))
}

fn git_head(dir: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Fetch + place one package (and, recursively, its requires) into the flat
/// `packages/` of the current project.
pub fn add(src: &str) -> Result<(), String> {
    let mut lock: Vec<LockEntry> = std::fs::read_to_string("package.lock")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut queue = vec![src.to_string()];
    while let Some(item) = queue.pop() {
        let (url, git_ref) = resolve_src(&item);
        let local = Path::new(&url);
        // local paths install directly; anything else is cloned shallow
        let (checkout, source_label): (PathBuf, String) = if local.exists() {
            (local.to_path_buf(), url.clone())
        } else {
            let tmp = std::env::temp_dir().join(format!("forte-pkg-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            let mut cmd = std::process::Command::new("git");
            cmd.args(["clone", "--depth", "1"]);
            if let Some(r) = &git_ref {
                cmd.args(["--branch", r]);
            }
            cmd.arg(&url).arg(&tmp);
            let out = cmd.output().map_err(|e| format!("git が実行できません: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "{item} を取得できません:\n{}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            (tmp, url.clone())
        };

        let (name, version, requires) = read_meta(&checkout)?;
        let dirname = format!("{name}_{version}");
        let dest = Path::new("packages").join(&dirname);
        if dest.exists() {
            println!("skip   : {dirname}(導入済み)");
        } else {
            copy_tree(&checkout, &dest)?;
            let commit = git_head(&checkout);
            let digest = tree_digest(&dest)?;
            println!("added  : packages/{dirname}  ← {item}");
            lock.retain(|e| !(e.name == name && e.version == version));
            lock.push(LockEntry { name, version, source: source_label, commit, digest });
        }
        // hoist dependencies into the SAME flat packages/ (no nesting, ever)
        for r in requires {
            queue.push(r);
        }
    }
    lock.sort_by(|a, b| a.name.cmp(&b.name));
    std::fs::write("package.lock", serde_json::to_string_pretty(&lock).unwrap())
        .map_err(|e| e.to_string())?;
    println!("lock   : package.lock を更新しました");
    Ok(())
}

/// `forte package update <name> [--force]` — re-fetch a vendored package and
/// bring it up to date, pip's ergonomics on fork semantics:
///
/// - pristine copy (tree digest matches the lock) → straight replacement;
/// - locally modified → THREE-WAY MERGE against the lock's recorded commit
///   (base = as fetched, ours = your edits, theirs = upstream now). Conflicts
///   or a merge that fails to compile abort with the file list — no
///   half-updated state. `--force` overwrites instead (your copy is backed
///   up next to it).
///
/// Either way the change is reported as a SEMANTIC diff — what the update
/// does to the music, not just to the text.
pub fn update(name: &str, force: bool) -> Result<(), String> {
    let mut lock: Vec<LockEntry> = serde_json::from_str(
        &std::fs::read_to_string("package.lock")
            .map_err(|_| "package.lock がありません(forte package add が作ります)".to_string())?,
    )
    .map_err(|e| format!("package.lock を読めません: {e}"))?;
    let idx = lock
        .iter()
        .position(|e| e.name == name.to_ascii_lowercase())
        .ok_or_else(|| {
            let names: Vec<&str> = lock.iter().map(|e| e.name.as_str()).collect();
            format!(
                "package '{name}' は lock にありません(あるもの: {})",
                if names.is_empty() { "なし".to_string() } else { names.join(", ") }
            )
        })?;
    let entry = &lock[idx];
    let old_dir = Path::new("packages").join(format!("{}_{}", entry.name, entry.version));
    if !old_dir.is_dir() {
        return Err(format!("{} がありません(forte package add {} で再取得)", old_dir.display(), entry.source));
    }

    // fetch upstream now (full clone: the merge base commit must be reachable)
    let (url, git_ref) = resolve_src(&entry.source);
    let tmp = std::env::temp_dir().join(format!("forte-upd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let checkout: PathBuf = if Path::new(&url).exists() {
        PathBuf::from(&url)
    } else {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["clone"]);
        if let Some(r) = &git_ref {
            cmd.args(["--branch", r]);
        }
        cmd.arg(&url).arg(&tmp);
        let out = cmd.output().map_err(|e| format!("git が実行できません: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{} を取得できません:\n{}",
                entry.source,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        tmp.clone()
    };
    let (new_name, new_version, _) = read_meta(&checkout)?;
    if new_name != entry.name {
        return Err(format!("取得先の package 名が違います: lock は '{}'、取得したのは '{new_name}'", entry.name));
    }

    let ours = snapshot_tree(&old_dir)?;
    let pristine = tree_digest(&old_dir)? == entry.digest || entry.digest.is_empty();
    let theirs = snapshot_tree_filtered(&checkout)?;

    let merged: crate::vcs::Snapshot = if pristine {
        println!("update : ローカル変更なし — {}@{new_version} に置き換えます", entry.name);
        theirs.clone()
    } else if force {
        // back up your copy, then take upstream wholesale
        let bak = old_dir.with_extension("bak");
        let _ = std::fs::remove_dir_all(&bak);
        std::fs::rename(&old_dir, &bak).map_err(|e| e.to_string())?;
        println!("update : --force — あなたの変更は {} に退避しました", bak.display());
        theirs.clone()
    } else {
        // three-way: base = the commit recorded at add time
        if entry.commit.is_empty() || Path::new(&url).exists() {
            return Err(
                "ローカル変更がありますが、マージの基準(取得時の commit)を辿れません。\n\
                 変更を自分のプロジェクト側へ移すか、--force(退避つき上書き)を使ってください"
                    .to_string(),
            );
        }
        let out = std::process::Command::new("git")
            .args(["checkout", "--detach", &entry.commit])
            .current_dir(&checkout)
            .output()
            .map_err(|e| format!("git が実行できません: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "基準 commit {} を checkout できません:\n{}",
                &entry.commit[..12.min(entry.commit.len())],
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let base = snapshot_tree_filtered(&checkout)?;
        let out = std::process::Command::new("git")
            .args(["checkout", "--detach", git_ref.as_deref().unwrap_or("HEAD@{1}")])
            .current_dir(&checkout)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("upstream へ戻れませんでした".to_string());
        }
        println!(
            "update : ローカル変更あり — 3方マージします(base {}, あなたの変更は保持)",
            &entry.commit[..12.min(entry.commit.len())]
        );
        merge_snapshots(&base, &ours, &theirs)?
    };

    // the merged tree must COMPILE before it may replace anything
    let stage = std::env::temp_dir().join(format!("forte-upd-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    for (rel, bytes) in &merged {
        let dst = stage.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dst, bytes).map_err(|e| e.to_string())?;
    }
    let mut compile_fails = Vec::new();
    for (rel, bytes) in &merged {
        if !rel.ends_with(".forte") {
            continue;
        }
        let src = String::from_utf8_lossy(bytes);
        let base_dir = stage
            .join(rel)
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        if crate::check_with_loader(&src, &crate::FsLoader, &base_dir).is_err() {
            compile_fails.push(rel.clone());
        }
    }
    if !compile_fails.is_empty() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(format!(
            "マージ結果がコンパイルできません({})。適用は中止しました — 上流の変更とあなたの変更が衝突しています",
            compile_fails.join(", ")
        ));
    }

    // the audible review: what this update does to the music
    let report = crate::semdiff::diff_snapshots(&ours, &merged);
    println!("---- 更新の意味差分(聴けるレビュー) ----");
    println!("{}", if report.is_empty() { "変更なし".to_string() } else { report });
    println!("------------------------------------------");

    // swap into place (the version may have moved the directory name)
    let new_dir = Path::new("packages").join(format!("{}_{new_version}", entry.name));
    if old_dir != new_dir && old_dir.exists() && !force {
        std::fs::remove_dir_all(&old_dir).map_err(|e| e.to_string())?;
    } else if old_dir == new_dir && !force {
        std::fs::remove_dir_all(&old_dir).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&new_dir);
    std::fs::rename(&stage, &new_dir).map_err(|e| e.to_string())?;

    let commit = if Path::new(&url).exists() { String::new() } else { git_head(&checkout) };
    let digest = tree_digest(&new_dir)?;
    println!(
        "updated: packages/{}_{}{} → {}_{new_version}  {digest}",
        entry.name, entry.version,
        if pristine { "" } else { "(あなたの変更を保持)" },
        entry.name
    );
    lock[idx].version = new_version;
    lock[idx].commit = commit;
    lock[idx].digest = digest;
    lock.sort_by(|a, b| a.name.cmp(&b.name));
    std::fs::write("package.lock", serde_json::to_string_pretty(&lock).unwrap())
        .map_err(|e| e.to_string())?;
    println!("lock   : package.lock を更新しました");
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Read a directory into a Snapshot (relative path → bytes).
fn snapshot_tree(dir: &Path) -> Result<crate::vcs::Snapshot, String> {
    let mut snap = crate::vcs::Snapshot::new();
    fn walk(root: &Path, dir: &Path, snap: &mut crate::vcs::Snapshot) -> Result<(), String> {
        let mut entries: Vec<_> =
            std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(root, &p, snap)?;
            } else {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                snap.insert(rel, std::fs::read(&p).map_err(|e| e.to_string())?);
            }
        }
        Ok(())
    }
    walk(dir, dir, &mut snap)?;
    Ok(snap)
}

/// Snapshot of a fetched checkout with the same exclusions `copy_tree`
/// applies when vendoring (no .git / .forte / nested packages / build junk).
fn snapshot_tree_filtered(dir: &Path) -> Result<crate::vcs::Snapshot, String> {
    let full = snapshot_tree(dir)?;
    Ok(full
        .into_iter()
        .filter(|(rel, _)| {
            let top = rel.split('/').next().unwrap_or("");
            !matches!(top, ".git" | ".forte" | "packages" | "target" | "node_modules")
                && !rel.ends_with(".wav")
                && !rel.ends_with(".lock")
        })
        .collect())
}

/// Per-file three-way merge over snapshots. Any conflict aborts the update.
fn merge_snapshots(
    base: &crate::vcs::Snapshot,
    ours: &crate::vcs::Snapshot,
    theirs: &crate::vcs::Snapshot,
) -> Result<crate::vcs::Snapshot, String> {
    let mut out = crate::vcs::Snapshot::new();
    let mut conflicts = Vec::new();
    let paths: std::collections::BTreeSet<&String> =
        base.keys().chain(ours.keys()).chain(theirs.keys()).collect();
    for path in paths {
        let b = base.get(path);
        let o = ours.get(path);
        let t = theirs.get(path);
        let winner: Option<Vec<u8>> = match (b, o, t) {
            // unchanged on our side → upstream wins (incl. deletion)
            (Some(b), Some(o), t) if b == o => t.cloned(),
            // unchanged upstream → our side wins (incl. deletion)
            (Some(b), o, Some(t)) if b == t => o.cloned(),
            // both added identically / both edited identically
            (_, Some(o), Some(t)) if o == t => Some(o.clone()),
            // added on one side only
            (None, Some(o), None) => Some(o.clone()),
            (None, None, Some(t)) => Some(t.clone()),
            // both deleted
            (Some(_), None, None) => None,
            // divergent edits (or both-added divergently): text files go
            // through merge3 — a missing base is an empty file
            (b, Some(o), Some(t)) => {
                let empty = Vec::new();
                let b = b.unwrap_or(&empty);
                match (std::str::from_utf8(b), std::str::from_utf8(o), std::str::from_utf8(t)) {
                    (Ok(bs), Ok(os), Ok(ts)) => {
                        let (merged, conflicted) =
                            crate::vcs::merge3(bs, os, ts, "あなたの変更", "上流");
                        if conflicted {
                            conflicts.push(path.clone());
                            None
                        } else {
                            Some(merged.into_bytes())
                        }
                    }
                    _ => {
                        conflicts.push(path.clone());
                        None
                    }
                }
            }
            // edit vs delete — a human has to decide
            (Some(_), Some(_), None) | (Some(_), None, Some(_)) => {
                conflicts.push(path.clone());
                None
            }
            (None, None, None) => None,
        };
        if let Some(bytes) = winner {
            out.insert(path.clone(), bytes);
        }
    }
    if conflicts.is_empty() {
        Ok(out)
    } else {
        Err(format!(
            "マージ衝突: {}。適用は中止しました — 該当ファイルの変更を自分のプロジェクト側へ移すか、--force(退避つき上書き)を使ってください",
            conflicts.join(", ")
        ))
    }
}

/// `forte package list` — what this project has, with each package's own words.
pub fn list() -> Result<(), String> {
    let dir = Path::new("packages");
    if !dir.is_dir() {
        println!("packages/ がありません(forte package add で取り込みます)");
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    for p in entries {
        match read_meta(&p) {
            Ok((name, version, _)) => {
                // pull desc/license straight from the meta block
                let src = std::fs::read_to_string(p.join("package.forte")).unwrap_or_default();
                let ast = crate::parser::parse(&src).ok();
                let (desc, license, sponsor) = ast
                    .as_ref()
                    .and_then(|a| a.blocks.last())
                    .map(|b| {
                        (
                            b.body.desc.clone().unwrap_or_default(),
                            b.body.license.clone().unwrap_or_default(),
                            b.body.sponsor.clone().unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default();
                println!(
                    "{name} {version}{}",
                    if license.is_empty() { String::new() } else { format!("  [{license}]") }
                );
                if !desc.is_empty() {
                    println!("  {desc}");
                }
                if !sponsor.is_empty() {
                    println!("  support: {sponsor}");
                }
            }
            Err(_) => println!("{}(package.forte なし)", p.display()),
        }
    }
    Ok(())
}

/// `forte package verify` — prove every vendored package is exactly what
/// package.lock recorded: present, and byte-identical (tree digest).
pub fn verify() -> Result<(), String> {
    let lock: Vec<LockEntry> = serde_json::from_str(
        &std::fs::read_to_string("package.lock")
            .map_err(|_| "package.lock がありません(forte package add が作ります)".to_string())?,
    )
    .map_err(|e| format!("package.lock を読めません: {e}"))?;
    let mut bad = 0;
    for e in &lock {
        let dirname = format!("{}_{}", e.name, e.version);
        let dest = Path::new("packages").join(&dirname);
        if !dest.is_dir() {
            println!("MISSING : packages/{dirname}(forte package add {} で再取得)", e.source);
            bad += 1;
            continue;
        }
        if e.digest.is_empty() {
            println!("no-digest: packages/{dirname}(古い lock。add し直すと記録されます)");
            continue;
        }
        let actual = tree_digest(&dest)?;
        if actual == e.digest {
            println!("OK      : packages/{dirname}  {actual}");
        } else {
            println!("MISMATCH: packages/{dirname}  lock {} ≠ 実体 {actual}", e.digest);
            bad += 1;
        }
    }
    // vendored directories the lock doesn't know about
    if let Ok(rd) = std::fs::read_dir("packages") {
        let mut extras: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && !lock.iter().any(|e| {
                        p.file_name().map(|n| n.to_string_lossy() == format!("{}_{}", e.name, e.version))
                            == Some(true)
                    })
            })
            .collect();
        extras.sort();
        for p in extras {
            println!("unlocked: {}(lock に記録がありません)", p.display());
        }
    }
    if bad > 0 {
        Err(format!("{bad} 件の package が lock と一致しません"))
    } else {
        println!("verify  : すべて lock どおりです");
        Ok(())
    }
}

/// Every sounding root in a package file: the song, or each top-level block
/// (a block library builds its LAST block, so each block is rotated into
/// root position and compiled on its own).
fn file_model_hashes(path: &Path) -> Result<Vec<(String, String)>, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
    let parsed = crate::parser::parse(&src)
        .map_err(|ds| format!("{}: {}", path.display(), ds.first().map(|d| d.to_string()).unwrap_or_default()))?;
    let hash_project = |p: &dawcore::model::Project| -> String {
        // strip source positions before hashing: they exist for code-jumps
        // and must never make a comment-only edit look like a sound change
        let mut p = p.clone();
        for t in &mut p.tracks {
            t.src_line = 0;
            for a in &mut t.arranger {
                a.src_line = 0;
            }
        }
        format!("{:016x}", crate::fnv1a64(serde_json::to_string(&p).unwrap_or_default().as_bytes()))
    };
    let mut out = Vec::new();
    if parsed.song.is_some() {
        match crate::compile_with_loader(&src, &crate::FsLoader, &base) {
            Ok(p) => out.push(("song".into(), hash_project(&p))),
            Err(_) => out.push(("song".into(), "broken".into())),
        }
        return Ok(out);
    }
    // rotate each block into root position by appending a one-placement song
    for b in &parsed.blocks {
        let probe = format!(
            "{src}\nsong \"__probe\" {{\n  tempo 120bpm\n  play {} at bars(1..1)\n}}\n",
            b.name
        );
        match crate::compile_with_loader(&probe, &crate::FsLoader, &base) {
            Ok(p) => out.push((b.name.clone(), hash_project(&p))),
            Err(_) => out.push((b.name.clone(), "broken".into())),
        }
    }
    Ok(out)
}

/// `forte package sounddiff <OLD_DIR> <NEW_DIR>` — which sounds changed
/// between two versions of a package, and the version bump that means.
/// blocks/ and songs/ are compared by compiled-MODEL digest (comment and
/// formatting edits stay "unchanged"); instruments/ by source bytes.
pub fn sounddiff(old: &str, new: &str) -> Result<(), String> {
    let mut changed = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;

    let list = |root: &Path, sub: &str| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(root.join(sub))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".forte"))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };

    for sub in ["blocks", "songs", "instruments"] {
        let old_root = Path::new(old);
        let new_root = Path::new(new);
        let a = list(old_root, sub);
        let b = list(new_root, sub);
        for f in &a {
            if !b.contains(f) {
                println!("removed  : {sub}/{f}");
                removed += 1;
            }
        }
        for f in &b {
            if !a.contains(f) {
                println!("added    : {sub}/{f}");
                added += 1;
                continue;
            }
            let (op, np) = (old_root.join(sub).join(f), new_root.join(sub).join(f));
            if sub == "instruments" {
                // devices have no single sounding root — compare source bytes
                let same = std::fs::read(&op).ok() == std::fs::read(&np).ok();
                if !same {
                    println!("changed  : {sub}/{f}(ソース)");
                    changed += 1;
                }
                continue;
            }
            let (ha, hb) = (file_model_hashes(&op)?, file_model_hashes(&np)?);
            for (name, h) in &hb {
                match ha.iter().find(|(n, _)| n == name) {
                    Some((_, old_h)) if old_h != h => {
                        println!("changed  : {sub}/{f} — {name}(model {old_h} → {h})");
                        changed += 1;
                    }
                    Some(_) => {}
                    None => {
                        println!("added    : {sub}/{f} — {name}");
                        added += 1;
                    }
                }
            }
            for (name, _) in &ha {
                if !hb.iter().any(|(n, _)| n == name) {
                    println!("removed  : {sub}/{f} — {name}");
                    removed += 1;
                }
            }
        }
    }

    let bump = if changed > 0 || removed > 0 {
        "major(音が変わる/消える変更があります)"
    } else if added > 0 {
        "minor(追加のみ。既存の音は不変)"
    } else {
        "patch(モデル不変 — コメント・整形のみ)"
    };
    println!("→ recommended bump: {bump}");
    Ok(())
}

/// Render the GitHub search response for `forte package search`.
/// Split from the HTTP call so the formatting is testable offline.
pub fn render_search(json: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("応答を読めません: {e}"))?;
    if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
        return Err(format!("GitHub API: {msg}"));
    }
    let items = v["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return Ok("該当する package はありません(topic:forte-package で検索しています)".into());
    }
    let mut out = String::new();
    for it in &items {
        let full = it["full_name"].as_str().unwrap_or("?");
        let desc = it["description"].as_str().unwrap_or("");
        let stars = it["stargazers_count"].as_u64().unwrap_or(0);
        out.push_str(&format!("{full}  ★{stars}\n"));
        if !desc.is_empty() {
            out.push_str(&format!("  {desc}\n"));
        }
        out.push_str(&format!("  取り込み: forte package add github:{full}\n"));
    }
    Ok(out.trim_end().to_string())
}

/// `forte package search <query>` — discover packages on GitHub. The
/// convention: a Forte package repository carries the topic
/// `forte-package`; search matches name/description within that topic.
pub fn search(query: &str) -> Result<(), String> {
    let q = format!(
        "topic:forte-package{}{}",
        if query.is_empty() { "" } else { " " },
        query
    );
    let url = format!(
        "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page=20",
        q.replace(' ', "+")
    );
    let out = std::process::Command::new("curl")
        .args(["-s", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: forte-cli", &url])
        .output()
        .map_err(|e| format!("curl が実行できません: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "検索できません(ネットワーク?): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    println!("{}", render_search(&String::from_utf8_lossy(&out.stdout))?);
    Ok(())
}

/// `forte init <name>` — scaffold a project that is ALSO a distributable
/// package: meta, role directories, flat packages/, and a forte VCS repo.
pub fn init_project(name: &str) -> Result<String, String> {
    let dir = Path::new(name);
    if dir.exists() {
        return Err(format!("{name} は既に存在します"));
    }
    std::fs::create_dir_all(dir.join("blocks")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("songs")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("packages")).map_err(|e| e.to_string())?;
    // the package is named after the directory, not the path given on the
    // command line — `forte init /some/deep/path/my-album` is still "my-album"
    let base = dir
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    // "my-album" → "MyAlbum": each non-alphanumeric boundary starts a word
    let block_name: String = {
        let mut out = String::new();
        let mut upper = true;
        for ch in base.chars() {
            if ch.is_alphanumeric() {
                out.push(if upper { ch.to_ascii_uppercase() } else { ch });
                upper = false;
            } else {
                upper = true;
            }
        }
        if out.is_empty() { "Package".into() } else { out }
    };
    std::fs::write(
        dir.join("package.forte"),
        format!(
            "// {base} — a Forte package. This folder is both your project and\n\
             // the unit of distribution: push it to GitHub and others can\n\
             // `forte package add github:you/{base}`.\n\
             block {block_name} {{\n  desc \"Describe this package in one line.\"\n  tags \"\"\n  license \"CC-BY-NC-SA-4.0\"\n  version \"0.1.0\"\n  // requires \"github:fortelang/forte@main\"\n}}\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    let repo_msg = {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        std::env::set_current_dir(dir).map_err(|e| e.to_string())?;
        let r = crate::vcs::Repo::init(".");
        std::env::set_current_dir(cwd).map_err(|e| e.to_string())?;
        r?
    };
    Ok(format!(
        "created: {name}/(package.forte + blocks/ songs/ packages/)\n{repo_msg}\n\
         次: cd {name} && forte package add github:… で素材を取り込み、blocks/ に block を書く"
    ))
}
