//! Performance capture → code (roadmap 1.4, SRS-REC-001 MIDI side): played
//! notes become a `notes` literal, because in Forte a performance is not an
//! opaque recording of events — it is source code you can read and edit.

use dawcore::model::NOTE_NAMES;

/// One captured note: start/length in beats, MIDI pitch.
#[derive(Clone, Copy, Debug)]
pub struct PlayedNote {
    pub start: f64,
    pub len: f64,
    pub pitch: u8,
}

fn pitch_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[(p % 12) as usize], p as i32 / 12 - 1)
}

fn fmt_beats(b: f64) -> String {
    // quantized values are multiples of the grid; keep them short and exact
    if (b - b.round()).abs() < 1e-9 {
        format!("{}", b.round() as i64)
    } else {
        let s = format!("{b}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Quantize a performance to `grid` beats (e.g. 0.25 = 1/16 in 4/4) and
/// render it as the body of a `notes` literal: simultaneous notes group into
/// chords, silences become explicit rests. Returns None for an empty take.
pub fn transcribe(notes: &[PlayedNote], grid: f64) -> Option<String> {
    if notes.is_empty() || grid <= 0.0 {
        return None;
    }
    let t0 = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);

    // quantize, then group by identical start cell
    let mut q: Vec<(i64, i64, u8)> = notes
        .iter()
        .map(|n| {
            let start = ((n.start - t0) / grid).round() as i64;
            let len = ((n.len / grid).round() as i64).max(1);
            (start, len, n.pitch)
        })
        .collect();
    q.sort_by_key(|&(s, _, p)| (s, p));

    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0i64;
    let mut i = 0;
    while i < q.len() {
        let (start, mut len, _) = q[i];
        // a gap before this event is an explicit rest
        if start > cursor {
            out.push(format!("_:{}", fmt_beats((start - cursor) as f64 * grid)));
        }
        // gather every note starting in the same cell → chord
        let mut chord: Vec<u8> = Vec::new();
        while i < q.len() && q[i].0 == start {
            chord.push(q[i].2);
            len = len.max(q[i].1);
            i += 1;
        }
        chord.dedup();
        let dur = fmt_beats(len as f64 * grid);
        if chord.len() == 1 {
            out.push(format!("{}:{dur}", pitch_name(chord[0])));
        } else {
            let names: Vec<String> = chord.iter().map(|&p| pitch_name(p)).collect();
            out.push(format!("[{}]:{dur}", names.join(" ")));
        }
        cursor = start + len;
    }
    Some(out.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melody_with_gap_becomes_notes_and_rests() {
        let played = [
            PlayedNote { start: 10.02, len: 0.24, pitch: 60 }, // ~C4, sloppy timing
            PlayedNote { start: 10.27, len: 0.26, pitch: 64 },
            PlayedNote { start: 11.01, len: 0.52, pitch: 67 }, // gap of ~0.5 before
        ];
        let s = transcribe(&played, 0.25).unwrap();
        assert_eq!(s, "C4:0.25 E4:0.25 _:0.5 G4:0.5");
    }

    #[test]
    fn simultaneous_notes_group_into_a_chord() {
        let played = [
            PlayedNote { start: 0.01, len: 1.0, pitch: 60 },
            PlayedNote { start: 0.04, len: 0.9, pitch: 64 },
            PlayedNote { start: 0.02, len: 1.1, pitch: 67 },
        ];
        let s = transcribe(&played, 0.25).unwrap();
        assert_eq!(s, "[C4 E4 G4]:1");
    }

    #[test]
    fn transcription_is_valid_notes_literal() {
        let played = [
            PlayedNote { start: 0.0, len: 0.5, pitch: 62 },
            PlayedNote { start: 0.5, len: 0.5, pitch: 66 },
            PlayedNote { start: 1.0, len: 1.0, pitch: 69 },
        ];
        let body = transcribe(&played, 0.25).unwrap();
        let src = format!(
            "song \"t\" {{ tempo 120bpm track A {{ instrument prisma() play notes`{body}` at bars(1..2) }} }}"
        );
        crate::compile_str(&src).expect("transcription must compile");
    }

    #[test]
    fn empty_take_is_none() {
        assert!(transcribe(&[], 0.25).is_none());
    }
}
