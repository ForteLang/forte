//! Read-only visualization data derived from a compiled project (the code is
//! the only editable truth — views are projections of it, SYS-EDT-003).
//! Consumed by the browser editor's arrangement canvas, the VSCode webview
//! and `forte viz`.

use dawcore::model::Project;

pub fn viz_json(p: &Project) -> serde_json::Value {
    let beats_per_bar = p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64;
    let tracks: Vec<serde_json::Value> = p
        .tracks
        .iter()
        .map(|t| {
            let clips: Vec<serde_json::Value> = t
                .arranger
                .iter()
                .map(|a| {
                    let notes: Vec<[f64; 4]> = a
                        .clip
                        .notes
                        .iter()
                        .map(|n| [n.pitch as f64, n.start, n.length, n.velocity as f64 / 127.0])
                        .collect();
                    serde_json::json!({
                        "start": a.start, "duration": a.duration,
                        "length": a.clip.length, "notes": notes,
                        // 1-based source line of the play (for placed block
                        // instances: the placement line) — the code-jump and
                        // drag/resize target (move_at_line)
                        "line": a.src_line,
                    })
                })
                .collect();
            serde_json::json!({
                "name": t.name,
                "color": t.color,
                "line": t.src_line,
                "fx": t.kind == dawcore::model::TrackKind::Effect,
                // what plays and what shapes it — the composer view reads these
                "instrument": t.devices.first().map(|d| d.kind.label()).unwrap_or(""),
                "inserts": t.devices.iter().skip(1).map(|d| d.kind.label()).collect::<Vec<_>>(),
                // the mixer strip's read side (writes go through set_track)
                "volume": t.volume,
                "pan": t.pan,
                "clips": clips,
            })
        })
        .collect();
    serde_json::json!({
        "tempo": p.tempo,
        "beatsPerBar": beats_per_bar,
        "lengthBeats": dawcore::bounce::arrangement_len(p),
        "tracks": tracks,
    })
}
