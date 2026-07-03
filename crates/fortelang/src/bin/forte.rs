//! `forte` CLI — the v0 toolchain slice.
//!
//!   forte check <song.forte>              parse + compile, report diagnostics
//!   forte build <song.forte> [-o out.wav] render WAV + build.manifest.json
//!   forte play  <song.forte> [--for SECS] play live; reloads on file change

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") if args.len() >= 2 => check(&args[1]),
        Some("build") if args.len() >= 2 => {
            let out = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
                .cloned();
            build(&args[1], out)
        }
        Some("lsp") => ExitCode::from(fortelang::lsp::run() as u8),
        #[cfg(not(target_family = "wasm"))]
        Some("hub") if args.len() >= 2 => hub_cmd(&args[1..]),
        #[cfg(not(target_family = "wasm"))]
        Some("play") if args.len() >= 2 => {
            let for_secs = args
                .iter()
                .position(|a| a == "--for")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse::<f64>().ok());
            play(&args[1], for_secs)
        }
        _ => {
            eprintln!("usage: forte check <song.forte>");
            eprintln!("       forte build <song.forte> [-o out.wav]");
            eprintln!("       forte play  <song.forte> [--for SECS]");
            eprintln!("       forte lsp");
            eprintln!("       forte hub publish <file.forte> [--as NAME] [--hub DIR]");
            eprintln!("       forte hub fork <NAME> <DEST-DIR>   [--hub DIR]");
            eprintln!("       forte hub release <NAME>           [--hub DIR]");
            eprintln!("       forte hub verify <NAME>            [--hub DIR]");
            eprintln!("       forte hub lineage <NAME>           [--hub DIR]");
            eprintln!("       forte hub list                     [--hub DIR]");
            eprintln!("       forte hub serve [--port 9377]      [--hub DIR]");
            ExitCode::from(2)
        }
    }
}

fn load(path: &str) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("{path}: 読めません: {e}");
        ExitCode::from(2)
    })
}

fn base_dir(path: &str) -> String {
    PathBuf::from(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn check(path: &str) -> ExitCode {
    let src = match load(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match fortelang::check_with_loader(&src, &fortelang::FsLoader, &base_dir(path)) {
        Ok(fortelang::Checked::Song(p)) => {
            println!(
                "OK: song をコンパイルしました({} tracks, tempo {} bpm, {} 小節)",
                p.tracks.len(),
                p.tempo,
                (dawcore::bounce::arrangement_len(&p) / (p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64)).ceil()
            );
            ExitCode::SUCCESS
        }
        Ok(fortelang::Checked::DeviceLibrary { devices }) => {
            println!("OK: デバイスライブラリを検証しました({devices} devices)");
            ExitCode::SUCCESS
        }
        Err(diags) => {
            for d in &diags {
                eprintln!("{path}:{d}");
            }
            eprintln!("エラー {} 件", diags.len());
            ExitCode::FAILURE
        }
    }
}

fn build(path: &str, out: Option<String>) -> ExitCode {
    let src = match load(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let project = match fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base_dir(path)) {
        Ok(p) => p,
        Err(diags) => {
            for d in &diags {
                eprintln!("{path}:{d}");
            }
            eprintln!("エラー {} 件", diags.len());
            return ExitCode::FAILURE;
        }
    };

    let out = out.unwrap_or_else(|| {
        PathBuf::from(path).with_extension("wav").to_string_lossy().into_owned()
    });
    const TAIL_BEATS: f64 = 8.0;
    if let Err(e) = dawcore::bounce::render_wav(&project, PathBuf::from(&out).as_path(), TAIL_BEATS)
    {
        eprintln!("レンダリング失敗: {e}");
        return ExitCode::FAILURE;
    }
    let info = fortelang::render_digest(&project, TAIL_BEATS);

    let manifest = serde_json::json!({
        "forte_manifest": 0,
        "source": { "path": path, "fnv1a64": format!("{:016x}", fortelang::fnv1a64(src.as_bytes())) },
        "engine": { "name": "dawcore", "version": env!("CARGO_PKG_VERSION") },
        "render": {
            "sample_rate": 48000,
            "seconds": info.seconds,
            "f32_digest_fnv1a64": format!("{:016x}", info.f32_digest),
            "peak": info.peak,
            "rms": info.rms,
        },
        "output": out,
    });
    let mpath = PathBuf::from(&out).with_extension("manifest.json");
    if let Err(e) = std::fs::write(&mpath, serde_json::to_string_pretty(&manifest).unwrap()) {
        eprintln!("マニフェスト書き込み失敗: {e}");
        return ExitCode::FAILURE;
    }

    println!("built  : {out} ({:.1}s @ 48kHz)", info.seconds);
    println!("digest : {:016x} (f32, fnv1a64)", info.f32_digest);
    println!("proof  : {}", mpath.display());
    ExitCode::SUCCESS
}

/// Local hub: fork-lineage registry (no server yet — SYS-HUB-002 prototype).
#[cfg(not(target_family = "wasm"))]
fn hub_cmd(args: &[String]) -> ExitCode {
    let hub_dir = args
        .iter()
        .position(|a| a == "--hub")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("FORTE_HUB").ok())
        .unwrap_or_else(|| ".forte-hub".into());
    let hub = match fortelang::hub::Hub::open(&hub_dir) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("hub: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match args.first().map(String::as_str) {
        Some("publish") if args.len() >= 2 => {
            let name = args
                .iter()
                .position(|a| a == "--as")
                .and_then(|i| args.get(i + 1))
                .map(String::as_str);
            hub.publish(&args[1], name)
        }
        Some("fork") if args.len() >= 3 => hub.fork(&args[1], &args[2]),
        Some("serve") => {
            let port = args
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(9377);
            fortelang::hub_server::serve(hub, port).map(|_| String::new())
        }
        Some("release") if args.len() >= 2 => hub.release(&args[1]),
        Some("verify") if args.len() >= 2 => hub.verify(&args[1]),
        Some("lineage") if args.len() >= 2 => hub.lineage(&args[1]),
        Some("list") => hub.list(),
        _ => Err("usage: forte hub <publish|fork|lineage|list> …".into()),
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hub: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Live playback with hot reload: the song loops while the file is watched;
/// every successful recompile is swapped into the running engine without
/// stopping the transport — listen, edit, listen (SYS-EDT-002 minimal form).
#[cfg(not(target_family = "wasm"))]
fn play(path: &str, for_secs: Option<f64>) -> ExitCode {
    use dawcore::command::Command;
    use dawcore::model::Project;
    use dawcore::sync::full_sync;
    use std::io::Write as _;
    use std::time::{Duration, Instant, SystemTime};

    fn compile_file(path: &str) -> Result<Project, ExitCode> {
        let src = load(path)?;
        fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base_dir(path)).map_err(
            |diags| {
                for d in &diags {
                    eprintln!("{path}:{d}");
                }
                ExitCode::FAILURE
            },
        )
    }
    fn apply(handle: &mut dawcore::engine::EngineHandle, p: &Project, prev_slots: usize) {
        full_sync(handle, p);
        for slot in p.tracks.len()..prev_slots {
            handle.send(Command::RemoveTrack { slot });
        }
        let len = dawcore::bounce::arrangement_len(p);
        handle.send(Command::SetLoop { enabled: true, start: 0.0, end: len });
        handle.send(Command::SetLaunchQuant(0.0));
    }
    fn mtime(path: &str) -> Option<SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    let mut project = match compile_file(path) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let mut audio = fortelang::audio::start();
    if audio.silent {
        eprintln!("audio: 出力デバイスなし — 無音バックエンドで走行します({})", audio.device_name);
    } else {
        println!("audio: {}", audio.device_name);
    }
    apply(&mut audio.handle, &project, 0);
    audio.handle.send(Command::Play);
    println!(
        "playing: \"{}\" — {} tracks, tempo {} bpm(ループ再生中。ファイルを保存すると即反映、Ctrl+C で終了)",
        path,
        project.tracks.len(),
        project.tempo
    );

    let started = Instant::now();
    let mut last_mtime = mtime(path);
    let mut last_status = Instant::now();
    let beats_per_bar = project.time_sig.0 as f64 * 4.0 / project.time_sig.1 as f64;
    let mut bpb = beats_per_bar;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        audio.handle.collect_garbage();

        // hot reload on mtime change
        let m = mtime(path);
        if m != last_mtime {
            last_mtime = m;
            match compile_file(path) {
                Ok(p) => {
                    let prev = project.tracks.len();
                    apply(&mut audio.handle, &p, prev);
                    bpb = p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64;
                    println!(
                        "\nreloaded: {} tracks, tempo {} bpm",
                        p.tracks.len(),
                        p.tempo
                    );
                    project = p;
                }
                Err(_) => {
                    println!("(エラーのため直前の版を再生し続けます)");
                }
            }
        }

        if last_status.elapsed() >= Duration::from_millis(500) {
            last_status = Instant::now();
            let pos = audio.handle.shared.position_beats();
            let bar = (pos / bpb).floor() as i64 + 1;
            let beat = (pos % bpb).floor() as i64 + 1;
            print!(
                "\r  bar {bar:>3}.{beat} | peak {:>5.2} | voices {:>2} ",
                audio.handle.shared.master_peak(),
                audio.handle.shared.active_voices.load(std::sync::atomic::Ordering::Relaxed)
            );
            let _ = std::io::stdout().flush();
        }

        if let Some(t) = for_secs {
            if started.elapsed().as_secs_f64() >= t {
                println!();
                break;
            }
        }
    }
    ExitCode::SUCCESS
}
