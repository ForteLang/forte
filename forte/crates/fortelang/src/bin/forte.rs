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
        Some("test") => {
            let update = args.iter().any(|a| a == "--update");
            let paths: Vec<String> =
                args[1..].iter().filter(|a| !a.starts_with("--")).cloned().collect();
            ExitCode::from(fortelang::testing::run(&paths, update) as u8)
        }
        Some("build") if args.len() >= 2 => {
            let out = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|i| args.get(i + 1))
                .cloned();
            // the extension picks the format: .fortesong = playable container
            #[cfg(not(target_family = "wasm"))]
            if let Some(o) = out.as_deref().filter(|o| o.ends_with(".fortesong")) {
                match fortelang::songfile::build(&args[1]) {
                    Ok((bytes, summary)) => {
                        if let Err(e) = std::fs::write(o, &bytes) {
                            eprintln!("{o}: 書き込めません: {e}");
                            return ExitCode::FAILURE;
                        }
                        println!("built  : {o} ({summary})");
                        return ExitCode::SUCCESS;
                    }
                    Err(e) => {
                        eprintln!("build: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
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
        Some("analyze") if args.len() >= 2 => {
            let path = &args[1];
            let json = args.iter().any(|a| a == "--json");
            let stems = !args.iter().any(|a| a == "--no-stems");
            let profile = match args.iter().position(|a| a == "--against") {
                Some(i) => {
                    let Some(pf) = args.get(i + 1) else {
                        eprintln!("--against にはプロファイルのパスを続けます");
                        return ExitCode::from(2);
                    };
                    let body = match load(pf) {
                        Ok(s) => s,
                        Err(c) => return c,
                    };
                    match fortelang::analyze::Profile::from_json(&body) {
                        Ok(p) => Some(p),
                        Err(e) => {
                            eprintln!("{pf}: プロファイルを読めません: {e}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
                None => None,
            };
            let src = match load(path) {
                Ok(s) => s,
                Err(c) => return c,
            };
            match fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base_dir(path)) {
                Ok(p) => {
                    let sections = fortelang::song_sections(&src);
                    let a = fortelang::analyze::analyze(&p, &sections, stems);
                    let deltas = profile.as_ref().map(|pf| (pf, fortelang::analyze::compare(&a, pf)));
                    if json {
                        match &deltas {
                            Some((pf, ds)) => println!(
                                "{{\"analysis\":{},\"against\":{{\"profile\":{},\"pass\":{},\"deltas\":{}}}}}",
                                a.to_json(),
                                serde_json::to_string(&pf.name).unwrap_or_default(),
                                ds.iter().all(|d| d.ok),
                                serde_json::to_string_pretty(ds).unwrap_or_default(),
                            ),
                            None => println!("{}", a.to_json()),
                        }
                    } else {
                        print_analysis(&a);
                        if let Some((pf, ds)) = &deltas {
                            println!("-- 照合: {} --", pf.name);
                            for d in ds {
                                if d.ok {
                                    println!("  ✓ {} = {} (目標 {}..{})", d.metric, d.value, d.lo, d.hi);
                                } else {
                                    println!(
                                        "  ✗ {} = {} (目標 {}..{}, {}{})",
                                        d.metric,
                                        d.value,
                                        d.lo,
                                        d.hi,
                                        if d.delta < 0.0 { "" } else { "+" },
                                        d.delta
                                    );
                                }
                            }
                            let misses = ds.iter().filter(|d| !d.ok).count();
                            if misses == 0 {
                                println!("  合格: プロファイルの全目標を満たしています");
                            } else {
                                println!("  {misses} 項目が目標圏外です");
                            }
                        }
                    }
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
        // `forte edit song.forte '<json-op>' [--write]` — lossless structured
        // edits (the Studio GUI's write path). Prints the edited source to
        // stdout unless --write rewrites the file in place.
        // `forte edit song.forte --sites` — list editable pattern literals as
        // JSON (the read side GUIs bind grids/rolls to).
        Some("edit") if args.len() >= 3 && args[2] == "--sites" => {
            let path = &args[1];
            let src = match load(path) {
                Ok(s) => s,
                Err(c) => return c,
            };
            match fortelang::edit::pattern_sites(&src) {
                Ok(sites) => {
                    println!("{}", serde_json::to_string(&sites).unwrap_or_else(|_| "[]".into()));
                    ExitCode::SUCCESS
                }
                Err(d) => {
                    eprintln!("{path}:{d}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("edit") if args.len() >= 3 => {
            let path = &args[1];
            let json = if args[2] == "-" {
                let mut s = String::new();
                use std::io::Read as _;
                if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                    eprintln!("標準入力が読めません: {e}");
                    return ExitCode::FAILURE;
                }
                s
            } else {
                args[2].clone()
            };
            let write = args.iter().any(|a| a == "--write");
            let src = match load(path) {
                Ok(s) => s,
                Err(c) => return c,
            };
            let ops = match fortelang::edit::parse_ops(&json) {
                Ok(o) => o,
                Err(d) => {
                    eprintln!("{path}:{d}");
                    return ExitCode::FAILURE;
                }
            };
            match fortelang::edit::apply_ops(&src, &ops) {
                Ok(out) => {
                    if write {
                        if out != src {
                            if let Err(e) = std::fs::write(path, &out) {
                                eprintln!("{path}: 書き込めません: {e}");
                                return ExitCode::FAILURE;
                            }
                        }
                        println!("edited : {path}({} 件の編集)", ops.len());
                    } else {
                        print!("{out}");
                    }
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
        // bare `forte init` keeps the classic behaviour (repo in cwd);
        // `forte init NAME` scaffolds a distributable package project (#57)
        #[cfg(not(target_family = "wasm"))]
        Some("init") if args.len() >= 2 => {
            match fortelang::package::init_project(&args[1]) {
                Ok(msg) => {
                    println!("{msg}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("init: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("init") => vcs_print(fortelang::vcs::Repo::init(".")),
        #[cfg(not(target_family = "wasm"))]
        Some("remote") => {
            let result = match args.get(1).map(String::as_str) {
                Some("add") if args.len() >= 3 => fortelang::remote::add(&args[2]),
                _ => Err("usage: forte remote add <github:owner/repo | git-URL>".into()),
            };
            vcs_print(result)
        }
        #[cfg(not(target_family = "wasm"))]
        Some("push") => {
            let msg = args
                .iter()
                .position(|a| a == "-m")
                .and_then(|i| args.get(i + 1))
                .cloned();
            vcs_print(fortelang::remote::push(msg.as_deref()))
        }
        #[cfg(not(target_family = "wasm"))]
        Some("pull") => vcs_print(fortelang::remote::pull()),
        #[cfg(not(target_family = "wasm"))]
        Some("package") => {
            let result = match args.get(1).map(String::as_str) {
                Some("add") if args.len() >= 3 => fortelang::package::add(&args[2]),
                Some("update") if args.len() >= 3 => fortelang::package::update(
                    &args[2],
                    args.iter().any(|a| a == "--force"),
                ),
                Some("list") | None => fortelang::package::list(),
                Some("verify") => fortelang::package::verify(),
                Some("search") => fortelang::package::search(&args[3..].iter().fold(
                    args.get(2).cloned().unwrap_or_default(),
                    |acc, a| format!("{acc} {a}"),
                )),
                Some("sounddiff") if args.len() >= 4 => {
                    fortelang::package::sounddiff(&args[2], &args[3])
                }
                _ => Err("usage: forte package <add SRC | list | verify | search [QUERY] | sounddiff OLD NEW>".into()),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("package: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
            // --from bars(9) / --from 9 — start listening mid-song
            let from_bar = args
                .iter()
                .position(|a| a == "--from")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| {
                    s.trim_start_matches("bars(").trim_end_matches(')').parse::<u32>().ok()
                });
            // --block Name: audition ONE block of a library as the root
            let block = args
                .iter()
                .position(|a| a == "--block")
                .and_then(|i| args.get(i + 1))
                .cloned();
            // .fortesong file or an album directory → the player
            let target = std::path::Path::new(&args[1]);
            if args[1].ends_with(".fortesong") || (target.is_dir() && target.join("album.forte").is_file())
            {
                play_album(&args[1], args.iter().any(|a| a == "--verify"), for_secs)
            } else {
                play(&args[1], for_secs, from_bar, block.as_deref())
            }
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
                Some("edit") if args.len() >= 3 && args.iter().any(|a| a == "--watch") => {
                    fortelang::live::watch(&args[2])
                }
                Some("edit") if args.len() >= 3 => fortelang::live::edit(&args[2]),
                Some("new") if args.len() >= 3 => fortelang::live::new_instrument(&args[2]),
                Some("fix") if args.len() >= 4 => {
                    let mut assigns = Vec::new();
                    let mut bad = None;
                    for a in &args[3..] {
                        match a.split_once('=').and_then(|(k, v)| {
                            v.parse::<f64>().ok().map(|v| (k.trim().to_string(), v))
                        }) {
                            Some(kv) => assigns.push(kv),
                            None => bad = Some(a.clone()),
                        }
                    }
                    match bad {
                        Some(a) => Err(format!("'{a}' が読めません(cutoff=0.6 の形で)")),
                        None => fortelang::live::fix(&args[2], &assigns),
                    }
                }
                // instruments arrive as packages: add = forte package add
                Some("add") if args.len() >= 3 => fortelang::package::add(&args[2])
                    .map(|()| println!("楽器は forte instruments list に載りました(package として導入)")),
                Some("list") => fortelang::live::list(args.get(2).map(String::as_str)),
                // machine-readable name list for shell completion (hidden)
                Some("names") => fortelang::live::names(args.get(2).map(String::as_str)),
                None => fortelang::live::list(None),
                Some(other) => Err(format!(
                    "instruments のサブコマンドは list / play / edit / new / fix / add です。\n\
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
        // the catalog's data as static JSON (GitHub Pages has no /api)
        #[cfg(not(target_family = "wasm"))]
        Some("web") if args.get(1).map(String::as_str) == Some("index") => {
            match repo_root() {
                Some(root) => {
                    println!("{}", fortelang::browser::packages_json(&root));
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("web index: Forte リポジトリの中で実行してください");
                    ExitCode::FAILURE
                }
            }
        }
        Some("version") | Some("--version") | Some("-V") => {
            println!("forte {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: forte check <song.forte>");
            eprintln!("       forte build <song.forte> [-o out.wav | out.fortesong] [--stems]");
            eprintln!("       forte export <song.forte> [-o out.zip]  (曲+履歴+証明の自己完結 zip)");
            eprintln!("       forte play  <song.forte> [--for SECS] [--from bars(9)] [--block Name]  (タイムライン付き再生)");
            eprintln!("       forte play  <name.fortesong | ALBUM-DIR> [--verify]  (プレイヤー: n/p/space/q)");
            eprintln!("       forte repl                  (打った行がその場で鳴る)");
            eprintln!("       forte instruments list [QUERY | path.forte]  (カタログ。list bass / list 808 で絞り込み、path でファイル直接)");
            eprintln!("       forte instruments play <Name[(args)] | path/to/lib.forte[:Name(args)]>  (キーボードが鍵盤に。1..9/-/= でノブ)");
            eprintln!("       forte instruments edit <Name> [--watch] (instruments/ で編集。--watch は保存ごとに自動コミット)");
            eprintln!("       forte instruments new <Name>            (テンプレートから自作楽器を開始)");
            eprintln!("       forte instruments fix <Name> k=v …      (パラメータ固定の派生を instruments/ に書き出し)");
            eprintln!("       forte instruments add <github:owner/repo | PATH> (楽器 package を導入 = package add)");
            eprintln!("       forte instrument <Name[(args)]> [--from lib.forte]  (= instruments play)");
            eprintln!("       forte browser [--port 8000] [--no-open]  (ブラウザエディタを起動)");
            eprintln!("       forte web build             (ブラウザエディタの wasm を再ビルド)");
            eprintln!("       forte web index             (カタログ JSON を出力 — 静的ホスティング用)");
            eprintln!("       forte ci [quick]            (マージゲート: test+clippy/決定論/corpus/E2E)");
            eprintln!("       forte upgrade               (forte コマンド自体を更新)");
            eprintln!("       forte complete bash|zsh     (Tab 補完: source <(forte complete bash))");
            eprintln!("       forte fmt   <song.forte> [--check]");
            eprintln!("       forte edit  <song.forte> <JSON|-> [--write]  (構造編集: コメント/レイアウト保存のままトークンだけ置換)");
            eprintln!("       forte edit  <song.forte> --sites   (編集可能なパターンリテラル一覧を JSON で)");
            eprintln!("       forte test  [PATH…] [--update]  (digest 固定の回帰テスト: forte-test.lock と照合)");
            eprintln!("       forte viz   <song.forte>   (可視化 JSON を出力)");
            eprintln!("       forte analyze <song.forte> [--json] [--no-stems] [--against X.profile]  (聴取レポート + ジャンル目標との照合)");
            eprintln!("       forte lsp");
            eprintln!("       forte init [NAME]           (NAME 付きで package プロジェクトを作成 / なしで cwd をリポジトリに)");
            eprintln!("       forte package add <github:owner/repo[@ref] | URL | PATH>  (packages/ にフラット導入)");
            eprintln!("       forte package list          (導入済み package の一覧と説明)");
            eprintln!("       forte package update <name> [--force]  (再取得+3方マージ — 更新は聴けるレビュー)");
            eprintln!("       forte package verify        (packages/ が lock どおりかを digest で検証)");
            eprintln!("       forte package search [QUERY] (GitHub の topic:forte-package を検索)");
            eprintln!("       forte package sounddiff <OLD> <NEW> (どの音が変わったか + version bump 提案)");
            eprintln!("       forte remote add <github:owner/repo | git-URL>  (プロジェクトを GitHub と接続)");
            eprintln!("       forte push [-m \"メッセージ\"]   (プロジェクト全体を origin へ。これが配信)");
            eprintln!("       forte pull                  (origin から取り込み)");
            eprintln!("       forte status");
            eprintln!("       forte commit -m \"メッセージ\"");
            eprintln!("       forte log");
            eprintln!("       forte branch [NAME]");
            eprintln!("       forte checkout <branch|hash>");
            eprintln!("       forte merge <branch>        (競合しない編集は自動で合流)");
            eprintln!("       forte diff [REV [REV]]      (音楽の言葉で差分。既定 HEAD↔作業)");
            ExitCode::from(2)
        }
    }
}

/// Walk up from the cwd to the repository root (the dir holding Cargo.toml
/// with crates/), so repo-wide commands work from any subdirectory.
#[cfg(not(target_family = "wasm"))]
fn repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("forte/crates/fortelang/Cargo.toml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(not(target_family = "wasm"))]
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
    let core = root.join("forte");
    let cargo = |args: &[&str]| {
        let mut c = std::process::Command::new("cargo");
        c.args(args).current_dir(&core);
        c
    };
    let script = |path: &str| {
        let mut c = std::process::Command::new(core.join(path));
        c.current_dir(&core);
        c
    };
    let ok = run_step("1/4 cargo test", cargo(&["test", "--release", "-p", "dawcore", "-p", "fortelang"]))
        && run_step(
            "1/4 clippy (-D warnings)",
            cargo(&["clippy", "--release", "-p", "dawcore", "-p", "fortelang", "--all-targets", "--", "-D", "warnings"]),
        )
        && run_step("2/4 determinism gate", script("scripts/determinism_test.sh"))
        && (quick || run_step("3/4 corpus", script("scripts/check_corpus.sh")))
        && (quick || run_step("3.5/4 edit→sound latency", script("scripts/latency_bench.sh")))
        && (quick || {
            // E2E needs playwright; skip gracefully when absent
            if core.join("node_modules/playwright").is_dir() {
                let mut a = std::process::Command::new("node");
                a.arg(core.join("scripts/web_e2e.mjs")).current_dir(&core);
                run_step("4/4 web E2E", a)
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
            .current_dir(root.join("forte"));
        c
    });
    if !ok {
        eprintln!("hint: rustup target add wasm32-unknown-unknown");
        return ExitCode::FAILURE;
    }
    let src = root.join("forte/target/wasm32-unknown-unknown/release/forteweb.wasm");
    let dst = root.join("forte/web/forte.wasm");
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
        COMPREPLY=($(compgen -W "check build play export repl instrument instruments browser web ci upgrade version fmt viz lsp init status commit log branch checkout merge diff package remote push pull complete" -- "$cur"))
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
    // a prebuilt release binary beats compiling — try that path first
    #[cfg(not(target_family = "wasm"))]
    match fortelang::selfupdate::try_release_upgrade() {
        Ok(Some(msg)) => {
            println!("{msg}");
            return ExitCode::SUCCESS;
        }
        Ok(None) => {} // no release/asset for this platform → build from source
        Err(e) => println!("release バイナリを使えません({e})— ソースからビルドします"),
    }
    // find a checkout (crates/fortelang next to us or above the cwd)
    let mut checkout = None;
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            if dir.join("forte/crates/fortelang/Cargo.toml").is_file() {
                checkout = Some(dir.join("forte/crates/fortelang"));
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

/// `forte analyze` の人間向け表示(機械は --json を読む)。
fn print_analysis(a: &fortelang::analyze::Analysis) {
    use fortelang::analyze::BAND_NAMES;
    println!("== 聴取レポート ({:.1}s, {} bpm) ==", a.seconds, a.tempo);
    println!("-- ラウドネス --");
    println!(
        "  integrated {} LUFS / true peak {} dBTP / rms {} dB / crest {} dB",
        a.loudness.integrated_lufs, a.loudness.true_peak_db, a.loudness.rms_db, a.loudness.crest_db
    );
    println!("-- 帯域バランス (mix) --");
    let shares: Vec<String> = BAND_NAMES
        .iter()
        .zip(a.spectral.band_share_pct.iter())
        .map(|(n, v)| format!("{n} {v}%"))
        .collect();
    println!("  {}", shares.join(" / "));
    for t in &a.spectral.tracks {
        let shares: Vec<String> = BAND_NAMES
            .iter()
            .zip(t.band_share_pct.iter())
            .map(|(n, v)| format!("{n} {v}%"))
            .collect();
        println!("  track {} ({} dB): {}", t.name, t.rms_db, shares.join(" / "));
    }
    for m in a.spectral.masking.iter().take(3) {
        if m.overlap >= 0.6 {
            println!("  ⚠ 帯域かぶり {} × {} = {}", m.a, m.b, m.overlap);
        }
    }
    println!("-- ステレオ --");
    let bands: Vec<String> = BAND_NAMES
        .iter()
        .zip(a.stereo.band_side_mid_db.iter())
        .map(|(n, v)| format!("{n} {v}"))
        .collect();
    println!("  side/mid {} dB (帯域別 dB: {})", a.stereo.side_mid_db, bands.join(" / "));
    println!("-- リズム --");
    println!(
        "  譜面 {} 発 / 検出 {} 発 / 一致 {}% (平均ズレ {} ms)",
        a.rhythm.score_onsets, a.rhythm.audio_onsets, a.rhythm.matched_pct, a.rhythm.mean_offset_ms
    );
    for d in &a.rhythm.density_per_section {
        println!("  {}: {} 発/秒", d.name, d.onsets_per_second);
    }
    println!("-- 構成 --");
    for s in &a.structure.sections {
        println!(
            "  {} [{}s..{}s] rms {} dB / peak {} dB",
            s.name, s.start_s, s.end_s, s.rms_db, s.peak_db
        );
    }
    println!(
        "  無音 {} 箇所 ({}%)",
        a.structure.silences.len(),
        a.structure.silence_total_pct
    );
    println!("-- 調性 --");
    println!(
        "  推定 {} / 宣言 {}{}",
        a.tonality.estimated_key,
        a.tonality.declared_key,
        match (a.tonality.agrees, a.tonality.relative) {
            (Some(true), _) => " (一致)",
            (Some(false), true) => " (相対調 — 同じ音組織)",
            (Some(false), false) => " (⚠ 不一致)",
            (None, _) => "",
        }
    );
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

#[cfg(not(target_family = "wasm"))]
/// Terminal column count. The tty itself is asked first (TIOCGWINSZ on
/// stdout/stderr/stdin) — `stty` and `$COLUMNS` lie or vanish depending on
/// the platform, and a too-wide guess makes clamped lines wrap anyway,
/// which breaks the in-place redraw's cursor-up arithmetic.
fn term_cols() -> usize {
    #[cfg(unix)]
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        for fd in [libc::STDOUT_FILENO, libc::STDERR_FILENO, libc::STDIN_FILENO] {
            if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col >= 20 {
                return ws.ws_col as usize;
            }
        }
    }
    if let Ok(o) = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
    {
        if let Ok(t) = String::from_utf8(o.stdout) {
            if let Some(c) = t.split_whitespace().nth(1).and_then(|v| v.parse::<usize>().ok()) {
                if c >= 20 {
                    return c;
                }
            }
        }
    }
    std::env::var("COLUMNS").ok().and_then(|v| v.parse().ok()).filter(|&c| c >= 20).unwrap_or(80)
}

#[cfg(not(target_family = "wasm"))]
/// Clamp a frame line to the terminal width counting VISIBLE chars only
/// (ANSI escapes pass through). The in-place redraw moves the cursor up
/// by the logical line count — one wrapped line (a long `desc`) breaks
/// that math and the whole header scrolls away on every tick.
fn clamp_visible(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut vis = 0usize;
    let mut chars = s.chars();
    let mut truncated = false;
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            for c2 in chars.by_ref() {
                out.push(c2);
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if vis + 1 >= max {
            truncated = true;
            break;
        }
        out.push(c);
        vis += 1;
    }
    if truncated {
        out.push('…');
        out.push_str("\x1b[0m");
    }
    out
}

/// Live playback with hot reload: the song loops while the file is watched;
/// every successful recompile is swapped into the running engine without
/// stopping the transport — listen, edit, listen (SYS-EDT-002 minimal form).
#[cfg(not(target_family = "wasm"))]
fn play(path: &str, for_secs: Option<f64>, from_bar: Option<u32>, block: Option<&str>) -> ExitCode {
    use dawcore::command::Command;
    use dawcore::model::Project;
    use dawcore::sync::full_sync;
    use std::io::{IsTerminal, Write as _};
    use std::time::{Duration, Instant, SystemTime};

    fn compile_file(path: &str, block: Option<&str>) -> Result<Project, Vec<String>> {
        let mut src = std::fs::read_to_string(path).map_err(|e| vec![format!("{path}: {e}")])?;
        if let Some(b) = block {
            // an empty heir of the block becomes the LAST definition, so the
            // chosen block roots the build with its own tempo/key intact
            src.push_str(&format!("
block __Probe : {b} {{}}
"));
        }
        fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base_dir(path))
            .map_err(|diags| diags.iter().map(|d| format!("{path}:{d}")).collect())
    }
    fn apply(
        handle: &mut dawcore::engine::EngineHandle,
        p: &Project,
        prev_slots: usize,
        from_bar: Option<u32>,
    ) {
        full_sync(handle, p);
        for slot in p.tracks.len()..prev_slots {
            handle.send(Command::RemoveTrack { slot });
        }
        let len = dawcore::bounce::arrangement_len(p);
        // --from bars(9): the loop starts (and restarts) at that bar
        let bpb = p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64;
        let start = from_bar
            .map(|b| ((b.max(1) - 1) as f64 * bpb).min((len - bpb).max(0.0)))
            .unwrap_or(0.0);
        handle.send(Command::SetLoop { enabled: true, start, end: len });
        handle.send(Command::SetLaunchQuant(0.0));
        if start > 0.0 {
            // Stop parks the playhead at the loop start
            handle.send(Command::Stop);
        }
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
        let mut out = Vec::with_capacity(p.tracks.len() + 4);
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

    let mut project = match compile_file(path, block) {
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
    let mut cols = if tty { term_cols() } else { usize::MAX };
    if audio.silent {
        eprintln!("audio: 出力デバイスなし — 無音バックエンドで走行します({})", audio.device_name);
    } else {
        println!("audio: {}", audio.device_name);
    }
    apply(&mut audio.handle, &project, 0, from_bar);
    audio.handle.send(Command::Play);
    println!("playing: \"{path}\"");
    if !project.desc.is_empty() {
        // the piece's own words, printed ONCE above the redraw region — a
        // desc inside the repainted frame multiplies forever the moment the
        // line wraps (wrong width guess, resized terminal, anything)
        println!("{}", clamp_visible(&format!("\x1b[1m{}\x1b[0m — {}", project.name, project.desc), cols));
    }

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
            match compile_file(path, block) {
                Ok(p) => {
                    let prev = project.tracks.len();
                    apply(&mut audio.handle, &p, prev, from_bar);
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
                cols = term_cols(); // per frame: tracks live resizes (ioctl)
                let frame = render(&project, pos, loops, peak, &message);
                let mut out = String::new();
                if drawn > 0 {
                    out.push_str(&format!("\x1b[{drawn}A"));
                }
                out.push_str("\r\x1b[J");
                for line in &frame {
                    out.push_str(&clamp_visible(line, cols));
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

/// The album player (issue #53): `.fortesong` tracks with next/prev/pause.
/// A single .fortesong plays as a one-track album.
#[cfg(not(target_family = "wasm"))]
fn play_album(path: &str, verify: bool, for_secs: Option<f64>) -> ExitCode {
    use dawcore::command::Command;
    use std::io::{IsTerminal, Read as _, Write as _};
    use std::time::{Duration, Instant};

    let p = std::path::Path::new(path);
    let (title, artist, desc, track_paths) = if p.is_dir() {
        match fortelang::songfile::load_album(p) {
            Ok(Some(a)) => (a.title, a.artist, a.desc, a.tracks),
            Ok(None) => {
                eprintln!("play: {} に album.forte がありません", p.display());
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("play: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        (String::new(), String::new(), String::new(), vec![p.to_path_buf()])
    };

    // load every track up front: files digest is checked here, so a
    // tampered album refuses to play before the first note
    let mut songs = Vec::new();
    for t in &track_paths {
        match fortelang::songfile::load(&t.to_string_lossy()) {
            Ok(sf) => songs.push(sf),
            Err(e) => {
                eprintln!("play: {}: {e}", t.display());
                return ExitCode::FAILURE;
            }
        }
    }
    if verify {
        for sf in &songs {
            print!("verify {} … ", sf.name);
            let _ = std::io::stdout().flush();
            match fortelang::songfile::verify(sf) {
                Ok(msg) => println!("{msg}"),
                Err(e) => {
                    println!("{e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    let mut audio = fortelang::audio::start();
    if audio.silent {
        eprintln!("audio: 出力デバイスなし — 無音バックエンドで走行します({})", audio.device_name);
    }
    let tty = std::io::stdout().is_terminal();
    let _raw = if tty { Some(fortelang::live::RawTerm::enter()) } else { None };

    let mm_ss = |s: f64| format!("{}:{:04.1}", (s / 60.0) as i64, s % 60.0);
    let started = Instant::now();
    let mut idx = 0usize;
    let mut drawn = 0usize;
    'album: while idx < songs.len() {
        let sf = &songs[idx];
        let project = match fortelang::songfile::compile(sf) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("play: {}: {e}", sf.name);
                return ExitCode::FAILURE;
            }
        };
        let len_beats = dawcore::bounce::arrangement_len(&project).max(1.0);
        let total_secs = len_beats * 60.0 / project.tempo;

        audio.handle.send(Command::Stop);
        dawcore::sync::full_sync(&mut audio.handle, &project);
        for slot in project.tracks.len()..dawcore::model::MAX_TRACKS {
            audio.handle.send(Command::RemoveTrack { slot });
        }
        audio.handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
        audio.handle.send(Command::SetLaunchQuant(0.0));
        audio.handle.send(Command::Play);
        let mut paused = false;
        if !tty {
            println!("track {}/{}: {}{}", idx + 1, songs.len(), sf.name, if sf.artist.is_empty() { String::new() } else { format!(" — {}", sf.artist) });
        }

        loop {
            std::thread::sleep(Duration::from_millis(50));
            audio.handle.collect_garbage();

            // keys: n next / p prev / space pause / q quit
            if tty {
                let mut buf = [0u8; 8];
                let n = std::io::stdin().read(&mut buf).unwrap_or(0);
                for &b in &buf[..n] {
                    match b {
                        b'q' | 3 => {
                            println!();
                            break 'album;
                        }
                        b'n' => {
                            idx += 1;
                            if idx >= songs.len() {
                                println!();
                                break 'album;
                            }
                            continue 'album;
                        }
                        b'p' => {
                            idx = idx.saturating_sub(1);
                            continue 'album;
                        }
                        b' ' => {
                            paused = !paused;
                            audio.handle.send(if paused { Command::Pause } else { Command::Play });
                        }
                        _ => {}
                    }
                }
            }

            let pos = audio.handle.shared.position_beats();
            let secs = pos * 60.0 / project.tempo;

            // end of track (+2s tail for reverbs) → auto-advance
            if !paused && secs >= total_secs + 2.0 {
                idx += 1;
                if idx >= songs.len() {
                    println!();
                    break 'album;
                }
                continue 'album;
            }
            if let Some(t) = for_secs {
                if started.elapsed().as_secs_f64() >= t {
                    println!();
                    break 'album;
                }
            }

            if tty {
                let cols = term_cols(); // per frame: tracks live resizes (ioctl)
                let mut frame = Vec::with_capacity(songs.len() + 4);
                let head = if title.is_empty() {
                    format!("\x1b[1m♪ {}\x1b[0m{}", sf.name, if sf.artist.is_empty() { String::new() } else { format!(" — {}", sf.artist) })
                } else {
                    format!(
                        "\x1b[1m♪ {title}\x1b[0m{}  ({} tracks)",
                        if artist.is_empty() { String::new() } else { format!(" — {artist}") },
                        songs.len()
                    )
                };
                frame.push(head);
                if idx == 0 && !desc.is_empty() {
                    frame.push(format!("\x1b[2m{desc}\x1b[0m"));
                }
                if !sf.credits.is_empty() {
                    frame.push(format!("\x1b[2mcredits: {}\x1b[0m", sf.credits.join(", ")));
                }
                for (i, s) in songs.iter().enumerate() {
                    let mark = if i == idx { "▶" } else { " " };
                    let line = format!(
                        "{mark} {:>2}  {:<28} {}",
                        i + 1,
                        s.name,
                        mm_ss(s.seconds.max(0.0))
                    );
                    if i == idx {
                        frame.push(format!("\x1b[1m{line}\x1b[0m"));
                    } else {
                        frame.push(format!("\x1b[2m{line}\x1b[0m"));
                    }
                }
                const BAR_W: usize = 36;
                let frac = (secs / total_secs).clamp(0.0, 1.0);
                let fill = (frac * BAR_W as f64) as usize;
                let bar: String =
                    (0..BAR_W).map(|i| if i < fill { '█' } else { '·' }).collect();
                frame.push(format!(
                    "{} {} / {} ▕{bar}▏ space=pause n=next p=prev q=quit",
                    if paused { "⏸" } else { "▶" },
                    mm_ss(secs.min(total_secs)),
                    mm_ss(total_secs),
                ));
                let mut out = String::new();
                if drawn > 0 {
                    out.push_str(&format!("\x1b[{drawn}A"));
                }
                out.push_str("\r\x1b[J");
                for line in &frame {
                    out.push_str(&clamp_visible(line, cols));
                    out.push('\n');
                }
                drawn = frame.len();
                print!("{out}");
                let _ = std::io::stdout().flush();
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
