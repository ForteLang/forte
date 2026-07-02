//! Music literal parsing: `beat` step patterns and `notes` sequences → engine
//! notes. Pure functions of the literal text — no I/O, no state.

use crate::diag::{Diag, Pos};
use dawcore::model::Note;

/// Parse a pitch name like `C4`, `F#3`, `Bb2`. Middle C (C4) = MIDI 60.
pub fn parse_pitch(s: &str, pos: Pos) -> Result<u8, Diag> {
    let bytes: Vec<char> = s.chars().collect();
    if bytes.is_empty() {
        return Err(Diag::new("E-NOTE-001", pos, "空のピッチ名です"));
    }
    let base = match bytes[0].to_ascii_uppercase() {
        'C' => 0i32,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        other => {
            return Err(Diag::new(
                "E-NOTE-002",
                pos,
                format!("ピッチ名は C〜B で始めてください(見つかったのは '{other}')"),
            ))
        }
    };
    let mut i = 1;
    let mut acc = 0i32;
    while i < bytes.len() && (bytes[i] == '#' || bytes[i] == 'b') {
        acc += if bytes[i] == '#' { 1 } else { -1 };
        i += 1;
    }
    let oct: i32 = s[i..]
        .parse()
        .map_err(|_| Diag::new("E-NOTE-003", pos, format!("オクターブが読めません: {s}")))?;
    let midi = (oct + 1) * 12 + base + acc;
    if !(0..=127).contains(&midi) {
        return Err(Diag::new("E-NOTE-004", pos, format!("{s} は MIDI 音域(0..127)の外です")));
    }
    Ok(midi as u8)
}

/// `beat` literal: `x` = hit, `X` = accent, `-` = rest; whitespace is visual
/// grouping. The steps span exactly one bar of `beats_per_bar` beats.
/// Hits become notes at `pitch` lasting 60% of a step.
pub fn parse_beat(
    raw: &str,
    beats_per_bar: f64,
    pitch: u8,
    pos: Pos,
) -> Result<(Vec<Note>, f64), Diag> {
    let steps: Vec<char> = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if steps.is_empty() {
        return Err(Diag::new("E-BEAT-001", pos, "beat リテラルが空です"));
    }
    for &c in &steps {
        if c != 'x' && c != 'X' && c != '-' {
            return Err(Diag::new(
                "E-BEAT-002",
                pos,
                format!("beat リテラルで使えるのは x / X / - です(見つかったのは '{c}')"),
            ));
        }
    }
    let step_len = beats_per_bar / steps.len() as f64;
    let mut notes = Vec::new();
    for (i, &c) in steps.iter().enumerate() {
        if c == 'x' || c == 'X' {
            notes.push(Note {
                pitch,
                start: i as f64 * step_len,
                length: step_len * 0.6,
                velocity: if c == 'X' { 120 } else { 100 },
            });
        }
    }
    Ok((notes, beats_per_bar))
}

/// `notes` literal: whitespace-separated events placed sequentially.
///   `D2:1`          — pitch : duration in beats
///   `[D4 F4 A4]:2`  — chord
///   `_:1`           — rest
/// Durations accept `1`, `0.5`, `1/2`.
pub fn parse_notes(raw: &str, pos: Pos) -> Result<(Vec<Note>, f64), Diag> {
    let mut notes = Vec::new();
    let mut cursor = 0.0f64;
    let mut toks = raw.split_whitespace().peekable();

    // rejoin chord groups split by whitespace: `[D4 F4 A4]:2`
    let mut events: Vec<String> = Vec::new();
    while let Some(t) = toks.next() {
        if t.starts_with('[') && !t.contains(']') {
            let mut acc = t.to_string();
            for u in toks.by_ref() {
                acc.push(' ');
                acc.push_str(u);
                if u.contains(']') {
                    break;
                }
            }
            events.push(acc);
        } else {
            events.push(t.to_string());
        }
    }

    for ev in events {
        let (head, durs) = ev.rsplit_once(':').ok_or_else(|| {
            Diag::new("E-NOTE-005", pos, format!("イベントは `ピッチ:長さ` の形です: {ev}"))
        })?;
        let dur = parse_duration(durs, pos)?;
        if head == "_" {
            cursor += dur;
            continue;
        }
        let pitches: Vec<&str> = if head.starts_with('[') && head.ends_with(']') {
            head[1..head.len() - 1].split_whitespace().collect()
        } else {
            vec![head]
        };
        for ps in pitches {
            let pitch = parse_pitch(ps, pos)?;
            notes.push(Note { pitch, start: cursor, length: dur * 0.95, velocity: 100 });
        }
        cursor += dur;
    }
    if cursor <= 0.0 {
        return Err(Diag::new("E-NOTE-006", pos, "notes リテラルが空です"));
    }
    Ok((notes, cursor))
}

fn parse_duration(s: &str, pos: Pos) -> Result<f64, Diag> {
    if let Some((a, b)) = s.split_once('/') {
        let a: f64 = a
            .parse()
            .map_err(|_| Diag::new("E-NOTE-007", pos, format!("長さが読めません: {s}")))?;
        let b: f64 = b
            .parse()
            .map_err(|_| Diag::new("E-NOTE-007", pos, format!("長さが読めません: {s}")))?;
        if b == 0.0 {
            return Err(Diag::new("E-NOTE-008", pos, "長さの分母が 0 です"));
        }
        Ok(a / b)
    } else {
        s.parse()
            .map_err(|_| Diag::new("E-NOTE-007", pos, format!("長さが読めません: {s}")))
    }
}
