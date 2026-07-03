//! Local Hub: a file-based registry that implements the ecosystem's two core
//! rules before any server exists (SYS-HUB-002/003 prototype):
//!
//! 1. **Retrieval is fork-only.** There is no download/clone command; `fork`
//!    copies the files out *and* records a lineage event, and it stamps the
//!    copy with `.forte-lineage.json` so a later `publish` of the fork records
//!    `forked_from` — provenance by construction.
//! 2. **Publishing snapshots the transitive local imports**, so a hub entry
//!    is always self-contained and checkable.
//!
//! The registry uses monotonically increasing sequence numbers, not wall-clock
//! time, so hub state stays deterministic and diffable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LINEAGE_FILE: &str = ".forte-lineage.json";

#[derive(Serialize, Deserialize, Default)]
pub struct Registry {
    pub seq: u64,
    pub repos: BTreeMap<String, Repo>,
    pub events: Vec<Event>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Repo {
    pub versions: Vec<Version>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Version {
    pub v: u32,
    pub seq: u64,
    pub author: String,
    /// "song" or "library"
    pub kind: String,
    /// devices defined locally in the entry (library exports)
    pub devices: Vec<String>,
    /// rel path -> fnv1a64 content hash
    pub files: BTreeMap<String, String>,
    pub forked_from: Option<Origin>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Origin {
    pub repo: String,
    pub v: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    /// "publish" | "fork"
    pub kind: String,
    pub repo: String,
    pub v: u32,
    pub by: String,
}

pub struct Hub {
    root: PathBuf,
}

fn author() -> String {
    std::env::var("FORTE_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "anonymous".into())
}

impl Hub {
    pub fn open(root: &str) -> Result<Hub, String> {
        let root = PathBuf::from(root);
        std::fs::create_dir_all(root.join("store")).map_err(|e| e.to_string())?;
        Ok(Hub { root })
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("registry.json")
    }

    pub fn registry(&self) -> Result<Registry, String> {
        match std::fs::read_to_string(self.registry_path()) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
            Err(_) => Ok(Registry::default()),
        }
    }

    fn save(&self, reg: &Registry) -> Result<(), String> {
        std::fs::write(
            self.registry_path(),
            serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    /// Publish a `.forte` file (song or device library) plus its transitive
    /// local imports as a new version of `name`.
    pub fn publish(&self, entry: &str, name: Option<&str>) -> Result<String, String> {
        let entry_path = Path::new(entry);
        let base = entry_path.parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
        let file_name = entry_path
            .file_name()
            .ok_or("ファイル名がありません")?
            .to_string_lossy()
            .into_owned();
        let src = std::fs::read_to_string(entry).map_err(|e| format!("{entry}: {e}"))?;

        // must compile / validate before it can be published
        let checked = crate::check_with_loader(&src, &crate::FsLoader, &base)
            .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))?;
        let kind = match &checked {
            crate::Checked::Song(_) => "song",
            crate::Checked::DeviceLibrary { .. } => "library",
        };
        let file = crate::parser::parse(&src).map_err(|_| "parse".to_string())?;
        let devices: Vec<String> = file.devices.iter().map(|d| d.name.clone()).collect();

        // snapshot the entry + transitive local imports (self-contained repo)
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        collect_files(&file_name, &base, &mut files, 0)?;

        let name = name.unwrap_or(
            entry_path.file_stem().ok_or("ファイル名がありません")?.to_str().unwrap_or("song"),
        );
        let forked_from: Option<Origin> = std::fs::read_to_string(
            entry_path.parent().unwrap_or(Path::new("")).join(LINEAGE_FILE),
        )
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

        let mut reg = self.registry()?;
        reg.seq += 1;
        let repo = reg.repos.entry(name.to_string()).or_default();
        let v = repo.versions.len() as u32 + 1;

        let mut hashes = BTreeMap::new();
        for (rel, content) in &files {
            let dest = self.root.join("store").join(name).join(format!("v{v}")).join(rel);
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&dest, content).map_err(|e| e.to_string())?;
            hashes.insert(rel.clone(), format!("{:016x}", crate::fnv1a64(content.as_bytes())));
        }

        repo.versions.push(Version {
            v,
            seq: reg.seq,
            author: author(),
            kind: kind.into(),
            devices,
            files: hashes,
            forked_from: forked_from.clone(),
        });
        reg.events.push(Event { seq: reg.seq, kind: "publish".into(), repo: name.into(), v, by: author() });
        self.save(&reg)?;

        let lineage_note = forked_from
            .map(|o| format!("(forked from {} v{})", o.repo, o.v))
            .unwrap_or_default();
        Ok(format!("published: {name} v{v} [{kind}, {} files] {lineage_note}", files.len()))
    }

    /// The only way to take content out of the hub. Copies the latest version
    /// into `dest` and records the fork in the lineage ledger.
    pub fn fork(&self, name: &str, dest: &str) -> Result<String, String> {
        let mut reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let ver = repo.versions.last().ok_or("バージョンがありません")?.clone();

        let dest_dir = Path::new(dest);
        if dest_dir.exists() && dest_dir.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true)
        {
            return Err(format!("{dest} は空ではありません"));
        }
        std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;

        let src_dir = self.root.join("store").join(name).join(format!("v{}", ver.v));
        for rel in ver.files.keys() {
            let content = std::fs::read_to_string(src_dir.join(rel)).map_err(|e| e.to_string())?;
            let out = dest_dir.join(rel);
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(out, content).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            dest_dir.join(LINEAGE_FILE),
            serde_json::to_string_pretty(&Origin { repo: name.into(), v: ver.v }).unwrap(),
        )
        .map_err(|e| e.to_string())?;

        reg.seq += 1;
        reg.events.push(Event { seq: reg.seq, kind: "fork".into(), repo: name.into(), v: ver.v, by: author() });
        self.save(&reg)?;
        Ok(format!("forked: {name} v{} -> {dest} (系譜に記録済み)", ver.v))
    }

    /// Human-readable lineage: ancestry chain, forks of this repo, dependents.
    pub fn lineage(&self, name: &str) -> Result<String, String> {
        let reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let latest = repo.versions.last().ok_or("バージョンがありません")?;

        let mut out = String::new();
        out.push_str(&format!(
            "{name} v{} [{}] by {} — devices: {}\n",
            latest.v,
            latest.kind,
            latest.author,
            if latest.devices.is_empty() { "-".into() } else { latest.devices.join(", ") }
        ));

        // ancestry (forked_from chain)
        let mut origin = latest.forked_from.clone();
        let mut depth = 1;
        while let Some(o) = origin {
            out.push_str(&format!("{}└─ forked from: {} v{}\n", "  ".repeat(depth), o.repo, o.v));
            origin = reg
                .repos
                .get(&o.repo)
                .and_then(|r| r.versions.iter().find(|ver| ver.v == o.v))
                .and_then(|ver| ver.forked_from.clone());
            depth += 1;
        }

        // descendants: repos whose some version forked from this repo
        let mut kids = Vec::new();
        for (other, r) in &reg.repos {
            for ver in &r.versions {
                if let Some(o) = &ver.forked_from {
                    if o.repo == name {
                        kids.push(format!("{other} v{} (from v{})", ver.v, o.v));
                    }
                }
            }
        }
        if !kids.is_empty() {
            out.push_str(&format!("forks -> {}\n", kids.join(", ")));
        }
        let fork_events =
            reg.events.iter().filter(|e| e.kind == "fork" && e.repo == name).count();
        out.push_str(&format!("fork events: {fork_events}\n"));
        Ok(out)
    }

    pub fn list(&self) -> Result<String, String> {
        let reg = self.registry()?;
        if reg.repos.is_empty() {
            return Ok("(empty hub)".into());
        }
        let mut out = String::new();
        for (name, repo) in &reg.repos {
            if let Some(v) = repo.versions.last() {
                out.push_str(&format!("{name}\tv{}\t[{}]\tby {}\n", v.v, v.kind, v.author));
            }
        }
        Ok(out)
    }
}

/// Collect `rel` (relative to the entry's base dir) and its transitive local
/// imports into `files`.
fn collect_files(
    rel: &str,
    base: &str,
    files: &mut BTreeMap<String, String>,
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
    files.insert(rel.to_string(), src.clone());

    let file = crate::parser::parse(&src)
        .map_err(|ds| format!("{rel}: {}", ds.first().map(|d| d.to_string()).unwrap_or_default()))?;
    let rel_dir = Path::new(rel).parent().unwrap_or(Path::new("")).to_string_lossy().into_owned();
    for im in &file.imports {
        let child_rel = normalize(&format!("{rel_dir}/{}", im.path));
        collect_files(&child_rel, base, files, depth + 1)?;
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
