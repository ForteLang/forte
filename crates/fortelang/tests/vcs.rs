//! VCS end-to-end: init → commit → branch → checkout roundtrip, and the
//! semantic diff speaking music, not line numbers.

use fortelang::vcs::Repo;

const SONG_A: &str = r#"song "X" {
  tempo 108bpm
  section main = bars(1..4)
  track Keys {
    instrument prisma(wave: "square", cutoff: 0.45)
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
    instrument prisma(wave: "saw", cutoff: 0.45)
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

// ---------------------------------------------------------------------------
// merge
// ---------------------------------------------------------------------------

#[test]
fn disjoint_edits_merge_automatically() {
    let dir = scratch("merge-ok");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("base").unwrap();

    repo.create_branch("faster").unwrap();
    repo.checkout("faster").unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A.replace("tempo 108bpm", "tempo 120bpm")).unwrap();
    repo.commit("テンポ上げ").unwrap();

    repo.checkout("main").unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A.replace("volume 0.5", "volume 0.8")).unwrap();
    repo.commit("Keys 大きく").unwrap();

    let msg = repo.merge("faster").expect("disjoint edits must merge");
    assert!(msg.contains("merge faster"), "{msg}");
    assert!(!msg.contains("コンパイルできません"), "merged song must compile: {msg}");
    let merged = std::fs::read_to_string(dir.join("song.forte")).unwrap();
    assert!(merged.contains("tempo 120bpm") && merged.contains("volume 0.8"), "{merged}");
    // merge commit carries both parents
    let head = repo.head().unwrap().unwrap();
    assert_eq!(repo.commit_obj(&head).unwrap().parents.len(), 2);
    // and merging again is a no-op
    assert!(repo.merge("faster").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn conflicting_edits_leave_markers_and_resolution_records_both_parents() {
    let dir = scratch("merge-conflict");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("base").unwrap();

    repo.create_branch("loud").unwrap();
    repo.checkout("loud").unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A.replace("volume 0.5", "volume 0.9")).unwrap();
    repo.commit("loud").unwrap();

    repo.checkout("main").unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A.replace("volume 0.5", "volume 0.2")).unwrap();
    repo.commit("quiet").unwrap();

    let err = repo.merge("loud").expect_err("same-line edits must conflict");
    assert!(err.contains("同じ行を両方で編集"), "{err}");
    let marked = std::fs::read_to_string(dir.join("song.forte")).unwrap();
    assert!(marked.contains("<<<<<<< main") && marked.contains(">>>>>>> loud"), "{marked}");

    // resolve by keeping ours — the commit still records loud as parent #2
    std::fs::write(dir.join("song.forte"), SONG_A.replace("volume 0.5", "volume 0.2")).unwrap();
    let msg = repo.commit("解消").unwrap();
    assert!(msg.contains("解消"), "{msg}");
    let head = repo.head().unwrap().unwrap();
    assert_eq!(repo.commit_obj(&head).unwrap().parents.len(), 2, "resolution is a merge commit");
    assert!(repo.merge("loud").is_err(), "loud is now an ancestor");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fast_forward_when_we_have_nothing_of_our_own() {
    let dir = scratch("merge-ff");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), SONG_A).unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("base").unwrap();
    repo.create_branch("work").unwrap();
    repo.checkout("work").unwrap();
    std::fs::write(dir.join("song.forte"), SONG_B).unwrap();
    repo.commit("work").unwrap();
    repo.checkout("main").unwrap();
    let msg = repo.merge("work").unwrap();
    assert!(msg.contains("fast-forward"), "{msg}");
    assert_eq!(std::fs::read_to_string(dir.join("song.forte")).unwrap(), SONG_B);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clean_text_merge_with_broken_music_warns() {
    // branch A renames the section (and its own use); branch B adds a track
    // still playing at the old name. The lines are disjoint — text merges
    // cleanly — but the music is broken. The merge must say so.
    let base = r#"song "X" {
  tempo 100bpm
  section verse = bars(1..4)
  track A {
    instrument prisma()
    play beat`x---` at verse
  }
}
"#;
    let dir = scratch("merge-warn");
    let root = dir.to_str().unwrap();
    std::fs::write(dir.join("song.forte"), base).unwrap();
    Repo::init(root).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.commit("base").unwrap();

    repo.create_branch("rename").unwrap();
    repo.checkout("rename").unwrap();
    std::fs::write(dir.join("song.forte"), base.replace("verse", "chorus")).unwrap();
    repo.commit("セクション改名").unwrap();

    repo.checkout("main").unwrap();
    std::fs::write(
        dir.join("song.forte"),
        base.replace(
            "\n}\n",
            "\n  track B {\n    instrument prisma(wave: \"tri\")\n    play beat`--x-` at verse\n  }\n}\n",
        ),
    )
    .unwrap();
    repo.commit("トラック追加").unwrap();

    let msg = repo.merge("rename").expect("text-level merge is clean");
    assert!(
        msg.contains("コンパイルできません"),
        "semantically broken merge must warn: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
