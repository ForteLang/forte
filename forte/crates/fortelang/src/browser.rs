//! `forte browser` — serve the browser editor (web/) from the repository and
//! open it. No external server needed: a tiny zero-dependency static file
//! server, plus one JSON endpoint (`/api/packages`) for the package catalog.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

/// Walk up from `start` looking for the repository's `web/index.html`.
pub fn find_web_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(d) = dir {
        if d.join("forte/web/index.html").is_file() {
            return Some(d);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "json" | "webmanifest" => "application/json; charset=utf-8",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)
}

fn handle(root: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let target = head.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/");

    // the catalog's data: every packages/<pkg> with its meta and resources
    if path == "/api/packages" {
        let body = packages_json(root);
        return respond(&mut stream, "200 OK", "application/json; charset=utf-8", body.as_bytes());
    }

    // "/" lands on the editor; directory paths land on their index.html
    let rel = match path {
        "/" => "forte/web/index.html".to_string(),
        p => {
            let p = p.trim_start_matches('/');
            if p.ends_with('/') { format!("{p}index.html") } else { p.to_string() }
        }
    };
    // no path traversal: reject any component that isn't a normal name
    if PathBuf::from(&rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden");
    }
    let file = root.join(&rel);
    match std::fs::read(&file) {
        Ok(body) => respond(&mut stream, "200 OK", content_type(&file), &body),
        Err(_) => respond(&mut stream, "404 Not Found", "text/plain; charset=utf-8", b"not found"),
    }
}

/// Scan `<root>/packages/*/` into the catalog JSON: each package's meta
/// (from its package.forte) and its instruments/blocks/songs file lists.
fn packages_json(root: &Path) -> String {
    let mut pkgs = Vec::new();
    let dir = root.join("packages");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    entries.sort();
    for p in entries {
        let Ok(src) = std::fs::read_to_string(p.join("package.forte")) else { continue };
        let Ok(ast) = crate::parser::parse(&src) else { continue };
        let Some(meta) = ast.blocks.last() else { continue };
        let list = |sub: &str| -> Vec<String> {
            let mut files: Vec<String> = std::fs::read_dir(p.join(sub))
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| n.ends_with(".forte"))
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            files
        };
        // albums: album.forte meta + .fortesong tracks in filename order
        let mut albums = Vec::new();
        if let Ok(rd) = std::fs::read_dir(p.join("albums")) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|d| d.is_dir()).collect();
            dirs.sort();
            for d in dirs {
                let meta = std::fs::read_to_string(d.join("album.forte"))
                    .ok()
                    .and_then(|s| crate::parser::parse(&s).ok())
                    .and_then(|a| a.blocks.last().map(|b| b.body.clone()));
                let Some(m) = meta else { continue };
                let mut tracks: Vec<String> = std::fs::read_dir(&d)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| n.ends_with(".fortesong"))
                            .collect()
                    })
                    .unwrap_or_default();
                tracks.sort();
                albums.push(serde_json::json!({
                    "dir": d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    "title": m.name,
                    "artist": m.artist.clone().unwrap_or_default(),
                    "desc": m.desc.clone().unwrap_or_default(),
                    "tracks": tracks,
                }));
            }
        }
        pkgs.push(serde_json::json!({
            "dir": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            "name": meta.name,
            "desc": meta.body.desc.clone().unwrap_or_default(),
            "tags": meta.body.tags,
            "license": meta.body.license.clone().unwrap_or_default(),
            "sponsor": meta.body.sponsor.clone().unwrap_or_default(),
            "version": meta.body.version.clone().unwrap_or_default(),
            "instruments": list("instruments"),
            "blocks": list("blocks"),
            "songs": list("songs"),
            "albums": albums,
        }));
    }
    serde_json::json!({ "packages": pkgs }).to_string()
}

/// Try the platform opener; failure is fine (the URL is printed anyway).
fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(target_os = "macos"))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn run(port: u16, open: bool) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = find_web_root(&cwd).ok_or_else(|| {
        "forte/web/index.html が見つかりません(Forte リポジトリの中で実行してください。\n\
         wasm を作り直すには scripts/build_web.sh)"
            .to_string()
    })?;
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let url = format!("http://localhost:{port}/forte/web/");
    println!("browser editor: {url}(Ctrl+C で終了)");
    println!("packages      : http://localhost:{port}/forte/web/catalog.html");
    if open {
        open_url(&url);
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let root = root.clone();
        // one thread per connection is plenty for a local editor
        std::thread::spawn(move || {
            let _ = handle(&root, stream);
        });
    }
    Ok(())
}
