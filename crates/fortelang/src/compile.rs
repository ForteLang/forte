//! Compile the AST into a `dawcore::model::Project`. The v0 slice targets the
//! existing engine directly; the dedicated render-graph IR arrives with
//! forte-core (SAD §2).

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{Diag, Pos};
use crate::grid_build;
use crate::music;
use dawcore::model::{
    ArrangerClip, AudioClip, AutomationPoint, Clip, Device, DeviceKind, KeySignature, ModKind, ParamAutomation,
    ModRoute, Modulator, Note, Project, SampleSource, Scale, Track, TrackKind, TRACK_COLORS,
};

pub fn compile(
    file: &FileAst,
    assets: &HashMap<String, crate::AssetInfo>,
) -> Result<Project, Vec<Diag>> {
    let Some(song) = &file.song else {
        return Err(vec![Diag::new(
            "E-SONG-004",
            Pos { line: 1, col: 1 },
            "song がありません(このファイルはデバイスライブラリです — 検証は forte check)",
        )]);
    };
    let mut diags: Vec<Diag> = Vec::new();
    let mut p = Project::empty();

    // ---- user-defined devices ----------------------------------------------
    let mut user_devices: HashMap<&str, &DeviceAst> = HashMap::new();
    collect_devices(file, &mut user_devices, &mut diags);

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

    // song-level swing (MPC 表記: 0.5 = ストレート、2/3 = 完全シャッフル)
    let swing = match song.swing {
        Some((v, pos)) => {
            if !(0.5..=0.8).contains(&v) {
                diags.push(Diag::new(
                    "E-TIME-004",
                    pos,
                    format!("swing {v} は 0.5..0.8 の範囲外です(0.5 = ストレート、0.66 ≒ シャッフル)"),
                ));
                0.5
            } else {
                v
            }
        }
        None => 0.5,
    };

    for (ti, tast) in song.tracks.iter().enumerate() {
        let id = p.alloc_id();
        let color = TRACK_COLORS[ti % TRACK_COLORS.len()];
        let mut track = Track::new(id, tast.name.clone(), TrackKind::Instrument, color);

        // instrument (required unless the track only places recorded audio)
        let mut beat_pitch = 36u8; // C2 default; samplers use their sample root
        match &tast.instrument {
            Some(call) => match build_instrument(call, &user_devices, assets) {
                Ok((dev, root)) => {
                    beat_pitch = root;
                    track.devices[0] = dev;
                }
                Err(d) => diags.push(d),
            },
            None if !tast.audios.is_empty() && tast.plays.is_empty() => {
                track.devices.clear(); // pure audio track
            }
            None => diags.push(Diag::new(
                "E-TRACK-001",
                tast.pos,
                format!("Track '{}' に instrument がありません", tast.name),
            )),
        }

        // insert effects, in order; remember their names so automate/modulate
        // can target `<insert>.<param>` (first match wins on duplicates)
        let mut insert_devs: Vec<(String, usize)> = Vec::new();
        for call in &tast.inserts {
            match build_effect(call, &user_devices, p.tempo) {
                Ok(dev) => {
                    insert_devs.push((call.name.clone(), track.devices.len()));
                    track.devices.push(dev);
                }
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

        // automation lanes: volume, an instrument param, or `<insert>.<param>`
        for auto in &tast.automations {
            let param_target = if auto.target == "volume" {
                None
            } else {
                match resolve_target(&track, &insert_devs, &auto.target) {
                    Ok(t) => Some(t),
                    Err(msg) => {
                        diags.push(Diag::new("E-AUTO-001", auto.pos, msg));
                        continue;
                    }
                }
            };
            if !(0.0..=1.0).contains(&auto.from) || !(0.0..=1.0).contains(&auto.to) {
                diags.push(Diag::new(
                    "E-TYPE-002",
                    auto.pos,
                    format!("automate volume の値 {} → {} は 0..1 の範囲外です", auto.from, auto.to),
                ));
                continue;
            }
            let (a, b) = match &auto.at {
                AtRef::Bars(a, b) => (*a, *b),
                AtRef::Section(name, pos) => match sections.get(name.as_str()) {
                    Some(r) => *r,
                    None => {
                        diags.push(Diag::new("E-MOD-003", *pos, format!("section '{name}' が定義されていません")));
                        continue;
                    }
                },
            };
            if a == 0 || b < a {
                diags.push(Diag::new(
                    "E-TIME-001",
                    auto.pos,
                    format!("bars({a}..{b}) が不正です(小節は 1 始まり、開始 ≤ 終了)"),
                ));
                continue;
            }
            // ramp across the range; the last point holds so the value stays put
            let points = vec![
                AutomationPoint {
                    beat: (a - 1) as f64 * beats_per_bar,
                    value: auto.from as f32,
                    hold: false,
                },
                AutomationPoint { beat: b as f64 * beats_per_bar, value: auto.to as f32, hold: true },
            ];
            match param_target {
                None => track.volume_automation.extend(points),
                // ramps targeting the same param merge into ONE lane in beat
                // order — separate lanes would each cover the whole timeline
                // (eval holds the edge values) and the last one would win
                Some((di, pi)) => match track
                    .param_automation
                    .iter_mut()
                    .find(|pa| pa.device == di && pa.param == pi)
                {
                    Some(lane) => {
                        lane.points.extend(points);
                        lane.points.sort_by(|x, y| x.beat.total_cmp(&y.beat));
                    }
                    None => track
                        .param_automation
                        .push(ParamAutomation { device: di, param: pi, points }),
                },
            }
        }
        track.volume_automation.sort_by(|x, y| x.beat.total_cmp(&y.beat));

        // modulators plug into instrument or insert params; each lives on the
        // device it modulates (its routes get that device's index)
        for m in &tast.modulations {
            match build_lfo(m, &track, &insert_devs, p.tempo) {
                Ok((di, lfo)) => track.devices[di].modulators.push(lfo),
                Err(d) => diags.push(d),
            }
        }

        // plays → arranger clips
        for play in &tast.plays {
            let (mut notes, len_beats, clip_name) =
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
            if swing > 0.5 {
                // delay every off-beat 16th that sits exactly on the grid;
                // freely-timed notes are left alone
                let shift = (swing - 0.5) * 0.5;
                for n in &mut notes {
                    let idx = (n.start / 0.25).round();
                    if (n.start - idx * 0.25).abs() < 1e-9 && (idx as i64) % 2 == 1 {
                        n.start += shift;
                        n.length = (n.length - shift).max(0.05);
                    }
                }
            }
            clip.notes = notes;
            track.arranger.push(ArrangerClip {
                clip,
                start: (a - 1) as f64 * beats_per_bar,
                duration: (b - a + 1) as f64 * beats_per_bar,
            });
        }

        // recorded audio placements
        for ap in &tast.audios {
            let Some(info) = assets.get(&ap.name) else {
                let mut names: Vec<&str> = assets.keys().map(String::as_str).collect();
                names.sort();
                diags.push(Diag::new(
                    "E-PROV-003",
                    ap.pos,
                    format!("録音アセット '{}' が import されていません(あるもの: {})", ap.name, names.join(", ")),
                ));
                continue;
            };
            let (a, b) = match &ap.at {
                AtRef::Bars(a, b) => (*a, *b),
                AtRef::Section(name, pos) => match sections.get(name.as_str()) {
                    Some(r) => *r,
                    None => {
                        diags.push(Diag::new("E-MOD-003", *pos, format!("section '{name}' が定義されていません")));
                        continue;
                    }
                },
            };
            if a == 0 || b < a {
                diags.push(Diag::new("E-TIME-001", ap.pos, format!("bars({a}..{b}) が不正です")));
                continue;
            }
            track.audio_clips.push(AudioClip {
                name: ap.name.clone(),
                color,
                source: SampleSource::Asset(info.key.clone()),
                start: (a - 1) as f64 * beats_per_bar,
                duration: (b - a + 1) as f64 * beats_per_bar,
                gain: 0.9,
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
            match build_effect(call, &user_devices, p.tempo) {
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

fn collect_devices<'a>(
    file: &'a FileAst,
    user_devices: &mut HashMap<&'a str, &'a DeviceAst>,
    diags: &mut Vec<Diag>,
) {
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
}

/// Validate a device library (a file without a song): registry rules plus a
/// default instantiation of every device, so `forte check lib.forte` means
/// something.
pub fn validate_devices(file: &FileAst) -> Vec<Diag> {
    let mut diags = Vec::new();
    let mut user_devices: HashMap<&str, &DeviceAst> = HashMap::new();
    collect_devices(file, &mut user_devices, &mut diags);
    for d in file.devices.iter() {
        let probe = Call { name: d.name.clone(), args: Vec::new(), pos: d.pos };
        // probe with unbound take slots — the caller binds real takes later
        let takes: HashMap<String, Option<String>> =
            d.takes.iter().map(|(n, _)| (n.clone(), None)).collect();
        if let Err(e) = grid_build::instantiate(d, &probe, &takes) {
            diags.push(e);
        }
    }
    diags
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
                        let pos = arg.pos();
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
            Ok((notes, len, name.to_string()))
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

const INSTRUMENTS: &[&str] = &["sampler", "kit", "polymer", "grid"];
const EFFECTS: &[&str] =
    &["filter", "eq", "drive", "delay", "reverb", "comp", "chorus", "pump", "width"];

/// Build an instrument device. Returns the device plus the root pitch that
/// `beat` literals on this track trigger.
fn build_instrument(
    call: &Call,
    user_devices: &HashMap<&str, &DeviceAst>,
    assets: &HashMap<String, crate::AssetInfo>,
) -> Result<(Device, u8), Diag> {
    if let Some(dev_ast) = user_devices.get(call.name.as_str()) {
        if dev_ast.kind == "Effect" {
            return Err(Diag::new(
                "E-DEV-009",
                call.pos,
                format!("'{}' は Effect です(instrument ではなく insert で使います)", call.name),
            ));
        }
        // bind declared take slots from call-site args (take: myTake)
        let mut takes: HashMap<String, Option<String>> = HashMap::new();
        for (tname, _) in &dev_ast.takes {
            match call.args.iter().find(|(k, _)| k == tname) {
                Some((_, Arg::Ident(aname, apos))) => {
                    let Some(info) = assets.get(aname) else {
                        let mut names: Vec<&str> = assets.keys().map(String::as_str).collect();
                        names.sort();
                        return Err(Diag::new(
                            "E-PROV-003",
                            *apos,
                            format!(
                                "録音アセット '{aname}' が import されていません(あるもの: {})",
                                names.join(", ")
                            ),
                        ));
                    };
                    takes.insert(tname.clone(), Some(info.key.clone()));
                }
                Some((_, arg)) => {
                    return Err(Diag::new(
                        "E-TYPE-004",
                        arg.pos(),
                        format!("{}.{tname} は import した録音の名前で渡します", call.name),
                    ))
                }
                None => {
                    return Err(Diag::new(
                        "E-DEV-002",
                        call.pos,
                        format!(
                            "{} に take '{tname}' を渡してください(例: {tname}: <import した録音>)",
                            call.name
                        ),
                    ))
                }
            }
        }
        let graph = grid_build::instantiate(dev_ast, call, &takes)?;
        let mut dev = Device::new(DeviceKind::PolyGrid);
        // expose declared params (declaration order) so modulate / automate
        // can drive them at runtime — same values the graph was baked with
        dev.params = graph.param_binds.iter().map(|(_, v, _)| *v).collect();
        dev.grid = Some(graph);
        return Ok((dev, 36));
    }
    match call.name.as_str() {
        "sampler" => {
            let mut dev = Device::new(DeviceKind::Sampler);
            let mut root = 36u8;
            for (key, arg) in &call.args {
                match (key.as_str(), arg) {
                    // recorded take as the instrument: your voice becomes a synth
                    ("take", Arg::Ident(name, pos)) => {
                        let Some(info) = assets.get(name) else {
                            let mut names: Vec<&str> =
                                assets.keys().map(String::as_str).collect();
                            names.sort();
                            return Err(Diag::new(
                                "E-PROV-003",
                                *pos,
                                format!(
                                    "録音アセット '{name}' が import されていません(あるもの: {})",
                                    names.join(", ")
                                ),
                            ));
                        };
                        dev.sample = SampleSource::Asset(info.key.clone());
                        if root == 36 {
                            root = 60; // asset registry roots takes at C4
                        }
                    }
                    ("take", arg) => {
                        return Err(Diag::new(
                            "E-TYPE-004",
                            arg.pos(),
                            "sampler.take は import した名前で指定します(例: take: myTake)",
                        ))
                    }
                    // the note the take was performed at: play it there and it
                    // sounds untouched; everything else is repitched from it
                    ("root", Arg::Ident(s, pos)) => {
                        let p = music::parse_pitch(s, *pos)?;
                        if !(36..=84).contains(&p) {
                            return Err(Diag::new(
                                "E-TYPE-002",
                                *pos,
                                format!("root {s} は C2..C6 の範囲で指定してください"),
                            ));
                        }
                        root = p;
                        // asset roots are C4 (60): encode the offset in Pitch
                        dev.params[5] = 0.5 + (60.0 - p as f32) / 48.0;
                    }
                    ("root", arg) => {
                        return Err(Diag::new(
                            "E-TYPE-004",
                            arg.pos(),
                            "sampler.root は音名で指定します(例: root: A3)",
                        ))
                    }
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
                    // start/end trim the take; loop sustains it; reverse flips it —
                    // one recording becomes many instruments
                    _ => set_param(
                        &mut dev,
                        key,
                        arg,
                        &[
                            ("gain", 0), ("attack", 1), ("decay", 2), ("sustain", 3),
                            ("release", 4), ("pitch", 5), ("start", 6), ("end", 7),
                        ],
                        &[("loop", 8, &["off", "on"]), ("reverse", 9, &["off", "on"])],
                        call,
                    )?,
                }
            }
            if dev.sample == SampleSource::None {
                return Err(Diag::new(
                    "E-DEV-004",
                    call.pos,
                    "sampler には sample: \"Kick\" か take: <import した録音> の指定が必要です",
                ));
            }
            Ok((dev, root))
        }
        // pitch → take map: a drum kit built from recordings.
        // kit(C2: kickTake, D2: snareTake, gain: 0.9)
        "kit" => {
            let mut dev = Device::new(DeviceKind::Kit);
            for (key, arg) in &call.args {
                if let Ok(p) = music::parse_pitch(key, arg.pos()) {
                    let Arg::Ident(name, pos) = arg else {
                        return Err(Diag::new(
                            "E-TYPE-004",
                            arg.pos(),
                            format!("kit.{key} は import した録音の名前で指定します"),
                        ));
                    };
                    let Some(info) = assets.get(name) else {
                        let mut names: Vec<&str> = assets.keys().map(String::as_str).collect();
                        names.sort();
                        return Err(Diag::new(
                            "E-PROV-003",
                            *pos,
                            format!(
                                "録音アセット '{name}' が import されていません(あるもの: {})",
                                names.join(", ")
                            ),
                        ));
                    };
                    dev.kit.push((p, SampleSource::Asset(info.key.clone())));
                } else {
                    set_param(
                        &mut dev,
                        key,
                        arg,
                        &[("gain", 0), ("attack", 1), ("decay", 2), ("sustain", 3), ("release", 4)],
                        &[],
                        call,
                    )?;
                }
            }
            if dev.kit.is_empty() {
                return Err(Diag::new(
                    "E-DEV-004",
                    call.pos,
                    "kit には少なくとも 1 つのパッドが必要です(例: kit(C2: kickTake))",
                ));
            }
            dev.kit.sort_by_key(|(p, _)| *p);
            // beat literals trigger the lowest pad
            let root = dev.kit[0].0;
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

/// Resolve a runtime-controllable param name on the track's instrument
/// (device 0): builtin kinds use their fixed param table, grid instruments
/// use the device-declared params exposed via `param_binds`.
/// Resolve an automate/modulate target to (device index, param index).
/// Undotted names address the instrument (device 0); `<insert>.<param>`
/// addresses an insert effect by the name it was written with.
fn resolve_target(
    track: &Track,
    inserts: &[(String, usize)],
    name: &str,
) -> Result<(usize, usize), String> {
    if let Some((head, tail)) = name.split_once('.') {
        let Some(&(_, di)) = inserts.iter().find(|(n, _)| n.eq_ignore_ascii_case(head)) else {
            return Err(format!(
                "insert '{head}' はありません(このトラックの insert: {})",
                if inserts.is_empty() {
                    "なし".to_string()
                } else {
                    inserts.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
                }
            ));
        };
        return resolve_device_param(&track.devices[di], tail).map(|pi| (di, pi));
    }
    let Some(dev) = track.devices.first() else {
        return Err("instrument がありません".into());
    };
    resolve_device_param(dev, name).map(|pi| (0, pi))
}

fn resolve_device_param(dev: &Device, name: &str) -> Result<usize, String> {
    if matches!(dev.kind, DeviceKind::PolyGrid | DeviceKind::GridFx) {
        let names: Vec<&str> = dev
            .grid
            .as_ref()
            .map(|g| g.param_binds.iter().map(|(n, _, _)| n.as_str()).collect())
            .unwrap_or_default();
        return names.iter().position(|n| n.eq_ignore_ascii_case(name)).ok_or_else(|| {
            format!(
                "パラメータ '{name}' はありません(このデバイスの param: {})",
                if names.is_empty() { "なし".to_string() } else { names.join(", ") }
            )
        });
    }
    let params = dev.kind.params();
    if params.is_empty() {
        return Err(
            "このデバイスは実行時パラメータを持ちません(automate volume は使えます)".into()
        );
    }
    params.iter().position(|p| p.eq_ignore_ascii_case(name)).ok_or_else(|| {
        format!(
            "パラメータ '{name}' はありません(使えるもの: {})",
            params.join(", ").to_ascii_lowercase()
        )
    })
}

/// Lower `modulate cutoff with lfo(...) / steps(...) / random(...)` to an
/// engine [`Modulator`] routed at the track's instrument (device 0). Grid
/// instruments expose their declared `param`s; builtins use their tables.
/// Returns the device index the modulator lives on plus the modulator itself.
fn build_lfo(
    m: &ModulateAst,
    track: &Track,
    inserts: &[(String, usize)],
    bpm: f64,
) -> Result<(usize, Modulator), Diag> {
    if track.devices.is_empty() {
        return Err(Diag::new("E-LFO-002", m.pos, "modulate には instrument が必要です"));
    }
    let (di, idx) = resolve_target(track, inserts, &m.param)
        .map_err(|msg| Diag::new("E-LFO-001", m.pos, msg))?;
    let kind = match m.kind.as_str() {
        "steps" => ModKind::Steps,
        "random" => ModKind::Random,
        "adsr" => ModKind::Adsr,
        _ => ModKind::Lfo,
    };
    let mut lfo = Modulator::new(kind);
    let mut amount: Option<f32> = None;
    let mut every_beats: Option<f64> = None;
    let mut adsr = [0.01f32, 0.3, 0.6, 0.25]; // a, d, s, r defaults
    for (key, arg) in &m.args {
        match (key.as_str(), arg) {
            ("rate", Arg::Num(n, pos)) => {
                if !(0.0..=1.0).contains(n) {
                    return Err(Diag::new("E-TYPE-002", *pos, format!("lfo.rate = {n} は 0..1 の範囲外です")));
                }
                lfo.rate = *n as f32;
            }
            ("amount", Arg::Num(n, pos)) => {
                if !(-1.0..=1.0).contains(n) {
                    return Err(Diag::new("E-TYPE-003", *pos, format!("lfo.amount = {n} は -1..1 の範囲外です")));
                }
                amount = Some(*n as f32);
            }
            ("seq", Arg::Str(sq, pos)) => {
                let mut vals = Vec::new();
                for tok in sq.split_whitespace() {
                    match tok.parse::<f32>() {
                        Ok(v) if (0.0..=1.0).contains(&v) => vals.push(v),
                        _ => {
                            return Err(Diag::new(
                                "E-TYPE-002",
                                *pos,
                                format!("steps.seq は空白区切りの 0..1 の数値です(見つかったのは '{tok}')"),
                            ))
                        }
                    }
                }
                if vals.is_empty() {
                    return Err(Diag::new("E-TYPE-002", *pos, "steps.seq が空です"));
                }
                lfo.steps = vals;
            }
            ("every", Arg::Str(ev, pos)) => {
                // one step per this note value, tempo-synced: "1/4" "1/8" "1/16"
                let step_beats = match ev.as_str() {
                    "1/4" => 1.0,
                    "1/8" => 0.5,
                    "1/16" => 0.25,
                    "1/2" => 2.0,
                    other => {
                        return Err(Diag::new(
                            "E-TYPE-005",
                            *pos,
                            format!("every に '{other}' は使えません(1/2, 1/4, 1/8, 1/16)"),
                        ))
                    }
                };
                // the whole sequence is one modulator cycle
                every_beats = Some(step_beats);
            }
            ("smooth", Arg::Num(n, pos)) => {
                if !(0.0..=1.0).contains(n) {
                    return Err(Diag::new("E-TYPE-002", *pos, format!("random.smooth = {n} は 0..1 の範囲外です")));
                }
                lfo.value = 1.0 - *n as f32; // engine: value = 1-smoothing
            }
            ("shape", Arg::Str(s, pos)) => {
                lfo.shape = match s.to_ascii_lowercase().as_str() {
                    "sine" => 0,
                    "tri" => 1,
                    "saw" => 2,
                    "square" => 3,
                    other => {
                        return Err(Diag::new(
                            "E-TYPE-005",
                            *pos,
                            format!("lfo.shape に '{other}' は使えません(sine / tri / saw / square)"),
                        ))
                    }
                };
            }
            (k @ ("a" | "d" | "s" | "r"), Arg::Num(n, pos)) if kind == ModKind::Adsr => {
                if !(0.0..=1.0).contains(n) {
                    return Err(Diag::new("E-TYPE-002", *pos, format!("adsr.{k} = {n} は 0..1 の範囲外です")));
                }
                let slot = match k {
                    "a" => 0,
                    "d" => 1,
                    "s" => 2,
                    _ => 3,
                };
                adsr[slot] = *n as f32;
            }
            (other, arg) => {
                let pos = arg.pos();
                return Err(Diag::new(
                    "E-LFO-003",
                    pos,
                    format!("modulate の引数 '{other}' は不明です(rate, amount, shape, seq, every, smooth, adsr の a/d/s/r)"),
                ));
            }
        }
    }
    let Some(amount) = amount else {
        return Err(Diag::new("E-LFO-003", m.pos, "modulate には amount(-1..1)が必要です"));
    };
    if kind == ModKind::Adsr {
        // the engine reads the envelope stages from steps = [a, d, s, r]
        lfo.steps = adsr.to_vec();
    }
    if let Some(step_beats) = every_beats {
        // tempo-sync: the sequence (or one random cycle) advances one step per
        // `every`; the engine knob maps rate → 0.05..8.05 Hz over a full cycle
        let steps_per_cycle = if lfo.steps.is_empty() { 1.0 } else { lfo.steps.len() as f64 };
        let cycle_seconds = steps_per_cycle * step_beats * 60.0 / bpm;
        let hz = (1.0 / cycle_seconds).clamp(0.05, 8.05);
        lfo.rate = (((hz - 0.05) / 8.0) as f32).clamp(0.0, 1.0);
    }
    lfo.routes.push(ModRoute { param: idx, amount });
    Ok((di, lfo))
}

fn build_effect(
    call: &Call,
    user_devices: &HashMap<&str, &DeviceAst>,
    bpm: f64,
) -> Result<Device, Diag> {
    if let Some(dev_ast) = user_devices.get(call.name.as_str()) {
        if dev_ast.kind != "Effect" {
            return Err(Diag::new(
                "E-DEV-009",
                call.pos,
                format!("'{}' は Instrument です(insert ではなく instrument で使います)", call.name),
            ));
        }
        // Effect graphs reject sample nodes, so take slots stay unbound
        let takes: HashMap<String, Option<String>> =
            dev_ast.takes.iter().map(|(n, _)| (n.clone(), None)).collect();
        let graph = grid_build::instantiate(dev_ast, call, &takes)?;
        let mut dev = Device::new(DeviceKind::GridFx);
        // expose declared params (same as PolyGrid): configure() re-writes
        // these baked values, so a static render stays bit-identical
        dev.params = graph.param_binds.iter().map(|(_, v, _)| *v).collect();
        dev.grid = Some(graph);
        return Ok(dev);
    }
    #[allow(clippy::type_complexity)]
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
            "comp" => (
                DeviceKind::Comp,
                &[("thresh", 0), ("ratio", 1), ("attack", 2), ("release", 3), ("makeup", 4)],
                &[],
            ),
            "chorus" => (DeviceKind::Chorus, &[("rate", 0), ("depth", 1), ("mix", 2)], &[]),
            "pump" => (DeviceKind::Pump, &[("amount", 0), ("beats", 1)], &[]),
            "width" => (DeviceKind::Width, &[("amount", 0)], &[]),
            other => {
                return Err(Diag::new(
                    "E-DEV-001",
                    call.pos,
                    format!(
                        "effect '{other}' はありません(使えるもの: {}。または device … : Effect で自作)",
                        EFFECTS.join(", ")
                    ),
                ))
            }
        };
    let mut dev = Device::new(kind);
    if kind == DeviceKind::Pump {
        dev.params[1] = 1.0; // default: duck once per beat
    }
    for (key, arg) in &call.args {
        set_param(&mut dev, key, arg, params, opts, call)?;
    }
    if kind == DeviceKind::Pump {
        // the knob is in beats; the engine wants seconds per duck cycle
        dev.params[1] *= (60.0 / bpm) as f32;
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
        let Arg::Str(s, pos) = arg else {
            return Err(Diag::new(
                "E-TYPE-004",
                arg.pos(),
                format!("{}.{k} は文字列で指定します({})", call.name, choices.join(" / ")),
            ));
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
        let Arg::Num(n, pos) = arg else {
            return Err(Diag::new("E-TYPE-004", arg.pos(), format!("{}.{k} は数値で指定します", call.name)));
        };
        let n = *n;
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
