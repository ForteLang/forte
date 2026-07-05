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

/// Find `device NAME` in the standard library (or any .forte library file
/// reachable from the current directory) and return the import path.
fn find_device(name: &str) -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let std_dir = dir.join("lib/std");
        if std_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&std_dir) {
                let mut files: Vec<_> =
                    entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "forte")).collect();
                files.sort();
                for f in files {
                    if let Ok(src) = std::fs::read_to_string(&f) {
                        let pat = format!("device {name} ");
                        let pat2 = format!("device {name}:");
                        if src.contains(&pat) || src.contains(&pat2) {
                            return Some(f.to_string_lossy().into_owned());
                        }
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
    let name = call.split('(').next().unwrap_or(call).trim().to_string();
    // builtins need no import; anything else is looked up in lib/std
    let import = match from {
        Some(f) => Some(f.to_string()),
        None if matches!(name.as_str(), "polymer" | "grid" | "sampler") => None,
        None => Some(find_device(&name).ok_or_else(|| {
            format!(
                "instrument '{name}' が見つかりません(lib/std を探しました)。\n\
                 ファイル指定: forte instrument {name} --from path/to/lib.forte"
            )
        })?),
    };
    let src = live_source(call, import.as_deref());
    let project = crate::compile_with_loader(&src, &crate::FsLoader, ".").map_err(|ds| {
        ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
    })?;

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

    let _raw = RawTerm::enter();
    let mut stdin = std::io::stdin();
    let mut octave: i32 = 3; // C3 スタート(MIDI 48)
    let mut velocity: i32 = 100;
    let started = Instant::now();
    let mut offs: Vec<(u8, Instant)> = Vec::new();
    let mut played: Vec<crate::perform::PlayedNote> = Vec::new();
    const BPM: f64 = 120.0;

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
                print!("\r  oct {octave}  vel {velocity}          ");
                let _ = std::io::stdout().flush();
            }
            b'x' => {
                octave = (octave - 1).max(-1);
                print!("\r  oct {octave}  vel {velocity}          ");
                let _ = std::io::stdout().flush();
            }
            b'c' => {
                velocity = (velocity + 10).min(127);
                print!("\r  oct {octave}  vel {velocity}          ");
                let _ = std::io::stdout().flush();
            }
            b'v' => {
                velocity = (velocity - 10).max(1);
                print!("\r  oct {octave}  vel {velocity}          ");
                let _ = std::io::stdout().flush();
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
                        print!("\r♪ {:<4} oct {octave}  vel {velocity}     ", pitch_name(note));
                        let _ = std::io::stdout().flush();
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
