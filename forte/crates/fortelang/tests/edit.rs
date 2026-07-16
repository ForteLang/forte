//! Contract tests for the lossless edit layer (Studio P0, issue #135).
//!
//! The promise under test: a structured edit changes ONLY the bytes of the
//! tokens it targets. Comments (Japanese ones included), blank lines,
//! unusual-but-valid spacing and everything else survive byte-for-byte.

use fortelang::edit::{apply_ops, parse_ops};

/// A fixture that exercises the things a formatter would destroy: comments
/// at every level, a block comment, blank lines, and uneven spacing.
const SRC: &str = r#"// 見出しコメント: このファイルはテスト用のフィクスチャ
/* block comment
   spanning lines */

block Groove {
  tempo 112bpm
  key A minor

  section intro = bars(1..4)
  section drop  = bars(5..12)   // ドロップはここ

  let K = beat`x... x... x... x...`

  track Drums {
    instrument sampler(slices: 16,  choke: "on")
    insert   filter(type: "lp", cutoff: 0.8)
    play K at bars(1..8)
    play beat`.... x... .... x..-`   at bars(9..12)
  }

  track Bass {
    instrument mono(cutoff: 0.5)   // ベースの音色
    play notes`A1 . . . C2 . . .` at intro
  }
}

song "fixture" {
  tempo 96bpm

  block Inner {
    track T {
      instrument mono()
      play notes`A2 . A2 .` at bars(1..2)
    }
  }

  play Inner at bars(1..4)
  play Inner as Twin at bars(5..8)   // エイリアス配置
}
"#;

fn apply(src: &str, json: &str) -> String {
    let ops = parse_ops(json).expect("ops parse");
    apply_ops(src, &ops).expect("apply")
}

/// The whole point: everything outside the splice is byte-identical.
/// Diff the fixture line by line and assert exactly the expected lines moved.
fn assert_only_lines_changed(before: &str, after: &str, expect: &[(usize, &str)]) {
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    assert_eq!(b.len(), a.len(), "line count changed:\n{after}");
    let mut changed = Vec::new();
    for (i, (lb, la)) in b.iter().zip(a.iter()).enumerate() {
        if lb != la {
            changed.push((i + 1, *la));
        }
    }
    assert_eq!(
        changed, expect,
        "unexpected diff shape.\n--- before\n{before}\n--- after\n{after}"
    );
}

#[test]
fn set_tempo_touches_one_number_and_keeps_the_unit() {
    let out = apply(SRC, r#"{"op":"set_tempo","path":["Groove"],"bpm":118}"#);
    assert_only_lines_changed(SRC, &out, &[(6, "  tempo 118bpm")]);
}

#[test]
fn set_pattern_rewrites_between_the_backticks_only() {
    let out = apply(
        SRC,
        r#"{"op":"set_pattern","path":["Groove"],"let_name":"K","value":"x.x. x... x.x. x..."}"#,
    );
    assert_only_lines_changed(SRC, &out, &[(12, "  let K = beat`x.x. x... x.x. x...`")]);
}

#[test]
fn set_pattern_reaches_an_inline_play_literal() {
    let out = apply(
        SRC,
        r#"{"op":"set_pattern","path":["Groove"],"track":"Drums","play":1,"value":".... x... .x.. x..-"}"#,
    );
    // the odd multi-space layout around `at` survives
    assert_only_lines_changed(
        SRC,
        &out,
        &[(18, "    play beat`.... x... .x.. x..-`   at bars(9..12)")],
    );
}

#[test]
fn move_play_rewrites_only_the_bar_numbers() {
    let out = apply(SRC, r#"{"op":"move_play","path":["Groove"],"track":"Drums","play":0,"bars":[9,16]}"#);
    assert_only_lines_changed(SRC, &out, &[(17, "    play K at bars(9..16)")]);
}

#[test]
fn move_play_converts_a_section_ref_to_bars() {
    let out = apply(SRC, r#"{"op":"move_play","path":["Groove"],"track":"Bass","play":0,"bars":[5,8]}"#);
    assert_only_lines_changed(
        SRC,
        &out,
        &[(23, "    play notes`A1 . . . C2 . . .` at bars(5..8)")],
    );
}

#[test]
fn move_place_respects_the_alias_and_its_comment() {
    let out = apply(SRC, r#"{"op":"move_place","place":1,"block":"Twin","bars":[9,16]}"#);
    assert_only_lines_changed(
        SRC,
        &out,
        &[(38, "  play Inner as Twin at bars(9..16)   // エイリアス配置")],
    );
}

#[test]
fn move_place_guard_rejects_a_stale_index() {
    let ops = parse_ops(r#"{"op":"move_place","place":0,"block":"Nonesuch","bars":[9,16]}"#).unwrap();
    let err = apply_ops(SRC, &ops).unwrap_err();
    assert_eq!(err.code, "E-EDIT-003");
}

#[test]
fn set_arg_rewrites_a_value_in_place() {
    let out = apply(
        SRC,
        r#"{"op":"set_arg","path":["Groove"],"track":"Bass","target":"instrument","arg":"cutoff","value":0.62}"#,
    );
    assert_only_lines_changed(SRC, &out, &[(22, "    instrument mono(cutoff: 0.62)   // ベースの音色")]);
}

#[test]
fn set_arg_rewrites_a_string_and_keeps_odd_spacing() {
    let out = apply(
        SRC,
        r#"{"op":"set_arg","path":["Groove"],"track":"Drums","target":"insert:0","arg":"type","value":"hp"}"#,
    );
    assert_only_lines_changed(
        SRC,
        &out,
        &[(16, "    insert   filter(type: \"hp\", cutoff: 0.8)")],
    );
}

#[test]
fn set_arg_adds_a_missing_argument() {
    let out = apply(
        SRC,
        r#"{"op":"set_arg","path":["Groove"],"track":"Drums","target":"instrument","arg":"gain","value":1.2}"#,
    );
    assert_only_lines_changed(
        SRC,
        &out,
        &[(15, "    instrument sampler(slices: 16,  choke: \"on\", gain: 1.2)")],
    );
}

#[test]
fn set_arg_adds_parens_to_a_bare_call() {
    let out = apply(
        SRC,
        r#"{"op":"set_arg","path":["Inner"],"track":"T","target":"instrument","arg":"cutoff","value":0.4}"#,
    );
    assert_only_lines_changed(SRC, &out, &[(32, "      instrument mono(cutoff: 0.4)")]);
}

#[test]
fn set_section_moves_the_range_and_spares_the_comment() {
    let out = apply(SRC, r#"{"op":"set_section","path":["Groove"],"name":"drop","bars":[5,16]}"#);
    assert_only_lines_changed(SRC, &out, &[(10, "  section drop  = bars(5..16)   // ドロップはここ")]);
}

#[test]
fn add_place_appends_with_matching_indentation() {
    let out = apply(SRC, r#"{"op":"add_place","block":"Inner","bars":[9,12],"alias":"Third"}"#);
    let b: Vec<&str> = SRC.lines().collect();
    let a: Vec<&str> = out.lines().collect();
    assert_eq!(a.len(), b.len() + 1);
    assert_eq!(a[38], "  play Inner as Third at bars(9..12)");
    // everything before and after the inserted line is untouched
    assert_eq!(&a[..38], &b[..38]);
    assert_eq!(&a[39..], &b[38..]);
}

#[test]
fn remove_place_deletes_the_whole_line_including_its_comment() {
    let out = apply(SRC, r#"{"op":"remove_place","place":1,"block":"Twin"}"#);
    let b: Vec<&str> = SRC.lines().collect();
    let a: Vec<&str> = out.lines().collect();
    assert_eq!(a.len(), b.len() - 1);
    assert_eq!(&a[..37], &b[..37]);
    assert_eq!(&a[37..], &b[38..]);
}

#[test]
fn ops_compose_and_the_result_still_parses() {
    let out = apply(
        SRC,
        r#"[
          {"op":"set_tempo","path":["Groove"],"bpm":124},
          {"op":"move_play","path":["Groove"],"track":"Drums","play":0,"bars":[1,12]},
          {"op":"add_place","block":"Inner","bars":[9,12]},
          {"op":"set_arg","path":["Groove"],"track":"Bass","target":"instrument","arg":"cutoff","value":0.7}
        ]"#,
    );
    assert!(fortelang::parser::parse(&out).is_ok());
    assert!(out.contains("tempo 124bpm"));
    assert!(out.contains("play K at bars(1..12)"));
    assert!(out.contains("play Inner at bars(9..12)"));
    assert!(out.contains("mono(cutoff: 0.7)"));
    // the fixture's comments all survive a batch of edits
    assert!(out.contains("見出しコメント"));
    assert!(out.contains("block comment"));
    assert!(out.contains("// ドロップはここ"));
    assert!(out.contains("// ベースの音色"));
    assert!(out.contains("// エイリアス配置"));
}

#[test]
fn the_same_op_applied_twice_is_idempotent() {
    let op = r#"{"op":"set_arg","path":["Groove"],"track":"Bass","target":"instrument","arg":"cutoff","value":0.62}"#;
    let once = apply(SRC, op);
    let twice = apply(&once, op);
    assert_eq!(once, twice);
}

#[test]
fn a_pattern_value_with_a_backtick_is_refused() {
    let ops = parse_ops(r#"{"op":"set_pattern","let_name":"K","value":"x`x"}"#).unwrap();
    assert_eq!(apply_ops(SRC, &ops).unwrap_err().code, "E-EDIT-005");
}

#[test]
fn edits_never_emit_source_that_fails_to_parse() {
    // a pattern body the parser can't lex back (unclosed string in literal
    // position is fine — backticks accept anything except a backtick — so
    // instead: a section rename target that never existed)
    let ops = parse_ops(r#"{"op":"set_section","name":"nonesuch","bars":[1,2]}"#).unwrap();
    assert_eq!(apply_ops(SRC, &ops).unwrap_err().code, "E-EDIT-003");
}

#[test]
fn lexer_byte_spans_are_exact_and_monotonic() {
    let toks = fortelang::lexer::lex(SRC).unwrap();
    let mut prev_end = 0usize;
    for t in &toks {
        if matches!(t.tok, fortelang::lexer::Tok::Eof) {
            continue;
        }
        assert!(t.off >= prev_end, "token spans overlap at {}", t.pos);
        assert!(t.end > t.off, "empty span at {} ({:?})", t.pos, t.tok);
        prev_end = t.end;
        // spot-check: the bytes under an ident/number span ARE its text
        match &t.tok {
            fortelang::lexer::Tok::Ident(s) => assert_eq!(&SRC[t.off..t.end], s),
            fortelang::lexer::Tok::Str(s) => assert_eq!(&SRC[t.off..t.end], format!("\"{s}\"")),
            _ => {}
        }
    }
}

/// Mixer-side fixture: volume / level / pan / send statements to rewrite,
/// and a track that has none of them (the insertion path).
const MIX: &str = r#"song "mix" {
  tempo 120bpm

  track Drums {
    instrument sampler()
    volume 0.8
    pan  0.35   // ちょい左
    send Space 0.2
    play beat`x...` at bars(1..2)
  }

  track Bass {
    instrument mono()
    level -14
    play notes`A1 .` at bars(1..2)
  }
}
"#;

#[test]
fn set_track_rewrites_volume_in_place() {
    let out = apply(MIX, r#"{"op":"set_track","track":"Drums","field":"volume","value":0.62}"#);
    assert_only_lines_changed(MIX, &out, &[(6, "    volume 0.62")]);
}

#[test]
fn set_track_keeps_odd_spacing_and_the_comment() {
    let out = apply(MIX, r#"{"op":"set_track","track":"Drums","field":"pan","value":0.7}"#);
    assert_only_lines_changed(MIX, &out, &[(7, "    pan  0.7   // ちょい左")]);
}

#[test]
fn set_track_rewrites_a_negative_level() {
    let out = apply(MIX, r#"{"op":"set_track","track":"Bass","field":"level","value":-12}"#);
    assert_only_lines_changed(MIX, &out, &[(14, "    level -12")]);
}

#[test]
fn set_track_inserts_a_missing_statement() {
    let out = apply(MIX, r#"{"op":"set_track","track":"Bass","field":"volume","value":0.5}"#);
    let b: Vec<&str> = MIX.lines().collect();
    let a: Vec<&str> = out.lines().collect();
    assert_eq!(a.len(), b.len() + 1);
    // the statement lands as the track's first line, matching its indent
    assert_eq!(a[12], "    volume 0.5");
    assert_eq!(&a[..12], &b[..12]);
    assert_eq!(&a[13..], &b[12..]);
}

#[test]
fn set_track_rejects_an_unknown_field() {
    let ops = parse_ops(r#"{"op":"set_track","track":"Drums","field":"reverb","value":0.5}"#).unwrap();
    assert_eq!(apply_ops(MIX, &ops).unwrap_err().code, "E-EDIT-001");
}

#[test]
fn set_track_stays_inside_a_one_line_body() {
    let src = "song \"s\" {\n  track T { instrument mono() }\n}\n";
    let ops = parse_ops(r#"{"op":"set_track","track":"T","field":"volume","value":0.5}"#).unwrap();
    let out = apply_ops(src, &ops).unwrap();
    assert_eq!(out, "song \"s\" {\n  track T { volume 0.5 instrument mono() }\n}\n");
}

#[test]
fn set_send_rewrites_the_level() {
    let out = apply(MIX, r#"{"op":"set_send","track":"Drums","dest":"Space","level":0.35}"#);
    assert_only_lines_changed(MIX, &out, &[(8, "    send Space 0.35")]);
}

#[test]
fn set_send_adds_a_missing_send() {
    let out = apply(MIX, r#"{"op":"set_send","track":"Bass","dest":"Space","level":0.3}"#);
    let b: Vec<&str> = MIX.lines().collect();
    let a: Vec<&str> = out.lines().collect();
    assert_eq!(a.len(), b.len() + 1);
    assert_eq!(a[12], "    send Space 0.3");
    assert_eq!(&a[..12], &b[..12]);
    assert_eq!(&a[13..], &b[12..]);
}

#[test]
fn add_track_appends_with_instrument_and_starter_play() {
    let src = "song \"s\" {\n  tempo 100bpm\n\n  track A {\n    instrument sampler(sample: \"Kick\")\n    play beat`x...` at bars(1..1)\n  }\n}\n";
    let out = apply(
        src,
        r#"{"op":"add_track","name":"Bass","instrument":"prisma(wave: \"saw\")","play":"notes`C2:1` at bars(1..1)"}"#,
    );
    assert!(out.contains("  track Bass {\n    instrument prisma(wave: \"saw\")\n    play notes`C2:1` at bars(1..1)\n  }\n"), "{out}");
    // duplicates and brace smuggling are refused
    let dup = parse_ops(r#"{"op":"add_track","name":"A","instrument":"mono()"}"#).unwrap();
    assert_eq!(apply_ops(src, &dup).unwrap_err().code, "E-EDIT-003");
    let bad = parse_ops(r#"{"op":"add_track","name":"B","instrument":"mono() } track X {"}"#).unwrap();
    assert_eq!(apply_ops(src, &bad).unwrap_err().code, "E-EDIT-005");
}

#[test]
fn set_place_block_swaps_only_the_name() {
    // fixture line 38 = `  play Inner as Twin at bars(5..8)   // エイリアス配置`
    let out = apply(SRC, r#"{"op":"set_place_block","line":38,"to":"Other"}"#);
    assert_only_lines_changed(
        SRC,
        &out,
        &[(38, "  play Other as Twin at bars(5..8)   // エイリアス配置")],
    );
    let miss = parse_ops(r#"{"op":"set_place_block","line":1,"to":"X"}"#).unwrap();
    assert_eq!(apply_ops(SRC, &miss).unwrap_err().code, "E-EDIT-003");
}

#[test]
fn remove_at_line_deletes_the_placement_line() {
    let out = apply(SRC, r#"{"op":"remove_at_line","line":38}"#);
    let b: Vec<&str> = SRC.lines().collect();
    let a: Vec<&str> = out.lines().collect();
    assert_eq!(a.len(), b.len() - 1);
    assert!(!out.contains("as Twin"));
}

#[test]
fn rename_and_reorder_tracks() {
    let src = "song \"s\" {\n  track A {\n    instrument sampler(sample: \"Kick\")\n    play beat`x...` at bars(1..1)\n  }\n\n  track B {\n    instrument sampler(sample: \"Hat\")\n    play beat`..x.` at bars(1..1)\n  }\n}\n";
    let out = apply(src, r#"{"op":"rename_track","track":"A","to":"Drums"}"#);
    assert!(out.contains("track Drums {"), "{out}");
    assert!(!out.contains("track A {"));
    // duplicate name refused
    let dup = parse_ops(r#"{"op":"rename_track","track":"B","to":"Drums"}"#).unwrap();
    assert_eq!(apply_ops(&out, &dup).unwrap_err().code, "E-EDIT-003");
    // reorder: B moves earlier; the blank separator stays where it was
    let out2 = apply(src, r#"{"op":"move_track","track":"B","dir":-1}"#);
    let a_pos = out2.find("track A").unwrap();
    let b_pos = out2.find("track B").unwrap();
    assert!(b_pos < a_pos, "{out2}");
    assert!(fortelang::parser::parse(&out2).is_ok());
    // at the edge: byte-identical no-op
    let out3 = apply(src, r#"{"op":"move_track","track":"A","dir":-1}"#);
    assert_eq!(out3, src);
}

#[test]
fn automation_round_trip_set_add_remove() {
    let src = "song \"s\" {\n  track A {\n    instrument prisma()\n    insert filter(type: \"lp\", cutoff: 0.5)\n    automate cutoff from 0.2 to 0.9 over bars(1..8)\n    automate filter.cutoff from 0.1 to 0.7 over intro\n    play notes`C3:1` at bars(1..1)\n  }\n  section intro = bars(1..4)\n}\n";
    let sites = fortelang::edit::automation_sites(src).unwrap();
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].target, "cutoff");
    assert_eq!((sites[0].from, sites[0].to, sites[0].at.as_str()), (0.2, 0.9, "1..8"));
    assert_eq!(sites[1].target, "filter.cutoff");
    assert_eq!(sites[1].at, "intro");

    let out = apply(src, r#"{"op":"set_automation","track":"A","index":1,"from":0.3,"to":0.6}"#);
    assert!(out.contains("automate filter.cutoff from 0.3 to 0.6 over intro"), "{out}");

    let out2 = apply(&out, r#"{"op":"add_automation","track":"A","target":"volume","from":0,"to":1,"bars":[1,8]}"#);
    assert!(out2.contains("automate volume from 0 to 1 over bars(1..8)"), "{out2}");
    assert_eq!(fortelang::edit::automation_sites(&out2).unwrap().len(), 3);

    let out3 = apply(&out2, r#"{"op":"remove_automation","track":"A","index":0}"#);
    assert!(!out3.contains("from 0.2"), "{out3}");
    assert_eq!(fortelang::edit::automation_sites(&out3).unwrap().len(), 2);
}

#[test]
fn insert_chain_edits_add_remove_and_reorder() {
    let src = "song \"s\" {\n  track A {\n    instrument sampler(sample: \"Kick\")\n    insert filter(type: \"lp\", cutoff: 0.8)\n    insert drive(amount: 0.3)\n    play beat`x...` at bars(1..1)\n  }\n}\n";
    // add appends after the last insert
    let out = apply(src, r#"{"op":"add_insert","track":"A","call":"delay(time: 0.25, mix: 0.2)"}"#);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[5], "    insert delay(time: 0.25, mix: 0.2)");
    // reorder swaps only the call texts
    let out2 = apply(&out, r#"{"op":"move_insert","track":"A","from":2,"to":1}"#);
    let l2: Vec<&str> = out2.lines().collect();
    assert_eq!(l2[4], "    insert delay(time: 0.25, mix: 0.2)");
    assert_eq!(l2[5], "    insert drive(amount: 0.3)");
    // remove deletes the whole line
    let out3 = apply(&out2, r#"{"op":"remove_insert","track":"A","index":0}"#);
    assert!(!out3.contains("filter("), "{out3}");
    assert_eq!(out3.lines().count(), out2.lines().count() - 1);
    // a track with no inserts anchors the add after the instrument
    let bare = apply(src, r#"[{"op":"remove_insert","track":"A","index":1},{"op":"remove_insert","track":"A","index":0},{"op":"add_insert","track":"A","call":"space(mix: 0.2)"}]"#);
    let lb: Vec<&str> = bare.lines().collect();
    assert_eq!(lb[3], "    insert space(mix: 0.2)");
}

#[test]
fn set_instrument_swaps_the_whole_call() {
    let out = apply(
        SRC,
        r#"{"op":"set_instrument","path":["Groove"],"track":"Bass","call":"AcidBass(cutoff: 0.3)"}"#,
    );
    assert_only_lines_changed(SRC, &out, &[(22, "    instrument AcidBass(cutoff: 0.3)   // ベースの音色")]);
    // a bare call gets replaced too (Inner/T has `instrument mono()`… bare is mono() with parens; use it anyway)
    let bad = parse_ops(r#"{"op":"set_instrument","track":"T","call":"x() } track Y {"}"#).unwrap();
    assert_eq!(apply_ops(SRC, &bad).unwrap_err().code, "E-EDIT-005");
}

#[test]
fn remove_track_deletes_its_whole_span() {
    let src = "song \"s\" {\n  tempo 100bpm\n\n  track A {\n    instrument sampler(sample: \"Kick\")\n    play beat`x...` at bars(1..1)\n  }\n\n  track B {\n    instrument sampler(sample: \"Hat\")\n    play beat`..x.` at bars(1..1)\n  }\n}\n";
    let out = apply(src, r#"{"op":"remove_track","track":"A"}"#);
    assert!(!out.contains("track A"), "{out}");
    assert!(out.contains("track B"), "{out}");
    assert!(fortelang::parser::parse(&out).is_ok());
    let missing = parse_ops(r#"{"op":"remove_track","track":"Zed"}"#).unwrap();
    assert_eq!(apply_ops(src, &missing).unwrap_err().code, "E-EDIT-003");
}

#[test]
fn add_import_inserts_below_the_last_import() {
    let src = "// header comment\nimport { A } from \"./a.forte\"\n\nsong \"s\" {\n  tempo 100bpm\n}\n";
    let out = apply(src, r#"{"op":"add_import","names":["Groove"],"from":"../blocks/groove.forte"}"#);
    assert_eq!(
        out,
        "// header comment\nimport { A } from \"./a.forte\"\nimport { Groove } from \"../blocks/groove.forte\"\n\nsong \"s\" {\n  tempo 100bpm\n}\n"
    );
}

#[test]
fn add_import_keeps_leading_comments_on_top_when_first() {
    let src = "// header comment\n\nsong \"s\" {\n  tempo 100bpm\n}\n";
    let out = apply(src, r#"{"op":"add_import","names":["Groove"],"from":"../blocks/groove.forte"}"#);
    assert_eq!(
        out,
        "// header comment\n\nimport { Groove } from \"../blocks/groove.forte\"\nsong \"s\" {\n  tempo 100bpm\n}\n"
    );
}

#[test]
fn add_import_merges_missing_names_into_the_same_path() {
    let src = "import { A } from \"./lib.forte\"\n\nsong \"s\" {\n  tempo 100bpm\n}\n";
    let out = apply(src, r#"{"op":"add_import","names":["A","B"],"from":"./lib.forte"}"#);
    assert_eq!(out, "import { A, B } from \"./lib.forte\"\n\nsong \"s\" {\n  tempo 100bpm\n}\n");
    // fully present: byte-identical no-op
    let again = apply(&out, r#"{"op":"add_import","names":["A","B"],"from":"./lib.forte"}"#);
    assert_eq!(again, out);
}

#[test]
fn add_import_then_add_place_is_the_library_gesture() {
    let src = "song \"s\" {\n  tempo 100bpm\n}\n";
    let out = apply(
        src,
        r#"[{"op":"add_import","names":["Groove"],"from":"../blocks/groove.forte"},
           {"op":"add_place","block":"Groove","bars":[1,4]}]"#,
    );
    assert!(out.starts_with("import { Groove } from \"../blocks/groove.forte\"\n"));
    assert!(out.contains("  play Groove at bars(1..4)"));
}

#[test]
fn arg_sites_lists_instrument_and_inserts_with_set_arg_coordinates() {
    let sites = fortelang::edit::arg_sites(SRC).unwrap();
    let drums: Vec<_> = sites.iter().filter(|s| s.track == "Drums").collect();
    assert_eq!(drums.len(), 2);
    assert_eq!(drums[0].target, "instrument");
    assert_eq!(drums[0].name, "sampler");
    assert_eq!(drums[0].args[0].arg, "slices");
    assert_eq!(drums[0].args[0].num, Some(16.0));
    assert_eq!(drums[0].args[1].str.as_deref(), Some("on"));
    assert_eq!(drums[1].target, "insert:0");
    assert_eq!(drums[1].name, "filter");
    assert_eq!(drums[1].path, vec!["Groove"]);
    // the coordinates round-trip into set_arg
    let op = format!(
        r#"{{"op":"set_arg","path":["Groove"],"track":"{}","target":"{}","arg":"cutoff","value":0.3}}"#,
        drums[1].track, drums[1].target
    );
    let out = apply(SRC, &op);
    assert!(out.contains("cutoff: 0.3"));
}

#[test]
fn notes_round_trip_is_lossless_for_real_material() {
    // a real melody line (arcade-dawn), a chord, ties, accents, rests
    for raw in [
        "C5:0.5 Eb5:0.5 G5:0.5 C5:0.5 Ab4:0.5 C5:0.5 Eb5:1 Eb5:0.5 G5:0.5 Bb5:0.5 G5:0.5 D5:0.5 F5:0.5 G5:1",
        "[D4 F4 A4]:2 _:1 D4:1",
        "A1!:0.25 A1:0.25 C2~:0.25 D2!:0.25 _:0.5",
        "_:1 E4:0.5 Gb4:0.5 Ab4:1.5 _:0.5",
    ] {
        let doc = fortelang::edit::note_events(raw).unwrap();
        let out = fortelang::edit::serialize_notes(&doc).unwrap();
        let doc2 = fortelang::edit::note_events(&out).unwrap();
        assert_eq!(doc.len, doc2.len, "{raw} → {out}");
        assert_eq!(doc.notes.len(), doc2.notes.len(), "{raw} → {out}");
        for (a, b) in doc.notes.iter().zip(doc2.notes.iter()) {
            assert_eq!(a.pitch, b.pitch, "{raw} → {out}");
            assert!((a.start - b.start).abs() < 1e-6 && (a.dur - b.dur).abs() < 1e-6);
            assert_eq!((a.tie, a.accent), (b.tie, b.accent));
        }
    }
}

#[test]
fn notes_serializer_keeps_trailing_rests_and_makes_chords() {
    let raw = "C4:1 _:1 [E4 G4]:0.5 _:1.5";
    let doc = fortelang::edit::note_events(raw).unwrap();
    assert_eq!(doc.len, 4.0);
    assert_eq!(doc.notes.len(), 3);
    let out = fortelang::edit::serialize_notes(&doc).unwrap();
    assert_eq!(out, "C4:1 _:1 [E4 G4]:0.5 _:1.5");
}

#[test]
fn notes_serializer_refuses_a_partial_overlap() {
    let mut doc = fortelang::edit::note_events("C4:1 D4:1").unwrap();
    doc.notes[1].start = 0.5; // overlaps C4 but is not a chord
    assert_eq!(fortelang::edit::serialize_notes(&doc).unwrap_err().code, "E-EDIT-007");
}

#[test]
fn set_track_is_idempotent() {
    let op = r#"{"op":"set_track","track":"Bass","field":"volume","value":0.5}"#;
    let once = apply(MIX, op);
    let twice = apply(&once, op);
    assert_eq!(once, twice);
}

#[test]
fn move_at_line_finds_a_placement_by_source_line() {
    // fixture line 38 = `  play Inner as Twin at bars(5..8)   // エイリアス配置`
    let out = apply(SRC, r#"{"op":"move_at_line","line":38,"bars":[9,12]}"#);
    assert_only_lines_changed(
        SRC,
        &out,
        &[(38, "  play Inner as Twin at bars(9..12)   // エイリアス配置")],
    );
}

#[test]
fn move_at_line_finds_a_track_play_inside_a_block() {
    // fixture line 17 = `    play K at bars(1..8)` (block Groove / track Drums)
    let out = apply(SRC, r#"{"op":"move_at_line","line":17,"bars":[5,12]}"#);
    assert_only_lines_changed(SRC, &out, &[(17, "    play K at bars(5..12)")]);
}

#[test]
fn move_at_line_rejects_a_line_without_a_play() {
    let ops = parse_ops(r#"{"op":"move_at_line","line":1,"bars":[1,4]}"#).unwrap();
    assert_eq!(apply_ops(SRC, &ops).unwrap_err().code, "E-EDIT-003");
}
