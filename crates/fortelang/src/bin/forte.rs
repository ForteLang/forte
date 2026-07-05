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
            build(&args[1], out, args.iter().any(|a| a == "--stems"))
        }
        #[cfg(not(target_family = "wasm"))]
        Some("export") if args.len() >= 2 => {
            let out = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| {
                    PathBuf::from(&args[1]).with_extension("zip").to_string_lossy().into_owned()
                });
            match fortelang::export::export(&args[1]) {
                Ok(info) => {
                    if let Err(e) = std::fs::write(&out, &info.bytes) {
                        eprintln!("{out}: 書き込めません: {e}");
                        return ExitCode::FAILURE;
                    }
                    println!(
                        "exported: {out} ({} sources{}{})",
                        info.files,
                        if info.history_objects > 0 {
                            format!(" + 履歴 {} objects", info.history_objects)
                        } else {
                            String::new()
                        },
                        info.digest.map(|d| format!(", digest {d}")).unwrap_or_default(),
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("export: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("viz") if args.len() >= 2 => {
            let path = &args[1];
            let src = match load(path) {
                Ok(s) => s,
                Err(c) => return c,
            };
            match fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base_dir(path)) {
                Ok(p) => {
                    println!("{}", fortelang::viz::viz_json(&p));
                    ExitCode::SUCCESS
                }
                Err(diags) => {
                    for d in &diags {
                        eprintln!("{path}:{d}");
                    }
                    ExitCode::FAILURE
                }
            }
        }
        Some("fmt") if args.len() >= 2 => {
            let check = args.iter().any(|a| a == "--check");
            let path = &args[1];
            let src = match load(path) {
                Ok(s) => s,
                Err(c) => return c,
            };
            match fortelang::fmt::format(&src) {
                Ok(out) if out == src => {
                    println!("OK: {path} は正規形です");
                    ExitCode::SUCCESS
                }
                Ok(out) if check => {
                    eprintln!("{path}: 正規形ではありません(forte fmt で整形されます)");
                    let _ = out;
                    ExitCode::FAILURE
                }
                Ok(out) => {
                    if let Err(e) = std::fs::write(path, out) {
                        eprintln!("{path}: 書き込めません: {e}");
                        return ExitCode::FAILURE;
                    }
                    println!("formatted: {path}");
                    ExitCode::SUCCESS
                }
                Err(d) => {
                    eprintln!("{path}:{d}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("lsp") => ExitCode::from(fortelang::lsp::run() as u8),
        #[cfg(not(target_family = "wasm"))]
        Some("repl") => ExitCode::from(fortelang::repl::run() as u8),
        #[cfg(not(target_family = "wasm"))]
        Some("hub") if args.len() >= 2 => hub_cmd(&args[1..]),
        Some("init") => vcs_print(fortelang::vcs::Repo::init(".")),
        Some("status") => vcs_status(),
        Some("commit") => {
            let msg = args
                .iter()
                .position(|a| a == "-m")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default();
            vcs_print(fortelang::vcs::Repo::open(".").and_then(|r| r.commit(&msg)))
        }
        Some("log") => vcs_log(args.iter().any(|a| a == "--json")),
        Some("branch") => match fortelang::vcs::Repo::open(".") {
            Err(e) => vcs_print(Err(e)),
            Ok(repo) => match args.get(1) {
                Some(name) => vcs_print(repo.create_branch(name)),
                None => {
                    let cur = repo.current_branch().ok().flatten();
                    match repo.branches() {
                        Ok(bs) => {
                            for (name, hash) in bs {
                                let mark = if Some(&name) == cur.as_ref() { "*" } else { " " };
                                println!("{mark} {name} {}", &hash[..8]);
                            }
                            ExitCode::SUCCESS
                        }
                        Err(e) => vcs_print(Err(e)),
                    }
                }
            },
        },
        Some("checkout") if args.len() >= 2 => {
            vcs_print(fortelang::vcs::Repo::open(".").and_then(|r| r.checkout(&args[1])))
        }
        Some("merge") if args.len() >= 2 => {
            vcs_print(fortelang::vcs::Repo::open(".").and_then(|r| r.merge(&args[1])))
        }
        Some("diff") => vcs_diff(&args[1..]),
        #[cfg(not(target_family = "wasm"))]
        Some("play") if args.len() >= 2 => {
            let for_secs = args
                .iter()
                .position(|a| a == "--for")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse::<f64>().ok());
            play(&args[1], for_secs)
        }
        #[cfg(not(target_family = "wasm"))]
        Some("browser") => {
            let port = args
                .iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(8000);
            match fortelang::browser::run(port, !args.iter().any(|a| a == "--no-open")) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("browser: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(not(target_family = "wasm"))]
        Some("instruments") => {
            // the instruments workspace: browse / play / edit / add
            let sub = args.get(1).map(String::as_str);
            let result = match sub {
                Some("play") if args.len() >= 3 => {
                    let from = args
                        .iter()
                        .position(|a| a == "--from")
                        .and_then(|i| args.get(i + 1))
                        .cloned();
                    fortelang::live::run(&args[2], from.as_deref())
                }
                Some("edit") if args.len() >= 3 => fortelang::live::edit(&args[2]),
                Some("add") if args.len() >= 3 => {
                    // fork an instrument library from a hub into instruments/
                    let hub = args
                        .iter()
                        .position(|a| a == "--hub")
                        .and_then(|i| args.get(i + 1).cloned())
                        .or_else(|| std::env::var("FORTE_HUB").ok())
                        .unwrap_or_else(|| ".forte-hub".into());
                    let dest = format!("instruments/{}", args[2]);
                    let r = if fortelang::hub_git::is_git_url(&hub) {
                        fortelang::hub_git::GitHub::open(&hub, None).and_then(|h| h.fork(&args[2], &dest))
                    } else {
                        fortelang::hub::Hub::open(&hub).and_then(|h| h.fork(&args[2], &dest))
                    };
                    r.map(|msg| println!("{msg}")).map_err(|e| e.to_string())
                }
                Some("list") => fortelang::live::list(args.get(2).map(String::as_str)),
                // machine-readable name list for shell completion (hidden)
                Some("names") => fortelang::live::names(args.get(2).map(String::as_str)),
                None => fortelang::live::list(None),
                Some(other) => Err(format!(
                    "instruments のサブコマンドは list / play / edit / add です。\n\
                     一覧: forte instruments list {other}   演奏: forte instruments play {other}"
                )),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("instruments: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(not(target_family = "wasm"))]
        Some("instrument") if args.len() >= 2 => {
            let from = args
                .iter()
                .position(|a| a == "--from")
                .and_then(|i| args.get(i + 1))
                .cloned();
            match fortelang::live::run(&args[1], from.as_deref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("instrument: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("upgrade") => upgrade(),
        Some("complete") if args.len() >= 2 => complete(&args[1]),
        #[cfg(not(target_family = "wasm"))]
        Some("ci") => ci(args.get(1).map(String::as_str) == Some("quick")),
        #[cfg(not(target_family = "wasm"))]
        Some("web") if args.get(1).map(String::as_str) == Some("build") => web_build(),
        Some("version") | Some("--version") | Some("-V") => {
            println!("forte {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: forte check <song.forte>");
            eprintln!("       forte build <song.forte> [-o out.wav] [--stems]");
            eprintln!("       forte export <song.forte> [-o out.zip]  (曲+履歴+証明の自己完結 zip)");
            eprintln!("       forte play  <song.forte> [--for SECS]   (トラックタイムラインを表示しながら再生)");
            eprintln!("       forte repl                  (打った行がその場で鳴る)");
            eprintln!("       forte instruments list [QUERY]  (カタログ。list bass / list 808 で絞り込み)");
            eprintln!("       forte instruments play <Name[(args)]>  (キーボードが鍵盤に。1..9/-/= でノブ)");
            eprintln!("       forte instruments edit <Name>          (instruments/ にコピーして編集、自動コミットで履歴)");
            eprintln!("       forte instruments add <Name> [--hub URL] (hub から楽器ライブラリを fork)");
            eprintln!("       forte instrument <Name[(args)]> [--from lib.forte]  (= instruments play)");
            eprintln!("       forte browser [--port 8000] [--no-open]  (ブラウザエディタを起動)");
            eprintln!("       forte web build             (ブラウザエディタの wasm を再ビルド)");
            eprintln!("       forte ci [quick]            (マージゲート: test+clippy/決定論/corpus/E2E)");
            eprintln!("       forte upgrade               (forte コマンド自体を更新)");
            eprintln!("       forte complete bash|zsh     (Tab 補完: source <(forte complete bash))");
            eprintln!("       forte fmt   <song.forte> [--check]");
            eprintln!("       forte viz   <song.forte>   (可視化 JSON を出力)");
            eprintln!("       forte lsp");
            eprintln!("       forte init                  (このディレクトリをリポジトリに)");
            eprintln!("       forte status");
            eprintln!("       forte commit -m \"メッセージ\"");
            eprintln!("       forte log");
            eprintln!("       forte branch [NAME]");
            eprintln!("       forte checkout <branch|hash>");
            eprintln!("       forte merge <branch>        (競合しない編集は自動で合流)");
            eprintln!("       forte diff [REV [REV]]      (音楽の言葉で差分。既定 HEAD↔作業)");
            eprintln!("       forte hub publish <file.forte> [--as NAME] [--hub DIR|URL]");
            eprintln!("       forte hub fork <NAME> <DEST-DIR>   [--hub DIR|URL]");
            eprintln!("         --hub github:you/hub | git@github.com:you/hub.git — GitHub が hub になる");
            eprintln!("       forte hub signup <AUTHOR> --hub http://HOST:PORT  (自前サーバー時のみ)");
            eprintln!("       forte hub release <NAME>           [--hub DIR]");
            eprintln!("       forte hub verify <NAME>            [--hub DIR]");
            eprintln!("       forte hub lineage <NAME>           [--hub DIR]");
            eprintln!("       forte hub list                     [--hub DIR]");
            eprintln!("       forte hub serve [--port 9377]      [--hub DIR]");
            ExitCode::from(2)
        }
    }
}

/// Walk up from the cwd to the repository root (the dir holding Cargo.toml
/// with crates/), so repo-wide commands work from any subdirectory.
fn repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("crates/fortelang/Cargo.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn run_step(name: &str, mut cmd: std::process::Command) -> bool {
    println!("== {name} ==");
    match cmd.status() {
        Ok(s) if s.success() => true,
        Ok(_) => {
            eprintln!("FAILED: {name}");
            false
        }
        Err(e) => {
            eprintln!("FAILED: {name}: {e}");
            false
        }
    }
}

/// `forte ci [quick]` — the merge gate, all in one command (GitHub Actions is
/// off; this runs the same jobs locally). quick = tests + clippy + determinism.
#[cfg(not(target_family = "wasm"))]
fn ci(quick: bool) -> ExitCode {
    let Some(root) = repo_root() else {
        eprintln!("ci: run inside the Forte repository");
        return ExitCode::FAILURE;
    };
    let cargo = |args: &[&str]| {
        let mut c = std::process::Command::new("cargo");
        c.args(args).current_dir(&root);
        c
    };
    let script = |path: &str| {
        let mut c = std::process::Command::new(root.join(path));
        c.current_dir(&root);
        c
    };
    let ok = run_step("1/4 cargo test", cargo(&["test", "--release", "-p", "dawcore", "-p", "fortelang"]))
        && run_step(
            "1/4 clippy (-D warnings)",
            cargo(&["clippy", "--release", "-p", "dawcore", "-p", "fortelang", "--all-targets", "--", "-D", "warnings"]),
        )
        && run_step("2/4 determinism gate", script("scripts/determinism_test.sh"))
        && (quick || run_step("3/4 corpus", script("scripts/check_corpus.sh")))
        && (quick || {
            // E2E needs playwright; skip gracefully when absent
            if root.join("node_modules/playwright").is_dir() {
                let mut a = std::process::Command::new("node");
                a.arg(root.join("scripts/web_e2e.mjs")).current_dir(&root);
                let mut b = std::process::Command::new("node");
                b.arg(root.join("scripts/hub_e2e.mjs")).current_dir(&root);
                run_step("4/4 web E2E", a) && run_step("4/4 hub E2E", b)
            } else {
                println!("== 4/4 E2E == skip (playwright not installed)");
                true
            }
        });
    if ok {
        println!("OK: gate {} — clear to merge", if quick { "(quick)" } else { "(full)" });
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `forte web build` — compile the browser editor's wasm and place it in web/.
#[cfg(not(target_family = "wasm"))]
fn web_build() -> ExitCode {
    let Some(root) = repo_root() else {
        eprintln!("web build: run inside the Forte repository");
        return ExitCode::FAILURE;
    };
    let ok = run_step("wasm build (forteweb)", {
        let mut c = std::process::Command::new("cargo");
        c.args(["build", "--release", "-q", "-p", "forteweb", "--target", "wasm32-unknown-unknown"])
            .current_dir(&root);
        c
    });
    if !ok {
        eprintln!("hint: rustup target add wasm32-unknown-unknown");
        return ExitCode::FAILURE;
    }
    let src = root.join("target/wasm32-unknown-unknown/release/forteweb.wasm");
    let dst = root.join("web/forte.wasm");
    match std::fs::copy(&src, &dst) {
        Ok(bytes) => {
            println!("web/forte.wasm updated ({bytes} bytes) — try: forte browser");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("copy {} → {}: {e}", src.display(), dst.display());
            ExitCode::FAILURE
        }
    }
}

/// `forte complete bash|zsh` — emit a shell-completion script. Instrument
/// names complete dynamically via `forte instruments names`, so the library
/// can grow without regenerating anything:
///   bash: echo 'source <(forte complete bash)' >> ~/.bashrc
///   zsh : echo 'source <(forte complete zsh)'  >> ~/.zshrc
fn complete(shell: &str) -> ExitCode {
    const BASH: &str = r#"_forte() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local first="${COMP_WORDS[1]}"
    local second="${COMP_WORDS[2]}"
    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=($(compgen -W "check build play export repl instrument instruments browser web ci upgrade version fmt viz lsp init status commit log branch checkout merge diff hub complete" -- "$cur"))
        return
    fi
    if [ "$first" = "instruments" ]; then
        if [ "$COMP_CWORD" -eq 2 ]; then
            COMPREPLY=($(compgen -W "list play edit add" -- "$cur"))
            return
        fi
        case "$second" in
            play|edit) COMPREPLY=($(compgen -W "$(forte instruments names 2>/dev/null)" -- "$cur")); return;;
        esac
    fi
    if [ "$first" = "instrument" ] && [ "$COMP_CWORD" -eq 2 ]; then
        COMPREPLY=($(compgen -W "$(forte instruments names 2>/dev/null)" -- "$cur"))
        return
    fi
    COMPREPLY=($(compgen -f -- "$cur"))
}
complete -F _forte forte
"#;
    match shell {
        "bash" => {
            print!("{BASH}");
            ExitCode::SUCCESS
        }
        "zsh" => {
            // ride bash-compatible completion in zsh
            println!("autoload -U +X bashcompinit && bashcompinit");
            print!("{BASH}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("complete: '{other}' は未対応です(bash / zsh)");
            ExitCode::FAILURE
        }
    }
}

/// `forte upgrade` — rebuild/reinstall the CLI. Inside a checkout the local
/// sources win; anywhere else cargo pulls the repository.
fn upgrade() -> ExitCode {
    println!("forte {} — 更新を確認します…", env!("CARGO_PKG_VERSION"));
    // find a checkout (crates/fortelang next to us or above the cwd)
    let mut checkout = None;
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            if dir.join("crates/fortelang/Cargo.toml").is_file() {
                checkout = Some(dir.join("crates/fortelang"));
                break;
            }
            if !dir.pop() {
                break;
            }
        }
    }
    let status = match &checkout {
        Some(path) => {
            println!("checkout からインストールします: {}", path.display());
            std::process::Command::new("cargo")
                .args(["install", "--path"])
                .arg(path)
                .arg("--force")
                .status()
        }
        None => {
            println!("GitHub からインストールします: ForteLang/forte");
            std::process::Command::new("cargo")
                .args([
                    "install",
                    "--git",
                    "https://github.com/ForteLang/forte",
                    "fortelang",
                    "--force",
                ])
                .status()
        }
    };
    match status {
        Ok(s) if s.success() => {
            println!("upgraded: forte を更新しました(forte version で確認)");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!("upgrade: cargo install が失敗しました(上のログを確認してください)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!(
                "upgrade: cargo が見つかりません({e})。\n\
                 手動更新: cargo install --git https://github.com/ForteLang/forte fortelang --force"
            );
            ExitCode::FAILURE
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
        Ok(fortelang::Checked::BlockLibrary { blocks, devices, root }) => {
            println!(
                "OK: block ライブラリを検証しました({blocks} blocks{} — 末尾の block をルートとして {} tracks, {} 小節)",
                if devices > 0 { format!(", {devices} devices") } else { String::new() },
                root.tracks.len(),
                (dawcore::bounce::arrangement_len(&root)
                    / (root.time_sig.0 as f64 * 4.0 / root.time_sig.1 as f64))
                    .ceil()
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

fn build(path: &str, out: Option<String>, stems: bool) -> ExitCode {
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

    // open-stems: each non-effect track rendered soloed (sends included) —
    // a fork can rehearse against any subset, and every stem has a digest
    let mut stem_digests = serde_json::Map::new();
    if stems {
        let dir = PathBuf::from(&out).with_extension("").to_string_lossy().into_owned() + "-stems";
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("stems ディレクトリ作成失敗: {e}");
            return ExitCode::FAILURE;
        }
        for t in &project.tracks {
            if t.kind == dawcore::model::TrackKind::Effect {
                continue;
            }
            let soloed = fortelang::solo_project(&project, t.id);
            let safe: String = t.name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
            let wav = PathBuf::from(&dir).join(format!("{safe}.wav"));
            if let Err(e) = dawcore::bounce::render_wav(&soloed, &wav, TAIL_BEATS) {
                eprintln!("stem {} のレンダリング失敗: {e}", t.name);
                return ExitCode::FAILURE;
            }
            let sinfo = fortelang::render_digest(&soloed, TAIL_BEATS);
            stem_digests.insert(
                t.name.clone(),
                serde_json::json!({
                    "output": wav.to_string_lossy(),
                    "f32_digest_fnv1a64": format!("{:016x}", sinfo.f32_digest),
                    "rms": sinfo.rms,
                }),
            );
            println!("stem   : {} → {} ({:016x})", t.name, wav.display(), sinfo.f32_digest);
        }
    }

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
        "stems": stem_digests,
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

    // --hub git@github.com:you/hub.git / github:you/hub / *.git — the hub is
    // a git repository (GitHub hosts it; no server to run)
    if fortelang::hub_git::is_git_url(&hub_dir) {
        let open = || fortelang::hub_git::GitHub::open(&hub_dir, None);
        let result = match args.first().map(String::as_str) {
            Some("publish") if args.len() >= 2 => {
                let name = args
                    .iter()
                    .position(|a| a == "--as")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str);
                open().and_then(|h| h.publish(&args[1], name))
            }
            Some("fork") if args.len() >= 3 => open().and_then(|h| h.fork(&args[1], &args[2])),
            Some("release") if args.len() >= 2 => open().and_then(|h| h.release(&args[1])),
            Some("verify") if args.len() >= 2 => open().and_then(|h| h.verify(&args[1])),
            Some("lineage") if args.len() >= 2 => open().and_then(|h| h.lineage(&args[1])),
            Some("entry") if args.len() >= 2 => open().and_then(|h| h.entry_path(&args[1])),
            Some("list") => {
                let json = args.iter().any(|a| a == "--json");
                open().and_then(|h| h.list(json))
            }
            Some("serve") => {
                let port = args
                    .iter()
                    .position(|a| a == "--port")
                    .and_then(|i| args.get(i + 1))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(9377);
                open().and_then(|h| h.serve(port)).map(|_| String::new())
            }
            _ => Err("usage: forte hub <publish|fork|release|verify|lineage|list|entry|serve> … --hub <git-URL>".into()),
        };
        return match result {
            Ok(msg) => {
                println!("{msg}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("hub: {e}");
                ExitCode::FAILURE
            }
        };
    }

    // --hub http://host:9377 (or FORTE_HUB=http://…) targets a served hub
    if fortelang::hub_remote::is_url(&hub_dir) {
        let url = hub_dir;
        let token = std::env::var("FORTE_HUB_TOKEN").ok();
        let result = match args.first().map(String::as_str) {
            Some("signup") if args.len() >= 2 => fortelang::hub_remote::signup(&url, &args[1]),
            Some("publish") if args.len() >= 2 => {
                let name = args
                    .iter()
                    .position(|a| a == "--as")
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str);
                fortelang::hub_remote::publish(&url, token.as_deref(), &args[1], name)
            }
            Some("fork") if args.len() >= 3 => {
                fortelang::hub_remote::fork(&url, token.as_deref(), &args[1], &args[2])
            }
            Some("list") => fortelang::hub_remote::list(&url),
            _ => Err(
                "リモート hub で使えるのは signup / publish / fork / list です(release/verify はサーバー側で)"
                    .into(),
            ),
        };
        return match result {
            Ok(msg) => {
                println!("{msg}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("hub: {e}");
                ExitCode::FAILURE
            }
        };
    }

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
        Some("similar") if args.len() >= 2 => hub.similar(&args[1]).map(|v| {
            if v.is_empty() {
                "同じ進行を使う曲は(まだ)ありません".into()
            } else {
                v.into_iter()
                    .map(|(name, sig)| format!("{name}\t(進行 {sig})"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }),
        Some("list") if args.iter().any(|a| a == "--json") => {
            hub.repos_json().map(|v| v.to_string())
        }
        Some("list") => hub.list(),
        Some("entry") if args.len() >= 2 => hub.entry_path(&args[1]),
        _ => Err("usage: forte hub <publish|fork|lineage|list|entry> …".into()),
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
    use std::io::{IsTerminal, Write as _};
    use std::time::{Duration, Instant, SystemTime};

    fn compile_file(path: &str) -> Result<Project, Vec<String>> {
        let src = std::fs::read_to_string(path).map_err(|e| vec![format!("{path}: {e}")])?;
        fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base_dir(path))
            .map_err(|diags| diags.iter().map(|d| format!("{path}:{d}")).collect())
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

    const LANE_W: usize = 40;

    /// One frame of the in-place UI: header, per-track lanes with the
    /// playhead running through them (sounding tracks highlighted), status.
    fn render(p: &Project, pos: f64, loops: i64, peak: f32, message: &str) -> Vec<String> {
        let bpb = p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64;
        let len_beats = dawcore::bounce::arrangement_len(p).max(1.0);
        let bars = (len_beats / bpb).ceil().max(1.0);
        let total = len_beats * 60.0 / p.tempo;
        let mut out = Vec::with_capacity(p.tracks.len() + 3);
        out.push(format!(
            "♪ {} bpm {}/{} — {} bars({}:{:04.1})  save = hot reload, Ctrl+C = quit",
            p.tempo,
            p.time_sig.0,
            p.time_sig.1,
            bars as i64,
            (total / 60.0) as i64,
            total % 60.0,
        ));
        let frac = (pos / len_beats).clamp(0.0, 1.0);
        let head_col = ((frac * LANE_W as f64) as usize).min(LANE_W - 1);
        let name_w = p.tracks.iter().map(|t| t.name.chars().count()).max().unwrap_or(4).max(4);
        for t in &p.tracks {
            let mut lane = ['·'; LANE_W];
            let mut active = false;
            for a in &t.arranger {
                let s = ((a.start / len_beats) * LANE_W as f64).floor() as usize;
                let e = (((a.start + a.duration) / len_beats) * LANE_W as f64).ceil() as usize;
                for c in lane.iter_mut().take(e.min(LANE_W)).skip(s.min(LANE_W)) {
                    *c = '█';
                }
                if pos >= a.start && pos < a.start + a.duration {
                    active = true;
                }
            }
            // the playhead cuts through every lane
            lane[head_col] = if lane[head_col] == '█' { '┃' } else { '╎' };
            let lane_str: String = lane.iter().collect();
            let inst = if t.kind == dawcore::model::TrackKind::Effect {
                "(return)".to_string()
            } else {
                t.devices.first().map(|d| d.kind.label().to_string()).unwrap_or_default()
            };
            if active {
                out.push(format!("\x1b[1m▶ {:<name_w$} ▕{lane_str}▏ {inst}\x1b[0m", t.name));
            } else {
                out.push(format!("\x1b[2m  {:<name_w$} ▕{lane_str}▏ {inst}\x1b[0m", t.name));
            }
        }
        let bar = (pos / bpb).floor() as i64 + 1;
        let beat = (pos % bpb).floor() as i64 + 1;
        let secs = pos * 60.0 / p.tempo;
        out.push(format!(
            "▶ bar {bar:>3}.{beat}  {}:{:04.1} / {}:{:04.1}{}  peak {peak:>4.2}",
            (secs / 60.0) as i64,
            secs % 60.0,
            (total / 60.0) as i64,
            total % 60.0,
            if loops > 0 { format!("  loop {}", loops + 1) } else { String::new() },
        ));
        if !message.is_empty() {
            out.push(message.to_string());
        }
        out
    }

    let mut project = match compile_file(path) {
        Ok(p) => p,
        Err(errs) => {
            for e in errs {
                eprintln!("{e}");
            }
            return ExitCode::FAILURE;
        }
    };
    let mut audio = fortelang::audio::start();
    let tty = std::io::stdout().is_terminal();
    if audio.silent {
        eprintln!("audio: 出力デバイスなし — 無音バックエンドで走行します({})", audio.device_name);
    } else {
        println!("audio: {}", audio.device_name);
    }
    apply(&mut audio.handle, &project, 0);
    audio.handle.send(Command::Play);
    println!("playing: \"{path}\"");

    let started = Instant::now();
    let mut last_mtime = mtime(path);
    let mut last_status = Instant::now();
    let mut loops = 0i64;
    let mut last_pos = 0.0f64;
    let mut drawn = 0usize;
    let mut message = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(50));
        audio.handle.collect_garbage();

        // hot reload on mtime change
        let m = mtime(path);
        if m != last_mtime {
            last_mtime = m;
            match compile_file(path) {
                Ok(p) => {
                    let prev = project.tracks.len();
                    apply(&mut audio.handle, &p, prev);
                    project = p;
                    message = format!("reloaded ✓ ({} tracks)", project.tracks.len());
                }
                Err(errs) => {
                    // keep playing the previous version; show the first error
                    message = format!(
                        "✗ {}(直前の版を再生し続けます)",
                        errs.first().cloned().unwrap_or_default()
                    );
                }
            }
        }

        if last_status.elapsed() >= Duration::from_millis(if tty { 100 } else { 2000 }) {
            last_status = Instant::now();
            let pos = audio.handle.shared.position_beats();
            if pos < last_pos - 1.0 {
                loops += 1; // the loop wrapped
            }
            last_pos = pos;
            let peak = audio.handle.shared.master_peak();
            if tty {
                // in-place redraw: jump to the top of the block, wipe, repaint
                let frame = render(&project, pos, loops, peak, &message);
                let mut out = String::new();
                if drawn > 0 {
                    out.push_str(&format!("\x1b[{drawn}A"));
                }
                out.push_str("\r\x1b[J");
                for line in &frame {
                    out.push_str(line);
                    out.push('\n');
                }
                drawn = frame.len();
                print!("{out}");
                let _ = std::io::stdout().flush();
            } else {
                // piped: one plain line, no ANSI, no accumulation games
                let bpb = project.time_sig.0 as f64 * 4.0 / project.time_sig.1 as f64;
                println!(
                    "bar {:.0}.{:.0} peak {peak:.2}{}",
                    (pos / bpb).floor() + 1.0,
                    (pos % bpb).floor() + 1.0,
                    if message.is_empty() { String::new() } else { format!("  {message}") }
                );
                message.clear();
            }
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

// ---------------------------------------------------------------------------
// VCS subcommands
// ---------------------------------------------------------------------------

fn vcs_print(res: Result<String, String>) -> ExitCode {
    match res {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn vcs_status() -> ExitCode {
    let repo = match fortelang::vcs::Repo::open(".") {
        Ok(r) => r,
        Err(e) => return vcs_print(Err(e)),
    };
    let run = || -> Result<String, String> {
        let base = match repo.head()? {
            Some(h) => repo.read_tree(&repo.commit_obj(&h)?.tree)?,
            None => fortelang::vcs::Snapshot::new(),
        };
        let work = repo.working_snapshot()?;
        let (added, modified, deleted) = fortelang::vcs::Repo::changes(&base, &work);
        let branch = repo.current_branch()?.unwrap_or_else(|| "(detached)".into());
        let mut out = format!("ブランチ: {branch}\n");
        if added.is_empty() && modified.is_empty() && deleted.is_empty() {
            out.push_str("変更なし(クリーン)");
        } else {
            for p in &added {
                out.push_str(&format!("  + {p}\n"));
            }
            for p in &modified {
                out.push_str(&format!("  ~ {p}\n"));
            }
            for p in &deleted {
                out.push_str(&format!("  - {p}\n"));
            }
            out.push_str("(差分の中身は forte diff)");
        }
        Ok(out)
    };
    vcs_print(run())
}

fn vcs_log(json: bool) -> ExitCode {
    let run = || -> Result<String, String> {
        let repo = fortelang::vcs::Repo::open(".")?;
        let head = repo.head()?.ok_or("まだコミットがありません")?;
        if json {
            let entries: Vec<serde_json::Value> = repo
                .log(&head)?
                .into_iter()
                .map(|(hash, c)| {
                    serde_json::json!({
                        "hash": hash, "n": c.n, "author": c.author,
                        "message": c.message, "parents": c.parents,
                    })
                })
                .collect();
            return Ok(serde_json::Value::Array(entries).to_string());
        }
        let mut out = String::new();
        for (hash, c) in repo.log(&head)? {
            let merge = if c.parents.len() > 1 { " (merge)" } else { "" };
            out.push_str(&format!("#{:<3} {} {} — {}{merge}\n", c.n, &hash[..8], c.author, c.message));
        }
        out.pop();
        Ok(out)
    };
    vcs_print(run())
}

/// `forte diff`            — HEAD ↔ 作業ツリー
/// `forte diff REV`        — REV ↔ 作業ツリー
/// `forte diff REV REV`    — REV ↔ REV
fn vcs_diff(args: &[String]) -> ExitCode {
    let run = || -> Result<String, String> {
        let repo = fortelang::vcs::Repo::open(".")?;
        let (old, new) = match args {
            [] => {
                let head = repo.head()?.ok_or("まだコミットがありません")?;
                (repo.read_tree(&repo.commit_obj(&head)?.tree)?, repo.working_snapshot()?)
            }
            [rev] => (repo.snapshot_of(rev)?, repo.working_snapshot()?),
            [a, b, ..] => (repo.snapshot_of(a)?, repo.snapshot_of(b)?),
        };
        let report = fortelang::semdiff::diff_snapshots(&old, &new);
        Ok(if report.is_empty() { "変更なし".into() } else { report.trim_end().to_string() })
    };
    vcs_print(run())
}
