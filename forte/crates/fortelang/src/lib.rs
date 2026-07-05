//! `fortelang` — the Forte v0 language slice: parse `.forte` sources, check
//! them, and compile to a `dawcore` project that renders deterministically
//! (07-determinism-spike.md) on native and wasm from the same source.

pub mod ast;
#[cfg(not(target_family = "wasm"))]
pub mod audio;
#[cfg(not(target_family = "wasm"))]
pub mod browser;
pub mod calib;
pub mod compile;
pub mod diag;
#[cfg(not(target_family = "wasm"))]
pub mod export;
pub mod fmt;
pub mod frec;
pub mod grid_build;
#[cfg(not(target_family = "wasm"))]
pub mod hub;
#[cfg(not(target_family = "wasm"))]
pub mod hub_git;
#[cfg(not(target_family = "wasm"))]
pub mod hub_remote;
#[cfg(not(target_family = "wasm"))]
pub mod hub_server;
pub mod lexer;
#[cfg(not(target_family = "wasm"))]
pub mod live;
pub mod lsp;
pub mod music;
pub mod parser;
pub mod perform;
#[cfg(not(target_family = "wasm"))]
pub mod repl;
pub mod semdiff;
pub mod sha;
pub mod vcs;
pub mod viz;
pub mod zip;

use dawcore::command::Command;
use dawcore::engine::Engine;
use dawcore::model::Project;
use dawcore::sync::full_sync;
use diag::Diag;

/// Resolves `import { … } from "path"` to source text. Paths are as written
/// by the user, joined to the importing file's directory by the caller of
/// [`load`]. Environments without a filesystem (the browser wasm) use
/// [`NoLoader`].
pub trait ModuleLoader {
    fn load(&self, path: &str) -> Result<String, String>;
    /// Binary loads (recorded `.frec` assets). Text-only environments reject.
    fn load_bytes(&self, _path: &str) -> Result<Vec<u8>, String> {
        Err("この環境では録音アセットを読み込めません".into())
    }
}

/// Loader for environments without module resolution: every import errors.
pub struct NoLoader;
impl ModuleLoader for NoLoader {
    fn load(&self, _path: &str) -> Result<String, String> {
        Err("この環境ではローカル import を解決できません".into())
    }
}

/// Filesystem loader (CLI / LSP).
pub struct FsLoader;
impl ModuleLoader for FsLoader {
    fn load(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }
    fn load_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(path).map_err(|e| e.to_string())
    }
}

fn join_path(base_dir: &str, rel: &str) -> String {
    let joined = std::path::Path::new(base_dir).join(rel);
    // light normalisation so cycle detection catches `./a.forte` == `a.forte`
    let mut parts: Vec<String> = Vec::new();
    for c in joined.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.last().map(|p| p != "..").unwrap_or(false) {
                    parts.pop();
                } else {
                    parts.push("..".into());
                }
            }
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    parts.join("/")
}

/// Recursively resolve a file's imports into its device list.
fn resolve_imports(
    file: &mut ast::FileAst,
    base_dir: &str,
    loader: &dyn ModuleLoader,
    visited: &mut Vec<String>,
    diags: &mut Vec<Diag>,
) {
    let imports = std::mem::take(&mut file.imports);
    let mut imported: Vec<ast::DeviceAst> = Vec::new();
    let mut imported_blocks: Vec<ast::BlockAst> = Vec::new();
    for im in imports {
        let full = join_path(base_dir, &im.path);
        if visited.iter().any(|v| v == &full) {
            diags.push(Diag::new(
                "E-MOD-007",
                im.pos,
                format!("import が循環しています: {full}"),
            ));
            continue;
        }
        let src = match loader.load(&full) {
            Ok(s) => s,
            Err(e) => {
                diags.push(Diag::new("E-MOD-005", im.pos, format!("{} を読み込めません: {e}", im.path)));
                continue;
            }
        };
        let mut module = match parser::parse(&src) {
            Ok(m) => m,
            Err(mut ds) => {
                for d in &mut ds {
                    d.message = format!("{full}:{}:{} {}", d.pos.line, d.pos.col, d.message);
                    d.pos = im.pos; // point at the import site in the current file
                }
                diags.extend(ds);
                continue;
            }
        };
        visited.push(full.clone());
        let module_dir = std::path::Path::new(&full)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        resolve_imports(&mut module, &module_dir, loader, visited, diags);
        visited.pop();

        for name in &im.names {
            if let Some(d) = module.devices.iter().find(|d| d.name == *name) {
                if !imported.iter().any(|x: &ast::DeviceAst| x.name == *name) {
                    imported.push(d.clone());
                }
                continue;
            }
            if let Some(b) = module.blocks.iter().find(|b| b.name == *name) {
                if !imported_blocks.iter().any(|x: &ast::BlockAst| x.name == *name) {
                    imported_blocks.push(b.clone());
                }
                // an imported block needs the devices of its home module —
                // carry them along (first definition of a name wins)
                for d in &module.devices {
                    if !imported.iter().any(|x| x.name == d.name)
                        && !file.devices.iter().any(|x| x.name == d.name)
                    {
                        imported.push(d.clone());
                    }
                }
                continue;
            }
            let avail: Vec<&str> = module
                .devices
                .iter()
                .map(|d| d.name.as_str())
                .chain(module.blocks.iter().map(|b| b.name.as_str()))
                .collect();
            diags.push(Diag::new(
                "E-MOD-006",
                im.pos,
                format!("'{name}' は {} にありません(あるもの: {})", im.path, avail.join(", ")),
            ));
        }
    }
    // imported definitions come first; local definitions may not shadow them
    imported.append(&mut file.devices);
    file.devices = imported;
    imported_blocks.append(&mut file.blocks);
    file.blocks = imported_blocks;
}

/// What a `.forte` file turned out to be.
pub enum Checked {
    Song(Project),
    DeviceLibrary { devices: usize },
    /// A file of blocks (and possibly devices): its LAST top-level block was
    /// compiled as the build root — a block library is always playable.
    BlockLibrary { blocks: usize, devices: usize, root: Box<Project> },
}

/// Parse, resolve imports, and compile or validate. `base_dir` is the
/// directory of the source file (for relative imports).
/// A resolved recorded asset: registered in the engine's sample registry
/// under its content-hash key.
pub struct AssetInfo {
    pub key: String,
    pub seconds: f64,
}

/// Load, validate (provenance!) and register the file's recorded assets.
fn resolve_assets(
    file: &ast::FileAst,
    base_dir: &str,
    loader: &dyn ModuleLoader,
    diags: &mut Vec<Diag>,
) -> std::collections::HashMap<String, AssetInfo> {
    let mut out = std::collections::HashMap::new();
    for a in &file.assets {
        let full = join_path(base_dir, &a.path);
        let bytes = match loader.load_bytes(&full) {
            Ok(b) => b,
            Err(e) => {
                diags.push(Diag::new("E-MOD-005", a.pos, format!("{} を読み込めません: {e}", a.path)));
                continue;
            }
        };
        let rec = match frec::decode(&bytes, a.pos) {
            Ok(r) => r,
            Err(d) => {
                diags.push(d);
                continue;
            }
        };
        let key = format!("{:016x}", fnv1a64(&bytes));
        let seconds = rec.pcm.len() as f64 / rec.channels as f64 / rec.rate as f64;
        // mono-mix for the engine's shared sample buffer (v0: mono clips)
        let mono: Vec<f32> = if rec.channels == 2 {
            rec.pcm.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect()
        } else {
            rec.pcm.clone()
        };
        dawcore::samples::register_asset(
            &key,
            std::sync::Arc::new(dawcore::dsp::sampler::Sample::one_shot(
                mono.into(),
                rec.rate as f32,
                60,
            )),
        );
        out.insert(a.name.clone(), AssetInfo { key, seconds });
    }
    out
}

pub fn check_with_loader(
    src: &str,
    loader: &dyn ModuleLoader,
    base_dir: &str,
) -> Result<Checked, Vec<Diag>> {
    let mut file = parser::parse(src)?;
    let mut diags = Vec::new();
    resolve_imports(&mut file, base_dir, loader, &mut Vec::new(), &mut diags);
    let assets = resolve_assets(&file, base_dir, loader, &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    if file.song.is_some() {
        compile::compile(&file, &assets).map(Checked::Song)
    } else if !file.blocks.is_empty() {
        // block library: validate devices AND compile the last block as root
        let diags = compile::validate_devices(&file);
        if !diags.is_empty() {
            return Err(diags);
        }
        compile::compile(&file, &assets).map(|p| Checked::BlockLibrary {
            blocks: file.blocks.len(),
            devices: file.devices.len(),
            root: Box::new(p),
        })
    } else {
        let diags = compile::validate_devices(&file);
        if diags.is_empty() {
            Ok(Checked::DeviceLibrary { devices: file.devices.len() })
        } else {
            Err(diags)
        }
    }
}

/// Parse + compile a `.forte` song with import resolution.
pub fn compile_with_loader(
    src: &str,
    loader: &dyn ModuleLoader,
    base_dir: &str,
) -> Result<Project, Vec<Diag>> {
    match check_with_loader(src, loader, base_dir)? {
        Checked::Song(p) => Ok(p),
        Checked::BlockLibrary { root, .. } => Ok(*root),
        Checked::DeviceLibrary { .. } => Err(vec![Diag::new(
            "E-SONG-004",
            diag::Pos { line: 1, col: 1 },
            "song も block もありません(このファイルはデバイスライブラリです)",
        )]),
    }
}

/// Parse + compile a `.forte` source into an engine project (no imports —
/// used by the browser wasm; imports report E-MOD-005 there).
pub fn compile_str(src: &str) -> Result<Project, Vec<Diag>> {
    compile_with_loader(src, &NoLoader, "")
}

pub struct RenderInfo {
    pub f32_digest: u64,
    pub frames: usize,
    pub seconds: f64,
    pub peak: f32,
    pub rms: f64,
}

/// Render the arrangement offline (same engine as playback) and digest the
/// exact f32 bit stream — the build proof recorded in build.manifest.json
/// (SRS-BLD-001). FNV-1a 64 stands in for SHA-256 in the v0 slice.
/// Clone the project with one track soloed (returns stay soloed too, so the
/// stem keeps its sends) — how `forte build --stems` isolates a part.
pub fn solo_project(project: &Project, track_id: usize) -> Project {
    let mut p = project.clone();
    for t in &mut p.tracks {
        t.solo = t.id == track_id || t.kind == dawcore::model::TrackKind::Effect;
    }
    p
}

pub fn render_digest(project: &Project, tail_beats: f64) -> RenderInfo {
    const BLOCK: usize = 512;
    let sr = 48_000.0f32;
    let (mut engine, mut handle) = Engine::new(sr);
    full_sync(&mut handle, project);
    handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);

    let total_beats = dawcore::bounce::arrangement_len(project) + tail_beats.max(0.0);
    let seconds = total_beats * 60.0 / project.tempo;
    let total_samples = (seconds * sr as f64) as usize;

    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut update = |bytes: &[u8]| {
        for &b in bytes {
            digest ^= b as u64;
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };

    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    let mut bl = vec![0.0f32; BLOCK];
    let mut br = vec![0.0f32; BLOCK];
    let mut done = 0;
    while done < total_samples {
        let n = BLOCK.min(total_samples - done);
        engine.process(&mut bl, &mut br, n);
        for i in 0..n {
            for s in [bl[i], br[i]] {
                update(&s.to_bits().to_le_bytes());
                peak = peak.max(s.abs());
                sum_sq += (s as f64) * (s as f64);
            }
        }
        done += n;
    }
    RenderInfo {
        f32_digest: digest,
        frames: total_samples,
        seconds,
        peak,
        rms: (sum_sq / (total_samples.max(1) as f64 * 2.0)).sqrt(),
    }
}

/// FNV-1a 64 of arbitrary bytes (used for the source hash in the manifest).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
