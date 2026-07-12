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
pub mod lexer;
#[cfg(not(target_family = "wasm"))]
pub mod live;
pub mod lsp;
pub mod music;
#[cfg(not(target_family = "wasm"))]
pub mod package;
pub mod parser;
pub mod perform;
#[cfg(not(target_family = "wasm"))]
pub mod remote;
#[cfg(not(target_family = "wasm"))]
pub mod repl;
pub mod semdiff;
pub mod testing;
#[cfg(not(target_family = "wasm"))]
pub mod selfupdate;
pub mod sha;
#[cfg(not(target_family = "wasm"))]
pub mod songfile;
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
                    let mut b = b.clone();
                    // code-jumps for imported content land on the import line
                    b.import_line = Some(im.pos.line);
                    imported_blocks.push(b);
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
#[allow(clippy::large_enum_variant)] // one Checked per compile; boxing Song buys nothing
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

/// A resolved `dig` record: another song rendered to an asset. `beats` is
/// the musical length of the window (the asset carries +2 beats of tail
/// beyond it) — the sampler uses it to default `end` to the musical edge.
pub struct DigInfo {
    pub key: String,
    pub root: u8,
    pub beats: f64,
    /// The record's own tempo — lets the sampler warp it to the song.
    pub tempo: f64,
}

/// Resolve every `sample X = dig("song.forte", ...)` in the file: compile
/// the referenced song (its own imports, bounces and digs included),
/// render its full arrangement with the same deterministic engine, window
/// it by skip/beats, and register the result as a sample asset. Crate
/// digging — your own songs are the records.
fn resolve_digs(
    file: &ast::FileAst,
    base_dir: &str,
    loader: &dyn ModuleLoader,
    stack: &mut Vec<String>,
    diags: &mut Vec<Diag>,
) -> std::collections::HashMap<String, DigInfo> {
    // collect dig-lets from the song root and every block
    fn collect<'a>(blocks: &'a [ast::BlockAst], out: &mut Vec<&'a ast::SampleLetAst>) {
        for b in blocks {
            out.extend(b.body.sample_lets.iter().filter(|sl| sl.dig.is_some()));
            collect(&b.body.blocks, out);
        }
    }
    let mut lets: Vec<&ast::SampleLetAst> = Vec::new();
    collect(&file.blocks, &mut lets);
    if let Some(song) = &file.song {
        lets.extend(song.sample_lets.iter().filter(|sl| sl.dig.is_some()));
    }
    let mut out = std::collections::HashMap::new();
    for sl in lets {
        let path = sl.dig.as_ref().unwrap();
        let full = join_path(base_dir, path);
        if stack.iter().any(|v| v == &full) {
            diags.push(Diag::new(
                "E-DIG-002",
                sl.pos,
                format!("dig が循環しています: {full}"),
            ));
            continue;
        }
        let root = match music::parse_pitch(&sl.note, sl.pos) {
            Ok(v) => v,
            Err(d) => {
                diags.push(d);
                continue;
            }
        };
        let src = match loader.load(&full) {
            Ok(s) => s,
            Err(e) => {
                diags.push(Diag::new("E-MOD-005", sl.pos, format!("{path} を読み込めません: {e}")));
                continue;
            }
        };
        let module_dir = std::path::Path::new(&full)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        stack.push(full.clone());
        let mut src_sections: Vec<(String, (u32, u32))> = Vec::new();
        let project = (|| -> Result<Project, Vec<Diag>> {
            let mut module = parser::parse(&src)?;
            // remember the source's section names for `section:` windowing
            if let Some(song) = &module.song {
                src_sections =
                    song.sections.iter().map(|x| (x.name.clone(), x.bars)).collect();
            } else if let Some(b) = module.blocks.last() {
                src_sections =
                    b.body.sections.iter().map(|x| (x.name.clone(), x.bars)).collect();
            }
            let mut mdiags = Vec::new();
            resolve_imports(&mut module, &module_dir, loader, &mut Vec::new(), &mut mdiags);
            let massets = resolve_assets(&module, &module_dir, loader, &mut mdiags);
            let mdigs = resolve_digs(&module, &module_dir, loader, stack, &mut mdiags);
            if !mdiags.is_empty() {
                return Err(mdiags);
            }
            compile::compile(&module, &massets, &mdigs)
        })();
        stack.pop();
        let project = match project {
            Ok(p) => p,
            Err(ds) => {
                for d in ds {
                    diags.push(Diag::new(
                        "E-DIG-003",
                        sl.pos,
                        format!("dig({path}) のコンパイルに失敗: {full}:{}:{} {}", d.pos.line, d.pos.col, d.message),
                    ));
                }
                continue;
            }
        };
        let record_len = dawcore::bounce::arrangement_len(&project);
        // `section: "drop"` resolves to the SOURCE's declared bars — it
        // survives the record being rearranged, unlike skip/beats
        let window_bars = match &sl.section {
            Some((nm, spos)) => match src_sections.iter().find(|(n, _)| n == nm) {
                Some((_, b)) => Some(*b),
                None => {
                    let names: Vec<&str> =
                        src_sections.iter().map(|(n, _)| n.as_str()).collect();
                    diags.push(Diag::new(
                        "E-DIG-005",
                        *spos,
                        format!(
                            "dig 先に section '{nm}' がありません(あるもの: {})",
                            if names.is_empty() { "なし".to_string() } else { names.join(", ") }
                        ),
                    ));
                    None
                }
            },
            None => sl.bars,
        };
        // `bars: a..b` windows by the SOURCE's meter — no beat arithmetic
        let (want_skip, want_beats) = match window_bars {
            Some((a, b)) if a >= 1 && b >= a => {
                let bpb = project.time_sig.0 as f64 * 4.0 / project.time_sig.1 as f64;
                ((a - 1) as f64 * bpb, (b - a + 1) as f64 * bpb)
            }
            Some((a, b)) => {
                diags.push(Diag::new(
                    "E-DIG-004",
                    sl.pos,
                    format!("dig の bars({a}..{b}) が不正です(小節は 1 始まり、開始 ≤ 終了)"),
                ));
                (sl.skip, sl.beats)
            }
            None => (sl.skip, sl.beats),
        };
        let skip = want_skip.max(0.0).min(record_len);
        let want = if want_beats > 0.0 { want_beats } else { record_len - skip };
        let beats = want.min(record_len - skip).max(0.25);
        let key = render_dig_to_sample(&project, root, skip, beats);
        out.insert(sl.name.clone(), DigInfo { key, root, beats, tempo: project.tempo });
    }
    out
}

/// Render a whole project and cut the [skip, skip+beats+2) window out of it
/// as a registered sample asset (both channels — the record keeps its
/// stereo field). The 2 tail beats are REAL audio — whatever the record
/// played next — so slice ends ring into truth, not silence.
fn render_dig_to_sample(project: &Project, root: u8, skip: f64, beats: f64) -> String {
    let (_full_key, sample) = render_to_sample(project, 2.0, root);
    let spb = 48_000.0 * 60.0 / project.tempo; // samples per beat
    let s = ((skip * spb) as usize).min(sample.data.len());
    let e = (((skip + beats + 2.0) * spb) as usize).min(sample.data.len());
    let l: Vec<f32> = sample.data[s..e].to_vec();
    let r: Vec<f32> = match &sample.right {
        Some(rc) => rc[s..e].to_vec(),
        None => l.clone(),
    };
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for v in l.iter().chain(r.iter()) {
        for &b in &v.to_bits().to_le_bytes() {
            digest ^= b as u64;
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let key = format!("dig-{digest:016x}");
    dawcore::samples::register_asset(
        &key,
        std::sync::Arc::new(dawcore::dsp::sampler::Sample::stereo(l.into(), r.into(), 48_000.0, root)),
    );
    key
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
    let digs = resolve_digs(&file, base_dir, loader, &mut Vec::new(), &mut diags);
    if !diags.is_empty() {
        return Err(diags);
    }
    if file.song.is_some() {
        compile::compile(&file, &assets, &digs).map(Checked::Song)
    } else if !file.blocks.is_empty() {
        // block library: validate devices AND compile the last block as root
        let diags = compile::validate_devices(&file);
        if !diags.is_empty() {
            return Err(diags);
        }
        compile::compile(&file, &assets, &digs).map(|p| Checked::BlockLibrary {
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

/// Bounce-to-sample: render a project offline into a mono [`Sample`] and
/// register it in the in-memory asset store, keyed by its content digest.
/// The render is the same deterministic engine as playback, so the asset —
/// and everything a sampler later does to it — is bit-identical on every
/// machine. `root` is the MIDI note at which the sample plays back at its
/// bounced pitch (the sampler repitches relative to it).
/// Where cached bounce/dig renders live: $FORTE_RENDER_CACHE, else
/// ~/.cache/forte/renders. None disables the cache (no home, wasm).
#[cfg(not(target_family = "wasm"))]
fn render_cache_dir() -> Option<std::path::PathBuf> {
    let dir = match std::env::var_os("FORTE_RENDER_CACHE") {
        Some(d) if !d.is_empty() => std::path::PathBuf::from(d),
        _ => std::path::PathBuf::from(std::env::var_os("HOME")?).join(".cache/forte/renders"),
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Digest the render INPUTS (the compiled project is deterministic and
/// serializes stably — no maps in the model), so a cache hit is exactly
/// the render that would have happened. The version salt invalidates
/// entries written before the format/render semantics changed (v2 =
/// stereo bounces; v3 = saturate/space switch-index fix).
#[cfg(not(target_family = "wasm"))]
fn render_cache_key(project: &Project, tail_beats: f64, root: u8) -> Option<String> {
    let json = serde_json::to_string(project).ok()?;
    let mut h = fnv1a64(json.as_bytes());
    for &b in tail_beats.to_le_bytes().iter().chain(std::iter::once(&root)).chain(b"v3") {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("{h:016x}"))
}

pub fn render_to_sample(
    project: &dawcore::model::Project,
    tail_beats: f64,
    root: u8,
) -> (String, std::sync::Arc<dawcore::dsp::sampler::Sample>) {
    use dawcore::command::Command;
    use dawcore::engine::Engine;
    use dawcore::sync::full_sync;
    const BLOCK: usize = 512;
    let sr = 48_000.0f32;

    // ---- the audio cache: bounces and digs are deterministic, so they
    // land on disk once and every later compile just reads them back —
    // this is what makes dig-heavy songs compile in milliseconds
    #[cfg(not(target_family = "wasm"))]
    let cache_file = render_cache_key(project, tail_beats, root)
        .and_then(|k| render_cache_dir().map(|d| d.join(format!("{k}.f32"))));
    #[cfg(not(target_family = "wasm"))]
    if let Some(path) = &cache_file {
        if let Ok(bytes) = std::fs::read(path) {
            if bytes.len() >= 8 {
                let n = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
                // v2 layout: [n][L f32 × n][R f32 × n]
                if bytes.len() == 8 + n * 8 {
                    let mut digest = 0xcbf2_9ce4_8422_2325u64;
                    for &b in &bytes[8..] {
                        digest ^= b as u64;
                        digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                    let read_ch = |off: usize| -> Vec<f32> {
                        bytes[off..off + n * 4]
                            .chunks_exact(4)
                            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                            .collect()
                    };
                    let l = read_ch(8);
                    let r = read_ch(8 + n * 4);
                    let key = format!("bounce-{digest:016x}");
                    let sample = std::sync::Arc::new(dawcore::dsp::sampler::Sample::stereo(
                        l.into(),
                        r.into(),
                        sr,
                        root,
                    ));
                    dawcore::samples::register_asset(&key, sample.clone());
                    return (key, sample);
                }
            }
        }
    }
    let (mut engine, mut handle) = Engine::new(sr);
    full_sync(&mut handle, project);
    handle.send(Command::SetLoop { enabled: false, start: 0.0, end: f64::MAX / 4.0 });
    handle.send(Command::SetLaunchQuant(0.0));
    handle.send(Command::Play);

    let total_beats = dawcore::bounce::arrangement_len(project) + tail_beats.max(0.0);
    let seconds = total_beats * 60.0 / project.tempo;
    let total_samples = (seconds * sr as f64) as usize;

    let mut data = Vec::with_capacity(total_samples);
    let mut data_r = Vec::with_capacity(total_samples);
    let mut bl = vec![0.0f32; BLOCK];
    let mut br = vec![0.0f32; BLOCK];
    let mut done = 0;
    while done < total_samples {
        let n = BLOCK.min(total_samples - done);
        engine.process(&mut bl, &mut br, n);
        data.extend_from_slice(&bl[..n]);
        data_r.extend_from_slice(&br[..n]);
        done += n;
    }
    // the asset key digests BOTH channels (L stream then R stream) — the
    // same digest a cache hit recomputes from the file body
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for v in data.iter().chain(data_r.iter()) {
        for &b in &v.to_bits().to_le_bytes() {
            digest ^= b as u64;
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let key = format!("bounce-{digest:016x}");
    // drop the render to disk as audio (atomically), so the next compile
    // of anything that bounces or digs this exact project is a file read
    #[cfg(not(target_family = "wasm"))]
    if let Some(path) = &cache_file {
        let mut bytes = Vec::with_capacity(8 + data.len() * 8);
        bytes.extend_from_slice(&(data.len() as u64).to_le_bytes());
        for v in data.iter().chain(data_r.iter()) {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let tmp = path.with_extension("f32.tmp");
        if std::fs::write(&tmp, &bytes).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
    let sample = std::sync::Arc::new(dawcore::dsp::sampler::Sample::stereo(
        data.into(),
        data_r.into(),
        sr,
        root,
    ));
    dawcore::samples::register_asset(&key, sample.clone());
    (key, sample)
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
