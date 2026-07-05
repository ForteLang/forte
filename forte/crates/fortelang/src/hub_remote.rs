//! Remote hub client: the same `forte hub` verbs pointed at a served hub
//! (`--hub http://host:9377`). Zero dependencies — a minimal HTTP/1.1 client
//! over std::net, matching the server in `hub_server`. v1 speaks plain HTTP;
//! for the open internet put a TLS reverse proxy in front of the server.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

use crate::hub::{base64_decode, base64_encode, LINEAGE_FILE};

/// `--hub` values starting with http:// are remote hubs.
pub fn is_url(hub: &str) -> bool {
    hub.starts_with("http://") || hub.starts_with("https://")
}

fn request(
    method: &str,
    url: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&serde_json::Value>,
) -> Result<(u16, serde_json::Value), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("v1 のリモート hub は http:// のみです(TLS はリバースプロキシで)")?;
    let host = rest.trim_end_matches('/');
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:80") };

    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    let body_bytes = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
    if body.is_some() {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body_bytes.len()
        ));
    }
    req.push_str("\r\n");

    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("{addr}: 接続できません: {e}"))?;
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(&body_bytes).map_err(|e| e.to_string())?;

    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).map_err(|e| e.to_string())?;
    let header_end = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("サーバーの応答が読めません")?;
    let head = String::from_utf8_lossy(&resp[..header_end]).into_owned();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("サーバーの応答が読めません")?;
    let body_start = header_end + 4;
    let json: serde_json::Value =
        serde_json::from_slice(&resp[body_start..]).unwrap_or(serde_json::Value::Null);
    Ok((status, json))
}

fn expect_ok(status: u16, v: &serde_json::Value) -> Result<(), String> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(v["error"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {status}")))
}

/// `forte hub signup <author> --hub <url>` — the token is printed exactly once.
pub fn signup(url: &str, author: &str) -> Result<String, String> {
    let (status, v) =
        request("POST", url, "/api/signup", None, Some(&serde_json::json!({ "author": author })))?;
    expect_ok(status, &v)?;
    let token = v["token"].as_str().ok_or("サーバーがトークンを返しませんでした")?;
    Ok(format!(
        "registered: {author}\ntoken: {token}\n(この 1 回しか表示されません。export FORTE_HUB_TOKEN={token} で保存を)"
    ))
}

pub fn list(url: &str) -> Result<String, String> {
    let (status, v) = request("GET", url, "/api/repos", None, None)?;
    expect_ok(status, &v)?;
    let repos = v["repos"].as_array().cloned().unwrap_or_default();
    if repos.is_empty() {
        return Ok("(empty hub)".into());
    }
    let mut out = String::new();
    for r in repos {
        out.push_str(&format!(
            "{}\tv{}\t[{}]\tby {}{}\n",
            r["name"].as_str().unwrap_or("?"),
            r["v"].as_u64().unwrap_or(0),
            r["kind"].as_str().unwrap_or("?"),
            r["author"].as_str().unwrap_or("?"),
            r["forked_from"]
                .as_object()
                .map(|o| format!(
                    "\t(forked from {} v{})",
                    o["repo"].as_str().unwrap_or("?"),
                    o["v"].as_u64().unwrap_or(0)
                ))
                .unwrap_or_default(),
        ));
    }
    Ok(out.trim_end().into())
}

/// Remote publish: snapshot + takes, and — from a clean repository — the
/// whole VCS history, pushed in one request.
pub fn publish(
    url: &str,
    token: Option<&str>,
    entry: &str,
    name: Option<&str>,
) -> Result<String, String> {
    let entry_path = Path::new(entry);
    let file_name = entry_path
        .file_name()
        .ok_or("ファイル名がありません")?
        .to_string_lossy()
        .into_owned();
    let name = name
        .unwrap_or(entry_path.file_stem().ok_or("ファイル名がありません")?.to_str().unwrap_or("song"))
        .to_string();
    let (_, files) = crate::hub::collect_snapshot(entry)?;

    let mut text = serde_json::Map::new();
    let mut assets = serde_json::Map::new();
    for (rel, bytes) in &files {
        if rel.ends_with(".frec") {
            assets.insert(rel.clone(), serde_json::json!(base64_encode(bytes)));
        } else {
            text.insert(
                rel.clone(),
                serde_json::json!(String::from_utf8_lossy(bytes).into_owned()),
            );
        }
    }
    let mut body = serde_json::json!({
        "name": name, "entry": file_name, "files": text, "assets": assets,
    });

    // a clean repository pushes its history along
    let base = entry_path.parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
    let repo = crate::vcs::Repo::open(if base.is_empty() { "." } else { &base })
        .ok()
        .filter(|r| r.is_clean().unwrap_or(false));
    if let Some(repo) = repo {
        if let Ok(Some(head)) = repo.head() {
            let mut objects = serde_json::Map::new();
            for hash in repo.reachable(&head)? {
                objects.insert(hash.clone(), serde_json::json!(base64_encode(&repo.object_raw(&hash)?)));
            }
            body["objects"] = serde_json::Value::Object(objects);
            body["head"] = serde_json::json!(head);
        }
    }

    let (status, v) = request("POST", url, "/api/publish", token, Some(&body))?;
    expect_ok(status, &v)?;
    Ok(v["ok"].as_str().unwrap_or("published").to_string())
}

/// Remote fork: download the snapshot (and its history, when published) and
/// materialize a working copy with the lineage stamp committed — exactly what
/// a local fork produces.
pub fn fork(
    url: &str,
    token: Option<&str>,
    name: &str,
    dest: &str,
) -> Result<String, String> {
    let dest_dir = Path::new(dest);
    if dest_dir.exists() && dest_dir.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err(format!("{dest} は空ではありません"));
    }
    let (status, v) = request("POST", url, &format!("/api/repos/{name}/fork"), token, None)?;
    expect_ok(status, &v)?;

    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let origin = &v["origin"];
    let ver = origin["v"].as_u64().unwrap_or(0);

    let history_note = if let (Some(objects), Some(head)) =
        (v["objects"].as_object(), v["head"].as_str())
    {
        // lay the pushed objects out like an object store, then clone from it
        let tmp = std::env::temp_dir()
            .join(format!("forte-fork-objects-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for (hash, val) in objects {
            if hash.len() < 3 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("オブジェクト名 '{hash}' が不正です"));
            }
            let bytes = base64_decode(val.as_str().unwrap_or_default())
                .ok_or_else(|| format!("{hash}: base64 が壊れています"))?;
            if crate::sha::sha256_hex(&bytes) != *hash {
                return Err(format!("オブジェクト {hash} の内容がハッシュと一致しません(改竄?)"));
            }
            let p = tmp.join(&hash[..2]).join(&hash[2..]);
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(p, bytes).map_err(|e| e.to_string())?;
        }
        let vrepo = crate::vcs::Repo::clone_into(dest, &tmp, head)?;
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::write(
            dest_dir.join(LINEAGE_FILE),
            serde_json::to_string_pretty(origin).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        vrepo.commit(&format!("fork {name} v{ver}"))?;
        format!("、履歴ごと({} から)", &head[..8.min(head.len())])
    } else {
        // no history published: plain snapshot + stamp
        let write = |rel: &str, bytes: &[u8]| -> Result<(), String> {
            if rel.starts_with('/') || rel.split('/').any(|c| c == "..") {
                return Err(format!("パス '{rel}' が不正です"));
            }
            let out = dest_dir.join(rel);
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(out, bytes).map_err(|e| e.to_string())
        };
        let empty = serde_json::Map::new();
        for (rel, val) in v["files"].as_object().unwrap_or(&empty) {
            write(rel, val.as_str().unwrap_or_default().as_bytes())?;
        }
        for (rel, val) in v["assets"].as_object().unwrap_or(&empty) {
            let bytes = base64_decode(val.as_str().unwrap_or_default())
                .ok_or_else(|| format!("{rel}: base64 が壊れています"))?;
            write(rel, &bytes)?;
        }
        std::fs::write(
            dest_dir.join(LINEAGE_FILE),
            serde_json::to_string_pretty(origin).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        String::new()
    };

    let mut fetched: BTreeMap<String, ()> = BTreeMap::new();
    for m in [v["files"].as_object(), v["assets"].as_object()].into_iter().flatten() {
        for k in m.keys() {
            fetched.insert(k.clone(), ());
        }
    }
    Ok(format!(
        "forked: {name} v{ver} -> {dest} ({} files、系譜に記録済み{history_note})",
        fetched.len()
    ))
}
