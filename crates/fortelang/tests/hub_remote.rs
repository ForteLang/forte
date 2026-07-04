//! The served hub, end to end over real HTTP: signup issues a token, publish
//! requires it (author comes from the token, never the body), history pushes
//! and pulls with the snapshot, and a remote fork materializes a working
//! repository with the lineage stamp committed.

use std::net::TcpListener;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("forte-hub-remote-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Bind port 0, serve a fresh hub on a background thread, return its URL.
fn spawn_hub(root: &std::path::Path) -> String {
    let hub = fortelang::hub::Hub::open(root.to_str().unwrap()).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _ = fortelang::hub_server::serve_on(hub, listener);
    });
    format!("http://127.0.0.1:{port}")
}

fn write_song(dir: &std::path::Path) {
    std::fs::write(
        dir.join("tune.forte"),
        "song \"Tune\" {\n  tempo 100bpm\n  track A {\n    instrument polymer()\n    play beat`x---` at bars(1..2)\n  }\n}\n",
    )
    .unwrap();
}

#[test]
fn remote_hub_signup_publish_fork_roundtrip() {
    let hub_root = scratch("server");
    let url = spawn_hub(&hub_root);

    // ---- signup: alice gets a token; the name can't be taken twice
    let msg = fortelang::hub_remote::signup(&url, "alice").expect("signup");
    let token = msg.lines().find_map(|l| l.strip_prefix("token: ")).expect("token line").to_string();
    assert!(fortelang::hub_remote::signup(&url, "alice").is_err(), "duplicate signup");

    // ---- once a user exists, publishing without a token is refused
    let songdir = scratch("alice-song");
    write_song(&songdir);
    fortelang::vcs::Repo::init(songdir.to_str().unwrap()).unwrap();
    let repo = fortelang::vcs::Repo::open(songdir.to_str().unwrap()).unwrap();
    repo.commit("最初の形").unwrap();
    let entry = songdir.join("tune.forte");
    let entry = entry.to_str().unwrap();

    let denied = fortelang::hub_remote::publish(&url, None, entry, None);
    assert!(denied.is_err() && denied.unwrap_err().contains("認証"), "no token → 401");
    assert!(
        fortelang::hub_remote::publish(&url, Some("wrong-token"), entry, None).is_err(),
        "bad token → 401"
    );

    // ---- authenticated publish carries the history; author comes from the token
    let msg = fortelang::hub_remote::publish(&url, Some(&token), entry, None).expect("publish");
    assert!(msg.contains("published: tune v1"), "{msg}");
    assert!(msg.contains("履歴 push"), "history travels: {msg}");

    let hub = fortelang::hub::Hub::open(hub_root.to_str().unwrap()).unwrap();
    let reg = hub.registry().unwrap();
    let ver = reg.repos["tune"].versions.last().unwrap();
    assert_eq!(ver.author, "alice", "author derived from the token");
    assert!(ver.commit.is_some(), "version records its head commit");

    // ---- remote list sees it
    let listing = fortelang::hub_remote::list(&url).expect("list");
    assert!(listing.contains("tune") && listing.contains("alice"), "{listing}");

    // ---- remote fork: a working repository with the author's history + stamp
    let forkdir = scratch("bob-fork");
    let _ = std::fs::remove_dir_all(&forkdir); // fork requires a non-existent/empty dest
    let msg = fortelang::hub_remote::fork(&url, None, "tune", forkdir.to_str().unwrap())
        .expect("fork");
    assert!(msg.contains("履歴ごと"), "{msg}");

    let vrepo = fortelang::vcs::Repo::open(forkdir.to_str().unwrap()).unwrap();
    assert!(vrepo.is_clean().unwrap(), "fork checkout matches its HEAD");
    let head = vrepo.head().unwrap().unwrap();
    let log = vrepo.log(&head).unwrap();
    let messages: Vec<&str> = log.iter().map(|(_, c)| c.message.as_str()).collect();
    assert!(messages.first().unwrap().starts_with("fork tune v1"), "{messages:?}");
    assert!(messages.contains(&"最初の形"), "original history came along: {messages:?}");
    assert!(forkdir.join(".forte-lineage.json").is_file());

    // ---- the fork edits, signs up as bob, and publishes back: lineage closes
    let token_bob = fortelang::hub_remote::signup(&url, "bob")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("token: ").map(String::from))
        .unwrap();
    let src = std::fs::read_to_string(forkdir.join("tune.forte")).unwrap();
    std::fs::write(forkdir.join("tune.forte"), src.replace("100bpm", "124bpm")).unwrap();
    vrepo.commit("テンポを上げた").unwrap();
    let msg = fortelang::hub_remote::publish(
        &url,
        Some(&token_bob),
        forkdir.join("tune.forte").to_str().unwrap(),
        Some("tune-bob"),
    )
    .expect("republish");
    assert!(msg.contains("forked from tune v1"), "provenance recorded: {msg}");

    let reg = hub.registry().unwrap();
    let ver = reg.repos["tune-bob"].versions.last().unwrap();
    assert_eq!(ver.author, "bob");
    assert_eq!(ver.forked_from.as_ref().unwrap().repo, "tune");

    let _ = std::fs::remove_dir_all(&hub_root);
    let _ = std::fs::remove_dir_all(&songdir);
    let _ = std::fs::remove_dir_all(&forkdir);
}

#[test]
fn corrupted_pushed_objects_are_rejected() {
    let hub_root = scratch("tamper");
    let hub = fortelang::hub::Hub::open(hub_root.to_str().unwrap()).unwrap();
    let mut objects = std::collections::BTreeMap::new();
    objects.insert(
        "deadbeef".to_string(),
        b"not the content that hashes to deadbeef".to_vec(),
    );
    let err = hub.import_objects("x", &objects, "deadbeef").unwrap_err();
    assert!(err.contains("一致しません"), "{err}");
    let _ = std::fs::remove_dir_all(&hub_root);
}
