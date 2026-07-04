//! Minimal HTTP front for the hub (std::net only — same zero-dependency
//! discipline as the LSP server). Read endpoints feed the browser lineage
//! page; publish/fork make it a multi-user hub.
//!
//!   GET  /api/repos                 registry summary
//!   GET  /api/repos/{name}          lineage detail (versions, releases, forks)
//!   GET  /api/repos/{name}/files    latest snapshot sources (browser player)
//!   POST /api/repos/{name}/fork     ledger a fork, return files + history
//!   POST /api/signup                register an author, get a token (once)
//!   POST /api/publish               push a snapshot (+VCS history); once any
//!                                   user is registered, requires
//!                                   `Authorization: Bearer <token>` and the
//!                                   author is derived from the token
//!
//! v1 speaks plain HTTP; put a TLS reverse proxy in front for the internet.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use crate::hub::Hub;

pub fn serve(hub: Hub, port: u16) -> Result<(), String> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("bind {port}: {e}"))?;
    println!("forte hub serve: http://127.0.0.1:{port}/api/repos");
    serve_on(hub, listener)
}

/// Serve on an already-bound listener (tests bind port 0 and pass it in).
pub fn serve_on(hub: Hub, listener: TcpListener) -> Result<(), String> {
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
    // read until end of headers
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 64 * 1024 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut line = head.lines().next().unwrap_or("").split_whitespace();
    let method = line.next().unwrap_or("").to_string();
    let target = line.next().unwrap_or("/").to_string();
    let path = target.split('?').next().unwrap_or("/");
    let query = target.split('?').nth(1).unwrap_or("");
    let token: Option<String> = head.lines().find_map(|l| {
        l.to_ascii_lowercase()
            .strip_prefix("authorization:")
            .map(|_| l.splitn(2, ':').nth(1).unwrap_or("").trim())
            .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
            .map(str::trim)
            .map(String::from)
    });

    // read the body (publish posts a whole snapshot; takes make these large)
    let content_length: usize = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(str::trim).map(String::from))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    const MAX_BODY: usize = 64 * 1024 * 1024;
    if content_length > MAX_BODY {
        let response = "HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n";
        return stream.write_all(response.as_bytes());
    }
    let mut body_bytes = buf[header_end..].to_vec();
    while body_bytes.len() < content_length {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body_bytes.extend_from_slice(&tmp[..n]);
    }

    let (status, body) = route(hub, &method, path, query, &body_bytes, token.as_deref());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes())
}

fn route(
    hub: &Hub,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    token: Option<&str>,
) -> (&'static str, String) {
    let err = |code: &'static str, msg: &str| {
        (code, serde_json::json!({ "error": msg }).to_string())
    };
    if method == "OPTIONS" {
        return ("204 No Content", String::new());
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (method, parts.as_slice()) {
        ("GET", ["api", "lineage"]) => match hub.lineage_forest() {
            Ok(v) => ("200 OK", v.to_string()),
            Err(e) => err("500 Internal Server Error", &e),
        },
        ("GET", ["api", "repos"]) => match hub.repos_json() {
            Ok(v) => ("200 OK", v.to_string()),
            Err(e) => err("500 Internal Server Error", &e),
        },
        ("GET", ["api", "repos", name]) => match hub.repo_json(name) {
            Ok(v) => ("200 OK", v.to_string()),
            Err(e) => err("404 Not Found", &e),
        },
        ("GET", ["api", "repos", name, "files"]) => match hub.snapshot_files(name) {
            Ok((files, assets)) => (
                "200 OK",
                serde_json::json!({ "files": files, "assets": assets }).to_string(),
            ),
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
            // a valid token names the forker; ?by= is the anonymous fallback
            let token_author = token.and_then(|t| hub.auth(t));
            let by = token_author.as_deref().unwrap_or_else(|| {
                query.split('&').find_map(|kv| kv.strip_prefix("by=")).unwrap_or("")
            });
            match hub.fork_remote(name, by) {
                Ok(v) => ("200 OK", v.to_string()),
                Err(e) => err("404 Not Found", &e),
            }
        }
        ("POST", ["api", "signup"]) => {
            let v: serde_json::Value = match serde_json::from_slice(body) {
                Ok(v) => v,
                Err(e) => return err("400 Bad Request", &format!("JSON: {e}")),
            };
            let author = v["author"].as_str().unwrap_or("");
            match hub.signup(author) {
                Ok(tok) => (
                    "200 OK",
                    serde_json::json!({ "author": author, "token": tok }).to_string(),
                ),
                Err(e) => err("409 Conflict", &e),
            }
        }
        // the browser editor publishes back: a performance fork closes its loop.
        // once anyone has signed up, publishing requires a token — the author
        // comes from the token, never from the body (no impersonation).
        ("POST", ["api", "publish"]) => {
            let token_author = token.and_then(|t| hub.auth(t));
            if hub.requires_auth() && token_author.is_none() {
                return err(
                    "401 Unauthorized",
                    "この hub は認証必須です(forte hub signup <name> --hub <url> でトークンを取得し FORTE_HUB_TOKEN に)",
                );
            }
            match publish_body(hub, body, token_author.as_deref()) {
                Ok(msg) => ("200 OK", serde_json::json!({ "ok": msg }).to_string()),
                Err(e) => err("400 Bad Request", &e),
            }
        }
        // downloading release audio is intentionally NOT an endpoint: the
        // audio reproduces from the sources (fork it, build it, verify it)
        _ => err("404 Not Found", "no such route"),
    }
}

/// POST /api/publish body:
/// `{name, entry, author, files: {path: text}, assets: {path: base64},
///   objects?: {hash: base64}, head?: "…"}`.
/// Compile-validated against the posted snapshot before anything is stored.
/// `token_author` (from a valid Bearer token) always wins over body author.
fn publish_body(hub: &Hub, body: &[u8], token_author: Option<&str>) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("JSON: {e}"))?;
    let name = v["name"].as_str().filter(|s| !s.is_empty()).ok_or("name がありません")?;
    if !name.chars().all(|c| c.is_alphanumeric() || "-_".contains(c)) {
        return Err(format!("名前 '{name}' が不正です(英数字と -_)"));
    }
    let entry = v["entry"].as_str().filter(|s| !s.is_empty()).ok_or("entry がありません")?;
    let author = token_author.unwrap_or_else(|| v["author"].as_str().unwrap_or("browser"));

    let mut files = std::collections::BTreeMap::new();
    let bad_path = |p: &str| p.starts_with('/') || p.split('/').any(|c| c == "..");
    if let Some(map) = v["files"].as_object() {
        for (path, val) in map {
            if bad_path(path) {
                return Err(format!("パス '{path}' が不正です"));
            }
            files.insert(path.clone(), val.as_str().unwrap_or_default().as_bytes().to_vec());
        }
    }
    if let Some(map) = v["assets"].as_object() {
        for (path, val) in map {
            if bad_path(path) {
                return Err(format!("パス '{path}' が不正です"));
            }
            let bytes = crate::hub::base64_decode(val.as_str().unwrap_or_default())
                .ok_or_else(|| format!("{path}: base64 が壊れています"))?;
            files.insert(path.clone(), bytes);
        }
    }
    let mut msg = hub.publish_map(entry, files, name, Some(author))?;

    // pushed history: verified object-by-object, then the version gets its head
    if let (Some(map), Some(head)) = (v["objects"].as_object(), v["head"].as_str()) {
        let mut objects = std::collections::BTreeMap::new();
        for (hash, val) in map {
            let bytes = crate::hub::base64_decode(val.as_str().unwrap_or_default())
                .ok_or_else(|| format!("{hash}: base64 が壊れています"))?;
            objects.insert(hash.clone(), bytes);
        }
        let copied = hub.import_objects(name, &objects, head)?;
        msg.push_str(&format!(" 履歴 push: {} ({copied} objects)", &head[..8.min(head.len())]));
    }
    Ok(msg)
}
