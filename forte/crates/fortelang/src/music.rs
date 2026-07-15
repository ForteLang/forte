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
/// grouping. `x*3` is a ratchet — the step subdivides into 3 rapid retrigs
/// with decaying velocity. A literal of the form `euclid(k, n[, rot: r])`
/// expands to a Euclidean pattern (k hits spread evenly over n steps).
/// The steps span exactly `beats_per_bar` beats — one bar, or the `span:`
/// of a `cycle()` wrapper. Hits become notes at `pitch` lasting 60% of a
/// (sub)step.
pub fn parse_beat(
    raw: &str,
    beats_per_bar: f64,
    pitch: u8,
    pos: Pos,
) -> Result<(Vec<Note>, f64), Diag> {
    let expanded;
    let text = raw.trim();
    let src: &str = if let Some(rest) = text.strip_prefix("euclid(") {
        let inner = rest
            .strip_suffix(')')
            .ok_or_else(|| Diag::new("E-BEAT-003", pos, "euclid(…) の閉じ括弧がありません"))?;
        expanded = euclid_steps(inner, pos)?;
        &expanded
    } else {
        raw
    };

    // tokenize: one step per hit/rest char, an optional `*N` ratchet suffix
    let mut steps: Vec<(char, u32)> = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c != 'x' && c != 'X' && c != '-' && c != '.' {
            return Err(Diag::new(
                "E-BEAT-002",
                pos,
                format!("beat リテラルで使えるのは x(通常)/ X(アクセント)/ .(ゴースト)/ -(休符)です(見つかったのは '{c}')"),
            ));
        }
        let mut ratchet = 1u32;
        if chars.peek() == Some(&'*') {
            chars.next();
            let mut digits = String::new();
            while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                digits.push(chars.next().unwrap());
            }
            ratchet = digits.parse().unwrap_or(0);
            if !(2..=16).contains(&ratchet) {
                return Err(Diag::new(
                    "E-BEAT-004",
                    pos,
                    format!("ラチェット *{digits} は 2..16 の範囲で書いてください"),
                ));
            }
            if c == '-' {
                return Err(Diag::new("E-BEAT-004", pos, "休符 - にラチェットは付けられません"));
            }
        }
        steps.push((c, ratchet));
    }
    if steps.is_empty() {
        return Err(Diag::new("E-BEAT-001", pos, "beat リテラルが空です"));
    }

    let step_len = beats_per_bar / steps.len() as f64;
    let mut notes = Vec::new();
    for (i, &(c, ratchet)) in steps.iter().enumerate() {
        if c == '-' {
            continue;
        }
        let base: f64 = if c == 'X' { 120.0 } else if c == '.' { 55.0 } else { 100.0 };
        let sub = step_len / ratchet as f64;
        for r in 0..ratchet {
            // retrigs decay: 100, 78, 61, … — the classic stutter shape
            let vel = (base * 0.78f64.powi(r as i32)).round().max(25.0) as u8;
            notes.push(Note {
                pitch,
                start: i as f64 * step_len + r as f64 * sub,
                length: sub * 0.6,
                velocity: vel,
            });
        }
    }
    Ok((notes, beats_per_bar))
}

/// `euclid(k, n[, rot: r])` — Bjorklund's algorithm: k hits spread as
/// evenly as possible over n steps ("x--x--x-" for 3,8). `rot` rotates the
/// pattern. Positional or named (`hits:` / `steps:` / `rot:`) arguments.
fn euclid_steps(inner: &str, pos: Pos) -> Result<String, Diag> {
    let mut k: Option<i64> = None;
    let mut n: Option<i64> = None;
    let mut rot: i64 = 0;
    for (i, part) in inner.split(',').enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, val) = match part.split_once(':') {
            Some((key, v)) => (key.trim(), v.trim()),
            None => ("", part),
        };
        let num: i64 = val
            .parse()
            .map_err(|_| Diag::new("E-BEAT-003", pos, format!("euclid の引数が読めません: {part}")))?;
        match (key, i) {
            ("hits", _) | ("", 0) => k = Some(num),
            ("steps", _) | ("", 1) => n = Some(num),
            ("rot", _) | ("", 2) => rot = num,
            _ => {
                return Err(Diag::new(
                    "E-BEAT-003",
                    pos,
                    format!("euclid の引数は euclid(打数, ステップ数[, rot: 回転]) です(見つかったのは {part})"),
                ))
            }
        }
    }
    let (Some(k), Some(n)) = (k, n) else {
        return Err(Diag::new(
            "E-BEAT-003",
            pos,
            "euclid(打数, ステップ数[, rot: 回転]) の形で書いてください".to_string(),
        ));
    };
    if !(1..=128).contains(&n) || !(1..=n).contains(&k) {
        return Err(Diag::new(
            "E-BEAT-003",
            pos,
            format!("euclid({k}, {n}) は範囲外です(1 ≤ 打数 ≤ ステップ数 ≤ 128)"),
        ));
    }
    let mut s = String::with_capacity(n as usize);
    for i in 0..n {
        let j = (i + rot).rem_euclid(n);
        // evenly distributed hits with the first on step 0 (tresillo shape:
        // euclid(3,8) = x--x--x-)
        let hit = (j * k).rem_euclid(n) < k;
        s.push(if hit { 'x' } else { '-' });
    }
    Ok(s)
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
        // `C2~:0.5` — the tie: hold into the next note so a mono/glide
        // instrument slides instead of retriggering (the 303 notation)
        let (head, tied) = match head.strip_suffix('~') {
            Some(h) => (h, true),
            None => (head, false),
        };
        // accent: `C2!:0.5` lifts velocity (mirrors beat's X)
        let (head, accent) = match head.strip_suffix('!') {
            Some(h) => (h, true),
            None => (head, false),
        };
        let pitches: Vec<&str> = if head.starts_with('[') && head.ends_with(']') {
            head[1..head.len() - 1].split_whitespace().collect()
        } else {
            vec![head]
        };
        let length = if tied { dur * 1.02 } else { dur * 0.95 };
        let velocity = if accent { 120 } else { 100 };
        for ps in pitches {
            let pitch = parse_pitch(ps, pos)?;
            notes.push(Note { pitch, start: cursor, length, velocity });
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
    /// slash-chord bass (`C/G`): the pitch class under the chord
    pub bass_pc: Option<u8>,
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
    // the jazz/neo-soul shelf (#130)
    ("m7b5", &[0, 3, 6, 10]),
    ("dim7", &[0, 3, 6, 9]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("9", &[0, 4, 7, 10, 14]),
    ("maj9", &[0, 4, 7, 11, 14]),
    ("m9", &[0, 3, 7, 10, 14]),
    ("add9", &[0, 4, 7, 14]),
    ("madd9", &[0, 3, 7, 14]),
    ("7sus4", &[0, 5, 7, 10]),
];

/// Natural major / natural minor degree offsets for roman-numeral chords.
const MAJOR_DEG: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR_DEG: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];

/// `prog` literal: bars separated by `|`; chords within a bar share its time
/// equally. `prog`Em | C G | D``. Symbols are absolute (`Am7`, `F/A`) or
/// roman-numeral degrees resolved against `key` — `prog`ii7 | V7 | Imaj7``
/// in C major is `Dm7 | G7 | Cmaj7`; case picks the triad (`IV` major,
/// `iv` minor) unless an explicit quality overrides it.
pub fn parse_prog(
    raw: &str,
    beats_per_bar: f64,
    key: (u8, bool),
    pos: Pos,
) -> Result<(Vec<ChordEvent>, f64), Diag> {
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
            // slash bass first: everything after '/' names the bass note
            let (body, bass_pc) = match sym.split_once('/') {
                Some((b, bass)) => {
                    let (pc, _) = parse_chord_root(bass, sym, pos)?;
                    (b, Some(pc))
                }
                None => (sym, None),
            };
            let (root_pc, intervals) = match parse_degree(body, key) {
                Some(v) => v?,
                None => parse_chord(body, pos)?,
            };
            events.push(ChordEvent { root_pc, intervals, bass_pc, start: cursor, dur });
            cursor += dur;
        }
    }
    Ok((events, cursor))
}

/// Roman-numeral degree: optional accidental, numeral (case = triad),
/// then any QUALITIES suffix. Returns None when `sym` isn't a numeral.
fn parse_degree(sym: &str, key: (u8, bool)) -> Option<Result<(u8, Vec<i32>), Diag>> {
    let (acc, rest) = match sym.chars().next()? {
        'b' if sym.len() > 1 && matches!(sym.chars().nth(1), Some('i' | 'v' | 'I' | 'V')) => {
            (-1i32, &sym[1..])
        }
        '#' => (1i32, &sym[1..]),
        _ => (0i32, sym),
    };
    let numeral_len = rest.chars().take_while(|c| matches!(c, 'i' | 'v' | 'I' | 'V')).count();
    if numeral_len == 0 || numeral_len > 3 {
        return None;
    }
    let (numeral, quality) = rest.split_at(numeral_len);
    let lower = numeral.to_ascii_lowercase();
    let degree = match lower.as_str() {
        "i" => 0usize,
        "ii" => 1,
        "iii" => 2,
        "iv" => 3,
        "v" => 4,
        "vi" => 5,
        "vii" => 6,
        _ => return None,
    };
    let minor_case = numeral.chars().all(|c| c.is_ascii_lowercase());
    if numeral.chars().any(|c| c.is_ascii_lowercase()) != minor_case {
        return None; // mixed case is not a numeral
    }
    let scale = if key.1 { &MINOR_DEG } else { &MAJOR_DEG };
    let root_pc = (((key.0 as i32 + scale[degree] + acc) % 12) + 12) % 12;
    // quality: explicit suffix wins; bare/7 follow the numeral's case
    let intervals: Vec<i32> = match quality {
        "" => {
            if minor_case { vec![0, 3, 7] } else { vec![0, 4, 7] }
        }
        "7" => {
            if minor_case { vec![0, 3, 7, 10] } else { vec![0, 4, 7, 10] }
        }
        q => match QUALITIES.iter().find(|(name, _)| *name == q) {
            Some((_, iv)) => iv.to_vec(),
            None => return None, // fall through to the absolute parser's error
        },
    };
    Some(Ok((root_pc as u8, intervals)))
}

/// The root letter (+accidental) of an absolute chord symbol.
fn parse_chord_root(s: &str, whole: &str, pos: Pos) -> Result<(u8, usize), Diag> {
    let chars: Vec<char> = s.chars().collect();
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
                format!("コード '{whole}' のルート音が読めません(C〜B で始めてください)"),
            ))
        }
    };
    let mut i = 1;
    let mut acc = 0i32;
    if i < chars.len() && (chars[i] == '#' || chars[i] == 'b') {
        acc = if chars[i] == '#' { 1 } else { -1 };
        i += 1;
    }
    Ok(((((base + acc) % 12 + 12) % 12) as u8, i))
}

fn parse_chord(sym: &str, pos: Pos) -> Result<(u8, Vec<i32>), Diag> {
    let (pc, used) = parse_chord_root(sym, sym, pos)?;
    let quality: String = sym.chars().skip(used).collect();
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
    Ok((pc, intervals))
}

/// Transposition-invariant signature of a progression: each chord as
/// (root interval from the first chord, quality intervals). `Em | C | G | D`
/// and `Am | F | C | G` share one signature — that is how "songs built on the
/// same progression" are found regardless of key (SRS-PLY-002).
pub fn prog_signature(events: &[ChordEvent]) -> String {
    let Some(first) = events.first() else { return String::new() };
    events
        .iter()
        .map(|ev| {
            let rel = ((ev.root_pc as i32 - first.root_pc as i32) % 12 + 12) % 12;
            let ivs: Vec<String> = ev.intervals.iter().map(|i| i.to_string()).collect();
            format!("{rel}:{}", ivs.join("."))
        })
        .collect::<Vec<_>>()
        .join(" ")
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
/// per chord. A slash chord's bass note wins over the root.
pub fn prog_bass(events: &[ChordEvent], rate: Option<f64>) -> Vec<Note> {
    let mut notes = Vec::new();
    for ev in events {
        let step = rate.unwrap_or(ev.dur).min(ev.dur);
        let mut t = 0.0;
        while t < ev.dur - 1e-9 {
            let len = step.min(ev.dur - t);
            notes.push(Note {
                pitch: 36 + ev.bass_pc.unwrap_or(ev.root_pc),
                start: ev.start + t,
                length: len * 0.9,
                velocity: 100,
            });
            t += step;
        }
    }
    notes
}

/// Voiced block chords (#130): `voicing` shapes each chord, `lead` walks
/// voices by NEAREST MOTION from chord to chord instead of jumping in
/// parallel root position, `register` centers the stack (octave number,
/// default 4 ≈ middle C). A slash bass is added below the voicing.
/// The un-voiced `prog_chords` stays untouched — old songs keep their bits.
pub fn prog_chords_voiced(
    events: &[ChordEvent],
    voicing: &str,
    register: i32,
    lead: bool,
    pos: Pos,
) -> Result<Vec<Note>, Diag> {
    let center = (12 * (register + 1)).clamp(24, 96); // octave N → MIDI center
    let mut notes = Vec::new();
    let mut prev: Vec<i32> = Vec::new();
    for ev in events {
        // close position around the register center
        let mut tones: Vec<i32> = ev
            .intervals
            .iter()
            .map(|iv| {
                let pc = (ev.root_pc as i32 + iv) % 12;
                // place each tone in the octave closest to center
                let mut p = center - 6 + ((pc - (center - 6)).rem_euclid(12));
                if p - center > 6 {
                    p -= 12;
                }
                p
            })
            .collect();
        tones.sort_unstable();
        tones.dedup();
        // voice leading: move each PREVIOUS voice to the nearest tone of the
        // NEW chord (octave-free), then make sure every chord tone sounds
        if lead && !prev.is_empty() {
            let pcs: Vec<i32> = tones.iter().map(|t| t.rem_euclid(12)).collect();
            let mut led: Vec<i32> = prev
                .iter()
                .map(|&v| {
                    // nearest pitch (any octave) whose class is in the chord
                    let mut best = v;
                    let mut best_d = i32::MAX;
                    for &pc in &pcs {
                        for oct in -1..=1 {
                            let cand = v + ((pc - v).rem_euclid(12)) + 12 * oct;
                            let d = (cand - v).abs();
                            if d < best_d {
                                best_d = d;
                                best = cand;
                            }
                        }
                    }
                    best
                })
                .collect();
            // any chord tone still missing joins nearest to the stack's mean
            let mean: i32 = led.iter().sum::<i32>() / led.len().max(1) as i32;
            for &pc in &pcs {
                if !led.iter().any(|v| v.rem_euclid(12) == pc) {
                    let mut cand = mean - 6 + ((pc - (mean - 6)).rem_euclid(12));
                    if cand - mean > 6 {
                        cand -= 12;
                    }
                    led.push(cand);
                }
            }
            // three guards, learned from the first real song (without them
            // an 8-chord walk GREW a voice per chord and ratcheted an
            // octave downward until it left the piano):
            // 1. tether every voice to the register center
            for v in led.iter_mut() {
                while *v < center - 18 {
                    *v += 12;
                }
                while *v > center + 18 {
                    *v -= 12;
                }
            }
            led.sort_unstable();
            led.dedup();
            // 2. one voice per pitch class — keep the copy nearest center
            let mut pruned: Vec<i32> = Vec::with_capacity(led.len());
            for &v in &led {
                match pruned.iter().position(|q| q.rem_euclid(12) == v.rem_euclid(12)) {
                    Some(i) => {
                        if (v - center).abs() < (pruned[i] - center).abs() {
                            pruned[i] = v;
                        }
                    }
                    None => pruned.push(v),
                }
            }
            // 3. cap at the chord's own size, dropping the farthest voice
            while pruned.len() > pcs.len().max(1) {
                let far = pruned
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| (*v - center).abs())
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                pruned.remove(far);
            }
            pruned.sort_unstable();
            tones = pruned;
        }
        // the NEXT chord leads from the CLOSE-position stack; voicing shapes
        // are applied on a copy each chord so drop2/open never compound
        prev = tones.clone();
        // voicing shapes applied AFTER leading (they re-space, not re-choose)
        match voicing {
            "close" => {}
            "open" => {
                // spread: drop every second voice from the top down an octave
                let n = tones.len();
                for (i, t) in tones.iter_mut().enumerate() {
                    if (n - 1 - i) % 2 == 1 {
                        *t -= 12;
                    }
                }
                tones.sort_unstable();
            }
            "drop2" => {
                // second voice from the top drops an octave
                let n = tones.len();
                if n >= 2 {
                    tones[n - 2] -= 12;
                    tones.sort_unstable();
                }
            }
            other => {
                return Err(Diag::new(
                    "E-PAT-002",
                    pos,
                    format!("voicing '{other}' はありません(close / open / drop2)"),
                ))
            }
        }
        // slash bass under the stack
        if let Some(b) = ev.bass_pc {
            let low = tones.first().copied().unwrap_or(center);
            let mut bp = low - 1 - ((low - 1 - b as i32).rem_euclid(12));
            if bp < 24 {
                bp += 12;
            }
            notes.push(Note {
                pitch: bp.clamp(0, 127) as u8,
                start: ev.start,
                length: ev.dur * 0.95,
                velocity: 95,
            });
        }
        for &t in &tones {
            notes.push(Note {
                pitch: t.clamp(0, 127) as u8,
                start: ev.start,
                length: ev.dur * 0.95,
                velocity: 90,
            });
        }
    }
    Ok(notes)
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

pub(crate) fn parse_duration(s: &str, pos: Pos) -> Result<f64, Diag> {
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
