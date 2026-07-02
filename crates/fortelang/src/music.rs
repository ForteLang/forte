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

/// One chord in a progression: pitch class of the root plus intervals.
#[derive(Clone, Debug)]
pub struct ChordEvent {
    pub root_pc: u8,
    pub intervals: Vec<i32>,
    pub start: f64,
    pub dur: f64,
}

const QUALITIES: &[(&str, &[i32])] = &[
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("min", &[0, 3, 7]),
    ("7", &[0, 4, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("m7", &[0, 3, 7, 10]),
    ("min7", &[0, 3, 7, 10]),
    ("dim", &[0, 3, 6]),
    ("aug", &[0, 4, 8]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
];

/// `prog` literal: bars separated by `|`; chords within a bar share its time
/// equally. `prog`Em | C G | D``.
pub fn parse_prog(raw: &str, beats_per_bar: f64, pos: Pos) -> Result<(Vec<ChordEvent>, f64), Diag> {
    let mut events = Vec::new();
    let mut cursor = 0.0f64;
    let segments: Vec<&str> = raw.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Err(Diag::new("E-PROG-001", pos, "prog リテラルが空です"));
    }
    for seg in segments {
        let chords: Vec<&str> = seg.split_whitespace().collect();
        let dur = beats_per_bar / chords.len() as f64;
        for sym in chords {
            let (root_pc, intervals) = parse_chord(sym, pos)?;
            events.push(ChordEvent { root_pc, intervals, start: cursor, dur });
            cursor += dur;
        }
    }
    Ok((events, cursor))
}

fn parse_chord(sym: &str, pos: Pos) -> Result<(u8, Vec<i32>), Diag> {
    let chars: Vec<char> = sym.chars().collect();
    let base = match chars.first().map(|c| c.to_ascii_uppercase()) {
        Some('C') => 0i32,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => {
            return Err(Diag::new(
                "E-PROG-002",
                pos,
                format!("コード '{sym}' のルート音が読めません(C〜B で始めてください)"),
            ))
        }
    };
    let mut i = 1;
    let mut acc = 0i32;
    if i < chars.len() && (chars[i] == '#' || chars[i] == 'b') {
        acc = if chars[i] == '#' { 1 } else { -1 };
        i += 1;
    }
    let quality: String = chars[i..].iter().collect();
    let intervals = QUALITIES
        .iter()
        .find(|(q, _)| *q == quality)
        .map(|(_, iv)| iv.to_vec())
        .ok_or_else(|| {
            let names: Vec<&str> =
                QUALITIES.iter().map(|(q, _)| if q.is_empty() { "(メジャー)" } else { *q }).collect();
            Diag::new(
                "E-PROG-002",
                pos,
                format!("コード '{sym}' のクオリティ '{quality}' が読めません(使えるもの: {})", names.join(", ")),
            )
        })?;
    Ok(((((base + acc) % 12 + 12) % 12) as u8, intervals))
}

/// Block chords: every chord tone held for the chord's duration (root oct 3).
pub fn prog_chords(events: &[ChordEvent]) -> Vec<Note> {
    let mut notes = Vec::new();
    for ev in events {
        for &iv in &ev.intervals {
            notes.push(Note {
                pitch: (48 + ev.root_pc as i32 + iv) as u8,
                start: ev.start,
                length: ev.dur * 0.95,
                velocity: 90,
            });
        }
    }
    notes
}

/// Root-note bass line (oct 2); `rate` subdivides each chord, default one note
/// per chord.
pub fn prog_bass(events: &[ChordEvent], rate: Option<f64>) -> Vec<Note> {
    let mut notes = Vec::new();
    for ev in events {
        let step = rate.unwrap_or(ev.dur).min(ev.dur);
        let mut t = 0.0;
        while t < ev.dur - 1e-9 {
            let len = step.min(ev.dur - t);
            notes.push(Note {
                pitch: 36 + ev.root_pc,
                start: ev.start + t,
                length: len * 0.9,
                velocity: 100,
            });
            t += step;
        }
    }
    notes
}

/// Arpeggio over the chord tones (root oct 4) at `rate` beats per step.
pub fn prog_arp(events: &[ChordEvent], rate: f64, style: &str, pos: Pos) -> Result<Vec<Note>, Diag> {
    let mut notes = Vec::new();
    for ev in events {
        let mut tones: Vec<i32> = ev.intervals.iter().map(|iv| 60 + ev.root_pc as i32 + iv).collect();
        tones.sort_unstable();
        let seq: Vec<i32> = match style {
            "up" => tones.clone(),
            "down" => tones.iter().rev().copied().collect(),
            "updown" => {
                let mut s = tones.clone();
                s.extend(tones.iter().rev().skip(1).take(tones.len().saturating_sub(2)).copied());
                s
            }
            other => {
                return Err(Diag::new(
                    "E-PAT-002",
                    pos,
                    format!("arp の style '{other}' はありません(up / down / updown)"),
                ))
            }
        };
        let mut t = 0.0;
        let mut idx = 0usize;
        while t < ev.dur - 1e-9 {
            notes.push(Note {
                pitch: seq[idx % seq.len()] as u8,
                start: ev.start + t,
                length: (rate * 0.9).min(ev.dur - t),
                velocity: 95,
            });
            idx += 1;
            t += rate;
        }
    }
    Ok(notes)
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
