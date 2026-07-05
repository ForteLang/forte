//! The instruments workshop (issue #44): `new` scaffolds, `fix` derives a
//! fixed-parameter variant that shadows the packaged original, and the
//! workspace joins the catalog.

use std::path::Path;
use std::process::Command;

fn forte(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_forte"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run forte");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn new_fix_and_workspace_catalog() {
    let base = std::env::temp_dir().join(format!("forte-workshop-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // a minimal installed package with one two-device library
    let lib = base.join("packages").join("mini_0.1.0").join("instruments");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(
        lib.join("bass.forte"),
        "// mini/bass — two basses.\n\
         device DeepBass : Instrument {\n  param cutoff = 0.4 in 0..1\n  node o = osc(shape: \"saw\")\n  node f = svf(in: o, cutoff: cutoff)\n  out gain(in: f, mod: adsr())\n}\n\
         device ThinBass : Instrument {\n  param cutoff = 0.7 in 0..1\n  node o = osc(shape: \"square\")\n  node f = svf(in: o, cutoff: cutoff)\n  out gain(in: f, mod: adsr())\n}\n",
    )
    .unwrap();

    // new: template scaffold, committed, discoverable
    let (ok, out, err) = forte(&base, &["instruments", "new", "MyLead"]);
    assert!(ok, "new: {err}");
    assert!(out.contains("instruments/mylead.forte"), "new output: {out}");
    let src = std::fs::read_to_string(base.join("instruments").join("mylead.forte")).unwrap();
    assert!(src.contains("device MyLead : Instrument"), "template: {src}");
    // it must actually compile as a device library
    let (ok, _, err) = forte(&base, &["check", "instruments/mylead.forte"]);
    assert!(ok, "template must validate: {err}");
    // a second `new` with the same name refuses
    let (ok, _, err) = forte(&base, &["instruments", "new", "MyLead"]);
    assert!(!ok && err.contains("既に"), "duplicate new: {err}");

    // fix: rewrite ONE device's default, leave its file siblings alone
    let (ok, out, err) = forte(&base, &["instruments", "fix", "DeepBass", "cutoff=0.9"]);
    assert!(ok, "fix: {err}");
    assert!(out.contains("cutoff=0.9"), "fix output: {out}");
    let fixed = std::fs::read_to_string(base.join("instruments").join("bass.forte")).unwrap();
    assert!(fixed.contains("param cutoff = 0.9 in 0..1"), "fixed default: {fixed}");
    assert!(fixed.contains("param cutoff = 0.7 in 0..1"), "sibling untouched: {fixed}");
    // the workspace copy shadows the package for name resolution
    let (_, names, _) = forte(&base, &["instruments", "names", "Deep"]);
    assert!(names.contains("DeepBass"), "names: {names}");

    // fix validates the param name and range against the declaration
    let (ok, _, err) = forte(&base, &["instruments", "fix", "DeepBass", "reso=0.5"]);
    assert!(!ok && err.contains("cutoff"), "unknown param lists declared: {err}");
    let (ok, _, err) = forte(&base, &["instruments", "fix", "DeepBass", "cutoff=1.5"]);
    assert!(!ok && err.contains("範囲"), "range: {err}");

    // the catalog shows the workspace
    let (ok, out, _) = forte(&base, &["instruments", "list", "mylead"]);
    assert!(ok);
    assert!(out.contains("workspace"), "catalog marks the workspace: {out}");

    let _ = std::fs::remove_dir_all(&base);
}
