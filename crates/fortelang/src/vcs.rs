//! Forte VCS — GitHub のようにバージョン管理ができる作曲ツール、の「バージョン
//! 管理」本体。git と同じ content-addressed オブジェクトストアを `.forte/` に
//! 持ち、blob(ファイル)/ tree(ディレクトリ)/ commit(スナップショット+
//! 親)を SHA-256 で指す。
//!
//! 設計メモ:
//! - 追跡対象は音楽のソースのみ: `*.forte` と `*.frec`。ビルド成果物
//!   (wav / manifest)は音のソースから再現できるので追跡しない。
//! - commit に壁時計は入れない(hub と同じ思想)。順序は親チェーンが持つ。
//! - checkout は作業ツリーがクリーンなときだけ(v1)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::sha::sha256_hex;

const DIR: &str = ".forte";

/// A snapshot of the tracked files: repo-root-relative path (with `/`
/// separators) → file bytes.
pub type Snapshot = BTreeMap<String, Vec<u8>>;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Commit {
    pub tree: String,
    pub parents: Vec<String>,
    pub author: String,
    pub message: String,
    /// 1-based length of the ancestor chain — a human-friendly "commit #".
    pub n: u64,
}

pub struct Repo {
    root: PathBuf,   // project root (the directory containing .forte/)
    store: PathBuf,  // .forte/
}

fn author() -> String {
    std::env::var("FORTE_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "anonymous".into())
}

impl Repo {
    /// Create `.forte/` in `dir`. Errors if one already exists.
    pub fn init(dir: &str) -> Result<String, String> {
        let store = Path::new(dir).join(DIR);
        if store.exists() {
            return Err(format!("{} は既にリポジトリです", dir));
        }
        std::fs::create_dir_all(store.join("objects")).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(store.join("refs/heads")).map_err(|e| e.to_string())?;
        std::fs::write(store.join("HEAD"), "ref: main\n").map_err(|e| e.to_string())?;
        Ok(format!("initialized: {}/{DIR} (ブランチ main)", dir))
    }

    /// Find the repo by walking up from `dir` (like git).
    pub fn open(dir: &str) -> Result<Repo, String> {
        let mut cur = std::path::absolute(dir).map_err(|e| e.to_string())?;
        loop {
            if cur.join(DIR).is_dir() {
                return Ok(Repo { store: cur.join(DIR), root: cur });
            }
            if !cur.pop() {
                return Err("リポジトリではありません(forte init で作成)".into());
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- object store ------------------------------------------------------

    fn object_path(&self, hash: &str) -> PathBuf {
        self.store.join("objects").join(&hash[..2]).join(&hash[2..])
    }

    fn put(&self, kind: &str, body: &[u8]) -> Result<String, String> {
        let mut data = format!("{kind} {}\0", body.len()).into_bytes();
        data.extend_from_slice(body);
        let hash = sha256_hex(&data);
        let path = self.object_path(&hash);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, &data).map_err(|e| e.to_string())?;
        }
        Ok(hash)
    }

    fn get(&self, hash: &str) -> Result<(String, Vec<u8>), String> {
        let data = std::fs::read(self.object_path(hash))
            .map_err(|_| format!("オブジェクト {} がありません", &hash[..hash.len().min(8)]))?;
        let nul = data.iter().position(|&b| b == 0).ok_or("壊れたオブジェクト")?;
        let header = String::from_utf8_lossy(&data[..nul]);
        let kind = header.split(' ').next().unwrap_or("").to_string();
        Ok((kind, data[nul + 1..].to_vec()))
    }

    // ---- snapshots ----------------------------------------------------------

    /// Read the tracked files (`*.forte`, `*.frec`) from the working tree.
    pub fn working_snapshot(&self) -> Result<Snapshot, String> {
        let mut snap = Snapshot::new();
        walk(&self.root, &self.root, &mut snap)?;
        Ok(snap)
    }

    /// Store a snapshot as nested trees; returns the root tree hash.
    fn write_tree(&self, snap: &Snapshot) -> Result<String, String> {
        // group into a nested structure by first path component
        #[derive(Default)]
        struct Node {
            files: BTreeMap<String, Vec<u8>>,
            dirs: BTreeMap<String, Node>,
        }
        let mut rootn = Node::default();
        for (path, bytes) in snap {
            let mut node = &mut rootn;
            let parts: Vec<&str> = path.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                node = node.dirs.entry(part.to_string()).or_default();
            }
            node.files.insert(parts[parts.len() - 1].to_string(), bytes.clone());
        }
        fn store(repo: &Repo, node: &Node) -> Result<String, String> {
            let mut entries: Vec<serde_json::Value> = Vec::new();
            for (name, bytes) in &node.files {
                let h = repo.put("blob", bytes)?;
                entries.push(serde_json::json!({"name": name, "kind": "blob", "hash": h}));
            }
            for (name, child) in &node.dirs {
                let h = store(repo, child)?;
                entries.push(serde_json::json!({"name": name, "kind": "tree", "hash": h}));
            }
            let body = serde_json::to_vec(&entries).map_err(|e| e.to_string())?;
            repo.put("tree", &body)
        }
        store(self, &rootn)
    }

    /// Load a tree object (recursively) back into a snapshot.
    pub fn read_tree(&self, tree_hash: &str) -> Result<Snapshot, String> {
        let mut snap = Snapshot::new();
        self.read_tree_into("", tree_hash, &mut snap)?;
        Ok(snap)
    }

    fn read_tree_into(&self, prefix: &str, hash: &str, snap: &mut Snapshot) -> Result<(), String> {
        let (kind, body) = self.get(hash)?;
        if kind != "tree" {
            return Err(format!("{} は tree ではありません", &hash[..8]));
        }
        let entries: Vec<serde_json::Value> =
            serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        for e in entries {
            let name = e["name"].as_str().unwrap_or_default();
            let h = e["hash"].as_str().unwrap_or_default();
            let path =
                if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
            match e["kind"].as_str() {
                Some("blob") => {
                    let (_, bytes) = self.get(h)?;
                    snap.insert(path, bytes);
                }
                Some("tree") => self.read_tree_into(&path, h, snap)?,
                _ => return Err("壊れた tree エントリ".into()),
            }
        }
        Ok(())
    }

    // ---- refs ---------------------------------------------------------------

    fn head_ref(&self) -> Result<Option<String>, String> {
        let head = std::fs::read_to_string(self.store.join("HEAD")).map_err(|e| e.to_string())?;
        Ok(head.strip_prefix("ref: ").map(|b| b.trim().to_string()))
    }

    fn branch_path(&self, name: &str) -> PathBuf {
        self.store.join("refs/heads").join(name)
    }

    pub fn branch_hash(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.branch_path(name)).ok().map(|s| s.trim().to_string())
    }

    /// Commit hash HEAD points at (None on an unborn branch).
    pub fn head(&self) -> Result<Option<String>, String> {
        match self.head_ref()? {
            Some(branch) => Ok(self.branch_hash(&branch)),
            None => {
                let raw =
                    std::fs::read_to_string(self.store.join("HEAD")).map_err(|e| e.to_string())?;
                Ok(Some(raw.trim().to_string()))
            }
        }
    }

    pub fn current_branch(&self) -> Result<Option<String>, String> {
        self.head_ref()
    }

    pub fn branches(&self) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        let dir = self.store.join("refs/heads");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(h) = self.branch_hash(&name) {
                    out.push((name, h));
                }
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn create_branch(&self, name: &str) -> Result<String, String> {
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c)) {
            return Err(format!("ブランチ名 '{name}' が不正です(英数字と -_. )"));
        }
        if self.branch_hash(name).is_some() {
            return Err(format!("ブランチ '{name}' は既にあります"));
        }
        let head = self.head()?.ok_or("まだコミットがありません")?;
        std::fs::write(self.branch_path(name), format!("{head}\n")).map_err(|e| e.to_string())?;
        Ok(format!("branch: {name} @ {}", &head[..8]))
    }

    // ---- commits ------------------------------------------------------------

    pub fn commit_obj(&self, hash: &str) -> Result<Commit, String> {
        let (kind, body) = self.get(hash)?;
        if kind != "commit" {
            return Err(format!("{} は commit ではありません", &hash[..hash.len().min(8)]));
        }
        serde_json::from_slice(&body).map_err(|e| e.to_string())
    }

    pub fn commit(&self, message: &str) -> Result<String, String> {
        if message.trim().is_empty() {
            return Err("コミットメッセージが空です(-m \"…\")".into());
        }
        let branch = self.head_ref()?.ok_or("HEAD がブランチを指していません(checkout <branch> で戻る)")?;
        let snap = self.working_snapshot()?;
        if snap.is_empty() {
            return Err("追跡対象(*.forte / *.frec)が見つかりません".into());
        }
        let tree = self.write_tree(&snap)?;
        let parent = self.branch_hash(&branch);
        if let Some(ref p) = parent {
            if self.commit_obj(p)?.tree == tree {
                return Err("変更がありません(nothing to commit)".into());
            }
        }
        let n = match &parent {
            Some(p) => self.commit_obj(p)?.n + 1,
            None => 1,
        };
        let c = Commit {
            tree,
            parents: parent.clone().into_iter().collect(),
            author: author(),
            message: message.to_string(),
            n,
        };
        let body = serde_json::to_vec(&c).map_err(|e| e.to_string())?;
        let hash = self.put("commit", &body)?;
        std::fs::write(self.branch_path(&branch), format!("{hash}\n")).map_err(|e| e.to_string())?;
        Ok(format!("[{branch} #{n} {}] {message} ({} files)", &hash[..8], snap.len()))
    }

    /// Resolve a revision: branch name, unique hash prefix, or HEAD.
    pub fn resolve(&self, rev: &str) -> Result<String, String> {
        if rev == "HEAD" {
            return self.head()?.ok_or("まだコミットがありません".into());
        }
        if let Some(h) = self.branch_hash(rev) {
            return Ok(h);
        }
        if rev.len() >= 4 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
            let mut matches = Vec::new();
            let objdir = self.store.join("objects");
            if rev.len() >= 2 {
                let sub = objdir.join(&rev[..2]);
                if let Ok(rd) = std::fs::read_dir(&sub) {
                    for entry in rd.flatten() {
                        let tail = entry.file_name().to_string_lossy().into_owned();
                        let full = format!("{}{}", &rev[..2], tail);
                        if full.starts_with(rev) && self.commit_obj(&full).is_ok() {
                            matches.push(full);
                        }
                    }
                }
            }
            match matches.len() {
                1 => return Ok(matches.remove(0)),
                0 => {}
                _ => return Err(format!("'{rev}' は曖昧です({} 件一致)", matches.len())),
            }
        }
        Err(format!("リビジョン '{rev}' が見つかりません(ブランチ名か commit ハッシュ)"))
    }

    /// History from a commit back to the root (newest first).
    pub fn log(&self, from: &str) -> Result<Vec<(String, Commit)>, String> {
        let mut out = Vec::new();
        let mut cur = Some(from.to_string());
        while let Some(h) = cur {
            let c = self.commit_obj(&h)?;
            cur = c.parents.first().cloned();
            out.push((h, c));
        }
        Ok(out)
    }

    // ---- status / checkout --------------------------------------------------

    /// (added, modified, deleted) of the working tree vs a snapshot.
    pub fn changes(base: &Snapshot, work: &Snapshot) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();
        for (path, bytes) in work {
            match base.get(path) {
                None => added.push(path.clone()),
                Some(old) if old != bytes => modified.push(path.clone()),
                _ => {}
            }
        }
        for path in base.keys() {
            if !work.contains_key(path) {
                deleted.push(path.clone());
            }
        }
        (added, modified, deleted)
    }

    pub fn is_clean(&self) -> Result<bool, String> {
        let base = match self.head()? {
            Some(h) => self.read_tree(&self.commit_obj(&h)?.tree)?,
            None => Snapshot::new(),
        };
        let work = self.working_snapshot()?;
        let (a, m, d) = Self::changes(&base, &work);
        Ok(a.is_empty() && m.is_empty() && d.is_empty())
    }

    /// Restore the tracked files of `rev` into the working tree. Tracked files
    /// not present in `rev` are removed. Refuses on a dirty tree.
    pub fn checkout(&self, rev: &str) -> Result<String, String> {
        if !self.is_clean()? {
            return Err(
                "作業ツリーに未コミットの変更があります(forte status で確認、commit してから)"
                    .into(),
            );
        }
        let is_branch = self.branch_hash(rev).is_some();
        let hash = self.resolve(rev)?;
        let target = self.read_tree(&self.commit_obj(&hash)?.tree)?;
        let current = self.working_snapshot()?;
        for path in current.keys() {
            if !target.contains_key(path) {
                std::fs::remove_file(self.root.join(path)).map_err(|e| e.to_string())?;
            }
        }
        for (path, bytes) in &target {
            let dest = self.root.join(path);
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
        }
        if is_branch {
            std::fs::write(self.store.join("HEAD"), format!("ref: {rev}\n"))
                .map_err(|e| e.to_string())?;
            Ok(format!("checkout: ブランチ {rev} ({} files)", target.len()))
        } else {
            std::fs::write(self.store.join("HEAD"), format!("{hash}\n"))
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "checkout: {} ({} files) — ブランチから外れています(戻るには forte checkout main)",
                &hash[..8],
                target.len()
            ))
        }
    }

    /// Snapshot of a revision (for diff).
    pub fn snapshot_of(&self, rev: &str) -> Result<Snapshot, String> {
        let hash = self.resolve(rev)?;
        self.read_tree(&self.commit_obj(&hash)?.tree)
    }
}

/// Walk the project, collecting tracked files. Skips hidden directories,
/// build output and dependency directories.
fn walk(root: &Path, dir: &Path, snap: &mut Snapshot) -> Result<(), String> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            walk(root, &path, snap)?;
        } else if name.ends_with(".forte") || name.ends_with(".frec") || name == ".forte-lineage.json" {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            snap.insert(rel, bytes);
        }
    }
    Ok(())
}
