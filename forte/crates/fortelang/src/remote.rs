//! `forte remote` — a project's link to GitHub (issue #52).
//!
//! Distribution in Forte IS GitHub: a project made by `forte init` becomes a
//! package the moment it is pushed. `forte remote add` wires the project to a
//! git URL, and `forte push` / `forte pull` move the whole project (source,
//! assets, `.forte/` history) through ordinary git — no separate hub.

use std::path::Path;
use std::process::Command;

/// `github:owner/repo` → clone URL; everything else passes through.
fn resolve_url(src: &str) -> String {
    match src.strip_prefix("github:") {
        Some(rest) => format!("https://github.com/{rest}.git"),
        None => src.to_string(),
    }
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git が実行できません: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `forte remote add <github:owner/repo | git-URL>` — connect the project in
/// the cwd to GitHub. Initialises git if needed; replaces an existing origin.
pub fn add(src: &str) -> Result<String, String> {
    let url = resolve_url(src);
    if !Path::new(".git").exists() {
        git(&["init"])?;
    }
    if git(&["remote", "get-url", "origin"]).is_ok() {
        git(&["remote", "set-url", "origin", &url])?;
    } else {
        git(&["remote", "add", "origin", &url])?;
    }
    Ok(format!(
        "remote : origin = {url}\n次: forte push で公開(取り込む側は forte package add {src})"
    ))
}

fn origin() -> Result<String, String> {
    git(&["remote", "get-url", "origin"])
        .map_err(|_| "remote がありません(forte remote add github:you/repo)".to_string())
}

fn current_branch() -> String {
    git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into())
}

/// `forte push` — stage everything, commit (message from the forte VCS HEAD
/// when available), and push the current branch to origin.
pub fn push(message: Option<&str>) -> Result<String, String> {
    let url = origin()?;
    git(&["add", "-A"])?;
    let staged = git(&["diff", "--cached", "--name-only"])?;
    if !staged.is_empty() {
        // prefer the musician's own words: the latest forte commit message
        let msg = message
            .map(str::to_string)
            .or_else(|| {
                let repo = crate::vcs::Repo::open(".").ok()?;
                let head = repo.head().ok()??;
                let log = repo.log(&head).ok()?;
                log.first().map(|(_, c)| c.message.clone()).filter(|m| !m.is_empty())
            })
            .unwrap_or_else(|| "forte push".into());
        git(&["commit", "-m", &msg])?;
    }
    let branch = current_branch();
    git(&["push", "-u", "origin", &branch])?;
    Ok(format!("pushed : {branch} → {url}"))
}

/// `forte pull` — fetch and integrate the current branch from origin.
pub fn pull() -> Result<String, String> {
    let url = origin()?;
    let branch = current_branch();
    git(&["pull", "origin", &branch])?;
    Ok(format!("pulled : {branch} ← {url}"))
}
