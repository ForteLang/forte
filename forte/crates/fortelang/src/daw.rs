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
fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, String, Vec<u8>)> {
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
    Ok((method, target, body))
}

fn handle(web_root: &Path, project: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    let (method, target, body) = read_request(&mut stream)?;
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));

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
                 // space で再生。グリッド / ロール / ミキサーを触ると、この\n\
                 // コードがそのまま書き換わります。\n\
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
