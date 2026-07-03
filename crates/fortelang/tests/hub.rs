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
fn broken_sources_cannot_be_published() {
    let hub_dir = temp_dir("hub3");
    let work = temp_dir("work3");
    let hub = Hub::open(&hub_dir).unwrap();
    let bad = format!("{work}/bad.forte");
    std::fs::write(&bad, "song \"X\" { track A { } }").unwrap();
    let err = hub.publish(&bad, None).err().expect("must fail");
    assert!(err.contains("E-"), "diagnostics surface in the error: {err}");
}
