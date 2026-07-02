//! `forte` CLI — the v0 toolchain slice.
//!
//!   forte check <song.forte>              parse + compile, report diagnostics
//!   forte build <song.forte> [-o out.wav] render WAV + build.manifest.json

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
        _ => {
            eprintln!("usage: forte check <song.forte>");
            eprintln!("       forte build <song.forte> [-o out.wav]");
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

fn check(path: &str) -> ExitCode {
    let src = match load(path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match fortelang::compile_str(&src) {
        Ok(p) => {
            println!(
                "OK: song をコンパイルしました({} tracks, tempo {} bpm, {} 小節)",
                p.tracks.len(),
                p.tempo,
                (dawcore::bounce::arrangement_len(&p) / (p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64)).ceil()
            );
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
    let project = match fortelang::compile_str(&src) {
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
