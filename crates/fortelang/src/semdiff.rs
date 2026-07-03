//! Semantic diff — 「何が変わったか」を行番号ではなく音楽の言葉で言う。
//! 全てがコードである Forte だから可能な芸当: 両バージョンをコンパイルして
//! **モデル同士**を比較する。コンパイルできないファイルは行差分に落ちる。

use std::collections::BTreeMap;

use dawcore::model::{Project, Track};

use crate::vcs::Snapshot;
use crate::ModuleLoader;

/// Loader over a VCS snapshot (or any path→bytes map).
pub struct SnapLoader<'a>(pub &'a Snapshot);

impl ModuleLoader for SnapLoader<'_> {
    fn load(&self, path: &str) -> Result<String, String> {
        self.0
            .get(path)
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .ok_or_else(|| format!("{path} はこのスナップショットにありません"))
    }
    fn load_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        self.0.get(path).cloned().ok_or_else(|| format!("{path} はこのスナップショットにありません"))
    }
}

fn base_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn compile_in(snap: &Snapshot, path: &str) -> Option<Project> {
    let src = String::from_utf8_lossy(snap.get(path)?).into_owned();
    crate::compile_with_loader(&src, &SnapLoader(snap), base_dir(path)).ok()
}

/// Diff two snapshots. Returns a human report (empty string = no changes).
pub fn diff_snapshots(old: &Snapshot, new: &Snapshot) -> String {
    let mut out = String::new();
    let (added, modified, deleted) = crate::vcs::Repo::changes(old, new);
    for path in &added {
        out.push_str(&format!("+ {path} (新規)\n"));
    }
    for path in &deleted {
        out.push_str(&format!("- {path} (削除)\n"));
    }
    for path in &modified {
        out.push_str(&format!("~ {path}\n"));
        if path.ends_with(".forte") {
            match (compile_in(old, path), compile_in(new, path)) {
                (Some(a), Some(b)) => {
                    let lines = diff_projects(&a, &b);
                    if lines.is_empty() {
                        out.push_str("    モデルは同一(コメント・整形のみの変更)\n");
                    }
                    for line in lines {
                        out.push_str(&format!("    {line}\n"));
                    }
                }
                _ => {
                    // device library or a broken version: fall back to lines
                    for line in line_diff(
                        &String::from_utf8_lossy(&old[path]),
                        &String::from_utf8_lossy(&new[path]),
                    ) {
                        out.push_str(&format!("    {line}\n"));
                    }
                }
            }
        } else {
            out.push_str(&format!(
                "    バイナリ変更 ({} → {} bytes)\n",
                old[path].len(),
                new[path].len()
            ));
        }
    }

    // a library / asset edit changes the sound of songs that import it, even
    // though the song file itself is untouched — say it where it will be heard
    if !(modified.is_empty() && deleted.is_empty()) {
        for (path, bytes) in new {
            if !path.ends_with(".forte")
                || modified.contains(path)
                || old.get(path) != Some(bytes)
            {
                continue;
            }
            if let (Some(a), Some(b)) = (compile_in(old, path), compile_in(new, path)) {
                let lines = diff_projects(&a, &b);
                if !lines.is_empty() {
                    out.push_str(&format!("~ {path} (import 経由で音が変わります)\n"));
                    for line in lines {
                        out.push_str(&format!("    {line}\n"));
                    }
                }
            }
        }
    }
    out
}

/// The music-vocabulary diff of two compiled songs.
pub fn diff_projects(a: &Project, b: &Project) -> Vec<String> {
    let mut out = Vec::new();
    if a.tempo != b.tempo {
        out.push(format!("tempo: {} → {} bpm", a.tempo, b.tempo));
    }
    if a.time_sig != b.time_sig {
        out.push(format!(
            "meter: {}/{} → {}/{}",
            a.time_sig.0, a.time_sig.1, b.time_sig.0, b.time_sig.1
        ));
    }
    if a.key.root != b.key.root || a.key.scale != b.key.scale {
        out.push("key が変わりました".into());
    }
    let beats_per_bar = b.time_sig.0 as f64 * 4.0 / b.time_sig.1 as f64;

    // track id → name maps for send resolution
    let a_names: BTreeMap<usize, &str> =
        a.tracks.iter().map(|t| (t.id, t.name.as_str())).collect();
    let b_names: BTreeMap<usize, &str> =
        b.tracks.iter().map(|t| (t.id, t.name.as_str())).collect();

    let a_by: BTreeMap<&str, &Track> = a.tracks.iter().map(|t| (t.name.as_str(), t)).collect();
    let b_by: BTreeMap<&str, &Track> = b.tracks.iter().map(|t| (t.name.as_str(), t)).collect();

    for (name, _) in a_by.iter().filter(|(n, _)| !b_by.contains_key(*n)) {
        out.push(format!("track {name}: 削除"));
    }
    for (name, t) in b_by.iter().filter(|(n, _)| !a_by.contains_key(*n)) {
        out.push(format!("track {name}: 追加 ({} クリップ)", t.arranger.len()));
    }
    for (name, ta) in &a_by {
        let Some(tb) = b_by.get(name) else { continue };
        for line in diff_track(ta, tb, beats_per_bar, &a_names, &b_names) {
            out.push(format!("track {name}: {line}"));
        }
    }
    out
}

fn diff_track(
    a: &Track,
    b: &Track,
    beats_per_bar: f64,
    a_names: &BTreeMap<usize, &str>,
    b_names: &BTreeMap<usize, &str>,
) -> Vec<String> {
    let mut out = Vec::new();

    // devices (instrument + inserts), positional
    let chain = |t: &Track| t.devices.iter().map(|d| d.kind.label().to_string()).collect::<Vec<_>>();
    let (ca, cb) = (chain(a), chain(b));
    if ca != cb {
        out.push(format!("デバイスチェーン: [{}] → [{}]", ca.join(", "), cb.join(", ")));
    } else {
        for (i, (da, db)) in a.devices.iter().zip(&b.devices).enumerate() {
            let names = da.kind.params();
            for (pi, (pa, pb)) in da.params.iter().zip(&db.params).enumerate() {
                if pa != pb {
                    let pname = names
                        .get(pi)
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_else(|| format!("param{pi}"));
                    out.push(format!(
                        "{} の {pname}: {} → {}",
                        cb[i],
                        param_str(da.kind, pi, *pa),
                        param_str(db.kind, pi, *pb)
                    ));
                }
            }
            // user-defined devices live in the grid graph, not in params
            let grid_json = |d: &dawcore::model::Device| {
                d.grid.as_ref().and_then(|g| serde_json::to_string(g).ok())
            };
            if grid_json(da) != grid_json(db) {
                out.push(format!("{} のパッチ(ノードグラフ)が変わりました", cb[i]));
            }
            if da.modulators.len() != db.modulators.len() {
                out.push(format!(
                    "{} のモジュレータ: {} → {} 基",
                    cb[i],
                    da.modulators.len(),
                    db.modulators.len()
                ));
            }
        }
    }

    if a.volume != b.volume {
        out.push(format!("volume: {} → {}", a.volume, b.volume));
    }
    if a.pan != b.pan {
        out.push(format!("pan: {} → {}", a.pan, b.pan));
    }

    // sends, resolved to return names
    let sends = |t: &Track, names: &BTreeMap<usize, &str>| -> BTreeMap<String, f32> {
        t.sends
            .iter()
            .map(|(id, lvl)| (names.get(id).unwrap_or(&"?").to_string(), *lvl))
            .collect()
    };
    let (sa, sb) = (sends(a, a_names), sends(b, b_names));
    if sa != sb {
        let fmt = |m: &BTreeMap<String, f32>| {
            m.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
        };
        out.push(format!("send: [{}] → [{}]", fmt(&sa), fmt(&sb)));
    }

    if a.volume_automation.len() != b.volume_automation.len()
        || a.volume_automation
            .iter()
            .zip(&b.volume_automation)
            .any(|(x, y)| x.beat != y.beat || x.value != y.value)
    {
        out.push(format!(
            "オートメーション: {} → {} 点",
            a.volume_automation.len(),
            b.volume_automation.len()
        ));
    }

    // arranger clips matched by start beat
    fn clips(t: &Track) -> BTreeMap<u64, &dawcore::model::ArrangerClip> {
        t.arranger.iter().map(|c| (c.start.to_bits(), c)).collect()
    }
    let (ka, kb) = (clips(a), clips(b));
    let bar = |beat: f64| (beat / beats_per_bar) as u32 + 1;
    for (start, c) in &ka {
        if !kb.contains_key(start) {
            let s = f64::from_bits(*start);
            out.push(format!("小節 {}..{}: 配置を削除", bar(s), bar(s + c.duration - 1e-9)));
        }
    }
    for (start, c) in &kb {
        if !ka.contains_key(start) {
            let s = f64::from_bits(*start);
            out.push(format!(
                "小節 {}..{}: 配置を追加 ({} ノート)",
                bar(s),
                bar(s + c.duration - 1e-9),
                c.clip.notes.len()
            ));
        }
    }
    for (start, cb2) in &kb {
        let Some(ca2) = ka.get(start) else { continue };
        let s = f64::from_bits(*start);
        let notes = |c: &dawcore::model::ArrangerClip| -> Vec<(u64, u8, u64)> {
            let mut v: Vec<(u64, u8, u64)> = c
                .clip
                .notes
                .iter()
                .map(|n| (n.start.to_bits(), n.pitch, n.length.to_bits()))
                .collect();
            v.sort_unstable();
            v
        };
        let (na, nb) = (notes(ca2), notes(cb2));
        if na != nb {
            let plus = nb.iter().filter(|n| !na.contains(n)).count();
            let minus = na.iter().filter(|n| !nb.contains(n)).count();
            out.push(format!(
                "小節 {}..{}: ノート変更 (+{plus} -{minus})",
                bar(s),
                bar(s + cb2.duration - 1e-9)
            ));
        } else if ca2.duration != cb2.duration {
            out.push(format!("小節 {} からの配置の長さが変わりました", bar(s)));
        }
    }

    // audio clips by name
    let audio = |t: &Track| -> Vec<String> { t.audio_clips.iter().map(|c| c.name.clone()).collect() };
    let (aa, ab) = (audio(a), audio(b));
    if aa != ab {
        out.push(format!("audio 配置: [{}] → [{}]", aa.join(", "), ab.join(", ")));
    }

    out
}

/// Discrete option params read as their choice name, not the stored index.
fn param_str(kind: dawcore::model::DeviceKind, idx: usize, v: f32) -> String {
    use dawcore::model::DeviceKind as K;
    let choices: &[&str] = match (kind, idx) {
        (K::Polymer, 0) => &["sine", "saw", "square", "tri"],
        (K::Filter, 0) => &["lp", "hp", "bp", "notch"],
        _ => &[],
    };
    match choices.get(v as usize) {
        Some(name) if v.fract() == 0.0 => (*name).to_string(),
        _ => format!("{v}"),
    }
}

/// Minimal line diff (LCS-free: common prefix/suffix + middle as -/+) — only
/// the fallback for files we can't compile. Kept dumb on purpose.
pub fn line_diff(a: &str, b: &str) -> Vec<String> {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    let mut pre = 0;
    while pre < la.len() && pre < lb.len() && la[pre] == lb[pre] {
        pre += 1;
    }
    let mut post = 0;
    while post < la.len() - pre && post < lb.len() - pre && la[la.len() - 1 - post] == lb[lb.len() - 1 - post]
    {
        post += 1;
    }
    let mut out = Vec::new();
    for l in &la[pre..la.len() - post] {
        out.push(format!("- {l}"));
    }
    for l in &lb[pre..lb.len() - post] {
        out.push(format!("+ {l}"));
    }
    out
}
