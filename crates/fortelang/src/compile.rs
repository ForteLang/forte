//! Compile the AST into a `dawcore::model::Project`. The v0 slice targets the
//! existing engine directly; the dedicated render-graph IR arrives with
//! forte-core (SAD §2).

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{Diag, Pos};
use crate::grid_build;
use crate::music;
use dawcore::model::{
    ArrangerClip, Clip, Device, DeviceKind, KeySignature, Note, Project, SampleSource, Scale,
    Track, TrackKind, TRACK_COLORS,
};

pub fn compile(file: &FileAst) -> Result<Project, Vec<Diag>> {
    let song = &file.song;
    let mut diags: Vec<Diag> = Vec::new();
    let mut p = Project::empty();

    // ---- user-defined devices ----------------------------------------------
    let mut user_devices: HashMap<&str, &DeviceAst> = HashMap::new();
    for d in &file.devices {
        if INSTRUMENTS.contains(&d.name.as_str()) || EFFECTS.contains(&d.name.as_str()) {
            diags.push(Diag::new(
                "E-DEV-008",
                d.pos,
                format!("device '{}' はビルトイン名と衝突しています", d.name),
            ));
            continue;
        }
        if user_devices.insert(d.name.as_str(), d).is_some() {
            diags.push(Diag::new("E-MOD-002", d.pos, format!("device '{}' が重複しています", d.name)));
        }
    }

    // ---- song header ------------------------------------------------------
    match song.tempo {
        Some((t, pos)) => {
            if !(20.0..=400.0).contains(&t) {
                diags.push(Diag::new("E-TIME-003", pos, format!("tempo {t} は 20..400 bpm の範囲外です")));
            }
            p.tempo = t;
        }
        None => diags.push(Diag::new(
            "E-SONG-001",
            Pos { line: 1, col: 1 },
            format!("song \"{}\" に tempo がありません(例: tempo 96bpm)", song.name),
        )),
    }
    if let Some(((num, den), pos)) = song.meter {
        if num == 0 || !(den == 2 || den == 4 || den == 8 || den == 16) {
            diags.push(Diag::new("E-TIME-004", pos, format!("拍子 {num}/{den} は解釈できません")));
        } else {
            p.time_sig = (num, den);
        }
    }
    if let Some(((root, scale), pos)) = &song.key {
        match parse_key(root, scale) {
            Some(k) => p.key = k,
            None => diags.push(Diag::new(
                "E-SONG-002",
                *pos,
                format!("キー '{root} {scale}' が解釈できません(例: D minor)"),
            )),
        }
    }
    // beats are engine quarter-notes: 4/4 -> 4, 6/8 -> 3
    let beats_per_bar = p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64;

    // ---- lets (evaluated lazily at the play site: beat literals need the
    //      instrument's root pitch) ----------------------------------------
    let mut lets: HashMap<&str, &PatternLit> = HashMap::new();
    for l in &song.lets {
        if lets.insert(l.name.as_str(), &l.value).is_some() {
            diags.push(Diag::new("E-MOD-002", l.pos, format!("let '{}' が重複しています", l.name)));
        }
    }

    // ---- sections ----------------------------------------------------------
    let mut sections: HashMap<&str, (u32, u32)> = HashMap::new();
    for s in &song.sections {
        let (a, b) = s.bars;
        if a == 0 || b < a {
            diags.push(Diag::new(
                "E-TIME-001",
                s.pos,
                format!("section {} = bars({a}..{b}) が不正です(小節は 1 始まり、開始 ≤ 終了)", s.name),
            ));
        }
        if sections.insert(s.name.as_str(), s.bars).is_some() {
            diags.push(Diag::new("E-MOD-002", s.pos, format!("section '{}' が重複しています", s.name)));
        }
    }

    // ---- tracks -----------------------------------------------------------
    if song.tracks.is_empty() {
        diags.push(Diag::new(
            "E-SONG-003",
            Pos { line: 1, col: 1 },
            "track がひとつもありません",
        ));
    }
    for (ti, tast) in song.tracks.iter().enumerate() {
        let id = p.alloc_id();
        let color = TRACK_COLORS[ti % TRACK_COLORS.len()];
        let mut track = Track::new(id, tast.name.clone(), TrackKind::Instrument, color);

        // instrument (required)
        let mut beat_pitch = 36u8; // C2 default; samplers use their sample root
        match &tast.instrument {
            Some(call) => match build_instrument(call, &user_devices) {
                Ok((dev, root)) => {
                    beat_pitch = root;
                    track.devices[0] = dev;
                }
                Err(d) => diags.push(d),
            },
            None => diags.push(Diag::new(
                "E-TRACK-001",
                tast.pos,
                format!("Track '{}' に instrument がありません", tast.name),
            )),
        }

        // insert effects, in order
        for call in &tast.inserts {
            match build_effect(call) {
                Ok(dev) => track.devices.push(dev),
                Err(d) => diags.push(d),
            }
        }

        // mixer
        if let Some((v, pos)) = tast.volume {
            if !(0.0..=1.0).contains(&v) {
                diags.push(Diag::new("E-TYPE-002", pos, format!("volume {v} は 0..1 の範囲外です")));
            } else {
                track.volume = v as f32;
            }
        }
        if let Some((v, pos)) = tast.pan {
            if !(-1.0..=1.0).contains(&v) {
                diags.push(Diag::new("E-TYPE-003", pos, format!("pan {v} は -1..1 の範囲外です")));
            } else {
                track.pan = v as f32;
            }
        }

        // plays → arranger clips
        for play in &tast.plays {
            let (notes, len_beats, clip_name) =
                match eval_pattern(&play.pattern, &lets, beats_per_bar, beat_pitch) {
                    Ok(v) => v,
                    Err(d) => {
                        diags.push(d);
                        continue;
                    }
                };
            let (a, b) = match &play.at {
                AtRef::Bars(a, b) => (*a, *b),
                AtRef::Section(name, pos) => match sections.get(name.as_str()) {
                    Some(r) => *r,
                    None => {
                        let mut names: Vec<&str> = sections.keys().copied().collect();
                        names.sort();
                        diags.push(Diag::new(
                            "E-MOD-003",
                            *pos,
                            format!("section '{name}' が定義されていません(定義済み: {})", names.join(", ")),
                        ));
                        continue;
                    }
                },
            };
            if a == 0 || b < a {
                diags.push(Diag::new(
                    "E-TIME-001",
                    play.pos,
                    format!("bars({a}..{b}) が不正です(小節は 1 始まり、開始 ≤ 終了)"),
                ));
                continue;
            }
            let mut clip = Clip::new(clip_name, color);
            clip.length = len_beats;
            clip.notes = notes;
            track.arranger.push(ArrangerClip {
                clip,
                start: (a - 1) as f64 * beats_per_bar,
                duration: (b - a + 1) as f64 * beats_per_bar,
            });
        }

        p.tracks.push(track);
    }

    // ---- return (effect) tracks -------------------------------------------
    let mut return_ids: HashMap<&str, usize> = HashMap::new();
    for (ri, rast) in song.returns.iter().enumerate() {
        let id = p.alloc_id();
        let color = TRACK_COLORS[(song.tracks.len() + ri) % TRACK_COLORS.len()];
        let mut track = Track::new(id, rast.name.clone(), TrackKind::Effect, color);
        for call in &rast.inserts {
            match build_effect(call) {
                Ok(dev) => track.devices.push(dev),
                Err(d) => diags.push(d),
            }
        }
        if let Some((v, pos)) = rast.volume {
            if !(0.0..=1.0).contains(&v) {
                diags.push(Diag::new("E-TYPE-002", pos, format!("volume {v} は 0..1 の範囲外です")));
            } else {
                track.volume = v as f32;
            }
        }
        if let Some((v, pos)) = rast.pan {
            if !(-1.0..=1.0).contains(&v) {
                diags.push(Diag::new("E-TYPE-003", pos, format!("pan {v} は -1..1 の範囲外です")));
            } else {
                track.pan = v as f32;
            }
        }
        if return_ids.insert(rast.name.as_str(), id).is_some() {
            diags.push(Diag::new("E-MOD-002", rast.pos, format!("return '{}' が重複しています", rast.name)));
        }
        p.tracks.push(track);
    }

    // ---- resolve sends (returns may be declared anywhere in the song) -----
    for (ti, tast) in song.tracks.iter().enumerate() {
        for (dest, level, pos) in &tast.sends {
            let Some(&dest_id) = return_ids.get(dest.as_str()) else {
                let mut names: Vec<&str> = return_ids.keys().copied().collect();
                names.sort();
                diags.push(Diag::new(
                    "E-MOD-004",
                    *pos,
                    format!("return '{dest}' が定義されていません(定義済み: {})", names.join(", ")),
                ));
                continue;
            };
            if !(0.0..=1.0).contains(level) {
                diags.push(Diag::new("E-TYPE-002", *pos, format!("send レベル {level} は 0..1 の範囲外です")));
                continue;
            }
            p.tracks[ti].sends.push((dest_id, *level as f32));
        }
    }

    if diags.is_empty() {
        Ok(p)
    } else {
        Err(diags)
    }
}

/// Evaluate a pattern expression to notes. Returns (notes, length in beats,
/// clip display name).
fn eval_pattern(
    pref: &PatternRef,
    lets: &HashMap<&str, &PatternLit>,
    beats_per_bar: f64,
    beat_pitch: u8,
) -> Result<(Vec<Note>, f64, String), Diag> {
    match pref {
        PatternRef::Lit(lit) => eval_lit(lit, beats_per_bar, beat_pitch).map(|(n, l)| (n, l, "clip".into())),
        PatternRef::Name(name, pos) => {
            let lit = resolve_let(name, *pos, lets)?;
            eval_lit(lit, beats_per_bar, beat_pitch).map(|(n, l)| (n, l, name.clone()))
        }
        PatternRef::Fn { name, inner, args, pos } => {
            // inner must be a prog literal (directly or via a let)
            let lit = match inner.as_ref() {
                PatternRef::Lit(l) => l,
                PatternRef::Name(n, npos) => resolve_let(n, *npos, lets)?,
                PatternRef::Fn { .. } => {
                    return Err(Diag::new("E-PAT-003", *pos, format!("{name}() の入れ子はできません")))
                }
            };
            if lit.kind != "prog" {
                return Err(Diag::new(
                    "E-PAT-001",
                    *pos,
                    format!("{name}() には prog リテラル(コード進行)を渡します(見つかったのは {})", lit.kind),
                ));
            }
            let (events, len) = music::parse_prog(&lit.raw, beats_per_bar, lit.pos)?;
            let mut rate: Option<f64> = None;
            let mut style = "up".to_string();
            for (key, arg) in args {
                match (key.as_str(), arg) {
                    ("rate", Arg::Num(n, apos)) => {
                        if *n <= 0.0 || *n > beats_per_bar {
                            return Err(Diag::new(
                                "E-TIME-002",
                                *apos,
                                format!("rate {n} は 0 より大きく 1 小節({beats_per_bar} 拍)以下にしてください"),
                            ));
                        }
                        rate = Some(*n);
                    }
                    ("style", Arg::Str(s, _)) => style = s.clone(),
                    (other, arg) => {
                        let pos = match arg {
                            Arg::Num(_, p) | Arg::Str(_, p) => *p,
                        };
                        return Err(Diag::new(
                            "E-PAT-002",
                            pos,
                            format!("{name}() に '{other}' という引数はありません(rate, style)"),
                        ));
                    }
                }
            }
            let notes = match name.as_str() {
                "chords" => music::prog_chords(&events),
                "bass" => music::prog_bass(&events, rate),
                "arp" => music::prog_arp(&events, rate.unwrap_or(0.5), &style, *pos)?,
                other => {
                    return Err(Diag::new(
                        "E-PAT-002",
                        *pos,
                        format!("パターン関数 '{other}' はありません(chords / arp / bass)"),
                    ))
                }
            };
            Ok((notes, len, format!("{name}")))
        }
    }
}

fn resolve_let<'a>(
    name: &str,
    pos: Pos,
    lets: &HashMap<&str, &'a PatternLit>,
) -> Result<&'a PatternLit, Diag> {
    lets.get(name).copied().ok_or_else(|| {
        let mut names: Vec<&str> = lets.keys().copied().collect();
        names.sort();
        Diag::new(
            "E-MOD-001",
            pos,
            format!("パターン '{name}' が定義されていません(定義済み: {})", names.join(", ")),
        )
    })
}

/// Evaluate a bare literal; a bare `prog` plays block chords.
fn eval_lit(lit: &PatternLit, beats_per_bar: f64, beat_pitch: u8) -> Result<(Vec<Note>, f64), Diag> {
    match lit.kind.as_str() {
        "beat" => music::parse_beat(&lit.raw, beats_per_bar, beat_pitch, lit.pos),
        "notes" => music::parse_notes(&lit.raw, lit.pos),
        "prog" => {
            let (events, len) = music::parse_prog(&lit.raw, beats_per_bar, lit.pos)?;
            Ok((music::prog_chords(&events), len))
        }
        other => Err(Diag::new(
            "E-PARSE-009",
            lit.pos,
            format!("音楽リテラルは beat / notes / prog です(見つかったのは {other})"),
        )),
    }
}

// ---------------------------------------------------------------------------
// device registry (v0: fixed builtin set; @std packages arrive with forte-pkg)
// ---------------------------------------------------------------------------

const INSTRUMENTS: &[&str] = &["sampler", "polymer", "grid"];
const EFFECTS: &[&str] = &["filter", "eq", "drive", "delay", "reverb"];

/// Build an instrument device. Returns the device plus the root pitch that
/// `beat` literals on this track trigger.
fn build_instrument(
    call: &Call,
    user_devices: &HashMap<&str, &DeviceAst>,
) -> Result<(Device, u8), Diag> {
    if let Some(dev_ast) = user_devices.get(call.name.as_str()) {
        let graph = grid_build::instantiate(dev_ast, call)?;
        let mut dev = Device::new(DeviceKind::PolyGrid);
        dev.grid = Some(graph);
        return Ok((dev, 36));
    }
    match call.name.as_str() {
        "sampler" => {
            let mut dev = Device::new(DeviceKind::Sampler);
            let mut root = 36u8;
            for (key, arg) in &call.args {
                match (key.as_str(), arg) {
                    ("sample", Arg::Str(s, pos)) => {
                        let canon = match s.to_ascii_lowercase().as_str() {
                            "kick" => ("Kick", 36),
                            "snare" => ("Snare", 38),
                            "hat" => ("Hat", 42),
                            _ => {
                                return Err(Diag::new(
                                    "E-DEV-003",
                                    *pos,
                                    format!("ビルトインサンプルは Kick / Snare / Hat です(見つかったのは {s})。外部オーディオの import は仕様として存在しません(SYS-REC-001)"),
                                ))
                            }
                        };
                        dev.sample = SampleSource::Builtin(canon.0.into());
                        root = canon.1;
                    }
                    _ => set_param(
                        &mut dev,
                        key,
                        arg,
                        &[("gain", 0), ("attack", 1), ("decay", 2), ("sustain", 3), ("release", 4), ("pitch", 5)],
                        &[],
                        call,
                    )?,
                }
            }
            if dev.sample == SampleSource::None {
                return Err(Diag::new(
                    "E-DEV-004",
                    call.pos,
                    "sampler には sample: \"Kick\" などの指定が必要です",
                ));
            }
            Ok((dev, root))
        }
        "polymer" => {
            let mut dev = Device::new(DeviceKind::Polymer);
            for (key, arg) in &call.args {
                set_param(
                    &mut dev,
                    key,
                    arg,
                    &[
                        ("cutoff", 1), ("reso", 2), ("attack", 3), ("decay", 4),
                        ("sustain", 5), ("release", 6), ("detune", 7), ("sub", 8),
                        ("filtenv", 9),
                    ],
                    &[("wave", 0, &["sine", "saw", "square", "tri"])],
                    call,
                )?;
            }
            Ok((dev, 36))
        }
        "grid" => Ok((Device::poly_grid(), 36)),
        other => Err(Diag::new(
            "E-DEV-001",
            call.pos,
            format!(
                "instrument '{other}' はありません(使えるもの: {}。または device で自作)",
                INSTRUMENTS.join(", ")
            ),
        )),
    }
}

fn build_effect(call: &Call) -> Result<Device, Diag> {
    let (kind, params, opts): (DeviceKind, &[(&str, usize)], &[(&str, usize, &[&str])]) =
        match call.name.as_str() {
            "filter" => (
                DeviceKind::Filter,
                &[("cutoff", 1), ("reso", 2)],
                &[("type", 0, &["lp", "hp", "bp", "notch"])],
            ),
            "eq" => (DeviceKind::Eq, &[("low", 0), ("mid", 1), ("high", 2)], &[]),
            "drive" => (DeviceKind::Drive, &[("drive", 0), ("amount", 0)], &[]),
            "delay" => (
                DeviceKind::Delay,
                &[("time", 0), ("fdbk", 1), ("feedback", 1), ("mix", 2)],
                &[],
            ),
            "reverb" => (DeviceKind::Reverb, &[("size", 0), ("decay", 1), ("mix", 2)], &[]),
            other => {
                return Err(Diag::new(
                    "E-DEV-001",
                    call.pos,
                    format!("effect '{other}' はありません(使えるもの: {})", EFFECTS.join(", ")),
                ))
            }
        };
    let mut dev = Device::new(kind);
    for (key, arg) in &call.args {
        set_param(&mut dev, key, arg, params, opts, call)?;
    }
    Ok(dev)
}

/// Set one named argument on a device: numeric knobs are validated to 0..1,
/// string options are resolved to their discrete index.
fn set_param(
    dev: &mut Device,
    key: &str,
    arg: &Arg,
    params: &[(&str, usize)],
    opts: &[(&str, usize, &[&str])],
    call: &Call,
) -> Result<(), Diag> {
    let k = key.to_ascii_lowercase();
    if let Some((_, idx, choices)) = opts.iter().find(|(name, _, _)| *name == k) {
        let (s, pos) = match arg {
            Arg::Str(s, pos) => (s, pos),
            Arg::Num(_, pos) => {
                return Err(Diag::new(
                    "E-TYPE-004",
                    *pos,
                    format!("{}.{k} は文字列で指定します({})", call.name, choices.join(" / ")),
                ))
            }
        };
        match choices.iter().position(|c| *c == s.to_ascii_lowercase()) {
            Some(i) => {
                dev.params[*idx] = i as f32;
                Ok(())
            }
            None => Err(Diag::new(
                "E-TYPE-005",
                *pos,
                format!("{}.{k} に '{s}' は使えません({})", call.name, choices.join(" / ")),
            )),
        }
    } else if let Some((_, idx)) = params.iter().find(|(name, _)| *name == k) {
        let (n, pos) = match arg {
            Arg::Num(n, pos) => (*n, pos),
            Arg::Str(_, pos) => {
                return Err(Diag::new("E-TYPE-004", *pos, format!("{}.{k} は数値で指定します", call.name)))
            }
        };
        if !(0.0..=1.0).contains(&n) {
            return Err(Diag::new(
                "E-TYPE-002",
                *pos,
                format!("{}.{k} = {n} は 0..1 の範囲外です", call.name),
            ));
        }
        dev.params[*idx] = n as f32;
        Ok(())
    } else {
        let mut names: Vec<&str> = params.iter().map(|(n, _)| *n).collect();
        names.extend(opts.iter().map(|(n, _, _)| *n));
        Err(Diag::new(
            "E-DEV-002",
            call.pos,
            format!("{} に '{key}' というパラメータはありません(使えるもの: {})", call.name, names.join(", ")),
        ))
    }
}

fn parse_key(root: &str, scale: &str) -> Option<KeySignature> {
    let chars: Vec<char> = root.chars().collect();
    let base = match chars.first()?.to_ascii_uppercase() {
        'C' => 0i32,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let acc: i32 = chars[1..]
        .iter()
        .map(|&c| match c {
            '#' => 1,
            'b' => -1,
            _ => 99,
        })
        .sum();
    if acc.abs() > 2 {
        return None;
    }
    let scale = match scale.to_ascii_lowercase().as_str() {
        "major" | "maj" => Scale::Major,
        "minor" | "min" => Scale::Minor,
        "dorian" => Scale::Dorian,
        "phrygian" => Scale::Phrygian,
        "lydian" => Scale::Lydian,
        "mixolydian" => Scale::Mixolydian,
        "locrian" => Scale::Locrian,
        "harmonicminor" => Scale::HarmonicMinor,
        "chromatic" => Scale::Chromatic,
        _ => return None,
    };
    Some(KeySignature { root: (((base + acc) % 12 + 12) % 12) as u8, scale })
}
