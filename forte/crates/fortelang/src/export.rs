//! `forte export` — data portability (SRS-WEB-005). One self-contained,
//! deterministic zip: the sources (entry + imports + recorded takes), a build
//! manifest with the audio digest, and — when the song lives in a clean
//! repository — the full `.forte/` history. Unzip anywhere and you have the
//! song, its proof, and its past. No lock-in.

use std::path::Path;

use crate::vcs::Repo;
use crate::zip;

pub struct ExportInfo {
    pub bytes: Vec<u8>,
    pub files: usize,
    pub history_objects: usize,
    pub digest: Option<String>,
}

pub fn export(entry: &str) -> Result<ExportInfo, String> {
    let (entry_name, files) = crate::hub::collect_snapshot(entry)?;
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
