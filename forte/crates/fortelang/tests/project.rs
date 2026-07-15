//! Contract tests for the project read side (ADR D-15): `forte project`
//! inventories a forte-init package — songs as songs, blocks as blocks,
//! devices with their params — and reports broken files instead of hiding
//! the project behind them.

use std::path::{Path, PathBuf};

/// A fresh scratch dir per test (repo style — see tests/vcs.rs).
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forte-project-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn scaffold(root: &Path) {
    write(
        root,
        "package.forte",
        "block MyAlbum {\n  desc \"a test package\"\n  tags \"test, fixture\"\n  version \"0.1.0\"\n}\n",
    );
    write(
        root,
        "blocks/groove.forte",
        "import { SubBass } from \"../instruments/lead.forte\"\n\n\
         block Groove {\n  desc \"the groove\"\n  tempo 120bpm\n  let K = beat`x...`\n  track Drums {\n    instrument sampler()\n    play K at bars(1..2)\n  }\n}\n\n\
         block Fill : Groove {\n  track Drums {\n    instrument sampler()\n    play beat`x.x.` at bars(1..1)\n  }\n}\n",
    );
    write(
        root,
        "songs/one.forte",
        "import { Groove } from \"../blocks/groove.forte\"\n\n\
         song \"One\" {\n  desc \"the song\"\n  tempo 96bpm\n  section intro = bars(1..4)\n  track Bass {\n    instrument mono()\n    play notes`A1 .` at bars(1..2)\n  }\n  play Groove at bars(1..4)\n}\n",
    );
    write(
        root,
        "instruments/lead.forte",
        "device SubBass : Instrument {\n  param drive = 0.25 in 0.0..1.0\n  node o = osc(shape: \"sine\")\n  out gain(in: o, level: 0.9)\n}\n",
    );
    write(root, "blocks/broken.forte", "block Oops {\n"); // unclosed on purpose
}

#[test]
fn a_package_is_inventoried_by_element_kind() {
    let dir = scratch("inventory");
    scaffold(&dir);
    let v = fortelang::project::project_json(&dir).unwrap();

    assert_eq!(v["name"], "MyAlbum");
    assert_eq!(v["desc"], "a test package");
    assert_eq!(v["version"], "0.1.0");

    // songs open as songs: name, sections, tracks (with code-jump lines)
    let song = &v["songs"][0];
    assert_eq!(song["file"], "songs/one.forte");
    assert_eq!(song["song"]["name"], "One");
    assert_eq!(song["song"]["tempo"], 96.0);
    assert_eq!(song["song"]["sections"][0]["name"], "intro");
    assert_eq!(song["song"]["tracks"][0]["name"], "Bass");
    assert_eq!(song["song"]["places"], 1);
    assert_eq!(song["imports"][0]["names"][0], "Groove");

    // blocks open as blocks: every top-level block, inheritance included
    let blocks = v["blocks"].as_array().unwrap();
    let groove = blocks.iter().find(|f| f["file"] == "blocks/groove.forte").unwrap();
    assert_eq!(groove["blocks"][0]["name"], "Groove");
    assert_eq!(groove["blocks"][0]["desc"], "the groove");
    assert_eq!(groove["blocks"][0]["patterns"], 1);
    assert_eq!(groove["blocks"][1]["name"], "Fill");
    assert_eq!(groove["blocks"][1]["parent"], "Groove");

    // devices expose their set_arg-able params with ranges
    let dev = &v["instruments"][0]["devices"][0];
    assert_eq!(dev["name"], "SubBass");
    assert_eq!(dev["kind"], "Instrument");
    assert_eq!(dev["params"][0]["name"], "drive");
    assert_eq!(dev["params"][0]["range"][1], 1.0);
}

#[test]
fn a_broken_file_is_reported_not_hidden() {
    let dir = scratch("broken");
    scaffold(&dir);
    let v = fortelang::project::project_json(&dir).unwrap();
    let blocks = v["blocks"].as_array().unwrap();
    let broken = blocks.iter().find(|f| f["file"] == "blocks/broken.forte").unwrap();
    assert!(broken["error"].as_str().is_some_and(|e| !e.is_empty()));
    // and the healthy file in the same directory is still fully listed
    assert!(blocks.iter().any(|f| f["file"] == "blocks/groove.forte"));
}

#[test]
fn a_directory_without_a_manifest_is_refused() {
    let dir = scratch("no-manifest");
    let err = fortelang::project::project_json(&dir).unwrap_err();
    assert!(err.contains("package.forte"));
}
