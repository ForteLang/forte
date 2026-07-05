//! `forte browser` — serve the browser editor (web/) from the repository and
//! open it. No external server needed: a tiny static file server, the same
//! zero-dependency style as hub_server.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

/// Walk up from `start` looking for the repository's `web/index.html`.
pub fn find_web_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start.to_path_buf());
    while let Some(d) = dir {
        if d.join("web/index.html").is_file() {
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

    // "/" lands on the editor; directory paths land on their index.html
    let rel = match path {
        "/" => "web/index.html".to_string(),
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
        "web/index.html が見つかりません(Forte リポジトリの中で実行してください。\n\
         wasm を作り直すには scripts/build_web.sh)"
            .to_string()
    })?;
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let url = format!("http://localhost:{port}/web/");
    println!("browser editor: {url}(Ctrl+C で終了)");
    println!("hub lineage   : http://localhost:{port}/web/hub.html");
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
