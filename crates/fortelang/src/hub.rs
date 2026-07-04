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
    #[serde(default)]
    pub releases: Vec<Release>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Version {
    pub v: u32,
    pub seq: u64,
    pub author: String,
    /// "song" or "library"
    pub kind: String,
    /// the entry file within `files` (what compiles/builds)
    #[serde(default)]
    pub entry: String,
    /// devices defined locally in the entry (library exports)
    pub devices: Vec<String>,
    /// rel path -> fnv1a64 content hash
    pub files: BTreeMap<String, String>,
    pub forked_from: Option<Origin>,
    /// transposition-invariant chord-progression signatures found in the song
    #[serde(default)]
    pub progressions: Vec<String>,
    /// VCS commit this version was published from (None: no repo / dirty tree).
    /// When present, the full history is in store/<name>/objects and forks
    /// receive it.
    #[serde(default)]
    pub commit: Option<String>,
    /// non-builtin device names this song plays (cross-module dig:
    /// 「この楽器を使う曲」 is answered from these)
    #[serde(default)]
    pub uses: Vec<String>,
}

/// A deterministic build of a song version: anyone can `verify` that the
/// stored audio digest reproduces from the stored source (SRS-HUB-004 local).
#[derive(Serialize, Deserialize, Clone)]
pub struct Release {
    pub v: u32,
    pub seq: u64,
    pub digest: String, // fnv1a64 of the f32 sample stream
    pub seconds: f64,
    pub by: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Origin {
    pub repo: String,
    pub v: u32,
    /// exact commit forked from (when the origin published with history)
    #[serde(default)]
    pub commit: Option<String>,
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

        // snapshot the entry + transitive local imports and recorded takes
        let (_, files) = collect_snapshot(entry)?;

        let name = name
            .unwrap_or(entry_path.file_stem().ok_or("ファイル名がありません")?.to_str().unwrap_or("song"))
            .to_string();
        let mut msg = self.publish_map(&file_name, files, &name, None)?;

        // if the song lives in a VCS repo with a clean tree, the publish
        // carries the full history (this is what makes a hub fork a real fork)
        let vcs_head = crate::vcs::Repo::open(if base.is_empty() { "." } else { &base })
            .ok()
            .filter(|r| r.is_clean().unwrap_or(false))
            .and_then(|r| r.head().ok().flatten().map(|h| (r, h)));
        if let Some((vcs_repo, head)) = &vcs_head {
            let objdir = self.root.join("store").join(&name).join("objects");
            let copied = vcs_repo.export_objects(head, &objdir)?;
            let mut reg = self.registry()?;
            if let Some(v) = reg.repos.get_mut(&name).and_then(|r| r.versions.last_mut()) {
                v.commit = Some(head.clone());
            }
            self.save(&reg)?;
            msg.push_str(&format!(" 履歴 push: {} ({copied} objects)", &head[..8]));
        }
        Ok(msg)
    }

    /// Publish from an in-memory file map (the browser editor posts these).
    /// `files` keys are entry-relative paths; recorded takes are raw bytes.
    pub fn publish_map(
        &self,
        entry_name: &str,
        files: BTreeMap<String, Vec<u8>>,
        name: &str,
        author_override: Option<&str>,
    ) -> Result<String, String> {
        let src = String::from_utf8_lossy(
            files.get(entry_name).ok_or_else(|| format!("{entry_name} がありません"))?,
        )
        .into_owned();
        let base = Path::new(entry_name)
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned();

        // must compile / validate before it can be published — imports and
        // takes resolve from the snapshot itself (the clean room)
        let checked = crate::check_with_loader(&src, &crate::semdiff::SnapLoader(&files), &base)
            .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))?;
        let kind = match &checked {
            crate::Checked::Song(_) => "song",
            crate::Checked::DeviceLibrary { .. } => "library",
        };
        let file = crate::parser::parse(&src).map_err(|_| "parse".to_string())?;
        let devices: Vec<String> = file.devices.iter().map(|d| d.name.clone()).collect();
        let progressions = extract_progressions(&file);
        let uses = extract_uses(&file);
        let forked_from: Option<Origin> = files
            .get(LINEAGE_FILE)
            .and_then(|b| serde_json::from_slice(b).ok());
        let by = author_override.map(String::from).unwrap_or_else(author);

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
            hashes.insert(rel.clone(), format!("{:016x}", crate::fnv1a64(content)));
        }

        repo.versions.push(Version {
            v,
            seq: reg.seq,
            author: by.clone(),
            kind: kind.into(),
            entry: entry_name.to_string(),
            devices,
            files: hashes,
            forked_from: forked_from.clone(),
            progressions,
            commit: None,
            uses,
        });
        reg.events.push(Event { seq: reg.seq, kind: "publish".into(), repo: name.into(), v, by });
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

        let origin = Origin { repo: name.into(), v: ver.v, commit: ver.commit.clone() };
        let objects = self.root.join("store").join(name).join("objects");
        let history_note = if let (Some(head), true) = (&ver.commit, objects.is_dir()) {
            // real fork: the whole history moves in, and the provenance stamp
            // itself becomes a commit — lineage is part of the history
            let vrepo = crate::vcs::Repo::clone_into(dest, &objects, head)?;
            std::fs::write(
                dest_dir.join(LINEAGE_FILE),
                serde_json::to_string_pretty(&origin).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            vrepo.commit(&format!("fork {name} v{}", ver.v))?;
            format!("、履歴ごと({} から)", &head[..8])
        } else {
            // no history published: plain snapshot + stamp
            let src_dir = self.root.join("store").join(name).join(format!("v{}", ver.v));
            for rel in ver.files.keys() {
                let content = std::fs::read(src_dir.join(rel)).map_err(|e| e.to_string())?;
                let out = dest_dir.join(rel);
                if let Some(dir) = out.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(out, content).map_err(|e| e.to_string())?;
            }
            std::fs::write(
                dest_dir.join(LINEAGE_FILE),
                serde_json::to_string_pretty(&origin).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            String::new()
        };

        reg.seq += 1;
        reg.events.push(Event { seq: reg.seq, kind: "fork".into(), repo: name.into(), v: ver.v, by: author() });
        self.save(&reg)?;
        Ok(format!("forked: {name} v{} -> {dest} (系譜に記録済み{history_note})", ver.v))
    }

    /// Compile the stored snapshot of `name` (latest version). The hub store
    /// is the clean room: only files that were published exist there.
    fn build_snapshot(&self, name: &str, ver: &Version) -> Result<crate::RenderInfo, String> {
        if ver.kind != "song" {
            return Err(format!("'{name}' は {} です(release できるのは song)", ver.kind));
        }
        let dir = self.root.join("store").join(name).join(format!("v{}", ver.v));
        let entry = if ver.entry.is_empty() {
            return Err("エントリファイルが記録されていません(旧形式)".into());
        } else {
            dir.join(&ver.entry)
        };
        let src = std::fs::read_to_string(&entry).map_err(|e| e.to_string())?;
        let project =
            crate::compile_with_loader(&src, &crate::FsLoader, &dir.to_string_lossy())
                .map_err(|ds| ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))?;
        Ok(crate::render_digest(&project, 8.0))
    }

    /// Deterministically build the latest version of a song and record the
    /// audio digest in the ledger. The digest is the release's identity.
    pub fn release(&self, name: &str) -> Result<String, String> {
        let mut reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let ver = repo.versions.last().ok_or("バージョンがありません")?.clone();
        let info = self.build_snapshot(name, &ver)?;
        let digest = format!("{:016x}", info.f32_digest);

        // manifest lives next to the snapshot (the audio itself can always be
        // regenerated from it — that's the point)
        let dir = self.root.join("store").join(name).join(format!("v{}", ver.v));
        let manifest = serde_json::json!({
            "repo": name, "v": ver.v, "digest": digest,
            "seconds": info.seconds, "engine": env!("CARGO_PKG_VERSION"),
        });
        std::fs::write(
            dir.join("release.manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .map_err(|e| e.to_string())?;

        reg.seq += 1;
        let seq = reg.seq;
        let repo = reg.repos.get_mut(name).unwrap();
        repo.releases.push(Release {
            v: ver.v,
            seq,
            digest: digest.clone(),
            seconds: info.seconds,
            by: author(),
        });
        reg.events.push(Event { seq, kind: "release".into(), repo: name.into(), v: ver.v, by: author() });
        self.save(&reg)?;
        Ok(format!("released: {name} v{} — digest {digest} ({:.1}s)", ver.v, info.seconds))
    }

    /// Clean-room reproduction: rebuild the stored source and compare the
    /// digest with the ledger. Anyone can audit a release this way.
    pub fn verify(&self, name: &str) -> Result<String, String> {
        let mut reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let rel = repo.releases.last().ok_or_else(|| format!("'{name}' に release がありません"))?.clone();
        let ver = repo
            .versions
            .iter()
            .find(|v| v.v == rel.v)
            .ok_or("リリース対象のバージョンがありません")?
            .clone();
        let info = self.build_snapshot(name, &ver)?;
        let digest = format!("{:016x}", info.f32_digest);

        reg.seq += 1;
        let seq = reg.seq;
        reg.events.push(Event { seq, kind: "verify".into(), repo: name.into(), v: rel.v, by: author() });
        self.save(&reg)?;

        if digest == rel.digest {
            Ok(format!("VERIFIED: {name} v{} はソースから再現一致({digest})", rel.v))
        } else {
            Err(format!(
                "MISMATCH: {name} v{} — 台帳 {} / 再ビルド {digest}(ソースかエンジンが改竄・変更されています)",
                rel.v, rel.digest
            ))
        }
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
            let at = o.commit.as_deref().map(|c| format!(" @ {}", &c[..8])).unwrap_or_default();
            out.push_str(&format!("{}└─ forked from: {} v{}{at}\n", "  ".repeat(depth), o.repo, o.v));
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
        for rel in &repo.releases {
            let verified = reg
                .events
                .iter()
                .filter(|e| e.kind == "verify" && e.repo == name && e.v == rel.v)
                .count();
            out.push_str(&format!(
                "release v{}: digest {} ({:.1}s, verified {}回)\n",
                rel.v, rel.digest, rel.seconds, verified
            ));
        }
        let fork_events =
            reg.events.iter().filter(|e| e.kind == "fork" && e.repo == name).count();
        out.push_str(&format!("fork events: {fork_events}\n"));
        Ok(out)
    }

    /// Songs sharing at least one progression signature with `name` —
    /// key-independent, because signatures are transposition-invariant.
    pub fn similar(&self, name: &str) -> Result<Vec<(String, String)>, String> {
        let reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let mine = &repo.versions.last().ok_or("バージョンがありません")?.progressions;
        let mut out = Vec::new();
        for (other, r) in &reg.repos {
            if other == name {
                continue;
            }
            if let Some(v) = r.versions.last() {
                if let Some(shared) = v.progressions.iter().find(|p| mine.contains(p)) {
                    out.push((other.clone(), shared.clone()));
                }
            }
        }
        Ok(out)
    }

    /// Ledger a listen (SRS-HUB-007: events only — the economy comes later,
    /// but only because this data exists from day one).
    pub fn play_event(&self, name: &str, by: &str) -> Result<u64, String> {
        let mut reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let v = repo.versions.last().ok_or("バージョンがありません")?.v;
        reg.seq += 1;
        reg.events.push(Event {
            seq: reg.seq,
            kind: "play".into(),
            repo: name.into(),
            v,
            by: if by.is_empty() { "anonymous".into() } else { by.into() },
        });
        let plays = reg.events.iter().filter(|e| e.kind == "play" && e.repo == name).count() as u64;
        self.save(&reg)?;
        Ok(plays)
    }

    // ---- JSON views for the HTTP API / browser hub page --------------------

    pub fn repos_json(&self) -> Result<serde_json::Value, String> {
        let reg = self.registry()?;
        let repos: Vec<serde_json::Value> = reg
            .repos
            .iter()
            .filter_map(|(name, r)| {
                let v = r.versions.last()?;
                Some(serde_json::json!({
                    "name": name, "v": v.v, "kind": v.kind, "author": v.author,
                    "devices": v.devices,
                    "releases": r.releases.len(),
                    "forked_from": v.forked_from,
                }))
            })
            .collect();
        Ok(serde_json::json!({ "repos": repos }))
    }

    /// The fork forest: every repo as a node under the repo it forked from —
    /// the listener's family tree of the music (Phase 4 dig experience).
    pub fn lineage_forest(&self) -> Result<serde_json::Value, String> {
        let reg = self.registry()?;
        // name -> children names (a fork points at its origin's name)
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (name, r) in &reg.repos {
            if let Some(origin) = r.versions.last().and_then(|v| v.forked_from.as_ref()) {
                if reg.repos.contains_key(&origin.repo) && origin.repo != *name {
                    children.entry(origin.repo.as_str()).or_default().push(name);
                }
            }
        }
        let plays = |name: &str| {
            reg.events.iter().filter(|e| e.kind == "play" && e.repo == name).count()
        };
        fn node(
            reg: &Registry,
            children: &BTreeMap<&str, Vec<&str>>,
            plays: &dyn Fn(&str) -> usize,
            name: &str,
            seen: &mut std::collections::BTreeSet<String>,
        ) -> serde_json::Value {
            if !seen.insert(name.to_string()) {
                return serde_json::json!({ "name": name, "cycle": true, "children": [] });
            }
            let r = &reg.repos[name];
            let v = r.versions.last();
            let kids: Vec<serde_json::Value> = children
                .get(name)
                .map(|ks| ks.iter().map(|k| node(reg, children, plays, k, seen)).collect())
                .unwrap_or_default();
            serde_json::json!({
                "name": name,
                "v": v.map(|v| v.v).unwrap_or(0),
                "kind": v.map(|v| v.kind.clone()).unwrap_or_default(),
                "author": v.map(|v| v.author.clone()).unwrap_or_default(),
                "releases": r.releases.len(),
                "plays": plays(name),
                "children": kids,
            })
        }
        let mut seen = std::collections::BTreeSet::new();
        let roots: Vec<serde_json::Value> = reg
            .repos
            .iter()
            .filter(|(name, r)| {
                // a root either never forked, or forked from something unknown
                r.versions
                    .last()
                    .and_then(|v| v.forked_from.as_ref())
                    .map(|o| !reg.repos.contains_key(&o.repo) || o.repo == **name)
                    .unwrap_or(true)
            })
            .map(|(name, _)| node(&reg, &children, &plays, name, &mut seen))
            .collect();
        Ok(serde_json::json!({ "roots": roots }))
    }

    pub fn repo_json(&self, name: &str) -> Result<serde_json::Value, String> {
        let reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let latest = repo.versions.last().ok_or("バージョンがありません")?;
        let releases: Vec<serde_json::Value> = repo
            .releases
            .iter()
            .map(|rel| {
                let verified = reg
                    .events
                    .iter()
                    .filter(|e| e.kind == "verify" && e.repo == name && e.v == rel.v)
                    .count();
                serde_json::json!({
                    "v": rel.v, "digest": rel.digest, "seconds": rel.seconds,
                    "by": rel.by, "verified": verified,
                })
            })
            .collect();
        let forks: Vec<serde_json::Value> = reg
            .repos
            .iter()
            .flat_map(|(other, r)| {
                r.versions.iter().filter_map(move |ver| {
                    ver.forked_from.as_ref().filter(|o| o.repo == name).map(|o| {
                        serde_json::json!({ "name": other, "v": ver.v, "from_v": o.v })
                    })
                })
            })
            .collect();
        let fork_events =
            reg.events.iter().filter(|e| e.kind == "fork" && e.repo == name).count();
        let plays = reg.events.iter().filter(|e| e.kind == "play" && e.repo == name).count();
        let similar: Vec<serde_json::Value> = self
            .similar(name)
            .unwrap_or_default()
            .into_iter()
            .map(|(other, sig)| serde_json::json!({ "name": other, "progression": sig }))
            .collect();
        // cross-module dig: which songs play the devices this repo defines,
        // and which library defines each device this song plays
        let used_by: Vec<&String> = reg
            .repos
            .iter()
            .filter(|(other, r)| {
                *other != name
                    && r.versions.last().map_or(false, |v| {
                        v.uses.iter().any(|u| latest.devices.contains(u))
                    })
            })
            .map(|(other, _)| other)
            .collect();
        let device_sources: BTreeMap<&String, &String> = latest
            .uses
            .iter()
            .filter_map(|u| {
                reg.repos
                    .iter()
                    .find(|(other, r)| {
                        *other != name
                            && r.versions.last().map_or(false, |v| v.devices.contains(u))
                    })
                    .map(|(other, _)| (u, other))
            })
            .collect();
        Ok(serde_json::json!({
            "name": name, "v": latest.v, "kind": latest.kind, "author": latest.author,
            "entry": latest.entry, "devices": latest.devices,
            "forked_from": latest.forked_from,
            "forks": forks, "fork_events": fork_events, "releases": releases,
            "plays": plays, "similar": similar,
            "uses": latest.uses, "used_by": used_by, "device_sources": device_sources,
        }))
    }

    /// Latest snapshot's file contents (what the browser player compiles).
    /// (text files, binary assets as base64) of the latest version.
    pub fn snapshot_files(
        &self,
        name: &str,
    ) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>), String> {
        let reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let ver = repo.versions.last().ok_or("バージョンがありません")?;
        let dir = self.root.join("store").join(name).join(format!("v{}", ver.v));
        let mut text = BTreeMap::new();
        let mut assets = BTreeMap::new();
        for rel in ver.files.keys() {
            let bytes = std::fs::read(dir.join(rel)).map_err(|e| e.to_string())?;
            if rel.ends_with(".frec") {
                assets.insert(rel.clone(), base64_encode(&bytes));
            } else {
                text.insert(rel.clone(), String::from_utf8_lossy(&bytes).into_owned());
            }
        }
        Ok((text, assets))
    }

    /// Remote fork: record the ledger event and hand back the files plus the
    /// lineage stamp the client must keep with its copy.
    pub fn fork_remote(&self, name: &str, by: &str) -> Result<serde_json::Value, String> {
        let mut reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let ver = repo.versions.last().ok_or("バージョンがありません")?.clone();
        let (files, assets) = self.snapshot_files(name)?;
        reg.seq += 1;
        reg.events.push(Event {
            seq: reg.seq,
            kind: "fork".into(),
            repo: name.into(),
            v: ver.v,
            by: if by.is_empty() { "anonymous".into() } else { by.into() },
        });
        self.save(&reg)?;
        Ok(serde_json::json!({
            "origin": Origin { repo: name.into(), v: ver.v, commit: ver.commit.clone() },
            "entry": ver.entry,
            "files": files,
            "assets": assets,
        }))
    }

    /// Absolute path of the latest version's entry file inside the store —
    /// how Forte Studio's "Listen" plays a hub song without forking it.
    pub fn entry_path(&self, name: &str) -> Result<String, String> {
        let reg = self.registry()?;
        let repo = reg.repos.get(name).ok_or_else(|| format!("'{name}' は hub にありません"))?;
        let ver = repo.versions.last().ok_or("バージョンがありません")?;
        if ver.entry.is_empty() {
            return Err("エントリファイルが記録されていません(旧形式)".into());
        }
        let path = self.root.join("store").join(name).join(format!("v{}", ver.v)).join(&ver.entry);
        let abs = std::path::absolute(&path).map_err(|e| e.to_string())?;
        Ok(abs.to_string_lossy().into_owned())
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

/// Snapshot an entry from disk: the file itself, transitive local imports,
/// recorded takes and (if present) the lineage stamp. Returns
/// `(entry_name, path → bytes)` — the self-contained unit that publish and
/// `forte export` both operate on.
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

/// Non-builtin device names the song's tracks play (instruments + inserts).
fn extract_uses(file: &crate::ast::FileAst) -> Vec<String> {
    const BUILTIN: &[&str] =
        &["sampler", "polymer", "grid", "filter", "eq", "drive", "delay", "reverb"];
    let Some(song) = &file.song else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: &str| {
        if !BUILTIN.contains(&name) && !out.iter().any(|u| u == name) {
            out.push(name.to_string());
        }
    };
    for t in &song.tracks {
        if let Some(call) = &t.instrument {
            push(&call.name);
        }
        for ins in &t.inserts {
            push(&ins.name);
        }
    }
    out.sort();
    out
}

/// Pull the transposition-invariant signatures out of every prog literal in
/// the file (song lets and inline pattern literals).
fn extract_progressions(file: &crate::ast::FileAst) -> Vec<String> {
    let Some(song) = &file.song else { return Vec::new() };
    let beats_per_bar = song
        .meter
        .map(|((n, d), _)| n as f64 * 4.0 / d as f64)
        .unwrap_or(4.0);
    let pos = crate::diag::Pos { line: 1, col: 1 };
    let mut sigs: Vec<String> = Vec::new();
    let mut push = |lit: &crate::ast::PatternLit| {
        if lit.kind == "prog" {
            if let Ok((events, _)) = crate::music::parse_prog(&lit.raw, beats_per_bar, pos) {
                let sig = crate::music::prog_signature(&events);
                if !sig.is_empty() && !sigs.contains(&sig) {
                    sigs.push(sig);
                }
            }
        }
    };
    for l in &song.lets {
        push(&l.value);
    }
    for t in &song.tracks {
        for p in &t.plays {
            let mut pref = &p.pattern;
            loop {
                match pref {
                    crate::ast::PatternRef::Lit(l) => {
                        push(l);
                        break;
                    }
                    crate::ast::PatternRef::Fn { inner, .. } => pref = inner,
                    crate::ast::PatternRef::Name(..) => break,
                }
            }
        }
    }
    sigs
}

/// Collect `rel` (relative to the entry's base dir) and its transitive local
/// imports into `files`.
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
        if !files.contains_key(&child_rel) {
            let bytes = std::fs::read(Path::new(base).join(&child_rel))
                .map_err(|e| format!("{child_rel}: {e}"))?;
            files.insert(child_rel, bytes);
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

// ---------------------------------------------------------------------------
// base64 (standard alphabet, padded) — recorded takes cross HTTP/JSON as text
// ---------------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let raw: Vec<u8> = s.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    let mut out = Vec::with_capacity(raw.len() / 4 * 3);
    for chunk in raw.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let pads = chunk.iter().filter(|&&c| c == b'=').count();
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= if c == b'=' { 0 } else { val(c)? } << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 && pads < 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 && pads < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod b64_tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(37).wrapping_add(len as u8)).collect();
            assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data, "len {len}");
        }
        assert_eq!(base64_encode(b"forte"), "Zm9ydGU=");
    }
}
