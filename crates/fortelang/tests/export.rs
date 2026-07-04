//! `forte export`: one deterministic zip carries the sources, the recorded
//! takes, the build proof and the full history — unzip it anywhere and the
//! song, its digest and its past all survive.

use fortelang::export::export;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("forte-export-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn export_is_deterministic_and_self_contained() {
    let dir = scratch("full");
    std::fs::create_dir_all(dir.join("devices")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("devices/lib.forte"),
        "device Lead : Instrument {\n  node o = osc(shape: \"saw\")\n  out gain(in: o, mod: adsr())\n}\n",
    )
    .unwrap();
    let tone: Vec<f32> = (0..12_000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    let prov = serde_json::json!({
        "device_class": "microphone", "recorded_at": "2026-07-04T00:00:00Z",
        "by": "user:test", "session": "s1", "sig": "ed25519:stub",
    });
    std::fs::write(dir.join("assets/take-1.frec"), fortelang::frec::encode(48_000, 1, &tone, &prov))
        .unwrap();
    std::fs::write(
        dir.join("song.forte"),
        r#"import { Lead } from "./devices/lib.forte"
import voice from "./assets/take-1.frec"
song "Export" {
  tempo 120bpm
  track A { instrument Lead() play beat`x---` at bars(1..2) }
  track V { audio voice at bars(1..2) }
}"#,
    )
    .unwrap();

    // two commits of history
    let root = dir.to_str().unwrap();
    fortelang::vcs::Repo::init(root).unwrap();
    let repo = fortelang::vcs::Repo::open(root).unwrap();
    repo.commit("v1").unwrap();

    let entry = dir.join("song.forte");
    let a = export(entry.to_str().unwrap()).unwrap();
    let b = export(entry.to_str().unwrap()).unwrap();
    assert_eq!(a.bytes, b.bytes, "export must be byte-identical (deterministic)");
    assert!(a.digest.is_some(), "a song export carries its build proof");
    assert!(a.history_objects > 0, "a clean repo exports its history");

    // unzip into a fresh home: sources + proof + history all work
    let home = scratch("restored");
    for (path, bytes) in fortelang::zip::read(&a.bytes).unwrap() {
        let dest = home.join(&path);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, bytes).unwrap();
    }
    // the song compiles and reproduces the digest recorded in the manifest
    let src = std::fs::read_to_string(home.join("song.forte")).unwrap();
    let p = fortelang::compile_with_loader(&src, &fortelang::FsLoader, home.to_str().unwrap())
        .expect("restored song must compile");
    let digest = format!("{:016x}", fortelang::render_digest(&p, 8.0).f32_digest);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("export.manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["render"]["f32_digest_fnv1a64"], digest.as_str(), "proof travels");
    // the history is a working repository
    let restored = fortelang::vcs::Repo::open(home.to_str().unwrap()).unwrap();
    assert!(restored.is_clean().unwrap(), "restored tree matches its HEAD");
    let head = restored.head().unwrap().unwrap();
    assert_eq!(restored.log(&head).unwrap().last().unwrap().1.message, "v1");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn export_without_a_repo_still_carries_sources_and_proof() {
    let dir = scratch("norepo");
    std::fs::write(
        dir.join("tiny.forte"),
        "song \"T\" {\n  tempo 100bpm\n  track A {\n    instrument polymer()\n    play beat`x---` at bars(1..1)\n  }\n}\n",
    )
    .unwrap();
    let info = export(dir.join("tiny.forte").to_str().unwrap()).unwrap();
    assert!(info.digest.is_some());
    assert_eq!(info.history_objects, 0);
    let names: Vec<String> =
        fortelang::zip::read(&info.bytes).unwrap().into_iter().map(|(n, _)| n).collect();
    assert!(names.contains(&"tiny.forte".to_string()));
    assert!(names.contains(&"export.manifest.json".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}
