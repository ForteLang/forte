//! The git-backed hub: `--hub git@github.com:you/forte-hub.git` (or
//! `github:you/forte-hub`, or any path/URL git accepts that ends in `.git`).
//! A hub is just a git repository holding the same layout as a local hub
//! (registry.json + store/…), so GitHub — or GitLab, or a bare repo on a
//! NAS — hosts it: no server to run, auth is your existing git credentials,
//! and the ledger itself is versioned.
//!
//! Writes go through git's own compare-and-swap: sync the cached checkout,
//! apply the operation with the ordinary local-hub logic, commit, push. A
//! rejected push (someone else got there first) resets the checkout and
//! replays the operation on the fresh state — no merge, no conflict markers.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::hub::Hub;

/// Does this `--hub` value name a git remote (as opposed to a served URL or
/// a local directory)?
pub fn is_git_url(s: &str) -> bool {
    s.starts_with("github:") || s.starts_with("git@") || s.starts_with("ssh://") || s.ends_with(".git")
}

/// `github:user/repo` → the https remote.
fn expand(url: &str) -> String {
    match url.strip_prefix("github:") {
        Some(rest) => format!("https://github.com/{rest}.git"),
        None => url.to_string(),
    }
}

pub struct GitHub {
    remote: String,
    /// Local checkout the hub logic operates on.
    dir: PathBuf,
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    // never hang on a credential prompt — fail with git's message instead
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    let out = cmd.args(args).output().map_err(|e| format!("git が実行できません: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// A push rejected because the remote moved (someone published concurrently).
fn is_stale_push(err: &str) -> bool {
    err.contains("rejected") || err.contains("fetch first") || err.contains("non-fast-forward")
}

impl GitHub {
    /// Clone (first use) or sync the cached checkout of `url`.
    /// `cache_root` overrides the cache location (tests); default is
    /// `$FORTE_HUB_CACHE` or `~/.cache/forte/hub`.
    pub fn open(url: &str, cache_root: Option<PathBuf>) -> Result<GitHub, String> {
        let remote = expand(url);
        let root = cache_root
            .or_else(|| std::env::var("FORTE_HUB_CACHE").ok().map(PathBuf::from))
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                Path::new(&home).join(".cache/forte/hub")
            });
        let key = format!("{:016x}", crate::fnv1a64(remote.as_bytes()));
        let dir = root.join(key);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;

        let hub = GitHub { remote, dir };
        hub.sync()?;
        Ok(hub)
    }

    /// Bring the checkout to the remote's current state, discarding any
    /// half-applied local operation.
    fn sync(&self) -> Result<(), String> {
        if !self.dir.join(".git").exists() {
            git(None, &["clone", &self.remote, &self.dir.to_string_lossy()])
                .map_err(|e| format!("clone {}: {e}", self.remote))?;
            return Ok(());
        }
        git(Some(&self.dir), &["fetch", "origin"]).map_err(|e| format!("fetch: {e}"))?;
        // a checkout cloned while the remote was still empty has no
        // origin/HEAD — let git figure the default branch out now
        let _ = git(Some(&self.dir), &["remote", "set-head", "origin", "--auto"]);
        // an empty remote has no origin/HEAD yet — nothing to reset to
        if let Ok(head) = git(Some(&self.dir), &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
            let head = head.trim().to_string();
            // land on the remote's branch even if the local one is unborn
            let branch = head.strip_prefix("origin/").unwrap_or(&head).to_string();
            let _ = git(Some(&self.dir), &["checkout", "-B", &branch, &head]);
            git(Some(&self.dir), &["reset", "--hard", &head])?;
            git(Some(&self.dir), &["clean", "-fd"])?;
        }
        Ok(())
    }

    fn hub(&self) -> Result<Hub, String> {
        Hub::open(&self.dir.to_string_lossy())
    }

    /// The publisher's name: git identity first, then the usual fallbacks.
    pub fn author(&self) -> String {
        git(Some(&self.dir), &["config", "user.name"])
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                std::env::var("FORTE_AUTHOR")
                    .or_else(|_| std::env::var("USER"))
                    .unwrap_or_else(|_| "anonymous".into())
            })
    }

    fn commit_push(&self, msg: &str) -> Result<(), String> {
        git(Some(&self.dir), &["add", "-A"])?;
        let author = self.author();
        git(
            Some(&self.dir),
            &[
                "-c",
                &format!("user.name={author}"),
                "-c",
                "user.email=forte-hub@local",
                "commit",
                "-m",
                msg,
                "--allow-empty",
            ],
        )?;
        git(Some(&self.dir), &["push", "-u", "origin", "HEAD"]).map(|_| ())
    }

    /// Apply a mutating hub operation with the push as compare-and-swap:
    /// on a stale push, resync and replay the operation on the fresh state.
    fn transact(
        &self,
        msg: &str,
        op: impl Fn(&Hub) -> Result<String, String>,
    ) -> Result<String, String> {
        let mut last = String::new();
        for _attempt in 0..4 {
            let result = op(&self.hub()?)?;
            match self.commit_push(msg) {
                Ok(()) => return Ok(result),
                Err(e) if is_stale_push(&e) => {
                    last = e;
                    self.sync()?; // someone else pushed — replay on their state
                }
                Err(e) => return Err(format!("push {}: {e}", self.remote)),
            }
        }
        Err(format!("並行更新が続いて push できません: {last}"))
    }

    pub fn publish(&self, entry: &str, name: Option<&str>) -> Result<String, String> {
        let author = self.author();
        let mut msg = self.transact(&format!("publish by {author}"), |hub| {
            hub.publish_as(entry, name, Some(&author))
        })?;
        msg.push_str(&format!(" → {}", self.remote));
        Ok(msg)
    }

    /// Fork materializes locally once; only the ledger event replays on a
    /// push conflict (the working copy must not be written twice).
    pub fn fork(&self, name: &str, dest: &str) -> Result<String, String> {
        let by = self.author();
        let result = self.hub()?.fork_as(name, dest, Some(&by))?;
        let mut last = String::new();
        for _attempt in 0..4 {
            match self.commit_push(&format!("fork {name} by {by}")) {
                Ok(()) => return Ok(result),
                Err(e) if is_stale_push(&e) => {
                    last = e;
                    self.sync()?;
                    // the checkout was reset: re-record just the event
                    let hub = self.hub()?;
                    let v = hub
                        .registry()?
                        .repos
                        .get(name)
                        .and_then(|r| r.versions.last().map(|v| v.v))
                        .unwrap_or(0);
                    hub.record_event("fork", name, v, &by)?;
                }
                Err(e) => return Err(format!("push {}: {e}", self.remote)),
            }
        }
        Err(format!("並行更新が続いて push できません: {last}"))
    }

    pub fn release(&self, name: &str) -> Result<String, String> {
        let author = self.author();
        self.transact(&format!("release {name} by {author}"), |hub| hub.release(name))
    }

    pub fn verify(&self, name: &str) -> Result<String, String> {
        let author = self.author();
        self.transact(&format!("verify {name} by {author}"), |hub| hub.verify(name))
    }

    // ---- read-only: just sync + delegate --------------------------------

    pub fn list(&self, json: bool) -> Result<String, String> {
        let hub = self.hub()?;
        if json { hub.repos_json().map(|v| v.to_string()) } else { hub.list() }
    }

    pub fn lineage(&self, name: &str) -> Result<String, String> {
        self.hub()?.lineage(name)
    }

    pub fn entry_path(&self, name: &str) -> Result<String, String> {
        self.hub()?.entry_path(name)
    }

    /// Serve the synced checkout locally: the browser lineage page works
    /// against a GitHub-backed hub with no changes.
    pub fn serve(&self, port: u16) -> Result<(), String> {
        println!("serving synced checkout of {} (read-only view)", self.remote);
        crate::hub_server::serve(self.hub()?, port)
    }
}
