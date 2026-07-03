//! Minimal HTTP front for the local hub (std::net only — same zero-dependency
//! discipline as the LSP server). Read endpoints feed the browser lineage
//! page; POST /fork is the one mutating route, because forking is the one
//! thing listeners do.
//!
//!   GET  /api/repos                 registry summary
//!   GET  /api/repos/{name}          lineage detail (versions, releases, forks)
//!   GET  /api/repos/{name}/files    latest snapshot sources (browser player)
//!   POST /api/repos/{name}/fork     ledger a fork, return files + stamp

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use crate::hub::Hub;

pub fn serve(hub: Hub, port: u16) -> Result<(), String> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("bind {port}: {e}"))?;
    println!("forte hub serve: http://127.0.0.1:{port}/api/repos");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // errors on one connection must not take the hub down
        let _ = handle(&hub, stream);
    }
    Ok(())
}

fn handle(hub: &Hub, mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    // read until end of headers (no request bodies are used by this API)
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 64 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let mut line = head.lines().next().unwrap_or("").split_whitespace();
    let method = line.next().unwrap_or("");
    let target = line.next().unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/");
    let query = target.split('?').nth(1).unwrap_or("");

    let (status, body) = route(hub, method, path, query);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes())
}

fn route(hub: &Hub, method: &str, path: &str, query: &str) -> (&'static str, String) {
    let err = |code: &'static str, msg: &str| {
        (code, serde_json::json!({ "error": msg }).to_string())
    };
    if method == "OPTIONS" {
        return ("204 No Content", String::new());
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (method, parts.as_slice()) {
        ("GET", ["api", "repos"]) => match hub.repos_json() {
            Ok(v) => ("200 OK", v.to_string()),
            Err(e) => err("500 Internal Server Error", &e),
        },
        ("GET", ["api", "repos", name]) => match hub.repo_json(name) {
            Ok(v) => ("200 OK", v.to_string()),
            Err(e) => err("404 Not Found", &e),
        },
        ("GET", ["api", "repos", name, "files"]) => match hub.snapshot_files(name) {
            Ok(v) => ("200 OK", serde_json::json!({ "files": v }).to_string()),
            Err(e) => err("404 Not Found", &e),
        },
        ("POST", ["api", "repos", name, "play"]) => {
            let by = query.split('&').find_map(|kv| kv.strip_prefix("by=")).unwrap_or("");
            match hub.play_event(name, by) {
                Ok(plays) => ("200 OK", serde_json::json!({ "plays": plays }).to_string()),
                Err(e) => err("404 Not Found", &e),
            }
        }
        ("POST", ["api", "repos", name, "fork"]) => {
            let by = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("by="))
                .unwrap_or("");
            match hub.fork_remote(name, by) {
                Ok(v) => ("200 OK", v.to_string()),
                Err(e) => err("404 Not Found", &e),
            }
        }
        // downloading release audio is intentionally NOT an endpoint: the
        // audio reproduces from the sources (fork it, build it, verify it)
        _ => err("404 Not Found", "no such route"),
    }
}
