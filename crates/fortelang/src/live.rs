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

/// Find `device NAME` (case-insensitive) in the standard library reachable
/// from the current directory. Returns (import path, canonical name) so
/// `forte instruments subbass` still resolves to SubBass.
fn find_device(name: &str) -> Option<(String, String)> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let std_dir = dir.join("lib/std");
        if std_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&std_dir) {
                let mut files: Vec<_> =
                    entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "forte")).collect();
                files.sort();
                for f in files {
                    let Ok(src) = std::fs::read_to_string(&f) else { continue };
                    let Ok(ast) = crate::parser::parse(&src) else { continue };
                    if let Some(d) = ast.devices.iter().find(|d| d.name.eq_ignore_ascii_case(name)) {
                        return Some((f.to_string_lossy().into_owned(), d.name.clone()));
                    }
                }
            }
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// `forte instruments <arg>` does what you mean: an exact instrument name
/// (case-insensitive, optionally with `(args)`) enters play mode; anything
/// else filters the catalog.
pub fn play_or_list(arg: &str, from: Option<&str>) -> Result<(), String> {
    let bare = arg.split('(').next().unwrap_or(arg).trim();
    let is_builtin = matches!(bare.to_ascii_lowercase().as_str(), "polymer" | "grid" | "sampler");
    if from.is_some() || is_builtin || find_device(bare).is_some() {
        return run(arg, from);
    }
    list(Some(arg))
}

/// `forte instruments [QUERY]` — the catalog: every device in lib/std with
/// its params, the import line to copy, and how to audition it. QUERY
/// filters case-insensitively on device name or library name.
pub fn list(query: Option<&str>) -> Result<(), String> {
    // locate lib/std the same way `forte instrument` does
    let mut dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let std_dir = loop {
        let candidate = dir.join("lib/std");
        if candidate.is_dir() {
            break candidate;
        }
        if !dir.pop() {
            return Err("lib/std が見つかりません(Forte リポジトリの中で実行してください)".into());
        }
    };
    let q = query.map(str::to_ascii_lowercase);
    let matches = |name: &str, lib: &str| {
        q.as_deref().is_none_or(|q| {
            name.to_ascii_lowercase().contains(q) || lib.to_ascii_lowercase().contains(q)
        })
    };

    let mut files: Vec<_> = std::fs::read_dir(&std_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "forte"))
        .collect();
    files.sort();

    let mut shown = 0usize;
    let mut total = 0usize;
    for f in &files {
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
        println!("lib/std/{lib}.forte — {desc}");
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
    if matches("polymer", "builtin") || matches("sampler", "builtin") || matches("grid", "builtin")
    {
        println!("builtin(import 不要)");
        println!("  polymer        wave cutoff reso attack decay sustain release detune sub filtenv");
        println!("  sampler        sample:\"Kick|Snare|Hat\" または take: 録音(gain attack … pitch start end loop reverse)");
        println!("  grid           既定パッチのモジュラー音源");
        println!();
    }
    if shown == 0 {
        println!("'{}' に当たる楽器はありません(forte instruments で全 {total} 件)", query.unwrap_or(""));
    } else {
        println!("試聴: forte instrument <Name>      曲で使う: import {{ <Name> }} from \"lib/std/<lib>.forte\"");
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
            r.commit(&format!("import {file_name} from lib/std"))?;
            r
        }
    };
    if fresh {
        let _ = repo.commit(&format!("import {file_name} from lib/std"));
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

struct RawTerm;

impl RawTerm {
    fn enter() -> Self {
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
        None if matches!(lower.as_str(), "polymer" | "grid" | "sampler") => (lower, None),
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
    println!("   z/x オクターブ ↑/↓   c/v ベロシティ ↑/↓   q で終了(演奏が notes リテラルになります)");
    if !knobs.is_empty() {
        println!(
            "   ノブ: 1..{} で選択、-/= で下げ/上げ — {}",
            knobs.len(),
            knobs.iter().map(|(n, ..)| n.as_str()).collect::<Vec<_>>().join(" ")
        );
    }

    let _raw = RawTerm::enter();
    let mut stdin = std::io::stdin();
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

        let mut byte = [0u8; 1];
        let n = stdin.read(&mut byte).unwrap_or(0);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(4));
            continue;
        }
        match byte[0] {
            b'q' | 0x03 | 0x04 => break, // q / Ctrl+C / Ctrl+D
            b'z' => {
                octave = (octave + 1).min(7);
                status(None, octave, velocity, &knobs, sel);
            }
            b'x' => {
                octave = (octave - 1).max(-1);
                status(None, octave, velocity, &knobs, sel);
            }
            b'c' => {
                velocity = (velocity + 10).min(127);
                status(None, octave, velocity, &knobs, sel);
            }
            b'v' => {
                velocity = (velocity - 10).max(1);
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
