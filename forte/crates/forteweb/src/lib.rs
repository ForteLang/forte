//! Plain C-ABI wasm surface (no wasm-bindgen): small enough to instantiate
//! with zero imports both on the main thread (compile / diagnostics / build
//! digest) and inside an AudioWorkletProcessor (playback with hot reload).
//!
//! Protocol: write source bytes into the buffer returned by
//! `fw_src_prepare(len)`, then call `fw_compile`. 0 = success (the running
//! engine was updated in place, transport untouched); >0 = diagnostic count,
//! fetch JSON via `fw_diags_ptr/len`.

use std::collections::HashMap;

use dawcore::command::Command;
use dawcore::engine::{Engine, EngineHandle};
use dawcore::model::Project;
use dawcore::sync::full_sync;
use fortelang::ModuleLoader;

pub const MAX_FRAMES: usize = 128; // AudioWorklet render quantum

pub struct Ctx {
    engine: Engine,
    handle: EngineHandle,
    src: Vec<u8>,
    stage: Vec<u8>,
    /// Directory of the open file, project-relative ("" = project root) —
    /// imports in the buffer resolve against the module map from here.
    base: String,
    modules: HashMap<String, String>,
    assets: HashMap<String, Vec<u8>>,
    calib_probe: Vec<f32>,
    calib_rec: Vec<f32>,
    calib_conf: f32,
    perform: Vec<f32>,
    transcription: Vec<u8>,
    diags_json: Vec<u8>,
    viz_json: Vec<u8>,
    semdiff_out: Vec<u8>,
    edit_out: Vec<u8>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    prev_tracks: usize,
    project: Option<Project>,
}

/// Imports resolve against maps the page supplies (OPFS files + bundled demo
/// libraries; recorded takes as base64) — the browser's stand-in for a
/// filesystem.
struct MapLoader<'a> {
    text: &'a HashMap<String, String>,
    bin: &'a HashMap<String, Vec<u8>>,
}
impl ModuleLoader for MapLoader<'_> {
    fn load(&self, path: &str) -> Result<String, String> {
        self.text
            .get(path)
            .cloned()
            .ok_or_else(|| "エディタのファイル一覧にありません".into())
    }
    fn load_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        self.bin
            .get(path)
            .cloned()
            .ok_or_else(|| "録音アセットがエディタの一覧にありません".into())
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &c) in TABLE.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c == b'=' || c == b'\n' || c == b'\r' {
            continue;
        }
        let v = rev[c as usize];
        if v == 255 {
            return None;
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// # Safety
/// `ctx` must be a pointer previously returned by [`fw_new`].
unsafe fn ctx<'a>(ptr: *mut Ctx) -> &'a mut Ctx {
    &mut *ptr
}

#[no_mangle]
pub extern "C" fn fw_new(sample_rate: f32) -> *mut Ctx {
    let (engine, handle) = Engine::new(sample_rate);
    Box::into_raw(Box::new(Ctx {
        engine,
        handle,
        src: Vec::new(),
        stage: Vec::new(),
        base: String::new(),
        modules: HashMap::new(),
        assets: HashMap::new(),
        calib_probe: Vec::new(),
        calib_rec: Vec::new(),
        calib_conf: 0.0,
        perform: Vec::new(),
        transcription: Vec::new(),
        diags_json: b"[]".to_vec(),
        viz_json: b"null".to_vec(),
        semdiff_out: Vec::new(),
        edit_out: Vec::new(),
        out_l: vec![0.0; MAX_FRAMES],
        out_r: vec![0.0; MAX_FRAMES],
        prev_tracks: 0,
        project: None,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn fw_src_prepare(ptr: *mut Ctx, len: usize) -> *mut u8 {
    let c = ctx(ptr);
    c.src.clear();
    c.src.resize(len, 0);
    c.src.as_mut_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_compile(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    c.handle.collect_garbage();
    let src = match std::str::from_utf8(&c.src) {
        Ok(s) => s,
        Err(_) => {
            c.diags_json =
                br#"[{"line":1,"col":1,"code":"E-LEX-000","message":"invalid utf-8"}]"#.to_vec();
            return 1;
        }
    };
    let loader = MapLoader { text: &c.modules, bin: &c.assets };
    match fortelang::compile_with_loader(src, &loader, &c.base) {
        Ok(p) => {
            full_sync(&mut c.handle, &p);
            for slot in p.tracks.len()..c.prev_tracks {
                c.handle.send(Command::RemoveTrack { slot });
            }
            c.prev_tracks = p.tracks.len();
            let len = dawcore::bounce::arrangement_len(&p);
            c.handle.send(Command::SetLoop { enabled: true, start: 0.0, end: len });
            c.handle.send(Command::SetLaunchQuant(0.0));
            c.viz_json = viz_json(&p);
            c.project = Some(p);
            c.diags_json = b"[]".to_vec();
            0
        }
        Err(diags) => {
            let arr: Vec<serde_json::Value> = diags
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "line": d.pos.line, "col": d.pos.col,
                        "code": d.code, "message": d.message,
                    })
                })
                .collect();
            c.diags_json = serde_json::to_vec(&arr).unwrap_or_else(|_| b"[]".to_vec());
            diags.len() as i32
        }
    }
}

/// Stage a JSON object `{"path": "source", …}` of importable module files.
#[no_mangle]
pub unsafe extern "C" fn fw_modules_prepare(ptr: *mut Ctx, len: usize) -> *mut u8 {
    let c = ctx(ptr);
    c.stage.clear();
    c.stage.resize(len, 0);
    c.stage.as_mut_ptr()
}

/// Parse the staged module map. Returns the module count, or -1 on bad JSON.
#[no_mangle]
pub unsafe extern "C" fn fw_modules_commit(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    match serde_json::from_slice::<HashMap<String, String>>(&c.stage) {
        Ok(m) => {
            c.modules = m;
            c.modules.len() as i32
        }
        Err(_) => -1,
    }
}

/// Set the open file's project-relative directory from the staged bytes
/// (UTF-8; "" = project root). Imports resolve against the module map from
/// here — the project-mode (`forte daw`) counterpart of a real cwd.
#[no_mangle]
pub unsafe extern "C" fn fw_base_commit(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    match std::str::from_utf8(&c.stage) {
        Ok(s) => {
            c.base = s.to_string();
            0
        }
        Err(_) => -1,
    }
}

/// Parse a staged `{path: base64}` map of binary assets (.frec takes).
/// Returns the asset count, or -1 on bad JSON/base64.
#[no_mangle]
pub unsafe extern "C" fn fw_assets_commit(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    match serde_json::from_slice::<HashMap<String, String>>(&c.stage) {
        Ok(m) => {
            let mut out = HashMap::new();
            for (k, v) in m {
                match base64_decode(&v) {
                    Some(bytes) => {
                        out.insert(k, bytes);
                    }
                    None => return -1,
                }
            }
            c.assets = out;
            c.assets.len() as i32
        }
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fw_diags_ptr(ptr: *mut Ctx) -> *const u8 {
    ctx(ptr).diags_json.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_diags_len(ptr: *mut Ctx) -> usize {
    ctx(ptr).diags_json.len()
}

#[no_mangle]
pub unsafe extern "C" fn fw_play(ptr: *mut Ctx) {
    ctx(ptr).handle.send(Command::Play);
}

#[no_mangle]
pub unsafe extern "C" fn fw_stop(ptr: *mut Ctx) {
    ctx(ptr).handle.send(Command::Stop);
}

/// Jump the playhead to an absolute beat position (the seek bar).
#[no_mangle]
pub unsafe extern "C" fn fw_seek(ptr: *mut Ctx, beats: f64) {
    ctx(ptr).handle.send(Command::Seek(beats));
}

#[no_mangle]
pub unsafe extern "C" fn fw_position(ptr: *mut Ctx) -> f64 {
    ctx(ptr).handle.shared.position_beats()
}

/// Listener-side stem controls (open-stems): mute / solo an engine track.
#[no_mangle]
pub unsafe extern "C" fn fw_set_mute(ptr: *mut Ctx, track: usize, on: i32) {
    ctx(ptr).handle.send(Command::SetTrackMute { track, value: on != 0 });
}

#[no_mangle]
pub unsafe extern "C" fn fw_set_solo(ptr: *mut Ctx, track: usize, on: i32) {
    ctx(ptr).handle.send(Command::SetTrackSolo { track, value: on != 0 });
}

#[no_mangle]
pub unsafe extern "C" fn fw_master_peak(ptr: *mut Ctx) -> f32 {
    ctx(ptr).handle.shared.master_peak()
}

/// Per-track peak of the last block (meters). Slots follow the compiled
/// project's track order — the same order the viz JSON lists.
#[no_mangle]
pub unsafe extern "C" fn fw_track_peak(ptr: *mut Ctx, slot: usize) -> f32 {
    if slot >= dawcore::model::MAX_TRACKS {
        return 0.0;
    }
    ctx(ptr).handle.shared.track_peak(slot)
}

#[no_mangle]
pub unsafe extern "C" fn fw_out_l(ptr: *mut Ctx) -> *const f32 {
    ctx(ptr).out_l.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_out_r(ptr: *mut Ctx) -> *const f32 {
    ctx(ptr).out_r.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_process(ptr: *mut Ctx, frames: usize) {
    let c = ctx(ptr);
    let n = frames.min(MAX_FRAMES);
    // split_at_mut keeps the borrow checker happy without allocating
    let (l, r) = (&mut c.out_l, &mut c.out_r);
    c.engine.process(&mut l[..n], &mut r[..n], n);
}

/// Debug: how many tracks the live engine actually holds (0 = silence).
#[no_mangle]
pub unsafe extern "C" fn fw_debug_tracks(ptr: *mut Ctx) -> i32 {
    ctx(ptr).engine.debug_track_count() as i32
}

/// Read-only visualization data (shared implementation in fortelang::viz).
fn viz_json(p: &Project) -> Vec<u8> {
    serde_json::to_vec(&fortelang::viz::viz_json(p)).unwrap_or_else(|_| b"null".to_vec())
}

/// Semantic diff of two staged snapshots `{"old": {path: text}, "new": {path:
/// text}}` — the same music-vocabulary report as `forte diff`. Returns 0 and
/// stores the report (fw_semdiff_ptr/len), or -1 on bad JSON.
#[no_mangle]
pub unsafe extern "C" fn fw_semdiff(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    fn conv(v: Option<&serde_json::Value>) -> Option<fortelang::vcs::Snapshot> {
        let map = v?.as_object()?;
        let mut snap = fortelang::vcs::Snapshot::new();
        for (k, val) in map {
            snap.insert(k.clone(), val.as_str()?.as_bytes().to_vec());
        }
        Some(snap)
    }
    let parsed: Option<(fortelang::vcs::Snapshot, fortelang::vcs::Snapshot)> =
        serde_json::from_slice::<serde_json::Value>(&c.stage)
            .ok()
            .and_then(|v| Some((conv(v.get("old"))?, conv(v.get("new"))?)));
    match parsed {
        Some((old, new)) => {
            let report = fortelang::semdiff::diff_snapshots(&old, &new);
            c.semdiff_out =
                if report.is_empty() { "変更なし".as_bytes().to_vec() } else { report.into_bytes() };
            0
        }
        None => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fw_semdiff_ptr(ptr: *mut Ctx) -> *const u8 {
    ctx(ptr).semdiff_out.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_semdiff_len(ptr: *mut Ctx) -> usize {
    ctx(ptr).semdiff_out.len()
}

// ---- lossless structured edits (Studio P0, issue #135) -----------------------

/// Apply `fortelang::edit` ops. Source: [`fw_src_prepare`]; ops JSON (one
/// object or an array): the staging buffer ([`fw_modules_prepare`]).
/// Returns 0 (edited source via `fw_edit_ptr/len`) or -1 (error message in
/// the same buffer). Comments and layout outside the edited tokens survive
/// byte-for-byte — this is the GUI's write path to the code.
#[no_mangle]
pub unsafe extern "C" fn fw_edit(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    let (src, ops_json) = match (std::str::from_utf8(&c.src), std::str::from_utf8(&c.stage)) {
        (Ok(s), Ok(o)) => (s, o),
        _ => {
            c.edit_out = b"invalid utf-8".to_vec();
            return -1;
        }
    };
    match fortelang::edit::parse_ops(ops_json)
        .and_then(|ops| fortelang::edit::apply_ops(src, &ops))
    {
        Ok(out) => {
            c.edit_out = out.into_bytes();
            0
        }
        Err(d) => {
            c.edit_out = d.to_string().into_bytes();
            -1
        }
    }
}

/// Editable music literals of the staged source ([`fw_src_prepare`]) as a
/// JSON array of sites — each carries the exact coordinates its
/// `set_pattern` op takes. Returns the site count (JSON via
/// `fw_edit_ptr/len`) or -1 (error message in the same buffer).
#[no_mangle]
pub unsafe extern "C" fn fw_pattern_sites(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    let src = match std::str::from_utf8(&c.src) {
        Ok(s) => s,
        Err(_) => {
            c.edit_out = b"invalid utf-8".to_vec();
            return -1;
        }
    };
    match fortelang::edit::pattern_sites(src) {
        Ok(sites) => {
            let n = sites.len() as i32;
            c.edit_out = serde_json::to_vec(&sites).unwrap_or_else(|_| b"[]".to_vec());
            n
        }
        Err(d) => {
            c.edit_out = d.to_string().into_bytes();
            -1
        }
    }
}

/// Instrument/insert calls of the staged source as JSON sites — each
/// carries the exact coordinates its `set_arg` op takes (the inspector's
/// read side). Same buffer protocol as [`fw_pattern_sites`].
#[no_mangle]
pub unsafe extern "C" fn fw_arg_sites(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    let src = match std::str::from_utf8(&c.src) {
        Ok(s) => s,
        Err(_) => {
            c.edit_out = b"invalid utf-8".to_vec();
            return -1;
        }
    };
    match fortelang::edit::arg_sites(src) {
        Ok(sites) => {
            let n = sites.len() as i32;
            c.edit_out = serde_json::to_vec(&sites).unwrap_or_else(|_| b"[]".to_vec());
            n
        }
        Err(d) => {
            c.edit_out = d.to_string().into_bytes();
            -1
        }
    }
}

/// Parse the STAGED bytes as a `notes` literal → `{"len":…, "notes":[…]}`
/// JSON in the edit buffer (the piano roll's read side). Returns the note
/// count, or -1 with the error message in the same buffer.
#[no_mangle]
pub unsafe extern "C" fn fw_notes_parse(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    let raw = match std::str::from_utf8(&c.stage) {
        Ok(s) => s,
        Err(_) => {
            c.edit_out = b"invalid utf-8".to_vec();
            return -1;
        }
    };
    match fortelang::edit::note_events(raw) {
        Ok(doc) => {
            let n = doc.notes.len() as i32;
            c.edit_out = serde_json::to_vec(&doc).unwrap_or_else(|_| b"{}".to_vec());
            n
        }
        Err(d) => {
            c.edit_out = d.to_string().into_bytes();
            -1
        }
    }
}

/// Serialize STAGED `{"len":…, "notes":[…]}` JSON back to idiomatic
/// `notes` text in the edit buffer (the piano roll's write side, fed to
/// `set_pattern`). Returns 0, or -1 with the error message in the buffer.
#[no_mangle]
pub unsafe extern "C" fn fw_notes_write(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    let doc: fortelang::edit::NotesDoc = match serde_json::from_slice(&c.stage) {
        Ok(d) => d,
        Err(e) => {
            c.edit_out = format!("bad notes json: {e}").into_bytes();
            return -1;
        }
    };
    match fortelang::edit::serialize_notes(&doc) {
        Ok(s) => {
            c.edit_out = s.into_bytes();
            0
        }
        Err(d) => {
            c.edit_out = d.to_string().into_bytes();
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn fw_edit_ptr(ptr: *mut Ctx) -> *const u8 {
    ctx(ptr).edit_out.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_edit_len(ptr: *mut Ctx) -> usize {
    ctx(ptr).edit_out.len()
}

#[no_mangle]
pub unsafe extern "C" fn fw_viz_ptr(ptr: *mut Ctx) -> *const u8 {
    ctx(ptr).viz_json.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_viz_len(ptr: *mut Ctx) -> usize {
    ctx(ptr).viz_json.len()
}

// ---- live performance (monitoring + transcription) ---------------------------

/// Live note input, routed to the first track's instrument (works with the
/// transport stopped — free-running monitoring).
#[no_mangle]
pub unsafe extern "C" fn fw_note(ptr: *mut Ctx, on: i32, pitch: u32, vel: f32) {
    let c = ctx(ptr);
    if on != 0 {
        c.handle.send(Command::NoteOn { track: 0, note: pitch as u8, velocity: vel });
    } else {
        c.handle.send(Command::NoteOff { track: 0, note: pitch as u8 });
    }
}

/// Stage `n` played notes as (start_beats, len_beats, pitch) triples.
#[no_mangle]
pub unsafe extern "C" fn fw_perform_buf(ptr: *mut Ctx, n: usize) -> *mut f32 {
    let c = ctx(ptr);
    c.perform.clear();
    c.perform.resize(n * 3, 0.0);
    c.perform.as_mut_ptr()
}

/// Quantize the staged performance to `grid` beats and render the body of a
/// `notes` literal. Returns its length (0 = empty take); fetch via
/// [`fw_transcribe_ptr`].
#[no_mangle]
pub unsafe extern "C" fn fw_transcribe(ptr: *mut Ctx, grid: f32) -> usize {
    let c = ctx(ptr);
    let notes: Vec<fortelang::perform::PlayedNote> = c
        .perform
        .chunks_exact(3)
        .map(|t| fortelang::perform::PlayedNote {
            start: t[0] as f64,
            len: t[1] as f64,
            pitch: t[2] as u8,
        })
        .collect();
    c.transcription = fortelang::perform::transcribe(&notes, grid as f64)
        .unwrap_or_default()
        .into_bytes();
    c.transcription.len()
}

#[no_mangle]
pub unsafe extern "C" fn fw_transcribe_ptr(ptr: *mut Ctx) -> *const u8 {
    ctx(ptr).transcription.as_ptr()
}

// ---- loopback calibration ---------------------------------------------------

/// The chirp probe the page must play (written into wasm memory).
#[no_mangle]
pub unsafe extern "C" fn fw_calib_probe(ptr: *mut Ctx, rate: f32, seconds: f32) -> *const f32 {
    let c = ctx(ptr);
    c.calib_probe = fortelang::calib::chirp(rate, seconds);
    c.calib_probe.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_calib_probe_len(ptr: *mut Ctx) -> usize {
    ctx(ptr).calib_probe.len()
}

/// Buffer for the recording captured while the probe played.
#[no_mangle]
pub unsafe extern "C" fn fw_calib_rec(ptr: *mut Ctx, len: usize) -> *mut f32 {
    let c = ctx(ptr);
    c.calib_rec.clear();
    c.calib_rec.resize(len, 0.0);
    c.calib_rec.as_mut_ptr()
}

/// Cross-correlate. Returns the lag in samples, or -1 when the probe was not
/// heard (confidence too low). Confidence via [`fw_calib_confidence`].
#[no_mangle]
pub unsafe extern "C" fn fw_calib_run(ptr: *mut Ctx) -> i32 {
    let c = ctx(ptr);
    match fortelang::calib::estimate_delay(&c.calib_probe, &c.calib_rec) {
        Some((lag, conf)) => {
            c.calib_conf = conf;
            lag as i32
        }
        None => {
            c.calib_conf = 0.0;
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn fw_calib_confidence(ptr: *mut Ctx) -> f32 {
    ctx(ptr).calib_conf
}

/// Offline build digest — byte-for-byte the same path as `forte build`
/// (48 kHz, 512-sample blocks, 8-beat tail). Returning the same value as the
/// native CLI proves browser/native bit-identity.
#[no_mangle]
pub unsafe extern "C" fn fw_digest(ptr: *mut Ctx) -> u64 {
    match &ctx(ptr).project {
        Some(p) => fortelang::render_digest(p, 8.0).f32_digest,
        None => 0,
    }
}
