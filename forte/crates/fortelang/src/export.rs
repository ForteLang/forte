//! `forte export` — data portability (SRS-WEB-005). One self-contained,
//! deterministic zip: the sources (entry + imports + recorded takes), a build
//! manifest with the audio digest, and — when the song lives in a clean
//! repository — the full `.forte/` history. Unzip anywhere and you have the
//! song, its proof, and its past. No lock-in.

use std::collections::BTreeMap;
use std::path::Path;

use crate::vcs::Repo;
use crate::zip;

const LINEAGE_FILE: &str = ".forte-lineage.json";

/// Entry + its import/asset closure as (relative path → bytes), plus the
/// lineage stamp when the song was forked from elsewhere.
pub fn collect_snapshot(entry: &str) -> Result<(String, BTreeMap<String, Vec<u8>>), String> {
    let entry_path = Path::new(entry);
    let base = entry_path.parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
    let file_name = entry_path
        .file_name()
        .ok_or("ファイル名がありません")?
        .to_string_lossy()
        .into_owned();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    collect_files(&file_name, &base, &mut files, 0)?;
    if let Ok(stamp) =
        std::fs::read(entry_path.parent().unwrap_or(Path::new("")).join(LINEAGE_FILE))
    {
        files.insert(LINEAGE_FILE.into(), stamp);
    }
    Ok((file_name, files))
}

fn collect_files(
    rel: &str,
    base: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("import が深すぎます(循環?)".into());
    }
    if files.contains_key(rel) {
        return Ok(());
    }
    let full = Path::new(base).join(rel);
    let src = std::fs::read_to_string(&full).map_err(|e| format!("{}: {e}", full.display()))?;
    files.insert(rel.to_string(), src.clone().into_bytes());

    let file = crate::parser::parse(&src)
        .map_err(|ds| format!("{rel}: {}", ds.first().map(|d| d.to_string()).unwrap_or_default()))?;
    let rel_dir = Path::new(rel).parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
    for im in &file.imports {
        let child_rel = normalize(&format!("{rel_dir}/{}", im.path));
        collect_files(&child_rel, base, files, depth + 1)?;
    }
    // recorded takes ride along as bytes (a song with vocals must be
    // publishable — the take IS the point of a performance fork)
    for asset in &file.assets {
        let child_rel = normalize(&format!("{rel_dir}/{}", asset.path));
        if let std::collections::btree_map::Entry::Vacant(e) = files.entry(child_rel) {
            let bytes = std::fs::read(Path::new(base).join(e.key()))
                .map_err(|err| format!("{}: {err}", e.key()))?;
            e.insert(bytes);
        }
    }
    Ok(())
}

fn normalize(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

pub struct ExportInfo {
    pub bytes: Vec<u8>,
    pub files: usize,
    pub history_objects: usize,
    pub digest: Option<String>,
}

pub fn export(entry: &str) -> Result<ExportInfo, String> {
    let (entry_name, files) = collect_snapshot(entry)?;
    let base_dir = Path::new(entry)
        .parent()
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .into_owned();

    // sources first, sorted (BTreeMap order) — deterministic layout
    let mut entries: Vec<(String, Vec<u8>)> =
        files.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    // build manifest: the export carries its own proof
    let src = String::from_utf8_lossy(&files[&entry_name]).into_owned();
    let map_base =
        Path::new(&entry_name).parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
    let mut digest = None;
    if let Ok(crate::Checked::Song(project)) =
        crate::check_with_loader(&src, &crate::semdiff::SnapLoader(&files), &map_base)
    {
        let info = crate::render_digest(&project, 8.0);
        let d = format!("{:016x}", info.f32_digest);
        entries.push((
            "export.manifest.json".into(),
            serde_json::to_vec_pretty(&serde_json::json!({
                "forte_export": 0,
                "entry": entry_name,
                "render": {
                    "sample_rate": 48000,
                    "seconds": info.seconds,
                    "f32_digest_fnv1a64": d,
                },
            }))
            .unwrap(),
        ));
        digest = Some(d);
    }

    // the past comes along: a clean repository exports its whole history
    let mut history_objects = 0;
    if let Ok(repo) = Repo::open(if base_dir.is_empty() { "." } else { &base_dir }) {
        if repo.is_clean().unwrap_or(false) {
            if let (Ok(Some(head)), Ok(Some(branch))) = (repo.head(), repo.current_branch()) {
                let mut hashes = repo.reachable(&head)?;
                hashes.sort();
                for hash in hashes {
                    entries.push((
                        format!(".forte/objects/{}/{}", &hash[..2], &hash[2..]),
                        repo.object_raw(&hash)?,
                    ));
                    history_objects += 1;
                }
                entries.push((format!(".forte/refs/heads/{branch}"), format!("{head}\n").into_bytes()));
                entries.push((".forte/HEAD".into(), format!("ref: {branch}\n").into_bytes()));
            }
        }
    }

    Ok(ExportInfo { bytes: zip::write(&entries), files: files.len(), history_objects, digest })
}
