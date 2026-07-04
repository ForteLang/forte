//! Fork-lineage lifecycle against the local hub: publish a library, fork it
//! (the only retrieval path), modify, republish — provenance must be recorded
//! by construction.

use fortelang::hub::{Hub, LINEAGE_FILE};

fn temp_dir(tag: &str) -> String {
    let d = std::env::temp_dir().join(format!("forte-hub-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn songs_dir() -> String {
    format!("{}/../../songs", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn publish_fork_republish_records_lineage() {
    let hub_dir = temp_dir("hub");
    let work = temp_dir("work");
    let hub = Hub::open(&hub_dir).unwrap();

    // 1) publish the device library
    let msg = hub.publish(&format!("{}/devices/warm.forte", songs_dir()), None).unwrap();
    assert!(msg.contains("warm v1"), "{msg}");
    assert!(msg.contains("library"));

    // 2) fork is the only way out, and it stamps provenance
    let dest = format!("{work}/mywarm");
    let msg = hub.fork("warm", &dest).unwrap();
    assert!(msg.contains("warm v1"), "{msg}");
    assert!(std::path::Path::new(&dest).join("warm.forte").exists());
    assert!(std::path::Path::new(&dest).join(LINEAGE_FILE).exists());

    // 3) modify the fork and publish under a new name
    let file = format!("{dest}/warm.forte");
    let src = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, src.replace("param cutoff = 0.6", "param cutoff = 0.4")).unwrap();
    let msg = hub.publish(&file, Some("colder")).unwrap();
    assert!(msg.contains("colder v1"), "{msg}");
    assert!(msg.contains("forked from warm v1"), "provenance must be recorded: {msg}");

    // 4) lineage shows ancestry from the child and forks from the parent
    let lin = hub.lineage("colder").unwrap();
    assert!(lin.contains("forked from: warm v1"), "{lin}");
    let lin = hub.lineage("warm").unwrap();
    assert!(lin.contains("colder v1"), "parent must list its forks: {lin}");
    assert!(lin.contains("fork events: 1"), "{lin}");

    // 5) registry is well-formed (fork-only rule: no other retrieval API exists)
    let reg = hub.registry().unwrap();
    assert_eq!(reg.repos.len(), 2);
    assert_eq!(reg.events.len(), 3); // publish, fork, publish
}

#[test]
fn publishing_a_song_snapshots_its_imports() {
    let hub_dir = temp_dir("hub2");
    let hub = Hub::open(&hub_dir).unwrap();

    let msg = hub.publish(&format!("{}/handmade.forte", songs_dir()), None).unwrap();
    assert!(msg.contains("handmade v1"), "{msg}");
    assert!(msg.contains("song"));
    assert!(msg.contains("2 files"), "entry + imported library: {msg}");

    // the stored snapshot is self-contained: fork it and check it compiles
    let work = temp_dir("work2");
    let dest = format!("{work}/song");
    hub.fork("handmade", &dest).unwrap();
    let entry = format!("{dest}/handmade.forte");
    let src = std::fs::read_to_string(&entry).unwrap();
    let base = format!("{dest}");
    fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base)
        .expect("forked snapshot must compile standalone");
}

#[test]
fn release_records_digest_and_verify_reproduces_it() {
    let hub_dir = temp_dir("hub4");
    let hub = Hub::open(&hub_dir).unwrap();
    hub.publish(&format!("{}/handmade.forte", songs_dir()), None).unwrap();

    // release: deterministic build, digest goes into the ledger
    let msg = hub.release("handmade").unwrap();
    assert!(msg.contains("digest"), "{msg}");
    let reg = hub.registry().unwrap();
    let rel = reg.repos["handmade"].releases.last().unwrap().clone();
    assert_eq!(rel.digest.len(), 16);

    // verify: clean-room rebuild reproduces the digest
    let msg = hub.verify("handmade").unwrap();
    assert!(msg.contains("VERIFIED"), "{msg}");

    // lineage shows the release and its verification count
    let lin = hub.lineage("handmade").unwrap();
    assert!(lin.contains(&rel.digest), "{lin}");
    assert!(lin.contains("verified 1回"), "{lin}");

    // libraries cannot be released
    hub.publish(&format!("{}/devices/warm.forte", songs_dir()), None).unwrap();
    assert!(hub.release("warm").is_err());
}

#[test]
fn verify_detects_tampered_sources() {
    let hub_dir = temp_dir("hub5");
    let hub = Hub::open(&hub_dir).unwrap();
    hub.publish(&format!("{}/handmade.forte", songs_dir()), None).unwrap();
    hub.release("handmade").unwrap();

    // tamper with the stored snapshot (simulates a compromised store);
    // note: device param *defaults* are overridden at the call site, so touch
    // something that actually reaches the audio — the filter resonance
    let stored = format!("{hub_dir}/store/handmade/v1/devices/warm.forte");
    let src = std::fs::read_to_string(&stored).unwrap();
    assert!(src.contains("reso: 0.3"));
    std::fs::write(&stored, src.replace("reso: 0.3", "reso: 0.9")).unwrap();

    let err = hub.verify("handmade").err().expect("tampering must be detected");
    assert!(err.contains("MISMATCH"), "{err}");
}

#[test]
fn similar_songs_found_across_keys_and_plays_ledgered() {
    let hub_dir = temp_dir("hub6");
    let hub = Hub::open(&hub_dir).unwrap();
    // Em|C|G|D (night-parade) and Am|F|C|G (night-drive): same progression,
    // different keys — the signature must be transposition-invariant
    hub.publish(&format!("{}/night-parade.forte", songs_dir()), None).unwrap();
    hub.publish(&format!("{}/night-drive.forte", songs_dir()), None).unwrap();

    let sim = hub.similar("night-drive").unwrap();
    assert_eq!(sim.len(), 1, "{sim:?}");
    assert_eq!(sim[0].0, "night-parade");
    let sim = hub.similar("night-parade").unwrap();
    assert!(sim.iter().any(|(n, _)| n == "night-drive"), "{sim:?}");

    // play events accumulate in the ledger and surface in the JSON view
    assert_eq!(hub.play_event("night-drive", "alice").unwrap(), 1);
    assert_eq!(hub.play_event("night-drive", "bob").unwrap(), 2);
    let detail = hub.repo_json("night-drive").unwrap();
    assert_eq!(detail["plays"], 2);
    assert!(detail["similar"][0]["name"] == "night-parade");
}

#[test]
fn broken_sources_cannot_be_published() {
    let hub_dir = temp_dir("hub3");
    let work = temp_dir("work3");
    let hub = Hub::open(&hub_dir).unwrap();
    let bad = format!("{work}/bad.forte");
    std::fs::write(&bad, "song \"X\" { track A { } }").unwrap();
    let err = hub.publish(&bad, None).err().expect("must fail");
    assert!(err.contains("E-"), "diagnostics surface in the error: {err}");
}

// ---------------------------------------------------------------------------
// hub × VCS: publish carries history, fork receives it
// ---------------------------------------------------------------------------

#[test]
fn fork_carries_the_full_history() {
    let dir = std::path::PathBuf::from(temp_dir("history"));
    let origin = dir.join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    std::fs::write(
        origin.join("song.forte"),
        "song \"H\" {\n  tempo 100bpm\n  track A {\n    instrument polymer()\n    play beat`x---` at bars(1..2)\n  }\n}\n",
    )
    .unwrap();

    // two commits of history at the origin
    let root = origin.to_str().unwrap();
    fortelang::vcs::Repo::init(root).unwrap();
    let repo = fortelang::vcs::Repo::open(root).unwrap();
    repo.commit("最初のスケッチ").unwrap();
    let src = std::fs::read_to_string(origin.join("song.forte")).unwrap();
    std::fs::write(origin.join("song.forte"), src.replace("100bpm", "112bpm")).unwrap();
    repo.commit("テンポ調整").unwrap();
    let origin_head = repo.head().unwrap().unwrap();

    let hub = Hub::open(dir.join("hub").to_str().unwrap()).unwrap();
    let msg = hub.publish(origin.join("song.forte").to_str().unwrap(), Some("hist")).unwrap();
    assert!(msg.contains("履歴 push"), "{msg}");

    // fork: history moves in, the stamp is a commit, work continues on top
    let fork_dir = dir.join("fork");
    let msg = hub.fork("hist", fork_dir.to_str().unwrap()).unwrap();
    assert!(msg.contains("履歴ごと"), "{msg}");
    let fork = fortelang::vcs::Repo::open(fork_dir.to_str().unwrap()).unwrap();
    assert!(fork.is_clean().unwrap(), "fork must land committed");
    let head = fork.head().unwrap().unwrap();
    let log = fork.log(&head).unwrap();
    assert_eq!(log.len(), 3, "2 origin commits + fork stamp");
    assert!(log[0].1.message.contains("fork hist v1"), "{}", log[0].1.message);
    assert_eq!(log[0].1.parents[0], origin_head, "fork commit sits on the origin head");
    assert_eq!(log[2].1.message, "最初のスケッチ");

    // the fork can diff back into the origin author's history
    let old = fork.snapshot_of(&log[2].0).unwrap();
    let new = fork.snapshot_of("HEAD").unwrap();
    let report = fortelang::semdiff::diff_snapshots(&old, &new);
    assert!(report.contains("tempo: 100 → 112 bpm"), "{report}");

    // registry remembers the exact commit; re-publish records forked_from
    let reg = hub.registry().unwrap();
    assert_eq!(reg.repos["hist"].versions[0].commit.as_deref(), Some(origin_head.as_str()));
    let msg = hub
        .publish(fork_dir.join("song.forte").to_str().unwrap(), Some("hist-fork"))
        .unwrap();
    assert!(msg.contains("forked from hist v1"), "{msg}");
    let reg = hub.registry().unwrap();
    let origin_rec = reg.repos["hist-fork"].versions[0].forked_from.clone().unwrap();
    assert_eq!(origin_rec.commit.as_deref(), Some(origin_head.as_str()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn publish_without_a_repo_still_works_snapshot_only() {
    let dir = std::path::PathBuf::from(temp_dir("nohistory"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("song.forte"),
        "song \"N\" {\n  tempo 100bpm\n  track A {\n    instrument polymer()\n    play beat`x---` at bars(1..2)\n  }\n}\n",
    )
    .unwrap();
    let hub = Hub::open(dir.join("hub").to_str().unwrap()).unwrap();
    let msg = hub.publish(dir.join("song.forte").to_str().unwrap(), Some("plain")).unwrap();
    assert!(!msg.contains("履歴 push"), "{msg}");
    let fork_dir = dir.join("fork");
    let msg = hub.fork("plain", fork_dir.to_str().unwrap()).unwrap();
    assert!(!msg.contains("履歴ごと"), "{msg}");
    assert!(fork_dir.join("song.forte").exists());
    assert!(fortelang::vcs::Repo::open(fork_dir.to_str().unwrap()).is_err(), "no repo expected");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn entry_path_points_into_the_store() {
    let dir = std::path::PathBuf::from(temp_dir("entry"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("song.forte"),
        "song \"E\" {\n  tempo 100bpm\n  track A {\n    instrument polymer()\n    play beat`x---` at bars(1..2)\n  }\n}\n",
    )
    .unwrap();
    let hub = Hub::open(dir.join("hub").to_str().unwrap()).unwrap();
    hub.publish(dir.join("song.forte").to_str().unwrap(), Some("mine")).unwrap();
    let entry = hub.entry_path("mine").unwrap();
    assert!(entry.ends_with("song.forte"), "{entry}");
    assert!(entry.contains("store"), "{entry}");
    // Studio's Listen plays this path directly — it must compile as-is
    let src = std::fs::read_to_string(&entry).unwrap();
    let base = std::path::Path::new(&entry).parent().unwrap().to_string_lossy().into_owned();
    assert!(fortelang::compile_with_loader(&src, &fortelang::FsLoader, &base).is_ok());
    assert!(hub.entry_path("nothere").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// performance fork loop: songs with recorded takes publish / release / fork
// ---------------------------------------------------------------------------

#[test]
fn songs_with_takes_publish_release_and_fork() {
    let dir = std::path::PathBuf::from(temp_dir("takes"));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let tone: Vec<f32> =
        (0..24_000).map(|i| (i as f32 * 330.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.4).collect();
    let prov = serde_json::json!({
        "device_class": "microphone", "recorded_at": "2026-07-04T00:00:00Z",
        "by": "user:test", "session": "s1", "sig": "ed25519:stub",
    });
    let frec = fortelang::frec::encode(48_000, 1, &tone, &prov);
    std::fs::write(dir.join("assets/take-1.frec"), &frec).unwrap();
    std::fs::write(
        dir.join("song.forte"),
        r#"import voice from "./assets/take-1.frec"
song "Vocal" {
  tempo 120bpm
  track Beat { instrument sampler(sample: "Kick") play beat`x---` at bars(1..2) }
  track Voice { audio voice at bars(1..2) }
}"#,
    )
    .unwrap();

    let hub = Hub::open(dir.join("hub").to_str().unwrap()).unwrap();
    let msg = hub.publish(dir.join("song.forte").to_str().unwrap(), Some("vocal")).unwrap();
    assert!(msg.contains("2 files") || msg.contains("3 files"), "take must ride along: {msg}");

    // clean-room release: the stored snapshot must contain the take bytes
    let rel = hub.release("vocal").unwrap();
    assert!(rel.contains("digest"), "{rel}");
    assert!(hub.verify("vocal").unwrap().contains("VERIFIED"), "release must reproduce");

    // fork receives the identical take
    let fork_dir = dir.join("fork");
    hub.fork("vocal", fork_dir.to_str().unwrap()).unwrap();
    let got = std::fs::read(fork_dir.join("assets/take-1.frec")).unwrap();
    assert_eq!(got, frec, "take bytes must survive the round trip");

    // in-memory publish (what the browser posts) with author override
    let mut files = std::collections::BTreeMap::new();
    files.insert(
        "song.forte".to_string(),
        std::fs::read(dir.join("song.forte")).unwrap(),
    );
    files.insert("assets/take-1.frec".to_string(), frec.clone());
    files.insert(
        fortelang::hub::LINEAGE_FILE.to_string(),
        serde_json::to_vec(&serde_json::json!({"repo": "vocal", "v": 1})).unwrap(),
    );
    let msg = hub.publish_map("song.forte", files, "vocal-kenta", Some("kenta")).unwrap();
    assert!(msg.contains("forked from vocal v1"), "{msg}");
    let reg = hub.registry().unwrap();
    assert_eq!(reg.repos["vocal-kenta"].versions[0].author, "kenta");

    // a broken snapshot must be rejected before anything is stored
    let mut bad = std::collections::BTreeMap::new();
    bad.insert("song.forte".to_string(), b"song \"X\" {".to_vec());
    assert!(hub.publish_map("song.forte", bad, "broken", None).is_err());
    assert!(hub.registry().unwrap().repos.get("broken").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lineage_forest_nests_forks_under_their_origin() {
    let dir = std::path::PathBuf::from(temp_dir("forest"));
    std::fs::create_dir_all(&dir).unwrap();
    let song = |name: &str| {
        format!("song \"{name}\" {{\n  tempo 100bpm\n  track A {{\n    instrument polymer()\n    play beat`x---` at bars(1..2)\n  }}\n}}\n")
    };
    std::fs::write(dir.join("root.forte"), song("Root")).unwrap();
    let hub = Hub::open(dir.join("hub").to_str().unwrap()).unwrap();
    hub.publish(dir.join("root.forte").to_str().unwrap(), Some("root-song")).unwrap();

    // two generations of forks
    let f1 = dir.join("f1");
    hub.fork("root-song", f1.to_str().unwrap()).unwrap();
    hub.publish(f1.join("root.forte").to_str().unwrap(), Some("gen1")).unwrap();
    let f2 = dir.join("f2");
    hub.fork("gen1", f2.to_str().unwrap()).unwrap();
    hub.publish(f2.join("root.forte").to_str().unwrap(), Some("gen2")).unwrap();
    // and an unrelated root
    std::fs::write(dir.join("solo.forte"), song("Solo")).unwrap();
    hub.publish(dir.join("solo.forte").to_str().unwrap(), Some("lonely")).unwrap();

    let forest = hub.lineage_forest().unwrap();
    let roots = forest["roots"].as_array().unwrap();
    let names: Vec<&str> = roots.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"root-song") && names.contains(&"lonely"), "{names:?}");
    assert!(!names.contains(&"gen1"), "forks must not appear as roots: {names:?}");

    let root = roots.iter().find(|r| r["name"] == "root-song").unwrap();
    let gen1 = &root["children"].as_array().unwrap()[0];
    assert_eq!(gen1["name"], "gen1");
    assert_eq!(gen1["children"].as_array().unwrap()[0]["name"], "gen2", "grandchild nests");
    let _ = std::fs::remove_dir_all(&dir);
}
