//! `forte instrument` — load any instrument and play it from the computer
//! keyboard, piano-roll style:
//!
//!   a w s e d f t g y h u j k …  =  C C# D D# E F F# G G# A A# B C …
//!   z / x = octave up / down      c / v = velocity up / down
//!
//! The jam is captured; on quit it is printed as a `notes` literal, because
//! in Forte a performance is source code.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use dawcore::command::Command;
use dawcore::model::NOTE_NAMES;
use dawcore::sync::full_sync;

/// White+black rows of a QWERTY keyboard as one chromatic run from C.
const KEYMAP: &[(u8, i32)] = &[
    (b'a', 0),
    (b'w', 1),
    (b's', 2),
    (b'e', 3),
    (b'd', 4),
    (b'f', 5),
    (b't', 6),
    (b'g', 7),
    (b'y', 8),
    (b'h', 9),
    (b'u', 10),
    (b'j', 11),
    (b'k', 12),
    (b'o', 13),
    (b'l', 14),
    (b'p', 15),
    (b';', 16),
];

/// How long a triggered note holds before the automatic note-off.
const GATE: Duration = Duration::from_millis(220);

fn pitch_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p as i32 / 12 - 1)
}


/// Every instrument library across installed packages, sorted:
/// packages/<pkg>/instruments/*.forte
fn instrument_files(pkg_root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(pkgs) = std::fs::read_dir(pkg_root) {
        for pkg in pkgs.flatten().map(|e| e.path()) {
            let inst = pkg.join("instruments");
            if let Ok(entries) = std::fs::read_dir(&inst) {
                files.extend(
                    entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x == "forte")),
                );
            }
        }
    }
    files.sort();
    files
}

/// Your own instruments: ./instruments/*.forte (the edit/new/fix workspace).
fn workspace_files() -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir("instruments")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "forte"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Find `device NAME` (case-insensitive): your instruments/ workspace wins
/// (so a `fix`ed variant shadows the packaged original), then every
/// installed package. Returns (import path, canonical name) so
/// `forte instruments subbass` still resolves to SubBass.
fn find_device(name: &str) -> Option<(String, String)> {
    let scan = |files: &[std::path::PathBuf]| -> Option<(String, String)> {
        for f in files {
            let Ok(src) = std::fs::read_to_string(f) else { continue };
            let Ok(ast) = crate::parser::parse(&src) else { continue };
            if let Some(d) = ast.devices.iter().find(|d| d.name.eq_ignore_ascii_case(name)) {
                return Some((f.to_string_lossy().into_owned(), d.name.clone()));
            }
        }
        None
    };
    if let Some(hit) = scan(&workspace_files()) {
        return Some(hit);
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let pkg_root = dir.join("packages");
        if pkg_root.is_dir() {
            return scan(&instrument_files(&pkg_root));
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// `forte instruments names [PREFIX]` — machine-readable name list, one per
/// line, for shell completion (`forte complete bash`). Dynamic on purpose:
/// the library keeps growing, so completion asks the CLI instead of a list.
pub fn names(prefix: Option<&str>) -> Result<(), String> {
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let pkg_root = loop {
        let candidate = dir.join("packages");
        if candidate.is_dir() {
            break candidate;
        }
        if !dir.pop() {
            return Ok(()); // outside a repo: no names, no error (completion stays quiet)
        }
    };
    let p = prefix.map(str::to_ascii_lowercase);
    let mut out: Vec<String> = vec!["prisma".into(), "mesh".into(), "sampler".into()];
    for f in workspace_files() {
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(ast) = crate::parser::parse(&src) else { continue };
        out.extend(ast.devices.iter().map(|d| d.name.clone()));
    }
    for f in instrument_files(&pkg_root) {
        let Ok(src) = std::fs::read_to_string(&f) else { continue };
        let Ok(ast) = crate::parser::parse(&src) else { continue };
        out.extend(ast.devices.iter().map(|d| d.name.clone()));
    }
    out.sort();
    out.dedup();
    for n in out {
        if p.as_deref().is_none_or(|p| n.to_ascii_lowercase().starts_with(p)) {
            println!("{n}");
        }
    }
    Ok(())
}

/// `forte instruments [QUERY]` — the catalog: every device in lib/std with
/// its params, the import line to copy, and how to audition it. QUERY
/// filters case-insensitively on device name or library name.
pub fn list(query: Option<&str>) -> Result<(), String> {
    // locate lib/std the same way `forte instrument` does
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let pkg_root = loop {
        let candidate = dir.join("packages");
        if candidate.is_dir() {
            break candidate;
        }
        if !dir.pop() {
            return Err("packages/ が見つかりません(Forte リポジトリの中で実行してください)".into());
        }
    };
    let q = query.map(str::to_ascii_lowercase);
    let matches = |name: &str, lib: &str| {
        q.as_deref().is_none_or(|q| {
            name.to_ascii_lowercase().contains(q) || lib.to_ascii_lowercase().contains(q)
        })
    };

    // your workspace first — the instruments you made or fixed
    let mut files = workspace_files();
    let ws_count = files.len();
    files.extend(instrument_files(&pkg_root));

    let mut shown = 0usize;
    let mut total = 0usize;
    for (i, f) in files.iter().enumerate() {
        let in_workspace = i < ws_count;
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        let Ok(ast) = crate::parser::parse(&src) else { continue };
        let lib = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        total += ast.devices.len();
        let hits: Vec<_> = ast.devices.iter().filter(|d| matches(&d.name, &lib)).collect();
        if hits.is_empty() {
            continue;
        }
        // the file's headline comment is its description ("// std/x — …")
        let head = src.lines().next().and_then(|l| l.strip_prefix("//")).unwrap_or("").trim();
        let desc = head.split_once('—').map(|(_, d)| d.trim()).unwrap_or(head);
        let rel = f.strip_prefix(pkg_root.parent().unwrap_or(&pkg_root)).unwrap_or(f);
        if in_workspace {
            println!("{}(あなたの workspace) — {desc}", f.display());
        } else {
            println!("{} — {desc}", rel.display());
        }
        for d in &hits {
            let params: Vec<String> = d
                .params
                .iter()
                .map(|p| format!("{} {}", p.name, p.default))
                .collect();
            println!(
                "  {:<14} {}",
                d.name,
                if params.is_empty() { "(パラメータなし)".to_string() } else { params.join("  ") }
            );
            shown += 1;
        }
        println!();
    }
    if matches("prisma", "builtin") || matches("sampler", "builtin") || matches("mesh", "builtin")
    {
        println!("builtin(import 不要)");
        println!("  prisma         wave cutoff reso attack decay sustain release detune sub filtenv");
        println!("  sampler        sample:\"Kick|Snare|Hat\" または take: 録音(gain attack … pitch start end loop reverse)");
        println!("  mesh           既定パッチのモジュラー音源");
        println!();
    }
    if shown == 0 {
        println!("'{}' に当たる楽器はありません(forte instruments で全 {total} 件)", query.unwrap_or(""));
    } else {
        println!("試聴: forte instruments play <Name>   曲で使う: import {{ <Name> }} from \"packages/<pkg>/instruments/<lib>.forte\"");
    }
    Ok(())
}

/// `forte instruments edit NAME` — your instruments workspace: the library
/// holding NAME is copied into ./instruments/ (a forte VCS repository), an
/// editor opens, and the change is committed automatically on exit — every
/// edit leaves history you can `forte log` / `forte diff` / fork from.
pub fn edit(name: &str) -> Result<(), String> {
    let (src_path, name) = find_device(name).ok_or_else(|| {
        format!("instrument '{name}' が見つかりません(一覧: forte instruments)")
    })?;
    let name = name.as_str();
    std::fs::create_dir_all("instruments").map_err(|e| e.to_string())?;
    let file_name = std::path::Path::new(&src_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("edited.forte")
        .to_string();
    let work = std::path::Path::new("instruments").join(&file_name);
    let fresh = !work.exists();
    if fresh {
        std::fs::copy(&src_path, &work).map_err(|e| e.to_string())?;
    }
    // the workspace is a forte VCS repository — history is automatic
    let repo = match crate::vcs::Repo::open("instruments") {
        Ok(r) => r,
        Err(_) => {
            crate::vcs::Repo::init("instruments")?;
            let r = crate::vcs::Repo::open("instruments")?;
            r.commit(&format!("import {file_name} from packages"))?;
            r
        }
    };
    if fresh {
        let _ = repo.commit(&format!("import {file_name} from packages"));
    }

    // open the user's editor (VSCode blocks with --wait); fall back to $EDITOR
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).ok();
    let status = match editor {
        Some(ed) => std::process::Command::new(ed).arg(&work).status(),
        None => std::process::Command::new("code").arg("--wait").arg(&work).status(),
    };
    match status {
        Ok(s) if s.success() => {}
        Ok(_) | Err(_) => {
            return Err(format!(
                "エディタを開けませんでした。$EDITOR を設定するか、直接編集してください: {}\n\
                 編集後: cd instruments && forte commit -m \"...\"",
                work.display()
            ))
        }
    }

    // validate, then auto-commit — the edit becomes history
    let src = std::fs::read_to_string(&work).map_err(|e| e.to_string())?;
    match crate::check_with_loader(&src, &crate::FsLoader, "instruments") {
        Ok(_) => {}
        Err(ds) => {
            println!("警告: 検証エラーがあります(コミットはします):");
            for d in ds {
                println!("  {d}");
            }
        }
    }
    match repo.commit(&format!("edit {name}")) {
        Ok(msg) => println!("{msg}"),
        Err(e) if e.contains("変更") => println!("変更なし(コミットしません)"),
        Err(e) => return Err(e),
    }
    println!(
        "instruments/{file_name} を編集しました。履歴: cd instruments && forte log\n\
         曲で使う: import {{ {name} }} from \"instruments/{file_name}\""
    );
    Ok(())
}

/// Open (or create) the instruments/ workspace repository.
fn workspace_repo() -> Result<crate::vcs::Repo, String> {
    std::fs::create_dir_all("instruments").map_err(|e| e.to_string())?;
    match crate::vcs::Repo::open("instruments") {
        Ok(r) => Ok(r),
        Err(_) => {
            crate::vcs::Repo::init("instruments")?;
            crate::vcs::Repo::open("instruments")
        }
    }
}

/// `forte instruments new MySynth` — a fresh instrument from the classic
/// template (osc → svf → adsr-shaped gain), committed into instruments/.
pub fn new_instrument(name: &str) -> Result<(), String> {
    if !name.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!("'{name}' は device 名にできません(英字始まりの英数字)"));
    }
    if let Some((path, canonical)) = find_device(name) {
        return Err(format!("'{canonical}' は既にあります({path})。編集: forte instruments edit {canonical}"));
    }
    let file_name = format!("{}.forte", name.to_ascii_lowercase());
    let work = std::path::Path::new("instruments").join(&file_name);
    if work.exists() {
        return Err(format!("{} が既にあります", work.display()));
    }
    let repo = workspace_repo()?;
    let template = format!(
        r#"// instruments/{file_name} — your instrument. Audition while you edit:
//   forte instruments play {name}
device {name} : Instrument {{
  param cutoff = 0.6 in 0..1
  param reso = 0.25 in 0..1
  param attack = 0.005 in 0..2
  param release = 0.2 in 0..4
  node o   = osc(shape: "saw")
  node f   = svf(in: o, cutoff: cutoff, reso: reso)
  node env = adsr(a: attack, d: 0.15, s: 0.7, r: release)
  out gain(in: f, mod: env)
}}
"#
    );
    std::fs::write(&work, template).map_err(|e| e.to_string())?;
    repo.commit(&format!("new {name}"))?;
    println!(
        "created: instruments/{file_name}\n\
         試聴: forte instruments play {name}   編集: forte instruments edit {name} --watch\n\
         曲で使う: import {{ {name} }} from \"instruments/{file_name}\""
    );
    Ok(())
}

/// `forte instruments fix Bass303 cutoff=0.6` — a derived instrument: the
/// library is copied into instruments/ and the device's param DEFAULTS are
/// rewritten there. The workspace copy shadows the packaged original, so
/// `forte instruments play Bass303` now speaks with the fixed values.
pub fn fix(name: &str, assigns: &[(String, f64)]) -> Result<(), String> {
    let (src_path, canonical) = find_device(name).ok_or_else(|| {
        format!("instrument '{name}' が見つかりません(一覧: forte instruments list)")
    })?;
    let src = std::fs::read_to_string(&src_path).map_err(|e| e.to_string())?;
    let ast = crate::parser::parse(&src)
        .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))?;
    let dev = ast
        .devices
        .iter()
        .find(|d| d.name == canonical)
        .ok_or_else(|| format!("device {canonical} を読めません"))?;
    // validate every assignment against the declaration before touching text
    for (key, value) in assigns {
        let decl = dev.params.iter().find(|p| &p.name == key).ok_or_else(|| {
            let names: Vec<&str> = dev.params.iter().map(|p| p.name.as_str()).collect();
            format!("param '{key}' は {canonical} にありません(あるもの: {})", names.join(", "))
        })?;
        let (lo, hi) = decl.range.unwrap_or((0.0, 1.0));
        if *value < lo || *value > hi {
            return Err(format!("{key} = {value} は範囲 {lo}..{hi} の外です"));
        }
    }

    // the device's block: from its `device` keyword to the matching brace
    let start = src[..src.len()]
        .match_indices("device")
        .map(|(i, _)| i)
        .find(|&i| {
            src[i..].split_whitespace().nth(1).map(|w| w.trim_end_matches(':')) == Some(canonical.as_str())
        })
        .ok_or("device 定義が見つかりません")?;
    let open = start + src[start..].find('{').ok_or("`{` がありません")?;
    let mut depth = 0usize;
    let mut end = open;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    // rewrite `param key = <num>` defaults inside that block only
    let mut block = src[start..end].to_string();
    for (key, value) in assigns {
        let mut out = String::with_capacity(block.len());
        let mut done = false;
        for line in block.lines() {
            let t = line.trim_start();
            if !done && t.starts_with("param ") && t[6..].trim_start().starts_with(key.as_str()) {
                let rest = t[6..].trim_start();
                let after = rest[key.len()..].trim_start();
                if let Some(rhs) = after.strip_prefix('=') {
                    // keep everything from `in` (the range) onward
                    let tail = rhs.find(" in ").map(|i| &rhs[i..]).unwrap_or("");
                    let indent = &line[..line.len() - t.len()];
                    out.push_str(&format!("{indent}param {key} = {value}{tail}"));
                    out.push('\n');
                    done = true;
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        block = out;
    }

    // lines() re-joins with a trailing newline the block never had
    let block = block.trim_end_matches('\n').to_string();
    let repo = workspace_repo()?;
    let file_name = std::path::Path::new(&src_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("fixed.forte")
        .to_string();
    let work = std::path::Path::new("instruments").join(&file_name);
    let new_src = format!("{}{}{}", &src[..start], block, &src[end..]);
    // sanity: the rewritten file must still parse
    crate::parser::parse(&new_src)
        .map_err(|ds| format!("fix 後のソースが壊れました: {}", ds.first().map(|d| d.to_string()).unwrap_or_default()))?;
    std::fs::write(&work, &new_src).map_err(|e| e.to_string())?;
    let assign_str: Vec<String> = assigns.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let _ = repo.commit(&format!("fix {canonical} {}", assign_str.join(" ")));
    println!(
        "fixed  : instruments/{file_name} — {canonical} {}
         workspace が package を上書きします。試聴: forte instruments play {canonical}
         元に戻す: rm instruments/{file_name}(または cd instruments && forte log で履歴から)",
        assign_str.join(" ")
    );
    Ok(())
}

/// `forte instruments edit NAME --watch` — the loop instead of the dialog:
/// the workspace copy is watched, and EVERY save validates + commits.
/// Run `forte instruments play NAME` in another terminal and turn the
/// saved change into sound immediately.
pub fn watch(name: &str) -> Result<(), String> {
    let (src_path, canonical) = find_device(name).ok_or_else(|| {
        format!("instrument '{name}' が見つかりません(一覧: forte instruments list)")
    })?;
    let repo = workspace_repo()?;
    let file_name = std::path::Path::new(&src_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("edited.forte")
        .to_string();
    let work = std::path::Path::new("instruments").join(&file_name);
    if !work.exists() {
        std::fs::copy(&src_path, &work).map_err(|e| e.to_string())?;
        let _ = repo.commit(&format!("import {file_name} from packages"));
    }
    println!(
        "watching: {}(保存ごとに検証+自動コミット。Ctrl+C で終了)
         別の端末で: forte instruments play {canonical}",
        work.display()
    );
    let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut last = mtime(&work);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let m = mtime(&work);
        if m == last {
            continue;
        }
        last = m;
        let src = std::fs::read_to_string(&work).map_err(|e| e.to_string())?;
        match crate::check_with_loader(&src, &crate::FsLoader, "instruments") {
            Ok(_) => match repo.commit(&format!("edit {canonical}")) {
                Ok(msg) => println!("✓ {msg}"),
                Err(e) if e.contains("変更") => {}
                Err(e) => println!("✗ commit: {e}"),
            },
            Err(ds) => {
                for d in ds.iter().take(3) {
                    println!("✗ {d}");
                }
            }
        }
    }
}

/// Compose the one-track live song for an instrument call.
pub fn live_source(call: &str, import: Option<&str>) -> String {
    let name = call.split('(').next().unwrap_or(call).trim();
    let import_line = import
        .map(|path| format!("import {{ {name} }} from \"{}\"\n", path.replace('\\', "/")))
        .unwrap_or_default();
    let call = if call.contains('(') { call.to_string() } else { format!("{call}()") };
    format!(
        "{import_line}song \"live\" {{\n  tempo 120bpm\n  track Live {{\n    instrument {call}\n    play beat`----` at bars(1..1)\n  }}\n}}\n"
    )
}

/// Raw-mode terminal guard (min 0 time 0 → non-blocking key reads);
/// restores canonical mode on drop. Shared by the instrument keyboard and
/// the album player.
pub struct RawTerm;

impl RawTerm {
    pub fn enter() -> Self {
        // min 0 time 0 → read() returns immediately when no key is waiting
        let _ = std::process::Command::new("stty")
            .args(["-icanon", "-echo", "min", "0", "time", "0"])
            .stdin(std::process::Stdio::inherit())
            .status();
        RawTerm
    }
}

impl Drop for RawTerm {
    fn drop(&mut self) {
        let _ = std::process::Command::new("stty")
            .args(["icanon", "echo"])
            .stdin(std::process::Stdio::inherit())
            .status();
    }
}

pub fn run(call: &str, from: Option<&str>) -> Result<(), String> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return Err("キーボード演奏には端末が必要です(パイプ経由では動きません)".into());
    }
    let typed = call.split('(').next().unwrap_or(call).trim().to_string();
    let args_part = call.strip_prefix(&typed).unwrap_or("");
    // builtins need no import; anything else is looked up (case-insensitively)
    // in lib/std — the canonical spelling wins so `subbass` finds SubBass
    let lower = typed.to_ascii_lowercase();
    let (name, import) = match from {
        Some(f) => (typed.clone(), Some(f.to_string())),
        None if matches!(lower.as_str(), "prisma" | "mesh" | "sampler") => (lower, None),
        None => {
            let (path, canonical) = find_device(&typed).ok_or_else(|| {
                format!(
                    "instrument '{typed}' が見つかりません(lib/std を探しました)。\n\
                 一覧: forte instruments   絞り込み: forte instruments 808\n\
                 ファイル指定: forte instrument {typed} --from path/to/lib.forte"
                )
            })?;
            (canonical, Some(path))
        }
    };
    let call = format!("{name}{args_part}");
    let src = live_source(&call, import.as_deref());
    let project = crate::compile_with_loader(&src, &crate::FsLoader, ".").map_err(|ds| {
        ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
    })?;

    // the instrument's live knobs: exposed device params (grid instruments)
    // or the builtin's parameter table, tweakable while playing
    let dev = &project.tracks[0].devices[0];
    let mut knobs: Vec<(String, f32, f32, f32)> = if let Some(g) = dev.grid.as_ref() {
        // declared ranges come from the device AST when we know the library
        let ranges: std::collections::HashMap<String, (f32, f32)> = import
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| crate::parser::parse(&s).ok())
            .and_then(|ast| ast.devices.into_iter().find(|d| d.name == name))
            .map(|d| {
                d.params
                    .iter()
                    .map(|p| {
                        let (lo, hi) = p.range.unwrap_or((0.0, 1.0));
                        (p.name.clone(), (lo as f32, hi as f32))
                    })
                    .collect()
            })
            .unwrap_or_default();
        g.param_binds
            .iter()
            .map(|(n, v, _)| {
                let (lo, hi) = ranges.get(n).copied().unwrap_or((0.0, 1.0));
                (n.clone(), *v, lo, hi)
            })
            .collect()
    } else {
        dev.kind
            .params()
            .iter()
            .zip(dev.params.iter())
            .map(|(n, v)| (n.to_ascii_lowercase(), *v, 0.0, 1.0))
            .collect()
    };
    knobs.truncate(9); // one digit key per knob

    let mut audio = crate::audio::start();
    if audio.silent {
        eprintln!("audio: 出力デバイスなし — 無音バックエンドです({})", audio.device_name);
    } else {
        println!("audio: {}", audio.device_name);
    }
    full_sync(&mut audio.handle, &project);

    println!("♪ {name} — キーボードが鍵盤になります(120bpm 相当で記録)");
    println!("   a w s e d f t g y h u j k o l p ;  =  C C# D D# E F F# G G# A A# B C…");
    println!("   z/x オクターブ ↓/↑   c/v ベロシティ ↓/↑   q で終了(演奏が notes リテラルになります)");
    if !knobs.is_empty() {
        println!(
            "   ノブ: 1..{} で選択、-/= で下げ/上げ — {}",
            knobs.len(),
            knobs.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>().join(" ")
        );
    }

    let _raw = RawTerm::enter();
    let mut stdin = std::io::stdin();
    // hot reload: a saved edit to the instrument's file re-syncs the sound
    let mtime = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut last_mtime = import.as_deref().and_then(mtime);
    let mut last_watch = Instant::now();
    let mut octave: i32 = 3; // C3 スタート(MIDI 48)
    let mut velocity: i32 = 100;
    let mut sel = 0usize;
    let started = Instant::now();
    let mut offs: Vec<(u8, Instant)> = Vec::new();
    let mut played: Vec<crate::perform::PlayedNote> = Vec::new();
    const BPM: f64 = 120.0;

    // one status line: note · oct/vel · every knob, the selected one bracketed
    let status = |note: Option<u8>, octave: i32, velocity: i32, knobs: &[(String, f32, f32, f32)], sel: usize| {
        let mut line = match note {
            Some(p) => format!("♪ {:<4}", pitch_name(p)),
            None => "♪     ".to_string(),
        };
        line.push_str(&format!(" oct{octave} vel{velocity}"));
        for (i, (n, v, ..)) in knobs.iter().enumerate() {
            if i == sel {
                line.push_str(&format!("  [{n} {v:.2}]"));
            } else {
                line.push_str(&format!("  {n} {v:.2}"));
            }
        }
        print!("\r{line}\x1b[K");
        let _ = std::io::stdout().flush();
    };

    loop {
        audio.handle.collect_garbage();
        // release notes whose gate elapsed
        let now = Instant::now();
        offs.retain(|&(note, due)| {
            if now >= due {
                audio.handle.send(Command::NoteOff { track: 0, note });
                false
            } else {
                true
            }
        });

        // watch the source file: save in your editor, hear it on the next note
        if last_watch.elapsed() >= Duration::from_millis(300) {
            last_watch = Instant::now();
            if let Some(path) = import.as_deref() {
                let m = mtime(path);
                if m != last_mtime {
                    last_mtime = m;
                    let src = live_source(&call, import.as_deref());
                    match crate::compile_with_loader(&src, &crate::FsLoader, ".") {
                        Ok(p) => {
                            full_sync(&mut audio.handle, &p);
                            // re-apply the knob values you already turned
                            for (i, (_, v, ..)) in knobs.iter().enumerate() {
                                audio.handle.send(Command::SetParam {
                                    track: 0,
                                    device: 0,
                                    param: i,
                                    value: *v,
                                });
                            }
                            print!("\r reloaded ✓\x1b[K\n");
                            status(None, octave, velocity, &knobs, sel);
                        }
                        Err(ds) => {
                            print!(
                                "\r ✗ {}\x1b[K\n",
                                ds.first().map(|d| d.to_string()).unwrap_or_default()
                            );
                            status(None, octave, velocity, &knobs, sel);
                        }
                    }
                }
            }
        }

        let mut byte = [0u8; 1];
        let n = stdin.read(&mut byte).unwrap_or(0);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        match byte[0] {
            b'q' | 0x03 | 0x04 => break, // q / Ctrl+C / Ctrl+D
            b'z' => {
                octave = (octave - 1).max(-1);
                status(None, octave, velocity, &knobs, sel);
            }
            b'x' => {
                octave = (octave + 1).min(7);
                status(None, octave, velocity, &knobs, sel);
            }
            b'c' => {
                velocity = (velocity - 10).max(1);
                status(None, octave, velocity, &knobs, sel);
            }
            b'v' => {
                velocity = (velocity + 10).min(127);
                status(None, octave, velocity, &knobs, sel);
            }
            // knobs: a digit selects, -/= turn (5% of the declared range),
            // applied live through the same path automation uses
            d @ b'1'..=b'9' if ((d - b'1') as usize) < knobs.len() => {
                sel = (d - b'1') as usize;
                status(None, octave, velocity, &knobs, sel);
            }
            k @ (b'-' | b'=' | b'+') if !knobs.is_empty() => {
                let (_, v, lo, hi) = &mut knobs[sel];
                let step = (*hi - *lo) * 0.05;
                *v = if k == b'-' { (*v - step).max(*lo) } else { (*v + step).min(*hi) };
                audio.handle.send(Command::SetParam { track: 0, device: 0, param: sel, value: *v });
                status(None, octave, velocity, &knobs, sel);
            }
            k => {
                if let Some(&(_, semi)) = KEYMAP.iter().find(|(key, _)| *key == k) {
                    let midi = (octave + 1) * 12 + semi;
                    if (0..=127).contains(&midi) {
                        let note = midi as u8;
                        audio.handle.send(Command::NoteOn {
                            track: 0,
                            note,
                            velocity: velocity as f32 / 127.0,
                        });
                        offs.push((note, now + GATE));
                        let beat = started.elapsed().as_secs_f64() * BPM / 60.0;
                        played.push(crate::perform::PlayedNote {
                            start: beat,
                            len: GATE.as_secs_f64() * BPM / 60.0,
                            pitch: note,
                        });
                        status(Some(note), octave, velocity, &knobs, sel);
                    }
                }
            }
        }
    }

    // flush hanging notes
    for (note, _) in offs.drain(..) {
        audio.handle.send(Command::NoteOff { track: 0, note });
    }
    println!();
    if let Some(lit) = crate::perform::transcribe(&played, 0.25) {
        // 1/16 grid at the session tempo — the jam as code, ready to paste
        println!("captured({} notes, 1/16 quantize):", played.len());
        println!("play notes`{lit}` at bars(1..4)");
    }
    Ok(())
}
