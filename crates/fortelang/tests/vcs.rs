//! VCS end-to-end: init → commit → branch → checkout roundtrip, and the
//! semantic diff speaking music, not line numbers.

use fortelang::vcs::Repo;

const SONG_A: &str = r#"song "X" {
  tempo 108bpm
  section main = bars(1..4)
  track Keys {
    instrument polymer(wave: "square", cutoff: 0.45)
    volume 0.5
    play notes`C4:1 E4:1 G4:1 _:1` at main
  }
}
"#;

/// Same song, three musical edits: tempo, wave, and a dropped section of play.
const SONG_B: &str = r#"song "X" {
  tempo 116bpm
  section main = bars(1..4)
  track Keys {
    instrument polymer(wave: "saw", cutoff: 0.45)
    volume 0.5
    play notes`C4:1 E4:1 G4:1 _:1` at main
  }
}
"#;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("forte-vcs-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn commit_log_branch_checkout_roundtrip() {
    let dir = scratch("roundtrip");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();

    Repo::init(root).unwrap();
    assert!(Repo::init(root).is_err(), "double init must fail");
    let repo = Repo::open(root).unwrap();
    assert!(!repo.is_clean().unwrap(), "untracked song = dirty");

    let msg = repo.commit("最初のスケッチ").unwrap();
    assert!(msg.contains("#1"), "{msg}");
    assert!(repo.is_clean().unwrap());
    assert!(repo.commit("何もない").is_err(), "empty commit must fail");

    // second commit on main
    std::fs::write(dir.join("song.forte"), SONG_B).unwrap();
    repo.commit("明るく速く").unwrap();
    let head = repo.head().unwrap().unwrap();
    let log = repo.log(&head).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].1.message, "明るく速く");
    assert_eq!(log[0].1.n, 2);
    assert_eq!(log[1].1.parents.len(), 0);

    // branch from #2, then walk back to #1 (detached) and out again
    repo.create_branch("idea").unwrap();
    let first = &log[1].0;
    repo.checkout(&first[..8]).unwrap(); // unique prefix resolves
    let restored = std::fs::read_to_string(dir.join("song.forte")).unwrap();
    assert_eq!(restored, SONG_A, "checkout must restore the old source exactly");
    assert!(repo.current_branch().unwrap().is_none(), "detached HEAD");
    assert!(repo.commit("迷子コミット").is_err(), "no commits while detached");

    repo.checkout("idea").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("song.forte")).unwrap(), SONG_B);
    assert_eq!(repo.current_branch().unwrap().as_deref(), Some("idea"));

    // dirty tree blocks checkout
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();
    assert!(repo.checkout("main").is_err(), "dirty tree must refuse checkout");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn semantic_diff_speaks_music() {
    let dir = scratch("semdiff");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("v1").unwrap();

    std::fs::write(dir.join("song.forte"), SONG_B).unwrap();
    let old = repo.snapshot_of("HEAD").unwrap();
    let new = repo.working_snapshot().unwrap();
    let report = fortelang::semdiff::diff_snapshots(&old, &new);
    assert!(report.contains("tempo: 108 → 116 bpm"), "{report}");
    assert!(report.contains("wave: square → saw"), "{report}");
    assert!(!report.contains("- "), "semantic diff must not fall back to lines: {report}");

    // comment-only edits: file changed, model identical
    repo.commit("v2").unwrap();
    std::fs::write(dir.join("song.forte"), format!("// メモ\n{SONG_B}")).unwrap();
    let report = fortelang::semdiff::diff_snapshots(
        &repo.snapshot_of("HEAD").unwrap(),
        &repo.working_snapshot().unwrap(),
    );
    assert!(report.contains("モデルは同一"), "{report}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn library_edit_surfaces_in_importing_song() {
    let dir = scratch("imports");
    std::fs::create_dir_all(dir.join("devices")).unwrap();
    std::fs::write(
        dir.join("devices/lib.forte"),
        r#"device Lead : Instrument {
  node o   = osc(shape: "saw")
  node env = adsr(a: 0.03, d: 0.25, s: 0.6, r: 0.3)
  node f   = svf(in: o, cutoff: 0.5, reso: 0.3)
  out gain(in: f, mod: env, level: 0.9)
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("song.forte"),
        r#"import { Lead } from "./devices/lib.forte"
song "X" {
  tempo 100bpm
  track A {
    instrument Lead()
    play notes`C3:1` at bars(1..2)
  }
}
"#,
    )
    .unwrap();
    let root = dir.to_str().unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("v1").unwrap();

    // edit only the library — the song file is untouched but sounds different
    let lib = std::fs::read_to_string(dir.join("devices/lib.forte")).unwrap();
    std::fs::write(dir.join("devices/lib.forte"), lib.replace("reso: 0.3", "reso: 0.7")).unwrap();
    let report = fortelang::semdiff::diff_snapshots(
        &repo.snapshot_of("HEAD").unwrap(),
        &repo.working_snapshot().unwrap(),
    );
    assert!(
        report.contains("song.forte (import 経由で音が変わります)"),
        "library edits must surface at the songs that hear them: {report}"
    );
    assert!(report.contains("パッチ(ノードグラフ)が変わりました"), "{report}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn object_store_is_content_addressed() {
    let dir = scratch("store");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("v1").unwrap();

    // identical content re-committed on a branch reuses the same tree object:
    // commit with the same tree is rejected as "no changes"
    repo.create_branch("same").unwrap();
    repo.checkout("same").unwrap();
    assert!(repo.commit("同じ内容").is_err());

    // unknown revision errors politely
    assert!(repo.resolve("no-such-branch").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
