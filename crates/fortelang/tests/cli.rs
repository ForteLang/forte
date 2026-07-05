//! The forte-command experience: `forte instrument` composes a playable
//! one-track song for any instrument, and `forte browser` finds the web root.

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[test]
fn live_source_compiles_for_builtins_and_std_instruments() {
    // builtin: no import needed
    let src = fortelang::live::live_source("prisma(wave: \"saw\")", None);
    fortelang::compile_str(&src).expect("builtin live song must compile");

    // std instrument via import (the path forte instrument resolves to)
    let lib = repo_root().join("packages/essentials_0.6.0/instruments/tb303.forte");
    let src = fortelang::live::live_source("Bass303", Some(lib.to_str().unwrap()));
    let base = repo_root();
    fortelang::compile_with_loader(&src, &fortelang::FsLoader, base.to_str().unwrap())
        .expect("Bass303 live song must compile");

    // bare名は () が補われる
    assert!(src.contains("instrument Bass303()"));
}

#[test]
fn browser_finds_the_web_root_from_nested_dirs() {
    let nested = repo_root().join("packages/essentials_0.6.0/blocks");
    let root = fortelang::browser::find_web_root(&nested).expect("web root from nested dir");
    assert!(root.join("web/index.html").is_file());
    assert_eq!(root, repo_root());
}
