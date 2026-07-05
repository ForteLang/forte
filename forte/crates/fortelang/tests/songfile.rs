//! `.fortesong` — the playable, self-contained build (issue #53): pack,
//! load with tamper check, reproduce the render proof, and album detection.

use std::path::Path;

fn setup(base: &Path) -> String {
    std::fs::create_dir_all(base.join("songs")).unwrap();
    std::fs::create_dir_all(base.join("instruments")).unwrap();
    // the project IS a package — its meta becomes the credits
    std::fs::write(
        base.join("package.forte"),
        "block Mini {\n  desc \"Test package.\"\n  version \"0.3.0\"\n  artist \"Mini Crew\"\n  sponsor \"https://example.com/support\"\n}\n",
    )
    .unwrap();
    // an import that climbs with ../ — the layout every package song uses
    std::fs::write(
        base.join("instruments").join("lead.forte"),
        "device Lead : Instrument {\n  param cutoff = 0.5 in 0..1\n  node o = osc(shape: \"saw\")\n  node f = svf(in: o, cutoff: cutoff)\n  out gain(in: f, mod: adsr())\n}\n",
    )
    .unwrap();
    std::fs::write(
        base.join("songs").join("tune.forte"),
        "import { Lead } from \"../instruments/lead.forte\"\nsong \"Tune\" {\n  desc \"One-bar test tune.\"\n  artist \"Test Artist\"\n  tempo 120bpm\n  track A {\n    instrument Lead(cutoff: 0.6)\n    play notes`C4:1 E4:1 G4:1 C5:1` at bars(1..1)\n  }\n}\n",
    )
    .unwrap();
    base.join("songs").join("tune.forte").to_string_lossy().into_owned()
}

#[test]
fn fortesong_roundtrip_tamper_and_album() {
    let base = std::env::temp_dir().join(format!("forte-songfile-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let entry = setup(&base);

    // build: sources with ../ imports land rebased under a common root
    let (bytes, summary) = fortelang::songfile::build(&entry).expect("build");
    assert!(summary.contains("2 sources"), "summary: {summary}");
    let fs_path = base.join("tune.fortesong");
    std::fs::write(&fs_path, &bytes).unwrap();

    // load: meta travels, files digest passes
    let sf = fortelang::songfile::load(&fs_path.to_string_lossy()).expect("load");
    assert_eq!(sf.name, "Tune");
    assert_eq!(sf.artist, "Test Artist");
    assert_eq!(sf.entry, "songs/tune.forte");
    assert!(sf.files.contains_key("instruments/lead.forte"), "rebased import");

    // credits: the source package's own meta rides along
    assert_eq!(sf.credits, vec!["Mini 0.3.0 (Mini Crew)"], "credits: {:?}", sf.credits);

    // the packed sources compile and reproduce the packed render digest
    let project = fortelang::songfile::compile(&sf).expect("compile");
    assert_eq!(project.name, "Tune");
    let ok = fortelang::songfile::verify(&sf).expect("verify");
    assert!(ok.contains(&sf.render_digest), "verify: {ok}");

    // tamper with a packed source → load refuses
    let entries = fortelang::zip::read(&bytes).expect("zip read");
    let tampered: Vec<(String, Vec<u8>)> = entries
        .into_iter()
        .map(|(n, d)| {
            if n == "songs/tune.forte" {
                (n, b"song \"Evil\" { tempo 120bpm }\n".to_vec())
            } else {
                (n, d)
            }
        })
        .collect();
    let bad_path = base.join("tampered.fortesong");
    std::fs::write(&bad_path, fortelang::zip::write(&tampered)).unwrap();
    let err = match fortelang::songfile::load(&bad_path.to_string_lossy()) {
        Err(e) => e,
        Ok(_) => panic!("tampered .fortesong must not load"),
    };
    assert!(err.contains("改竄"), "tamper error: {err}");

    // album: album.forte meta + lexicographic .fortesong order
    let album_dir = base.join("album");
    std::fs::create_dir_all(&album_dir).unwrap();
    std::fs::write(
        album_dir.join("album.forte"),
        "block TestAlbum {\n  desc \"Two takes of the tune.\"\n  artist \"Test Artist\"\n}\n",
    )
    .unwrap();
    std::fs::copy(&fs_path, album_dir.join("02-second.fortesong")).unwrap();
    std::fs::copy(&fs_path, album_dir.join("01-first.fortesong")).unwrap();
    let album = fortelang::songfile::load_album(&album_dir).expect("load_album").expect("is album");
    assert_eq!(album.title, "TestAlbum");
    assert_eq!(album.artist, "Test Artist");
    assert_eq!(album.tracks.len(), 2);
    assert!(album.tracks[0].to_string_lossy().ends_with("01-first.fortesong"), "sorted order");

    // a directory without album.forte is not an album
    assert!(fortelang::songfile::load_album(&base).expect("no album").is_none());

    let _ = std::fs::remove_dir_all(&base);
}
