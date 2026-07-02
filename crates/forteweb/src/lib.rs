//! Plain C-ABI wasm surface (no wasm-bindgen): small enough to instantiate
//! with zero imports both on the main thread (compile / diagnostics / build
//! digest) and inside an AudioWorkletProcessor (playback with hot reload).
//!
//! Protocol: write source bytes into the buffer returned by
//! `fw_src_prepare(len)`, then call `fw_compile`. 0 = success (the running
//! engine was updated in place, transport untouched); >0 = diagnostic count,
//! fetch JSON via `fw_diags_ptr/len`.

use dawcore::command::Command;
use dawcore::engine::{Engine, EngineHandle};
use dawcore::model::Project;
use dawcore::sync::full_sync;

pub const MAX_FRAMES: usize = 128; // AudioWorklet render quantum

pub struct Ctx {
    engine: Engine,
    handle: EngineHandle,
    src: Vec<u8>,
    diags_json: Vec<u8>,
    viz_json: Vec<u8>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    prev_tracks: usize,
    project: Option<Project>,
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
        diags_json: b"[]".to_vec(),
        viz_json: b"null".to_vec(),
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
    match fortelang::compile_str(src) {
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

#[no_mangle]
pub unsafe extern "C" fn fw_position(ptr: *mut Ctx) -> f64 {
    ctx(ptr).handle.shared.position_beats()
}

#[no_mangle]
pub unsafe extern "C" fn fw_master_peak(ptr: *mut Ctx) -> f32 {
    ctx(ptr).handle.shared.master_peak()
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

/// Read-only visualization data derived from the compiled project (the code
/// is the only editable truth — views are projections of it, SYS-EDT-003).
fn viz_json(p: &Project) -> Vec<u8> {
    let beats_per_bar = p.time_sig.0 as f64 * 4.0 / p.time_sig.1 as f64;
    let tracks: Vec<serde_json::Value> = p
        .tracks
        .iter()
        .map(|t| {
            let clips: Vec<serde_json::Value> = t
                .arranger
                .iter()
                .map(|a| {
                    let notes: Vec<[f64; 3]> = a
                        .clip
                        .notes
                        .iter()
                        .map(|n| [n.pitch as f64, n.start, n.length])
                        .collect();
                    serde_json::json!({
                        "start": a.start, "duration": a.duration,
                        "length": a.clip.length, "notes": notes,
                    })
                })
                .collect();
            serde_json::json!({
                "name": t.name,
                "color": t.color,
                "fx": t.kind == dawcore::model::TrackKind::Effect,
                "clips": clips,
            })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "tempo": p.tempo,
        "beatsPerBar": beats_per_bar,
        "lengthBeats": dawcore::bounce::arrangement_len(p),
        "tracks": tracks,
    }))
    .unwrap_or_else(|_| b"null".to_vec())
}

#[no_mangle]
pub unsafe extern "C" fn fw_viz_ptr(ptr: *mut Ctx) -> *const u8 {
    ctx(ptr).viz_json.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn fw_viz_len(ptr: *mut Ctx) -> usize {
    ctx(ptr).viz_json.len()
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
