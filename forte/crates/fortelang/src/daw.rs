//! `forte daw [PROJECT]` — THE Forte DAW, package-scoped (ADR D-15).
//!
//! One app: the browser editor (web/) opened on a real `forte init`
//! package. `forte browser` serves the same assets in demo/catalog mode;
//! `forte daw` adds the project API that turns it into the DAW — the
//! basic DAW works per song, Forte works per PACKAGE: define blocks,
//! vendor other people's packages, grow blocks into songs and albums,
//! all inside one project directory.
//!
//! The API (all project-relative, traversal-guarded):
//!
//! - `GET  /api/project`             — the editing inventory (`project_json`)
//! - `GET  /api/list?ext=.forte`     — every project file with that extension
//! - `GET  /api/modules`             — `{path: source}` of every `.forte` (import map)
//! - `GET  /api/assets`              — `{path: base64}` of every `.frec` take
//! - `GET  /api/src?path=REL`        — read one file
//! - `POST /api/src?path=REL`        — write one file (body = content)
//! - `POST /api/new?kind=block|song&name=N` — scaffold from a template
//! - `POST /api/pkg?spec=SRC`        — vendor a package (`forte package add`)
//! - `GET  /api/packages`            — the vendored-package catalog

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use crate::browser::{content_type, find_web_root, open_url, packages_json, respond};

/// Directories the file walk never enters (VCS internals, build junk).
const SKIP_DIRS: &[&str] = &[".git", ".forte", "target", "node_modules"];

pub fn run(project: &Path, port: u16, open: bool) -> Result<(), String> {
    let project = project
        .canonicalize()
        .map_err(|e| format!("{}: {e}", project.display()))?;
    if !project.join("package.forte").is_file() {
        return Err(format!(
            "{} に package.forte がありません(まず forte init <名前> でパッケージを作ってください)",
            project.display()
        ));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let web_root = find_web_root(&cwd)
        .or_else(|| std::env::current_exe().ok().and_then(|e| find_web_root(&e)))
        .ok_or_else(|| {
            "forte/web/index.html が見つかりません(Forte リポジトリの中で実行してください)".to_string()
        })?;
    // official starter material comes INSTALLED: a project with an empty
    // packages/ gets essentials vendored before the first window opens
    let no_packages = std::fs::read_dir(project.join("packages"))
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).count() == 0)
        .unwrap_or(true);
    if no_packages {
        if let Ok(rd) = std::fs::read_dir(web_root.join("packages")) {
            let mut starters: Vec<_> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.join("package.forte").is_file()
                        && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("essentials"))
                })
                .collect();
            starters.sort();
            if let (Some(spec), Ok(exe)) = (starters.first(), std::env::current_exe()) {
                println!("essentials を同梱中…(初回のみ)");
                let ok = std::process::Command::new(exe)
                    .args(["package", "add", &spec.to_string_lossy()])
                    .current_dir(&project)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                println!(
                    "{}",
                    if ok { "essentials 導入済み — パレットとライブラリに素材が入っています" } else { "essentials の自動導入に失敗(📦 ボタンから手動で追加できます)" }
                );
            }
        }
    }
    // the compiler the browser runs IS a build artifact: build it when it
    // is missing or older than the Rust sources (git pull safety)
    ensure_wasm(&web_root);
    // no wasm = the page cannot boot; a clear abort beats a broken window
    if !web_root.join("forte/web/forte.wasm").exists() {
        return Err(format!(
            "web/forte.wasm がありません(wasm ビルド失敗)。\n\
             修復: rustup target add wasm32-unknown-unknown && cd {} && forte web build\n\
             その後もう一度 forte daw を実行してください",
            web_root.display()
        ));
    }
    // a brand-new package opens PLAYING-READY: scaffold the demo song so
    // the first screen is a full arrangement, never an empty editor
    let count = |sub: &str| {
        std::fs::read_dir(project.join(sub))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".forte"))
                    .count()
            })
            .unwrap_or(0)
    };
    if count("songs") == 0
        && count("blocks") == 0
        && new_element(&project, "demo", "demo").is_ok()
    {
        println!("デモ曲を作成 (songs/demo.forte) — 開いたら space で鳴ります");
    }
    // the agent's briefing: a CLAUDE.md in the project teaches Claude Code
    // (running in the embedded terminal) how music is made here
    let agent_md = project.join("CLAUDE.md");
    if !agent_md.exists() {
        let _ = std::fs::write(&agent_md, AGENT_BRIEFING);
        println!("CLAUDE.md を作成(ターミナルの `claude` が最初から作曲手順を知っています)");
    }
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let url = format!("http://localhost:{port}/forte/web/");
    println!("Forte DAW: {url}(project: {}、Ctrl+C で終了)", project.display());
    if open {
        open_app(&url);
    }
    serve(listener, web_root, project);
    Ok(())
}

/// Build web/forte.wasm when it is absent or stale relative to the Rust
/// sources — `git pull` must never leave the DAW serving yesterday's
/// compiler (or a 404 that boots as 'not found').
fn ensure_wasm(web_root: &Path) {
    let wasm = web_root.join("forte/web/forte.wasm");
    let wasm_mtime = std::fs::metadata(&wasm).and_then(|m| m.modified()).ok();
    let mut newest_src: Option<std::time::SystemTime> = None;
    fn walk(dir: &Path, newest: &mut Option<std::time::SystemTime>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, newest);
            } else if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                    if newest.map(|n| t > n).unwrap_or(true) {
                        *newest = Some(t);
                    }
                }
            }
        }
    }
    walk(&web_root.join("forte/crates"), &mut newest_src);
    let stale = match (wasm_mtime, newest_src) {
        (None, _) => true,
        (Some(w), Some(srct)) => srct > w,
        (Some(_), None) => false,
    };
    if !stale {
        return;
    }
    println!("web/forte.wasm を{}ビルド中…(初回は数分)", if wasm_mtime.is_none() { "" } else { "更新" });
    let ok = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            std::process::Command::new(exe)
                .args(["web", "build"])
                .current_dir(web_root)
                .status()
                .ok()
        })
        .map(|st| st.success())
        .unwrap_or(false);
    if !ok {
        println!(
            "web/forte.wasm のビルドに失敗しました。手動で: cd {} && forte web build\n(初回は rustup target add wasm32-unknown-unknown が必要です)",
            web_root.display()
        );
    }
}

/// Open the DAW as a LOCAL APP window (chromeless `--app=` mode) when a
/// Chromium-family browser is installed; fall back to the default browser
/// tab otherwise. `FORTE_DAW_BROWSER` overrides the binary; the Studio
/// fork (ADR D-14, F4) is the real desktop shell — this is the interim.
fn open_app(url: &str) {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(over) = std::env::var_os("FORTE_DAW_BROWSER") {
        candidates.push(over.to_string_lossy().into_owned());
    }
    #[cfg(target_os = "macos")]
    candidates.extend(
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
        .iter()
        .map(|s| s.to_string()),
    );
    #[cfg(not(target_os = "macos"))]
    candidates.extend(
        ["google-chrome", "chromium", "chromium-browser", "brave", "microsoft-edge"]
            .iter()
            .map(|s| s.to_string()),
    );
    for bin in candidates {
        let found = std::path::Path::new(&bin).is_file()
            || std::process::Command::new("which")
                .arg(&bin)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
        if !found {
            continue;
        }
        if std::process::Command::new(&bin).arg(format!("--app={url}")).spawn().is_ok() {
            println!("app window: {bin}(--app モード)");
            return;
        }
    }
    open_url(url); // no Chromium family found: a normal browser tab
}

/// The accept loop, separated so tests can drive it on an ephemeral port.
pub fn serve(listener: TcpListener, web_root: PathBuf, project: PathBuf) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let (web_root, project) = (web_root.clone(), project.clone());
        std::thread::spawn(move || {
            let _ = handle(&web_root, &project, stream);
        });
    }
}

/// Read one HTTP request: request line, headers (only Content-Length is
/// interesting), then exactly the announced body.
fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, String, String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break buf.len();
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end.min(buf.len())]).into_owned();
    let mut lines = head.lines();
    let req = lines.next().unwrap_or("");
    let mut it = req.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let target = it.next().unwrap_or("/").to_string();
    let clen: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = buf[header_end.min(buf.len())..].to_vec();
    while body.len() < clen {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(clen);
    Ok((method, target, head, body))
}

fn handle(web_root: &Path, project: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    let (method, target, head, body) = read_request(&mut stream)?;
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));

    // the embedded terminal: `GET /term` upgrades to a WebSocket that pumps
    // a PTY running the user's shell in the PROJECT directory — Claude Code
    // (or any agent) runs right inside the DAW. localhost-only by bind.
    if method == "GET" && path.ends_with("/term") {
        return term::serve_terminal(stream, &head, project);
    }

    // the API mounts wherever the app is served from ("/api/…" and
    // "/forte/web/api/…" are the same call)
    if let Some(i) = path.find("/api/") {
        let ep = &path[i + "/api/".len()..];
        // starter packages ship with the forte repo itself: offer them for
        // one-click vendoring (the specs are local paths package add takes)
        if method == "GET" && ep == "starters" {
            let mut dirs: Vec<PathBuf> = std::fs::read_dir(web_root.join("packages"))
                .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("package.forte").is_file()).collect())
                .unwrap_or_default();
            dirs.sort();
            let list: Vec<serde_json::Value> = dirs
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "name": d.file_name().unwrap_or_default().to_string_lossy(),
                        "spec": d.to_string_lossy(),
                    })
                })
                .collect();
            return respond(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                serde_json::to_string(&list).unwrap_or_default().as_bytes(),
            );
        }
        return api(project, &method, ep, query, &body, &mut stream);
    }

    // static: same layout as `forte browser` so relative fetches keep working
    let rel = match path {
        "/" => "forte/web/index.html".to_string(),
        p => {
            let p = p.trim_start_matches('/');
            if p.ends_with('/') { format!("{p}index.html") } else { p.to_string() }
        }
    };
    if PathBuf::from(&rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden");
    }
    let file = web_root.join(&rel);
    match std::fs::read(&file) {
        Ok(b) => respond(&mut stream, "200 OK", content_type(&file), &b),
        Err(_) => respond(&mut stream, "404 Not Found", "text/plain; charset=utf-8", b"not found"),
    }
}

fn api(
    project: &Path,
    method: &str,
    ep: &str,
    query: &str,
    body: &[u8],
    stream: &mut TcpStream,
) -> std::io::Result<()> {
    let json = "application/json; charset=utf-8";
    let text = "text/plain; charset=utf-8";
    let q = |key: &str| -> Option<String> {
        query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| url_decode(v))
        })
    };
    match (method, ep) {
        ("GET", "project") => match crate::project::project_json(project) {
            Ok(v) => respond(stream, "200 OK", json, v.to_string().as_bytes()),
            Err(e) => respond(stream, "500 Internal Server Error", text, e.as_bytes()),
        },
        ("GET", "packages") => {
            respond(stream, "200 OK", json, packages_json(project).as_bytes())
        }
        ("GET", "stamp") => {
            let mut acc: u64 = 0;
            for f in list_files(project, ".forte") {
                if let Ok(md) = std::fs::metadata(project.join(&f)) {
                    let mt = md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    acc = acc
                        .wrapping_mul(1099511628211)
                        .wrapping_add(mt)
                        .wrapping_add(md.len())
                        .wrapping_add(f.len() as u64);
                }
            }
            respond(
                stream,
                "200 OK",
                json,
                serde_json::json!({ "stamp": format!("{acc:x}") }).to_string().as_bytes(),
            )
        }
        ("GET", "list") => {
            let ext = q("ext").unwrap_or_else(|| ".forte".into());
            let files = list_files(project, &ext);
            respond(stream, "200 OK", json, serde_json::to_string(&files).unwrap_or_default().as_bytes())
        }
        ("GET", "modules") => {
            let mut map = serde_json::Map::new();
            for f in list_files(project, ".forte") {
                if let Ok(s) = std::fs::read_to_string(project.join(&f)) {
                    map.insert(f, serde_json::Value::String(s));
                }
            }
            respond(stream, "200 OK", json, serde_json::Value::Object(map).to_string().as_bytes())
        }
        ("GET", "assets") => {
            let mut map = serde_json::Map::new();
            for f in list_files(project, ".frec") {
                if let Ok(b) = std::fs::read(project.join(&f)) {
                    map.insert(f, serde_json::Value::String(base64(&b)));
                }
            }
            respond(stream, "200 OK", json, serde_json::Value::Object(map).to_string().as_bytes())
        }
        ("GET", "src") => {
            let Some(rel) = q("path").filter(|p| safe_rel(p)) else {
                return respond(stream, "400 Bad Request", text, "path が必要です".as_bytes());
            };
            match std::fs::read(project.join(&rel)) {
                Ok(b) => respond(stream, "200 OK", text, &b),
                Err(_) => respond(stream, "404 Not Found", text, b"not found"),
            }
        }
        ("POST", "src") => {
            let Some(rel) = q("path").filter(|p| safe_rel(p)) else {
                return respond(stream, "400 Bad Request", text, "path が必要です".as_bytes());
            };
            let dst = project.join(&rel);
            if let Some(dir) = dst.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::write(&dst, body) {
                Ok(()) => respond(stream, "200 OK", json, b"{\"ok\":true}"),
                Err(e) => respond(stream, "500 Internal Server Error", text, e.to_string().as_bytes()),
            }
        }
        ("POST", "edit") => {
            // apply edit ops to a file ON DISK (not the open buffer) — the
            // mixer's route for tracks that live in an imported block
            let Some(rel) = q("path").filter(|p| safe_rel(p)) else {
                return respond(stream, "400 Bad Request", text, "path が必要です".as_bytes());
            };
            let dst = project.join(&rel);
            let src = match std::fs::read_to_string(&dst) {
                Ok(s) => s,
                Err(_) => return respond(stream, "404 Not Found", text, b"not found"),
            };
            let ops_json = String::from_utf8_lossy(body);
            let out = crate::edit::parse_ops(&ops_json)
                .and_then(|ops| crate::edit::apply_ops(&src, &ops));
            match out {
                Ok(new_src) => {
                    if let Err(e) = std::fs::write(&dst, &new_src) {
                        return respond(stream, "500 Internal Server Error", text, e.to_string().as_bytes());
                    }
                    respond(stream, "200 OK", text, new_src.as_bytes())
                }
                Err(d) => respond(stream, "400 Bad Request", text, d.to_string().as_bytes()),
            }
        }
        ("POST", "new") => {
            let (kind, name) = (q("kind").unwrap_or_default(), q("name").unwrap_or_default());
            match new_element(project, &kind, &name) {
                Ok(rel) => respond(
                    stream,
                    "200 OK",
                    json,
                    serde_json::json!({ "file": rel }).to_string().as_bytes(),
                ),
                Err(e) => respond(stream, "400 Bad Request", text, e.as_bytes()),
            }
        }
        ("POST", "export") => {
            // render the file to renders/<name>.wav via `forte build`
            let Some(rel) = q("path").filter(|p| safe_rel(p) && p.ends_with(".forte")) else {
                return respond(stream, "400 Bad Request", text, "path が必要です".as_bytes());
            };
            let stem = std::path::Path::new(&rel)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "out".into());
            let out_rel = format!("renders/{stem}.wav");
            let _ = std::fs::create_dir_all(project.join("renders"));
            let exe = std::env::current_exe().map_err(std::io::Error::other)?;
            let out = std::process::Command::new(exe)
                .args(["build", &rel, "-o", &out_rel])
                .current_dir(project)
                .output()?;
            if out.status.success() && project.join(&out_rel).is_file() {
                respond(
                    stream,
                    "200 OK",
                    json,
                    serde_json::json!({ "file": out_rel }).to_string().as_bytes(),
                )
            } else {
                let msg = [out.stdout.as_slice(), out.stderr.as_slice()].concat();
                respond(stream, "500 Internal Server Error", text, &msg)
            }
        }
        ("POST", "pkg") => {
            let Some(spec) = q("spec").filter(|s| !s.is_empty()) else {
                return respond(stream, "400 Bad Request", text, "spec が必要です".as_bytes());
            };
            // run in a subprocess so the add's cwd is the project without
            // touching this server's global cwd
            let exe = std::env::current_exe().map_err(std::io::Error::other)?;
            let out = std::process::Command::new(exe)
                .args(["package", "add", &spec])
                .current_dir(project)
                .output()?;
            let msg = [out.stdout.as_slice(), out.stderr.as_slice()].concat();
            if out.status.success() {
                respond(stream, "200 OK", text, &msg)
            } else {
                respond(stream, "500 Internal Server Error", text, &msg)
            }
        }
        _ => respond(stream, "404 Not Found", text, b"unknown api"),
    }
}

/// Project-relative file list (sorted), never entering VCS/build dirs.
pub(crate) fn list_files(project: &Path, ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, prefix: &str, ext: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            if p.is_dir() {
                if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                walk(&p, &format!("{prefix}{name}/"), ext, out);
            } else if name.ends_with(ext) {
                out.push(format!("{prefix}{name}"));
            }
        }
    }
    walk(project, "", ext, &mut out);
    out.sort();
    out
}

/// True when `rel` stays inside the project (no traversal, no absolutes).
pub(crate) fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && PathBuf::from(rel)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// Scaffold a new block/song from a template. Returns the created file's
/// project-relative path; refuses bad names and existing files.
pub(crate) fn new_element(project: &Path, kind: &str, name: &str) -> Result<String, String> {
    let ok_name = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok_name {
        return Err(format!("名前は英字で始まる英数字/-/_ で指定します(見つかったのは \"{name}\")"));
    }
    let (rel, body) = match kind {
        "block" => (
            format!("blocks/{name}.forte"),
            format!(
                "block {name} {{\n  desc \"\"\n\n  track Drums {{\n    instrument sampler(sample: \"Kick\")\n    play beat`x... x... x... x...` at bars(1..4)\n  }}\n}}\n"
            ),
        ),
        "song" => (
            format!("songs/{name}.forte"),
            format!(
                "song \"{name}\" {{\n  tempo 120bpm\n\n  track Drums {{\n    instrument sampler(sample: \"Kick\")\n    play beat`x... x... x... x...` at bars(1..4)\n  }}\n\n  track Bass {{\n    instrument prisma(wave: \"saw\", cutoff: 0.4, sub: 0.6)\n    volume 0.75\n    play notes`C2:1 _:1 Eb2:0.5 _:0.5 G1:1` at bars(1..4)\n  }}\n}}\n"
            ),
        ),
        // a one-click STARTER: built-ins only, grooves immediately, and
        // shows off grid + roll + sections + mixer statements to learn from
        "demo" => (
            format!("songs/{name}.forte"),
            format!(
                "// {name} — a starter groove made from the built-ins.\n\
                 // Press space to play. Touching the grid / roll / mixer\n\
                 // rewrites this very code.\n\
                 song \"{name}\" {{\n  tempo 122bpm\n\n  section groove = bars(1..8)\n  section lift   = bars(9..16)\n\n\
                 \x20 track Kick {{\n    instrument sampler(sample: \"Kick\")\n    volume 0.9\n    play beat`x... x... x... x...` at bars(1..16)\n  }}\n\n\
                 \x20 track Hats {{\n    instrument sampler(sample: \"Hat\")\n    volume 0.5\n    pan 0.15\n    play beat`..x. ..x. ..x. ..x.` at groove\n    play beat`..x. ..x. ..x. ..xx` at lift\n  }}\n\n\
                 \x20 track Snare {{\n    instrument sampler(sample: \"Snare\")\n    volume 0.7\n    play beat`.... x... .... x...` at bars(1..16)\n  }}\n\n\
                 \x20 track Bass {{\n    instrument prisma(wave: \"saw\", cutoff: 0.35, reso: 0.3, sub: 0.6)\n    volume 0.75\n    play notes`C2:0.5 _:0.5 C2:0.5 _:0.5 Eb2:0.5 _:0.5 G1:0.5 _:0.5` at bars(1..16)\n  }}\n\n\
                 \x20 track Pad {{\n    instrument prisma(wave: \"saw\", cutoff: 0.45, attack: 0.4, release: 0.6, unison: 5, spread: 0.7)\n    volume 0.5\n    play notes`[C3 Eb3 G3]:4 [Ab2 C3 Eb3]:4` at lift\n  }}\n}}\n"
            ),
        ),
        other => return Err(format!("kind は block / song / demo です(見つかったのは \"{other}\")")),
    };
    let dst = project.join(&rel);
    if dst.exists() {
        return Err(format!("{rel} は既に存在します"));
    }
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&dst, body).map_err(|e| e.to_string())?;
    Ok(rel)
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Standard base64 (with padding) — matches the decoder in forteweb.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// What Claude Code (in the embedded terminal) needs to know to compose
/// in this package — written into the project as CLAUDE.md on first open.
const AGENT_BRIEFING: &str = r#"# CLAUDE.md — composing in this Forte package

This directory is a Forte package: music as code. A human is likely
watching it in `forte daw` (the GUI); every file you edit here
hot-reloads there — and their GUI gestures write minimal diffs back
into these same files. One medium, two hands: re-read a file before
large edits.

## Layout

- `songs/` — full songs (`song "name" { … }`)
- `blocks/` — reusable few-bar ideas (`block Name { … }`); build here first
- `instruments/` — custom devices (`device Name : Instrument { … }`)
- `packages/` — vendored material (essentials ships 150+ instruments,
  50+ blocks — import them: `import { Bass303 } from
  "packages/essentials_0.6.0/instruments/tb303.forte"` relative to the
  importing file)

## Commands

- `forte check <file>` — parse + compile (do this after every edit)
- `forte play <file>` — hear it in the terminal
- `forte build <file> -o out.wav` — render
- `forte analyze <file>` — measure the render: your ears
- `forte project` — this package's inventory as JSON
- `forte edit <file> '<json-op>' --write` — structured edits that
  preserve comments/layout (ops: set_tempo, set_pattern, add_track,
  add_place, set_track, set_arg, add_insert, add_automation, …)

## Discipline

- Blocks first: write a short block, listen, only then place it in a
  song (`play Name at bars(a..b)`). Songs are arrangements of blocks.
- Full-length tracks need sections and a build/drop arc
  (`section drop = bars(33..48)`), never a flat loop.
- Patterns: `beat`x... x...`` (x hit / X accent / . rest),
  `notes`C4:1 _:0.5 [E4 G4]:2`` (pitch:beats, `_` rest, chords,
  `~` tie, `!` accent).
"#;

/// The embedded terminal: a WebSocket ⇆ PTY pump. The DAW's revolution is
/// composing WITH an agent — Claude Code runs in this shell, in the project
/// directory, editing the same files the GUI projects. Server binds
/// 127.0.0.1 only; the terminal is exactly as local as the DAW itself.
mod term {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::os::fd::{FromRawFd, RawFd};
    use std::os::unix::process::CommandExt;
    use std::path::Path;

    /// RFC 6455 handshake + frame pump.
    pub fn serve_terminal(mut stream: TcpStream, head: &str, project: &Path) -> std::io::Result<()> {
        let Some(key) = head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("sec-websocket-key").then(|| v.trim().to_string())
        }) else {
            return crate::browser::respond(&mut stream, "400 Bad Request", "text/plain", b"not a websocket");
        };
        let accept = {
            let digest = crate::sha::sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes());
            b64(&digest)
        };
        stream.write_all(
            format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            )
            .as_bytes(),
        )?;

        // PTY + the user's shell, homed in the project
        let (master, slave) = openpty()?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut cmd = std::process::Command::new(&shell);
        let (si, so, se) = unsafe {
            (
                std::process::Stdio::from_raw_fd(dup(slave)?),
                std::process::Stdio::from_raw_fd(dup(slave)?),
                std::process::Stdio::from_raw_fd(slave),
            )
        };
        cmd.current_dir(project)
            .env("TERM", "xterm-256color")
            .stdin(si)
            .stdout(so)
            .stderr(se);
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        let mut child = cmd.spawn()?;

        // reader thread: PTY output → binary ws frames
        let mut ws_out = stream.try_clone()?;
        let mut pty_out = unsafe { std::fs::File::from_raw_fd(dup(master)?) };
        let pump = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match pty_out.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if write_frame(&mut ws_out, 0x2, &buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = write_frame(&mut ws_out, 0x8, &[]); // close
        });

        // main loop: ws frames → PTY input; text frames carry resize JSON
        let mut pty_in = unsafe { std::fs::File::from_raw_fd(dup(master)?) };
        while let Ok((op, payload)) = read_frame(&mut stream) {
            match op {
                0x1 => {
                    // {"r":rows,"c":cols}
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&payload) {
                        let (r, c) = (
                            v["r"].as_u64().unwrap_or(24) as u16,
                            v["c"].as_u64().unwrap_or(80) as u16,
                        );
                        let ws = libc::winsize { ws_row: r, ws_col: c, ws_xpixel: 0, ws_ypixel: 0 };
                        unsafe { libc::ioctl(master, libc::TIOCSWINSZ as _, &ws) };
                    }
                }
                0x2 => {
                    if pty_in.write_all(&payload).is_err() {
                        break;
                    }
                }
                0x9 => {
                    let _ = write_frame(&mut stream, 0xA, &payload); // ping → pong
                }
                0x8 => break,
                _ => {}
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        unsafe { libc::close(master) };
        let _ = pump.join();
        Ok(())
    }

    fn openpty() -> std::io::Result<(RawFd, RawFd)> {
        let (mut m, mut s) = (0 as RawFd, 0 as RawFd);
        let rc = unsafe {
            libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok((m, s))
    }

    fn dup(fd: RawFd) -> std::io::Result<RawFd> {
        let d = unsafe { libc::dup(fd) };
        if d < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(d)
    }

    /// One ws frame (server side: client frames are masked). Fragmentation
    /// is not expected from xterm-sized messages and is treated as-is.
    fn read_frame(stream: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr)?;
        let op = hdr[0] & 0x0f;
        let masked = hdr[1] & 0x80 != 0;
        let mut len = (hdr[1] & 0x7f) as u64;
        if len == 126 {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext)?;
            len = u16::from_be_bytes(ext) as u64;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext)?;
            len = u64::from_be_bytes(ext);
        }
        if len > 1 << 20 {
            return Err(std::io::Error::other("frame too large"));
        }
        let mut mask = [0u8; 4];
        if masked {
            stream.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; len as usize];
        stream.read_exact(&mut payload)?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        Ok((op, payload))
    }

    fn write_frame(stream: &mut TcpStream, op: u8, payload: &[u8]) -> std::io::Result<()> {
        let mut out = Vec::with_capacity(payload.len() + 10);
        out.push(0x80 | op);
        if payload.len() < 126 {
            out.push(payload.len() as u8);
        } else if payload.len() < 1 << 16 {
            out.push(126);
            out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            out.push(127);
            out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
        out.extend_from_slice(payload);
        stream.write_all(&out)
    }

    /// Standard base64 with padding (the ws accept digest).
    fn b64(data: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(A[(n >> 18) as usize & 63] as char);
            out.push(A[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
            out.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
        }
        out
    }
}
