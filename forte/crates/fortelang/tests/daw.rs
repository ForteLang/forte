//! Contract tests for the `forte daw` project API (ADR D-15): the server
//! reads and writes REAL project files, scaffolds new elements, and never
//! escapes the project directory.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forte-daw-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Boot a server on an ephemeral port over a scratch project; returns
/// (port, project dir). The web root is the scratch dir too — only the
/// API is under test.
fn boot(name: &str) -> (u16, PathBuf) {
    let project = scratch(name);
    write(&project, "package.forte", "block T { desc \"t\" version \"0.0.1\" }\n");
    write(
        &project,
        "songs/one.forte",
        "song \"One\" {\n  tempo 100bpm\n  track A {\n    instrument mono()\n    play notes`A1 .` at bars(1..2)\n  }\n}\n",
    );
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (web, prj) = (project.clone(), project.clone());
    std::thread::spawn(move || fortelang::daw::serve(listener, web, prj));
    (port, project)
}

fn request(port: u16, req: &str, body: &[u8]) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    (head, buf[split..].to_vec())
}

fn get(port: u16, path: &str) -> (String, Vec<u8>) {
    request(port, &format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"), b"")
}

fn post(port: u16, path: &str, body: &[u8]) -> (String, Vec<u8>) {
    request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ),
        body,
    )
}

#[test]
fn the_project_api_reads_and_writes_real_files() {
    let (port, project) = boot("rw");

    // the inventory reflects the disk
    let (head, body) = get(port, "/api/project");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["songs"][0]["song"]["name"], "One");

    // read a file through the API
    let (head, body) = get(port, "/api/src?path=songs/one.forte");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(String::from_utf8_lossy(&body).contains("tempo 100bpm"));

    // write through the API and observe it on DISK — the DAW edits the project
    let newsrc = "song \"One\" {\n  tempo 128bpm\n  track A {\n    instrument mono()\n    play notes`A1 .` at bars(1..2)\n  }\n}\n";
    let (head, _) = post(port, "/api/src?path=songs/one.forte", newsrc.as_bytes());
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let on_disk = std::fs::read_to_string(project.join("songs/one.forte")).unwrap();
    assert_eq!(on_disk, newsrc);

    // the modules map serves every .forte for in-browser import resolution
    let (_, body) = get(port, "/api/modules");
    let m: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(m["songs/one.forte"].as_str().unwrap().contains("tempo 128bpm"));
}

#[test]
fn new_scaffolds_blocks_and_songs_inside_the_package() {
    let (port, project) = boot("new");
    let (head, body) = post(port, "/api/new?kind=block&name=Groove", b"");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["file"], "blocks/Groove.forte");
    let src = std::fs::read_to_string(project.join("blocks/Groove.forte")).unwrap();
    assert!(src.starts_with("block Groove {"));
    // and the scaffold parses
    assert!(fortelang::parser::parse(&src).is_ok());

    // an existing file is refused, as is a bad name
    let (head, _) = post(port, "/api/new?kind=block&name=Groove", b"");
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    let (head, _) = post(port, "/api/new?kind=song&name=2bad", b"");
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
}

#[test]
fn traversal_never_escapes_the_project() {
    let (port, project) = boot("guard");
    let (head, _) = get(port, "/api/src?path=../secret.txt");
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    let (head, _) = post(port, "/api/src?path=../evil.forte", b"x");
    assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    assert!(!project.parent().unwrap().join("evil.forte").exists());
}
