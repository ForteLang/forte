//! `forte upgrade` — the release side (issue #68). When a GitHub Release
//! carries a prebuilt binary for this platform, upgrading is a download,
//! not a compile; otherwise the caller falls back to `cargo install`.
//!
//! Asset naming convention (what a release must upload):
//!   forte-<arch>-<os>[.exe]   e.g. forte-x86_64-linux,
//!   forte-aarch64-macos, forte-x86_64-windows.exe

const REPO: &str = "ForteLang/forte";

/// Pick this platform's asset out of a releases/latest response.
/// Returns (tag, asset name, download url). Split from the HTTP call so
/// the selection is testable offline.
pub fn pick_asset(json: &str, os: &str, arch: &str) -> Option<(String, String, String)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = v["tag_name"].as_str()?.to_string();
    let want = format!("forte-{arch}-{os}");
    for a in v["assets"].as_array()? {
        let name = a["name"].as_str()?;
        if name == want || name == format!("{want}.exe") {
            return Some((tag, name.to_string(), a["browser_download_url"].as_str()?.to_string()));
        }
    }
    None
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args(args)
        .output()
        .map_err(|e| format!("curl が実行できません: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Try the prebuilt-binary path. `Ok(Some(msg))` = upgraded; `Ok(None)` =
/// no release / no asset for this platform / not applicable — the caller
/// falls back to building from source. `Err` = a release WAS found but
/// installing it failed (worth telling the user before falling back).
pub fn try_release_upgrade() -> Result<Option<String>, String> {
    if std::env::consts::OS == "windows" {
        // a running .exe cannot be replaced in place; keep the cargo path
        return Ok(None);
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let Ok(body) = curl(&["-s", "-m", "8", "-H", "User-Agent: forte-cli", &url]) else {
        return Ok(None); // offline: not an error, just no release path
    };
    let Some((tag, name, dl)) =
        pick_asset(&String::from_utf8_lossy(&body), std::env::consts::OS, std::env::consts::ARCH)
    else {
        return Ok(None);
    };

    println!("release {tag} の {name} をダウンロードします…");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let fresh = exe.with_extension("new");
    curl(&["-sL", "-m", "300", "-o", &fresh.to_string_lossy(), "-H", "User-Agent: forte-cli", &dl])
        .map_err(|e| format!("{name} を取得できません: {e}"))?;

    // sanity: an error page is not a binary
    let size = std::fs::metadata(&fresh).map(|m| m.len()).unwrap_or(0);
    if size < 100_000 {
        let _ = std::fs::remove_file(&fresh);
        return Err(format!("{name} が小さすぎます({size} bytes)— リリースが壊れている可能性"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    // swap: the running binary moves aside, the fresh one takes its path
    let old = exe.with_extension("old");
    std::fs::rename(&exe, &old).map_err(|e| format!("{} を退避できません: {e}", exe.display()))?;
    if let Err(e) = std::fs::rename(&fresh, &exe) {
        let _ = std::fs::rename(&old, &exe); // roll back
        return Err(format!("差し替えに失敗しました: {e}"));
    }
    let _ = std::fs::remove_file(&old);
    Ok(Some(format!("upgraded: {tag}({name})— forte version で確認")))
}
