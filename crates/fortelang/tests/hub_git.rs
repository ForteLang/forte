//! The git-backed hub, end to end against a real (local, bare) git remote:
//! publish pushes the snapshot + ledger + VCS history, fork pulls the whole
//! history down and stamps lineage, and a concurrent publish is resolved by
//! the push-reject → resync → replay loop. What GitHub hosts in production,
//! a bare repo hosts in this test — same plumbing.

use fortelang::hub_git::{is_git_url, GitHub};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("forte-hub-git-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sh_git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A cached checkout acting as one participant, with their own git identity.
fn actor(url: &str, cache: &std::path::Path, name: &str) -> GitHub {
    let hub = GitHub::open(url, Some(cache.to_path_buf())).unwrap();
    // the participant's identity is their git config
    let entries = std::fs::read_dir(cache).unwrap();
    for e in entries.flatten() {
        if e.path().join(".git").exists() {
            sh_git(&e.path(), &["config", "user.name", name]);
            sh_git(&e.path(), &["config", "user.email", &format!("{name}@test")]);
        }
    }
    hub
}

fn write_song(dir: &std::path::Path, tempo: u32) {
    std::fs::write(
        dir.join("tune.forte"),
        format!(
            "song \"Tune\" {{\n  tempo {tempo}bpm\n  track A {{\n    instrument prisma()\n    play beat`x---` at bars(1..2)\n  }}\n}}\n"
        ),
    )
    .unwrap();
}

#[test]
fn git_url_detection() {
    assert!(is_git_url("github:you/forte-hub"));
    assert!(is_git_url("git@github.com:you/forte-hub.git"));
    assert!(is_git_url("https://github.com/you/forte-hub.git"));
    assert!(is_git_url("/tmp/central.git"));
    assert!(!is_git_url("http://127.0.0.1:9377")); // served hub
    assert!(!is_git_url(".forte-hub")); // local dir
}

#[test]
fn publish_fork_and_concurrent_publish_roundtrip() {
    // ---- the "GitHub": a bare repository
    let root = scratch("central");
    let central = root.join("central.git");
    std::fs::create_dir_all(&central).unwrap();
    sh_git(&central, &["init", "--bare", "-b", "main"]);
    let url = central.to_string_lossy().into_owned();
    assert!(is_git_url(&url));

    // ---- alice publishes a song, with its VCS history
    let songdir = scratch("alice-song");
    write_song(&songdir, 100);
    fortelang::vcs::Repo::init(songdir.to_str().unwrap()).unwrap();
    let song_repo = fortelang::vcs::Repo::open(songdir.to_str().unwrap()).unwrap();
    song_repo.commit("最初の形").unwrap();

    let cache_a = scratch("cache-alice");
    let alice = actor(&url, &cache_a, "alice");
    let msg = alice.publish(songdir.join("tune.forte").to_str().unwrap(), None).unwrap();
    assert!(msg.contains("published: tune v1"), "{msg}");
    assert!(msg.contains("履歴 push"), "history travels: {msg}");

    // ---- bob, from a different machine (cache), sees and forks it
    let cache_b = scratch("cache-bob");
    let bob = actor(&url, &cache_b, "bob");
    let listing = bob.list(false).unwrap();
    assert!(listing.contains("tune") && listing.contains("alice"), "{listing}");

    let forkdir = root.join("bob-fork");
    let msg = bob.fork("tune", forkdir.to_str().unwrap()).unwrap();
    assert!(msg.contains("履歴ごと"), "{msg}");
    let vrepo = fortelang::vcs::Repo::open(forkdir.to_str().unwrap()).unwrap();
    assert!(vrepo.is_clean().unwrap());
    let head = vrepo.head().unwrap().unwrap();
    let messages: Vec<String> =
        vrepo.log(&head).unwrap().iter().map(|(_, c)| c.message.clone()).collect();
    assert!(messages.first().unwrap().starts_with("fork tune v1"), "{messages:?}");
    assert!(messages.iter().any(|m| m == "最初の形"), "alice の履歴が来る: {messages:?}");

    // the fork event reached the central ledger — alice sees it after a sync
    let alice = actor(&url, &cache_a, "alice");
    let lineage = alice.lineage("tune").unwrap();
    assert!(lineage.contains("fork events: 1"), "{lineage}");

    // ---- concurrent publish: alice pushes first, bob's stale checkout replays
    let song2 = scratch("alice-song2");
    write_song(&song2, 110);
    alice.publish(song2.join("tune.forte").to_str().unwrap(), Some("tune-two")).unwrap();

    // bob's cache has NOT seen tune-two (no sync since his fork). His publish
    // must hit a rejected push, resync, replay, and land cleanly.
    let song3 = scratch("bob-song3");
    write_song(&song3, 124);
    let msg = bob.publish(song3.join("tune.forte").to_str().unwrap(), Some("tune-bob")).unwrap();
    assert!(msg.contains("published: tune-bob v1"), "{msg}");

    // everyone converges: a fresh actor sees all three songs
    let cache_c = scratch("cache-carol");
    let carol = actor(&url, &cache_c, "carol");
    let listing = carol.list(false).unwrap();
    for name in ["tune", "tune-two", "tune-bob"] {
        assert!(listing.contains(name), "{name} missing:\n{listing}");
    }

    for d in [root, songdir, cache_a, cache_b, cache_c, song2, song3] {
        let _ = std::fs::remove_dir_all(&d);
    }
}
