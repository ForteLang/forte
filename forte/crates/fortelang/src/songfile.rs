//! `.fortesong` — the listener-side build format (issue #53).
//!
//! Not a WAV: a self-contained, *playable* build. Sources (entry + import
//! closure), recorded assets, and the build proof travel together in one
//! zip container, so a listener with forte installed hears the exact
//! deterministic render — and can open the code, fork it, and rework it.
//!
//! Layout inside the zip: the source snapshot at its relative paths, plus
//! `fortesong.manifest.json` with the entry name, the piece's own meta
//! (desc/tags/license/artist), the render digest, and a digest over the
//! source files themselves (checked at load; tampering fails fast).

use std::collections::BTreeMap;
use std::path::Path;

use crate::semdiff::SnapLoader;
use crate::zip;

const MANIFEST: &str = "fortesong.manifest.json";

/// A loaded (and files-digest-verified) .fortesong.
pub struct SongFile {
    pub entry: String,
    pub name: String,
    pub desc: String,
    pub artist: String,
    pub seconds: f64,
    pub render_digest: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// FNV-1a 64 over sorted (relative path, bytes) — same hash family as the
/// render digest.
fn files_digest(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut update = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (path, bytes) in files {
        update(path.as_bytes());
        update(&[0]);
        update(bytes);
        update(&[0]);
    }
    format!("{h:016x}")
}

fn base_dir(entry: &str) -> String {
    Path::new(entry).parent().unwrap_or(Path::new("")).to_string_lossy().into_owned()
}

/// Collect the entry's import/asset closure by ABSOLUTE path (unlike
/// `forte export`, imports may climb with `../` — package songs import
/// `../instruments/…`), then rebase everything onto the files' deepest
/// common ancestor so the snapshot keeps its real directory shape
/// (`songs/x.forte`, `instruments/y.forte`).
fn collect_rebased(entry: &str) -> Result<(String, BTreeMap<String, Vec<u8>>), String> {
    use std::path::PathBuf;
    fn walk(
        abs: &Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 16 {
            return Err("import が深すぎます(循環?)".into());
        }
        if files.contains_key(abs) {
            return Ok(());
        }
        let src = std::fs::read_to_string(abs).map_err(|e| format!("{}: {e}", abs.display()))?;
        files.insert(abs.to_path_buf(), src.clone().into_bytes());
        let file = crate::parser::parse(&src).map_err(|ds| {
            format!("{}: {}", abs.display(), ds.first().map(|d| d.to_string()).unwrap_or_default())
        })?;
        let dir = abs.parent().unwrap_or(Path::new(""));
        for im in &file.imports {
            let child = dir
                .join(&im.path)
                .canonicalize()
                .map_err(|e| format!("{}: {e}", im.path))?;
            walk(&child, files, depth + 1)?;
        }
        for a in &file.assets {
            let child =
                dir.join(&a.path).canonicalize().map_err(|e| format!("{}: {e}", a.path))?;
            if let std::collections::btree_map::Entry::Vacant(e) = files.entry(child) {
                let bytes =
                    std::fs::read(e.key()).map_err(|err| format!("{}: {err}", e.key().display()))?;
                e.insert(bytes);
            }
        }
        Ok(())
    }

    let entry_abs = Path::new(entry)
        .canonicalize()
        .map_err(|e| format!("{entry}: {e}"))?;
    let mut abs_files = BTreeMap::new();
    walk(&entry_abs, &mut abs_files, 0)?;

    // deepest common ancestor of every file's parent directory
    let mut root: PathBuf = entry_abs.parent().unwrap_or(Path::new("/")).to_path_buf();
    for p in abs_files.keys() {
        while !p.starts_with(&root) {
            root = root.parent().map(Path::to_path_buf).unwrap_or_default();
        }
    }
    let rel = |p: &Path| -> String {
        p.strip_prefix(&root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let entry_rel = rel(&entry_abs);
    let files: BTreeMap<String, Vec<u8>> =
        abs_files.into_iter().map(|(p, b)| (rel(&p), b)).collect();
    Ok((entry_rel, files))
}

fn compile_snapshot(
    entry: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<dawcore::model::Project, String> {
    let src = String::from_utf8_lossy(files.get(entry).ok_or("entry がスナップショットにありません")?)
        .into_owned();
    crate::compile_with_loader(&src, &SnapLoader(files), &base_dir(entry))
        .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
}

/// `forte build song.forte -o name.fortesong` — snapshot, render for the
/// proof, and pack. Returns (bytes, human summary).
pub fn build(entry: &str) -> Result<(Vec<u8>, String), String> {
    let (entry_name, files) = collect_rebased(entry)?;
    let project = compile_snapshot(&entry_name, &files)?;
    let info = crate::render_digest(&project, 8.0);
    let digest = format!("{:016x}", info.f32_digest);

    // artist comes from the entry's root meta (parse only; engine unaware)
    let artist = crate::parser::parse(&String::from_utf8_lossy(&files[&entry_name]))
        .ok()
        .and_then(|f| {
            f.song
                .as_ref()
                .and_then(|s| s.artist.clone())
                .or_else(|| f.blocks.last().and_then(|b| b.body.artist.clone()))
        })
        .unwrap_or_default();

    let manifest = serde_json::json!({
        "fortesong": 1,
        "entry": entry_name,
        "name": project.name,
        "desc": project.desc,
        "tags": project.tags,
        "license": project.license,
        "artist": artist,
        "render": {
            "sample_rate": 48000,
            "seconds": info.seconds,
            "tempo": project.tempo,
            "len_beats": dawcore::bounce::arrangement_len(&project),
            "f32_digest_fnv1a64": digest,
        },
        "files_digest_fnv1a64": files_digest(&files),
    });

    let mut entries: Vec<(String, Vec<u8>)> =
        files.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    entries.push((MANIFEST.into(), serde_json::to_vec_pretty(&manifest).unwrap()));
    let summary = format!(
        "{} sources, {:.1}s, digest {digest}",
        files.len(),
        info.seconds
    );
    Ok((zip::write(&entries), summary))
}

/// Open a .fortesong: unpack, verify the files digest (tamper check), and
/// hand back sources + meta. Rendering proof is checked by [`verify`].
pub fn load(path: &str) -> Result<SongFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let entries = zip::read(&bytes)?;
    let mut files = BTreeMap::new();
    let mut manifest: Option<serde_json::Value> = None;
    for (name, data) in entries {
        if name == MANIFEST {
            manifest = Some(
                serde_json::from_slice(&data).map_err(|e| format!("manifest を読めません: {e}"))?,
            );
        } else {
            files.insert(name, data);
        }
    }
    let m = manifest.ok_or("fortesong.manifest.json がありません(.fortesong ではない?)")?;
    let expect = m["files_digest_fnv1a64"].as_str().unwrap_or_default().to_string();
    let actual = files_digest(&files);
    if !expect.is_empty() && expect != actual {
        return Err(format!(
            "ソースが改竄されています: manifest {expect} ≠ 実体 {actual}"
        ));
    }
    Ok(SongFile {
        entry: m["entry"].as_str().unwrap_or_default().to_string(),
        name: m["name"].as_str().unwrap_or_default().to_string(),
        desc: m["desc"].as_str().unwrap_or_default().to_string(),
        artist: m["artist"].as_str().unwrap_or_default().to_string(),
        seconds: m["render"]["seconds"].as_f64().unwrap_or(0.0),
        render_digest: m["render"]["f32_digest_fnv1a64"].as_str().unwrap_or_default().to_string(),
        files,
    })
}

/// Compile the packed sources into a playable project.
pub fn compile(sf: &SongFile) -> Result<dawcore::model::Project, String> {
    compile_snapshot(&sf.entry, &sf.files)
}

/// Re-render and compare against the packed proof — "is this audio really
/// this code?" answered on the listener's own machine.
pub fn verify(sf: &SongFile) -> Result<String, String> {
    let project = compile(sf)?;
    let info = crate::render_digest(&project, 8.0);
    let actual = format!("{:016x}", info.f32_digest);
    if actual == sf.render_digest {
        Ok(format!("OK: 再現できました(digest {actual})"))
    } else {
        Err(format!("MISMATCH: manifest {} ≠ 再レンダー {actual}", sf.render_digest))
    }
}

/// An album: a directory holding `album.forte` (meta) + `*.fortesong`
/// tracks, ordered by filename (01- 02- convention).
pub struct Album {
    pub title: String,
    pub desc: String,
    pub artist: String,
    pub tracks: Vec<std::path::PathBuf>,
}

/// Detect + load an album directory. `Ok(None)` when `dir` is not an album.
pub fn load_album(dir: &Path) -> Result<Option<Album>, String> {
    let meta_path = dir.join("album.forte");
    if !meta_path.is_file() {
        return Ok(None);
    }
    let src = std::fs::read_to_string(&meta_path).map_err(|e| e.to_string())?;
    let ast = crate::parser::parse(&src)
        .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))?;
    let meta = ast.blocks.last().map(|b| &b.body).or(ast.song.as_ref());
    let (title, desc, artist) = meta
        .map(|s| {
            (
                s.name.clone(),
                s.desc.clone().unwrap_or_default(),
                s.artist.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    let mut tracks: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("fortesong"))
        .collect();
    tracks.sort();
    if tracks.is_empty() {
        return Err(format!("{} に .fortesong がありません", dir.display()));
    }
    Ok(Some(Album { title, desc, artist, tracks }))
}
