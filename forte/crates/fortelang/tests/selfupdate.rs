//! `forte upgrade` release path (#68): platform-asset selection from a
//! releases/latest response, offline.

const RELEASE: &str = r#"{
  "tag_name": "v0.7.0",
  "assets": [
    {"name": "forte-x86_64-linux",       "browser_download_url": "https://example.com/l"},
    {"name": "forte-aarch64-macos",      "browser_download_url": "https://example.com/m"},
    {"name": "forte-x86_64-windows.exe", "browser_download_url": "https://example.com/w"}
  ]
}"#;

#[test]
fn picks_the_platform_asset() {
    let (tag, name, url) =
        fortelang::selfupdate::pick_asset(RELEASE, "linux", "x86_64").expect("linux asset");
    assert_eq!((tag.as_str(), name.as_str(), url.as_str()), ("v0.7.0", "forte-x86_64-linux", "https://example.com/l"));

    let (_, name, _) =
        fortelang::selfupdate::pick_asset(RELEASE, "macos", "aarch64").expect("macos asset");
    assert_eq!(name, "forte-aarch64-macos");

    // .exe suffix matches for windows
    let (_, name, _) =
        fortelang::selfupdate::pick_asset(RELEASE, "windows", "x86_64").expect("windows asset");
    assert_eq!(name, "forte-x86_64-windows.exe");

    // no asset for the platform → None (caller falls back to cargo)
    assert!(fortelang::selfupdate::pick_asset(RELEASE, "macos", "x86_64").is_none());
    // an error body (rate limit page) → None, never a panic
    assert!(fortelang::selfupdate::pick_asset(r#"{"message":"Not Found"}"#, "linux", "x86_64").is_none());
}
