//! Forte VCS — GitHub のようにバージョン管理ができる作曲ツール、の「バージョン
//! 管理」本体。git と同じ content-addressed オブジェクトストアを `.forte/` に
//! 持ち、blob(ファイル)/ tree(ディレクトリ)/ commit(スナップショット+
//! 親)を SHA-256 で指す。
//!
//! 設計メモ:
//! - 追跡対象は音楽のソースのみ: `*.forte` と `*.frec`。ビルド成果物
//!   (wav / manifest)は音のソースから再現できるので追跡しない。
//! - commit に壁時計は入れない(決定論と同じ思想)。順序は親チェーンが持つ。
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
        let mid_merge = self.store.join("MERGE_HEAD").exists();
        if let Some(ref p) = parent {
            // an unchanged tree still commits mid-merge: the point is parent #2
            if !mid_merge && self.commit_obj(p)?.tree == tree {
                return Err("変更がありません(nothing to commit)".into());
            }
        }
        // a conflict resolution finishes the merge: MERGE_HEAD becomes parent #2
        let mut parents: Vec<String> = parent.clone().into_iter().collect();
        let merge_head = self.store.join("MERGE_HEAD");
        if let Ok(h) = std::fs::read_to_string(&merge_head) {
            let h = h.trim().to_string();
            if !h.is_empty() && !parents.contains(&h) {
                parents.push(h);
            }
        }
        let mut n = 1;
        for p in &parents {
            n = n.max(self.commit_obj(p)?.n + 1);
        }
        let c = Commit { tree, parents, author: author(), message: message.to_string(), n };
        let body = serde_json::to_vec(&c).map_err(|e| e.to_string())?;
        let hash = self.put("commit", &body)?;
        std::fs::write(self.branch_path(&branch), format!("{hash}\n")).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&merge_head);
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
        self.restore(&target)?;
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

    /// Overwrite the working tree's tracked files with a snapshot.
    fn restore(&self, target: &Snapshot) -> Result<(), String> {
        let current = self.working_snapshot()?;
        for path in current.keys() {
            if !target.contains_key(path) {
                std::fs::remove_file(self.root.join(path)).map_err(|e| e.to_string())?;
            }
        }
        for (path, bytes) in target {
            let dest = self.root.join(path);
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Snapshot of a revision (for diff).
    pub fn snapshot_of(&self, rev: &str) -> Result<Snapshot, String> {
        let hash = self.resolve(rev)?;
        self.read_tree(&self.commit_obj(&hash)?.tree)
    }

    // ---- history transport (project push / fork) ------------------------------

    /// Every object hash reachable from a commit: the commit chain plus all
    /// trees and blobs. This is what "publishing the history" means.
    pub fn reachable(&self, from: &str) -> Result<Vec<String>, String> {
        let mut out = std::collections::BTreeSet::new();
        let mut commits = vec![from.to_string()];
        while let Some(h) = commits.pop() {
            if !out.insert(h.clone()) {
                continue;
            }
            let c = self.commit_obj(&h)?;
            commits.extend(c.parents.clone());
            self.collect_tree(&c.tree, &mut out)?;
        }
        Ok(out.into_iter().collect())
    }

    fn collect_tree(
        &self,
        hash: &str,
        out: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), String> {
        if !out.insert(hash.to_string()) {
            return Ok(());
        }
        let (kind, body) = self.get(hash)?;
        if kind != "tree" {
            return Ok(()); // blob
        }
        let entries: Vec<serde_json::Value> =
            serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        for e in &entries {
            let h = e["hash"].as_str().unwrap_or_default();
            match e["kind"].as_str() {
                Some("tree") => self.collect_tree(h, out)?,
                _ => {
                    out.insert(h.to_string());
                }
            }
        }
        Ok(())
    }

    /// Raw stored bytes of one object (for export archives).
    pub fn object_raw(&self, hash: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.object_path(hash)).map_err(|e| e.to_string())
    }

    /// Copy all objects reachable from `from` into `dest` (an objects/-style
    /// directory). Content addressing makes this idempotent and incremental.
    pub fn export_objects(&self, from: &str, dest: &Path) -> Result<usize, String> {
        let mut copied = 0;
        for hash in self.reachable(from)? {
            let target = dest.join(&hash[..2]).join(&hash[2..]);
            if target.exists() {
                continue;
            }
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(self.object_path(&hash), &target).map_err(|e| e.to_string())?;
            copied += 1;
        }
        Ok(copied)
    }

    /// Create a repository at `dir` whose `main` is `head`, importing objects
    /// from an exported objects directory, and restore the working tree.
    /// This is the mechanics of a fork: full history, new home.
    pub fn clone_into(dir: &str, objects_src: &Path, head: &str) -> Result<Repo, String> {
        Repo::init(dir)?;
        let repo = Repo::open(dir)?;
        let rd = std::fs::read_dir(objects_src).map_err(|e| e.to_string())?;
        for sub in rd.flatten() {
            let prefix = sub.file_name().to_string_lossy().into_owned();
            if let Ok(files) = std::fs::read_dir(sub.path()) {
                for f in files.flatten() {
                    let dest = repo.store.join("objects").join(&prefix).join(f.file_name());
                    if !dest.exists() {
                        std::fs::create_dir_all(dest.parent().unwrap())
                            .map_err(|e| e.to_string())?;
                        std::fs::copy(f.path(), &dest).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        repo.commit_obj(head)
            .map_err(|_| format!("履歴に commit {} がありません", &head[..head.len().min(8)]))?;
        std::fs::write(repo.branch_path("main"), format!("{head}\n")).map_err(|e| e.to_string())?;
        let files = repo.read_tree(&repo.commit_obj(head)?.tree)?;
        repo.restore(&files)?;
        Ok(repo)
    }

    // ---- merge ---------------------------------------------------------------

    /// Nearest common ancestor (good enough for the fork-and-jam histories the
    /// tool produces; criss-cross merges pick the first hit).
    fn merge_base(&self, a: &str, b: &str) -> Result<Option<String>, String> {
        let mut ancestors = std::collections::BTreeSet::new();
        let mut queue = vec![a.to_string()];
        while let Some(h) = queue.pop() {
            if ancestors.insert(h.clone()) {
                queue.extend(self.commit_obj(&h)?.parents.clone());
            }
        }
        let mut queue = std::collections::VecDeque::from([b.to_string()]);
        let mut seen = std::collections::BTreeSet::new();
        while let Some(h) = queue.pop_front() {
            if ancestors.contains(&h) {
                return Ok(Some(h));
            }
            if seen.insert(h.clone()) {
                queue.extend(self.commit_obj(&h)?.parents.clone());
            }
        }
        Ok(None)
    }

    /// Merge `other` (branch or hash) into the current branch. Disjoint edits
    /// combine automatically (file level, then line level); overlapping edits
    /// leave `<<<<<<<`-marked files in the working tree and no commit is made.
    pub fn merge(&self, other: &str) -> Result<String, String> {
        let branch = self.head_ref()?.ok_or("HEAD がブランチを指していません(checkout <branch> で戻る)")?;
        if !self.is_clean()? {
            return Err("作業ツリーに未コミットの変更があります(commit してから merge)".into());
        }
        let ours_hash = self.head()?.ok_or("まだコミットがありません")?;
        let theirs_hash = self.resolve(other)?;
        if ours_hash == theirs_hash {
            return Err("同じコミットです(マージするものがありません)".into());
        }
        let base_hash = self
            .merge_base(&ours_hash, &theirs_hash)?
            .ok_or("共通の祖先がありません(別リポジトリの履歴?)")?;
        if base_hash == theirs_hash {
            return Err(format!("'{other}' は既に取り込み済みです"));
        }
        let theirs = self.read_tree(&self.commit_obj(&theirs_hash)?.tree)?;
        if base_hash == ours_hash {
            // fast-forward: we have nothing of our own, just move the branch
            self.restore(&theirs)?;
            std::fs::write(self.branch_path(&branch), format!("{theirs_hash}\n"))
                .map_err(|e| e.to_string())?;
            return Ok(format!("fast-forward: {branch} → {} ({} files)", &theirs_hash[..8], theirs.len()));
        }
        let base = self.read_tree(&self.commit_obj(&base_hash)?.tree)?;
        let ours = self.read_tree(&self.commit_obj(&ours_hash)?.tree)?;

        let mut merged = Snapshot::new();
        let mut conflicts: Vec<String> = Vec::new();
        let mut paths: std::collections::BTreeSet<&String> = std::collections::BTreeSet::new();
        paths.extend(base.keys());
        paths.extend(ours.keys());
        paths.extend(theirs.keys());
        for path in paths {
            let (b, o, t) = (base.get(path), ours.get(path), theirs.get(path));
            let take = |v: Option<&Vec<u8>>, m: &mut Snapshot| {
                if let Some(v) = v {
                    m.insert(path.clone(), v.clone());
                }
            };
            if o == t {
                take(o, &mut merged); // same on both sides (or both deleted)
            } else if o == b {
                take(t, &mut merged); // only theirs changed (or deleted)
            } else if t == b {
                take(o, &mut merged); // only ours changed (or deleted)
            } else if o.is_none() || t.is_none() {
                conflicts.push(format!("{path} (片方で編集、片方で削除)"));
                take(o.or(t), &mut merged); // keep the surviving edit for repair
            } else if !path.ends_with(".forte") && !path.ends_with(".json") {
                conflicts.push(format!("{path} (バイナリが両方で変更)"));
                take(o, &mut merged);
            } else {
                let (text, conflicted) = merge3(
                    &String::from_utf8_lossy(b.map(Vec::as_slice).unwrap_or_default()),
                    &String::from_utf8_lossy(o.unwrap()),
                    &String::from_utf8_lossy(t.unwrap()),
                    &branch,
                    other,
                );
                if conflicted {
                    conflicts.push(format!("{path} (同じ行を両方で編集)"));
                }
                merged.insert(path.clone(), text.into_bytes());
            }
        }

        if !conflicts.is_empty() {
            // leave the marked-up merge in the working tree for repair; the
            // resolving commit picks up MERGE_HEAD as its second parent
            self.restore(&merged)?;
            std::fs::write(self.store.join("MERGE_HEAD"), format!("{theirs_hash}\n"))
                .map_err(|e| e.to_string())?;
            return Err(format!(
                "競合があります — マーカー(<<<<<<<)を直して forte commit してください:\n  {}",
                conflicts.join("\n  ")
            ));
        }

        // merge commit with both parents
        let tree = self.write_tree(&merged)?;
        let n = self.commit_obj(&ours_hash)?.n.max(self.commit_obj(&theirs_hash)?.n) + 1;
        let c = Commit {
            tree,
            parents: vec![ours_hash.clone(), theirs_hash.clone()],
            author: author(),
            message: format!("merge {other}"),
            n,
        };
        let body = serde_json::to_vec(&c).map_err(|e| e.to_string())?;
        let hash = self.put("commit", &body)?;
        self.restore(&merged)?;
        std::fs::write(self.branch_path(&branch), format!("{hash}\n")).map_err(|e| e.to_string())?;

        // the musical safety net: does the merged song still compile?
        let mut warnings = String::new();
        for (path, bytes) in &merged {
            if !path.ends_with(".forte") {
                continue;
            }
            let src = String::from_utf8_lossy(bytes);
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if crate::check_with_loader(&src, &crate::semdiff::SnapLoader(&merged), dir).is_err() {
                warnings.push_str(&format!("\n⚠ {path} がコンパイルできません(forte check で確認を)"));
            }
        }
        Ok(format!(
            "[{branch} #{n} {}] merge {other} ({} files){warnings}",
            &hash[..8],
            merged.len()
        ))
    }
}

// ---------------------------------------------------------------------------
// line-level three-way merge (diff3 over an LCS matching)
// ---------------------------------------------------------------------------

/// One side's rewrite of base lines `s..e` into `lines`.
struct Edit {
    s: usize,
    e: usize,
    lines: Vec<String>,
}

/// Longest-common-subsequence matching between two line arrays (O(n·m) DP —
/// song sources are small).
fn lcs_edits(base: &[&str], side: &[&str]) -> Vec<Edit> {
    let (n, m) = (base.len(), side.len());
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[at(i, j)] = if base[i] == side[j] {
                dp[at(i + 1, j + 1)] + 1
            } else {
                dp[at(i + 1, j)].max(dp[at(i, j + 1)])
            };
        }
    }
    let mut edits = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut es, mut lines): (Option<usize>, Vec<String>) = (None, Vec::new());
    while i < n || j < m {
        if i < n && j < m && base[i] == side[j] {
            if let Some(s) = es.take() {
                edits.push(Edit { s, e: i, lines: std::mem::take(&mut lines) });
            }
            i += 1;
            j += 1;
        } else if j < m && (i == n || dp[at(i, j + 1)] >= dp[at(i + 1, j)]) {
            es.get_or_insert(i);
            lines.push(side[j].to_string());
            j += 1;
        } else {
            es.get_or_insert(i);
            i += 1;
        }
    }
    if let Some(s) = es {
        edits.push(Edit { s, e: n, lines });
    }
    edits
}

/// Rebuild one side's text for base range `s..e` from its edit script.
fn side_range(base: &[&str], edits: &[Edit], s: usize, e: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = s;
    for ed in edits {
        if ed.e < s || ed.s > e {
            continue;
        }
        while i < ed.s {
            out.push(base[i].to_string());
            i += 1;
        }
        out.extend(ed.lines.iter().cloned());
        i = ed.e;
    }
    while i < e {
        out.push(base[i].to_string());
        i += 1;
    }
    out
}

/// Three-way merge; returns (text, had_conflicts). Conflicting regions carry
/// git-style markers labelled with the branch names.
pub(crate) fn merge3(base: &str, ours: &str, theirs: &str, ours_name: &str, theirs_name: &str) -> (String, bool) {
    let b: Vec<&str> = base.lines().collect();
    let eo = lcs_edits(&b, &ours.lines().collect::<Vec<_>>());
    let et = lcs_edits(&b, &theirs.lines().collect::<Vec<_>>());

    // cluster overlapping edit regions from the two sides (closed intervals so
    // an insertion at a point collides with an edit covering that point)
    #[derive(Clone, Copy)]
    struct Region {
        s: usize,
        e: usize,
        ours: bool,
        theirs: bool,
    }
    let mut regions: Vec<Region> = eo
        .iter()
        .map(|ed| Region { s: ed.s, e: ed.e.max(ed.s), ours: true, theirs: false })
        .chain(et.iter().map(|ed| Region { s: ed.s, e: ed.e.max(ed.s), ours: false, theirs: true }))
        .collect();
    regions.sort_by_key(|r| (r.s, r.e));
    // edits not separated by at least one unchanged base line share a cluster
    // (git's rule: adjacent changes from both sides are a conflict candidate)
    let mut clusters: Vec<Region> = Vec::new();
    for r in regions {
        match clusters.last_mut() {
            Some(last) if r.s <= last.e => {
                last.e = last.e.max(r.e);
                last.ours |= r.ours;
                last.theirs |= r.theirs;
            }
            _ => clusters.push(r),
        }
    }

    let mut out: Vec<String> = Vec::new();
    let mut conflicted = false;
    let mut i = 0;
    for c in &clusters {
        while i < c.s.min(b.len()) {
            out.push(b[i].to_string());
            i += 1;
        }
        let e = c.e.min(b.len()).max(c.s.min(b.len()));
        let o_lines = side_range(&b, &eo, c.s.min(b.len()), e);
        let t_lines = side_range(&b, &et, c.s.min(b.len()), e);
        match (c.ours, c.theirs) {
            (true, false) => out.extend(o_lines),
            (false, true) => out.extend(t_lines),
            _ if o_lines == t_lines => out.extend(o_lines),
            _ => {
                conflicted = true;
                out.push(format!("<<<<<<< {ours_name}"));
                out.extend(o_lines);
                out.push("=======".into());
                out.extend(t_lines);
                out.push(format!(">>>>>>> {theirs_name}"));
            }
        }
        i = e;
    }
    while i < b.len() {
        out.push(b[i].to_string());
        i += 1;
    }
    let mut text = out.join("\n");
    text.push('\n');
    (text, conflicted)
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
