//! Formatter properties: canonical, idempotent, meaning-preserving, and the
//! shipped reference songs are already canonical.

#[test]
fn messy_input_normalizes() {
    let messy = "song \"X\" {\n\t tempo 120bpm   \n\n\n\n      track A {\ninstrument prisma()\n   play beat`x---` at bars(1..1)\n}\n}";
    let out = fortelang::fmt::format(messy).unwrap();
    let expected = "song \"X\" {\n  tempo 120bpm\n\n  track A {\n    instrument prisma()\n    play beat`x---` at bars(1..1)\n  }\n}\n";
    assert_eq!(out, expected);
}

#[test]
fn formatting_is_idempotent_and_meaning_preserving() {
    for song in ["first-light", "slow-circles", "night-parade", "handmade", "night-drive"] {
        let path = format!("{}/../../../packages/essentials_0.6.0/songs/{song}.forte", env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read_to_string(&path).unwrap();
        let once = fortelang::fmt::format(&src).unwrap();
        let twice = fortelang::fmt::format(&once).unwrap();
        assert_eq!(once, twice, "{song}: fmt must be idempotent");
        // reference songs ship in canonical form
        assert_eq!(once, src, "{song}: reference songs must be canonical");
    }
}

#[test]
fn comments_and_literals_survive() {
    let src = "// コメント\nsong \"X\" { /* block */\n  tempo 120bpm\n  let k = beat`x- -x`   // 末尾コメント\n  track A { instrument prisma() play k at bars(1..1) }\n}\n";
    let out = fortelang::fmt::format(src).unwrap();
    assert!(out.contains("// コメント"));
    assert!(out.contains("/* block */"));
    assert!(out.contains("beat`x- -x`"), "literal content untouched: {out}");
    assert!(out.contains("// 末尾コメント"));
}
