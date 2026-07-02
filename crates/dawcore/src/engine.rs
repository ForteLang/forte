//! The real-time audio engine. Runs entirely on the audio callback thread.
//!
//! Invariants on the audio thread: no allocation, no locks, no syscalls. All of
//! that happens on the UI thread; structural objects arrive pre-built via the
//! command channel and leave via the garbage channel.

use std::array;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::command::{Command, Garbage};
use crate::device::{build_dsp, BlockCtx, Dsp, NoteEvent};
use crate::dsp::sampler::Sample;
use crate::model::{self, Device, DeviceKind, ModKind, SampleSource, Track, MAX_TRACKS};
use crate::samples;

const CMD_CAPACITY: usize = 1024;
const GARBAGE_CAPACITY: usize = 1024;
const MAX_BLOCK: usize = 8192;

// ---------------------------------------------------------------------------
// Shared, lock-free readback (audio thread writes, UI thread reads)
// ---------------------------------------------------------------------------

pub struct Shared {
    pub playing: AtomicBool,
    position_beats: AtomicU64,
    master_peak: AtomicU32,
    track_peak: [AtomicU32; MAX_TRACKS],
    active_scene: [AtomicI32; MAX_TRACKS],
    pub active_voices: AtomicU32,
}

impl Shared {
    fn new() -> Self {
        Self {
            playing: AtomicBool::new(false),
            position_beats: AtomicU64::new(0),
            master_peak: AtomicU32::new(0),
            track_peak: array::from_fn(|_| AtomicU32::new(0)),
            active_scene: array::from_fn(|_| AtomicI32::new(-1)),
            active_voices: AtomicU32::new(0),
        }
    }
    pub fn position_beats(&self) -> f64 {
        f64::from_bits(self.position_beats.load(Ordering::Relaxed))
    }
    pub fn master_peak(&self) -> f32 {
        f32::from_bits(self.master_peak.load(Ordering::Relaxed))
    }
    pub fn track_peak(&self, slot: usize) -> f32 {
        f32::from_bits(self.track_peak[slot].load(Ordering::Relaxed))
    }
    pub fn active_scene(&self, slot: usize) -> i32 {
        self.active_scene[slot].load(Ordering::Relaxed)
    }
}

/// UI-side handle: send commands, reclaim garbage, read meters.
pub struct EngineHandle {
    pub cmd: HeapProd<Command>,
    pub garbage: HeapCons<Garbage>,
    pub shared: Arc<Shared>,
    pub sample_rate: f32,
}

impl EngineHandle {
    pub fn send(&mut self, cmd: Command) {
        // If the queue is momentarily full we drop the message rather than block
        // the UI; hot params are idempotent so the next one wins anyway.
        let _ = self.cmd.try_push(cmd);
    }
    /// Drop any heap objects the audio thread handed back. Call each UI frame.
    pub fn collect_garbage(&mut self) {
        while self.garbage.try_pop().is_some() {}
    }
}

// ---------------------------------------------------------------------------
// Engine-side data (audio thread owns these)
// ---------------------------------------------------------------------------

pub struct EngineNote {
    pub pitch: u8,
    pub start: f64,
    pub length: f64,
    pub velocity: f32,
}

pub struct EngineClip {
    pub length: f64,
    pub notes: Vec<EngineNote>,
}

/// Resolve a serialisable sample source into a shared buffer (UI thread only).
pub fn resolve_sample(src: &SampleSource) -> Option<Arc<Sample>> {
    match src {
        SampleSource::None => None,
        SampleSource::Builtin(name) => match name.as_str() {
            "Kick" => Some(samples::kick()),
            "Snare" => Some(samples::snare()),
            "Hat" => Some(samples::hat()),
            _ => None,
        },
        SampleSource::File(path) => samples::load_wav(std::path::Path::new(path), 60).ok(),
    }
}

pub struct EngineDevice {
    #[allow(dead_code)]
    kind: DeviceKind,
    enabled: bool,
    base_params: Vec<f32>,
    eff_params: Vec<f32>, // reused scratch: base + modulation
    dsp: Dsp,
}

/// A modulator targeting (device_index, param_index) pairs on its track.
pub struct EngineMod {
    kind: ModKind,
    phase: f32,
    rate: f32, // 0..1
    shape: u8,
    steps: Vec<f32>,
    value: f32, // macro value / random smoothing factor
    // runtime state
    rand_cur: f32,
    rand_target: f32,
    routes: Vec<(usize, usize, f32)>, // (device idx, param idx, amount)
}

pub struct EngineMods(pub Vec<EngineMod>);

pub struct EngineArrClip {
    start: f64,
    duration: f64,
    content_len: f64,
    notes: Vec<EngineNote>,
}

/// One automation point mirrored on the audio thread.
#[derive(Clone, Copy)]
pub struct EngineAutoPoint {
    pub beat: f64,
    pub value: f32,
    pub hold: bool,
}

pub struct EngineTrack {
    devices: Vec<EngineDevice>,
    mods: Vec<EngineMod>,
    gain: f32,
    pan: f32,
    mute: bool,
    solo: bool,
    is_effect: bool,
    /// Post-fader sends: (destination slot, level).
    sends: Vec<(usize, f32)>,
    /// Volume automation, sorted by beat. Overrides `gain` while playing.
    automation: Vec<EngineAutoPoint>,
    active_scene: i32,
    /// Quantized launch request awaiting the next quant boundary.
    pending_scene: Option<i32>,
    clips: Vec<Option<Box<EngineClip>>>,
    arranger: Vec<EngineArrClip>,
    audio_clips: Vec<EngineAudioClip>,
    pending_offs: Vec<(u8, f64)>,
    events: Vec<NoteEvent>,
    /// Reused buffer for the Note-FX chain (ping-pong with `events`).
    scratch_events: Vec<NoteEvent>,
    /// Live input (computer keyboard / MIDI) queued for the next block so it
    /// flows through the Note-FX chain like any scheduled note.
    live_events: Vec<NoteEvent>,
}

// ---------------------------------------------------------------------------
// Builders (run on the UI thread — allocation allowed here)
// ---------------------------------------------------------------------------

pub fn build_device(dev: &Device, sr: f32) -> Box<EngineDevice> {
    let mut dsp = build_dsp(dev, sr);
    // initial param application so a freshly added device sounds right
    match &mut dsp {
        Dsp::Note(fx) => fx.configure(&dev.params),
        Dsp::Inst(i) => i.configure(&dev.params),
        Dsp::Audio(fx) => fx.configure(&dev.params),
    }
    Box::new(EngineDevice {
        kind: dev.kind,
        enabled: dev.enabled,
        base_params: dev.params.clone(),
        eff_params: dev.params.clone(),
        dsp,
    })
}

pub struct EngineAudioClip {
    start: f64,
    duration: f64,
    data: Arc<[f32]>,
    src_sr: f32,
    gain: f32,
}

fn build_audio_clips(track: &Track) -> Vec<EngineAudioClip> {
    track
        .audio_clips
        .iter()
        .filter_map(|c| {
            let s = resolve_sample(&c.source)?;
            Some(EngineAudioClip {
                start: c.start,
                duration: c.duration,
                data: s.data.clone(),
                src_sr: s.sample_rate,
                gain: c.gain,
            })
        })
        .collect()
}

pub fn build_clip(clip: &model::Clip) -> Box<EngineClip> {
    Box::new(EngineClip {
        length: clip.length,
        notes: clip
            .notes
            .iter()
            .map(|n| EngineNote {
                pitch: n.pitch,
                start: n.start,
                length: n.length,
                velocity: n.velocity as f32 / 127.0,
            })
            .collect(),
    })
}

pub fn build_mods(track: &Track) -> Box<EngineMods> {
    // collect per-device modulators into flat track-level mods
    let mut mods = Vec::new();
    for (di, dev) in track.devices.iter().enumerate() {
        for m in &dev.modulators {
            mods.push(EngineMod {
                kind: m.kind,
                phase: 0.0,
                rate: m.rate,
                shape: m.shape,
                steps: m.steps.clone(),
                value: m.value,
                rand_cur: 0.0,
                rand_target: 0.0,
                routes: m.routes.iter().map(|r| (di, r.param, r.amount)).collect(),
            });
        }
    }
    Box::new(EngineMods(mods))
}

fn build_arranger(track: &Track) -> Vec<EngineArrClip> {
    track
        .arranger
        .iter()
        .map(|a| EngineArrClip {
            start: a.start,
            duration: a.duration,
            content_len: a.clip.length.max(0.0625),
            notes: a
                .clip
                .notes
                .iter()
                .map(|n| EngineNote {
                    pitch: n.pitch,
                    start: n.start,
                    length: n.length,
                    velocity: n.velocity as f32 / 127.0,
                })
                .collect(),
        })
        .collect()
}

pub fn build_track(track: &Track, sr: f32) -> Box<EngineTrack> {
    let devices = track.devices.iter().map(|d| *build_device(d, sr)).collect();
    let clips = track
        .clips
        .iter()
        .map(|c| c.as_ref().map(build_clip))
        .collect();
    let et = EngineTrack {
        devices,
        mods: build_mods(track).0,
        gain: track.volume,
        pan: track.pan,
        mute: track.mute,
        solo: track.solo,
        is_effect: track.kind == crate::model::TrackKind::Effect,
        sends: track.sends.clone(),
        automation: build_automation(track),
        active_scene: -1,
        pending_scene: None,
        clips,
        arranger: build_arranger(track),
        audio_clips: build_audio_clips(track),
        pending_offs: Vec::with_capacity(64),
        events: Vec::with_capacity(1024),
        scratch_events: Vec::with_capacity(1024),
        live_events: Vec::with_capacity(128),
    };
    Box::new(et)
}

pub fn build_automation(track: &Track) -> Vec<EngineAutoPoint> {
    let mut pts: Vec<EngineAutoPoint> = track
        .volume_automation
        .iter()
        .map(|p| EngineAutoPoint { beat: p.beat, value: p.value, hold: p.hold })
        .collect();
    pts.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
    pts
}

/// Piecewise evaluation honouring Bitwig 6's per-point `hold` behaviour.
fn eval_auto(points: &[EngineAutoPoint], beat: f64) -> Option<f32> {
    let first = points.first()?;
    if beat <= first.beat {
        return Some(first.value);
    }
    for w in points.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if beat < b.beat {
            if a.hold || (b.beat - a.beat) < 1e-9 {
                return Some(a.value);
            }
            let t = ((beat - a.beat) / (b.beat - a.beat)) as f32;
            return Some(a.value + (b.value - a.value) * t);
        }
    }
    points.last().map(|p| p.value)
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

pub struct Engine {
    sample_rate: f32,
    cmd: HeapCons<Command>,
    garbage: HeapProd<Garbage>,
    shared: Arc<Shared>,

    tracks: Vec<Option<Box<EngineTrack>>>,
    tempo: f64,
    playing: bool,
    position_beats: f64,

    loop_enabled: bool,
    loop_start: f64,
    loop_end: f64,
    launch_quant: f64,

    // metronome click synth
    metronome: bool,
    beats_per_bar: f64,
    click_env: f32,
    click_phase: f32,
    click_freq: f32,
    last_click_beat: i64,

    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    /// Audio-clip injection buffer for the track currently being rendered.
    audio_l: Vec<f32>,
    audio_r: Vec<f32>,
    /// Per-slot send buses feeding effect tracks (pre-allocated, audio thread
    /// never resizes them).
    send_l: Vec<Vec<f32>>,
    send_r: Vec<Vec<f32>>,
}

impl Engine {
    pub fn new(sample_rate: f32) -> (Engine, EngineHandle) {
        let cmd_rb = HeapRb::<Command>::new(CMD_CAPACITY);
        let (cmd_prod, cmd_cons) = cmd_rb.split();
        let garbage_rb = HeapRb::<Garbage>::new(GARBAGE_CAPACITY);
        let (garbage_prod, garbage_cons) = garbage_rb.split();
        let shared = Arc::new(Shared::new());

        let mut tracks = Vec::with_capacity(MAX_TRACKS);
        for _ in 0..MAX_TRACKS {
            tracks.push(None);
        }

        let engine = Engine {
            sample_rate,
            cmd: cmd_cons,
            garbage: garbage_prod,
            shared: shared.clone(),
            tracks,
            tempo: 120.0,
            playing: false,
            position_beats: 0.0,
            loop_enabled: false,
            loop_start: 0.0,
            loop_end: 32.0,
            launch_quant: 4.0,
            metronome: false,
            beats_per_bar: 4.0,
            click_env: 0.0,
            click_phase: 0.0,
            click_freq: 880.0,
            last_click_beat: -1,
            scratch_l: vec![0.0; MAX_BLOCK],
            scratch_r: vec![0.0; MAX_BLOCK],
            audio_l: vec![0.0; MAX_BLOCK],
            audio_r: vec![0.0; MAX_BLOCK],
            send_l: (0..MAX_TRACKS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            send_r: (0..MAX_TRACKS).map(|_| vec![0.0; MAX_BLOCK]).collect(),
        };

        let handle = EngineHandle { cmd: cmd_prod, garbage: garbage_cons, shared, sample_rate };
        (engine, handle)
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    fn recycle(&mut self, g: Garbage) {
        let _ = self.garbage.try_push(g);
    }

    fn drain_commands(&mut self) {
        while let Some(cmd) = self.cmd.try_pop() {
            self.apply(cmd);
        }
    }

    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Play => {
                self.playing = true;
            }
            Command::Stop => {
                self.playing = false;
                self.last_click_beat = -1;
                self.position_beats = if self.loop_enabled { self.loop_start } else { 0.0 };
                for t in self.tracks.iter_mut().flatten() {
                    t.pending_offs.clear();
                    t.pending_scene = None;
                    t.all_instruments_off();
                }
            }
            Command::SetTempo(bpm) => self.tempo = bpm.clamp(20.0, 300.0),
            Command::SetLoop { enabled, start, end } => {
                self.loop_enabled = enabled;
                self.loop_start = start.max(0.0);
                self.loop_end = end.max(start + 0.25);
            }
            Command::SetLaunchQuant(q) => self.launch_quant = q.max(0.0),
            Command::SetMetronome(on) => self.metronome = on,
            Command::SetSends { track, mut sends } => {
                if let Some(t) = self.track_mut(track) {
                    // swap contents so the displaced Vec rides the same Box back
                    std::mem::swap(&mut t.sends, &mut *sends);
                }
                self.recycle(Garbage::Sends(sends));
            }
            Command::SetAutomation { track, mut points } => {
                if let Some(t) = self.track_mut(track) {
                    std::mem::swap(&mut t.automation, &mut *points);
                }
                self.recycle(Garbage::Auto(points));
            }
            Command::LaunchClip { track, scene } => {
                let immediate = self.launch_quant <= 0.0 || !self.playing;
                if let Some(t) = self.track_mut(track) {
                    if immediate {
                        t.active_scene = scene as i32;
                        t.pending_scene = None;
                    } else {
                        t.pending_scene = Some(scene as i32);
                    }
                }
            }
            Command::LaunchScene(scene) => {
                let immediate = self.launch_quant <= 0.0 || !self.playing;
                for t in self.tracks.iter_mut().flatten() {
                    if scene < t.clips.len() && t.clips[scene].is_some() {
                        if immediate {
                            t.active_scene = scene as i32;
                            t.pending_scene = None;
                        } else {
                            t.pending_scene = Some(scene as i32);
                        }
                    }
                }
            }
            Command::StopTrack { track } => {
                if let Some(t) = self.track_mut(track) {
                    t.active_scene = -1;
                    t.pending_scene = None;
                }
            }
            Command::NoteOn { track, note, velocity } => {
                if let Some(t) = self.track_mut(track) {
                    t.queue_live(true, note, velocity);
                }
            }
            Command::NoteOff { track, note } => {
                if let Some(t) = self.track_mut(track) {
                    t.queue_live(false, note, 0.0);
                }
            }
            Command::SetTrackGain { track, value } => {
                if let Some(t) = self.track_mut(track) {
                    t.gain = value;
                }
            }
            Command::SetTrackPan { track, value } => {
                if let Some(t) = self.track_mut(track) {
                    t.pan = value;
                }
            }
            Command::SetTrackMute { track, value } => {
                if let Some(t) = self.track_mut(track) {
                    t.mute = value;
                }
            }
            Command::SetTrackSolo { track, value } => {
                if let Some(t) = self.track_mut(track) {
                    t.solo = value;
                }
            }
            Command::SetParam { track, device, param, value } => {
                if let Some(t) = self.track_mut(track) {
                    if let Some(d) = t.devices.get_mut(device) {
                        if let Some(p) = d.base_params.get_mut(param) {
                            *p = value;
                        }
                    }
                }
            }
            Command::SetDeviceEnabled { track, device, value } => {
                if let Some(t) = self.track_mut(track) {
                    if let Some(d) = t.devices.get_mut(device) {
                        d.enabled = value;
                    }
                }
            }
            Command::SetModulator { track, mod_index, rate, shape } => {
                if let Some(t) = self.track_mut(track) {
                    if let Some(m) = t.mods.get_mut(mod_index) {
                        m.rate = rate;
                        m.shape = shape;
                    }
                }
            }
            Command::SetGridParam { track, device, node, param, value } => {
                if let Some(t) = self.track_mut(track) {
                    if let Some(d) = t.devices.get_mut(device) {
                        if let Dsp::Inst(i) = &mut d.dsp {
                            i.set_node_param(node, param, value);
                        }
                    }
                }
            }
            Command::AddTrack { slot, track } => {
                if slot < MAX_TRACKS {
                    if let Some(old) = self.tracks[slot].take() {
                        self.recycle(Garbage::Track(old));
                    }
                    self.tracks[slot] = Some(track);
                }
            }
            Command::RemoveTrack { slot } => {
                if slot < MAX_TRACKS {
                    if let Some(old) = self.tracks[slot].take() {
                        self.recycle(Garbage::Track(old));
                    }
                }
            }
            Command::SetClip { track, scene, clip } => {
                let old = self.track_mut(track).and_then(|t| {
                    if scene < t.clips.len() {
                        let old = t.clips[scene].take();
                        t.clips[scene] = clip;
                        old
                    } else {
                        None
                    }
                });
                if let Some(old) = old {
                    self.recycle(Garbage::Clip(old));
                }
            }
            Command::AddDevice { track, device } => {
                if let Some(t) = self.track_mut(track) {
                    t.devices.push(*device);
                }
            }
            Command::RemoveDevice { track, device } => {
                let removed = self.track_mut(track).and_then(|t| {
                    if device < t.devices.len() {
                        Some(t.devices.remove(device))
                    } else {
                        None
                    }
                });
                if let Some(removed) = removed {
                    self.recycle(Garbage::Device(Box::new(removed)));
                }
            }
            Command::SetModRoutes { track, modulators } => {
                let old = self
                    .track_mut(track)
                    .map(|t| std::mem::replace(&mut t.mods, modulators.0));
                if let Some(old) = old {
                    self.recycle(Garbage::Mods(Box::new(EngineMods(old))));
                }
            }
        }
    }

    #[inline]
    fn track_mut(&mut self, slot: usize) -> Option<&mut EngineTrack> {
        self.tracks.get_mut(slot).and_then(|o| o.as_deref_mut())
    }

    /// Render `frames` of stereo audio. The audio backend calls this.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) {
        self.drain_commands();

        let frames = frames.min(out_l.len()).min(out_r.len()).min(MAX_BLOCK);
        for i in 0..frames {
            out_l[i] = 0.0;
            out_r[i] = 0.0;
        }

        let spb = self.sample_rate as f64 * 60.0 / self.tempo; // samples per beat
        let dt_beats = 1.0 / spb;
        let block_start = self.position_beats;
        let block_end = if self.playing {
            block_start + frames as f64 * dt_beats
        } else {
            block_start
        };
        let block_seconds = frames as f32 / self.sample_rate;

        // Launch quantization: commit pending launcher clips at the next quant
        // boundary that falls inside this block (Bitwig default = 1 bar).
        if self.playing && self.launch_quant > 0.0 {
            let q = self.launch_quant;
            let boundary = ((block_start / q) - 1e-9).ceil() * q;
            if boundary < block_end {
                for t in self.tracks.iter_mut().flatten() {
                    if let Some(ps) = t.pending_scene.take() {
                        t.active_scene = ps;
                    }
                }
            }
        }

        // Don't schedule notes past the loop end; the playhead wraps there.
        let sched_end = if self.loop_enabled && self.playing {
            block_end.min(self.loop_end)
        } else {
            block_end
        };

        let any_solo = self
            .tracks
            .iter()
            .flatten()
            .any(|t| t.solo);

        let mut master_peak = 0.0f32;

        // which slots are effect (return) tracks — guards stale sends
        let mut is_fx = [false; MAX_TRACKS];
        for (slot, t) in self.tracks.iter().enumerate() {
            if let Some(t) = t {
                is_fx[slot] = t.is_effect;
                if t.is_effect {
                    self.send_l[slot][..frames].fill(0.0);
                    self.send_r[slot][..frames].fill(0.0);
                }
            }
        }

        // ---- pass 1: source tracks (instrument/audio) -----------------------
        for slot in 0..self.tracks.len() {
            let Some(track) = self.tracks[slot].as_deref_mut() else { continue };
            if track.is_effect {
                continue;
            }

            // advance modulators (block rate) and bake effective params
            track.update_modulators(block_seconds);

            // schedule note events for this block, then run the Note-FX chain
            track.schedule(self.playing, block_start, sched_end, spb, frames);
            let ctx = BlockCtx {
                sample_rate: self.sample_rate,
                frames,
                start_beat: block_start,
                end_beat: sched_end,
                samples_per_beat: spb,
                playing: self.playing,
            };
            track.apply_note_fx(&ctx);

            // render audio clips into the injection buffer, then the track
            let has_audio = !track.audio_clips.is_empty();
            if has_audio {
                track.fill_audio(
                    &mut self.audio_l,
                    &mut self.audio_r,
                    self.playing,
                    block_start,
                    dt_beats,
                    self.sample_rate,
                    self.tempo,
                    frames,
                );
            }
            let inject = if has_audio {
                Some((&self.audio_l[..], &self.audio_r[..]))
            } else {
                None
            };
            track.render(inject, &mut self.scratch_l, &mut self.scratch_r, frames);

            // volume automation overrides the fader while the transport runs
            let vol = if self.playing {
                eval_auto(&track.automation, block_start).unwrap_or(track.gain)
            } else {
                track.gain
            };

            // mix into master with gain / pan / mute / solo
            let audible = track.solo || (!track.mute && !(any_solo && !track.solo));
            let g = if audible { vol * vol } else { 0.0 };
            let theta = (track.pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
            let (pl, pr) = (crate::dmath::cos(theta), crate::dmath::sin(theta));

            let mut tpeak = 0.0f32;
            for i in 0..frames {
                let l = self.scratch_l[i] * g * pl;
                let r = self.scratch_r[i] * g * pr;
                out_l[i] += l;
                out_r[i] += r;
                tpeak = tpeak.max(l.abs()).max(r.abs());
            }

            // post-fader sends into effect-track buses
            for &(dest, level) in &track.sends {
                if dest < MAX_TRACKS && is_fx[dest] && level > 0.0001 {
                    let dl = &mut self.send_l[dest];
                    let dr = &mut self.send_r[dest];
                    for i in 0..frames {
                        dl[i] += self.scratch_l[i] * g * pl * level;
                        dr[i] += self.scratch_r[i] * g * pr * level;
                    }
                }
            }

            self.shared.track_peak[slot].store(tpeak.to_bits(), Ordering::Relaxed);
            self.shared.active_scene[slot].store(
                if self.playing { track.active_scene } else { -1 },
                Ordering::Relaxed,
            );
        }

        // ---- pass 2: effect (return) tracks ---------------------------------
        for slot in 0..self.tracks.len() {
            if !is_fx[slot] {
                continue;
            }
            let Some(track) = self.tracks[slot].as_deref_mut() else { continue };

            track.update_modulators(block_seconds);
            track.render(
                Some((&self.send_l[slot], &self.send_r[slot])),
                &mut self.scratch_l,
                &mut self.scratch_r,
                frames,
            );

            let audible = track.solo || (!track.mute && !(any_solo && !track.solo));
            let g = if audible { track.gain * track.gain } else { 0.0 };
            let theta = (track.pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
            let (pl, pr) = (crate::dmath::cos(theta), crate::dmath::sin(theta));

            let mut tpeak = 0.0f32;
            for i in 0..frames {
                let l = self.scratch_l[i] * g * pl;
                let r = self.scratch_r[i] * g * pr;
                out_l[i] += l;
                out_r[i] += r;
                tpeak = tpeak.max(l.abs()).max(r.abs());
            }
            self.shared.track_peak[slot].store(tpeak.to_bits(), Ordering::Relaxed);
        }

        // master: metronome click, soft limiter, peak meter
        for i in 0..frames {
            if self.playing && self.metronome {
                let b = block_start + i as f64 * dt_beats;
                let fb = b.floor() as i64;
                if fb != self.last_click_beat && fb >= 0 {
                    self.last_click_beat = fb;
                    let downbeat = (fb as f64).rem_euclid(self.beats_per_bar) == 0.0;
                    self.click_freq = if downbeat { 1318.5 } else { 880.0 };
                    self.click_phase = 0.0;
                    self.click_env = 1.0;
                }
            }
            if self.click_env > 0.0005 {
                self.click_phase += self.click_freq / self.sample_rate;
                let s = crate::dmath::sin(self.click_phase * std::f32::consts::TAU) * self.click_env * 0.2;
                out_l[i] += s;
                out_r[i] += s;
                self.click_env *= 0.998;
            }
            out_l[i] = crate::dmath::tanh(out_l[i].clamp(-1.5, 1.5));
            out_r[i] = crate::dmath::tanh(out_r[i].clamp(-1.5, 1.5));
            master_peak = master_peak.max(out_l[i].abs()).max(out_r[i].abs());
        }

        if self.playing {
            if self.loop_enabled && block_end >= self.loop_end {
                // wrap the global playhead; release voices so nothing hangs
                for t in self.tracks.iter_mut().flatten() {
                    t.pending_offs.clear();
                    t.all_instruments_off();
                }
                self.position_beats = self.loop_start + (block_end - self.loop_end);
            } else {
                self.position_beats = block_end;
            }
        }

        // publish readback
        self.shared
            .position_beats
            .store(self.position_beats.to_bits(), Ordering::Relaxed);
        self.shared.master_peak.store(master_peak.to_bits(), Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
        let voices: usize = self
            .tracks
            .iter()
            .flatten()
            .map(|t| t.voice_count())
            .sum();
        self.shared.active_voices.store(voices as u32, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Per-track real-time processing
// ---------------------------------------------------------------------------

impl EngineTrack {
    fn voice_count(&self) -> usize {
        self.devices
            .iter()
            .map(|d| match &d.dsp {
                Dsp::Inst(i) => i.voices(),
                _ => 0,
            })
            .sum()
    }

    fn all_instruments_off(&mut self) {
        for d in &mut self.devices {
            match &mut d.dsp {
                Dsp::Inst(i) => i.reset(),
                Dsp::Note(fx) => fx.reset(),
                Dsp::Audio(_) => {}
            }
        }
        self.live_events.clear();
    }

    /// Run this block's events through the Note-FX chain, in device order.
    fn apply_note_fx(&mut self, ctx: &BlockCtx) {
        for di in 0..self.devices.len() {
            if !self.devices[di].enabled || !matches!(self.devices[di].dsp, Dsp::Note(_)) {
                continue;
            }
            self.scratch_events.clear();
            {
                let EngineTrack { devices, events, scratch_events, .. } = self;
                if let Dsp::Note(fx) = &mut devices[di].dsp {
                    fx.process(ctx, events, scratch_events);
                }
            }
            std::mem::swap(&mut self.events, &mut self.scratch_events);
            // off-before-on at equal sample positions so retriggers are clean
            self.events.sort_unstable_by_key(|e| (e.sample, e.on as u8));
        }
    }

    fn update_modulators(&mut self, block_seconds: f32) {
        // copy base params into the effective scratch
        for d in &mut self.devices {
            d.eff_params.copy_from_slice(&d.base_params);
        }
        // evaluate each modulator and apply to its routes, then advance state
        for m in &mut self.mods {
            let value = m.evaluate();
            for &(di, pi, amount) in &m.routes {
                if let Some(d) = self.devices.get_mut(di) {
                    if let Some(p) = d.eff_params.get_mut(pi) {
                        *p = (*p + value * amount).clamp(0.0, 1.0);
                    }
                }
            }
            m.advance(block_seconds);
        }
        // push effective params into the DSP objects
        for d in &mut self.devices {
            d.configure();
        }
    }

    fn schedule(&mut self, playing: bool, bs: f64, be: f64, spb: f64, frames: usize) {
        self.events.clear();

        // live input queued since the last block fires at the block start
        for e in self.live_events.drain(..) {
            if self.events.len() < self.events.capacity() {
                self.events.push(e);
            }
        }

        if playing && self.active_scene >= 0 {
            // A launcher clip is active: it overrides the arrangement on this
            // track and loops phase-locked to the global transport.
            let scene = self.active_scene as usize;
            if let Some(Some(clip)) = self.clips.get(scene) {
                let loop_len = clip.length.max(0.0625);
                for note in &clip.notes {
                    let mut k = (((bs - note.start) / loop_len).ceil() as i64).max(0);
                    loop {
                        let onset = note.start + k as f64 * loop_len;
                        if onset >= be {
                            break;
                        }
                        if onset >= bs {
                            push_event(&mut self.events, onset, bs, spb, frames, true, note.pitch, note.velocity);
                            let off = onset + note.length;
                            if off < be {
                                push_event(&mut self.events, off, bs, spb, frames, false, note.pitch, 0.0);
                            } else {
                                self.pending_offs.push((note.pitch, off));
                            }
                        }
                        k += 1;
                    }
                }
            }
        } else if playing {
            // Otherwise play this track's Arranger Timeline clips.
            for arr in &self.arranger {
                let region_start = arr.start;
                let region_end = arr.start + arr.duration;
                let w0 = bs.max(region_start);
                let w1 = be.min(region_end);
                if w0 >= w1 {
                    continue;
                }
                let content = arr.content_len;
                for note in &arr.notes {
                    let mut k = (((w0 - region_start - note.start) / content).ceil() as i64).max(0);
                    loop {
                        let onset = region_start + note.start + k as f64 * content;
                        if onset >= w1 {
                            break;
                        }
                        if onset >= w0 {
                            push_event(&mut self.events, onset, bs, spb, frames, true, note.pitch, note.velocity);
                            let off = (onset + note.length).min(region_end);
                            if off < be {
                                push_event(&mut self.events, off, bs, spb, frames, false, note.pitch, 0.0);
                            } else {
                                self.pending_offs.push((note.pitch, off));
                            }
                        }
                        k += 1;
                    }
                }
            }
        }

        // due note-offs from earlier blocks
        let events = &mut self.events;
        self.pending_offs.retain(|&(pitch, off)| {
            if off < be {
                let at = off.max(bs);
                push_event(events, at, bs, spb, frames, false, pitch, 0.0);
                false
            } else {
                true
            }
        });

        self.events.sort_unstable_by_key(|e| e.sample);
    }

    /// Render one block. `input` feeds effect (return) tracks from their send
    /// bus; source tracks start from their own instrument.
    fn render(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        buf_l: &mut [f32],
        buf_r: &mut [f32],
        frames: usize,
    ) {
        let is_effect = self.is_effect;
        let mut ev_idx = 0;
        for i in 0..frames {
            // fire any (note-FX-processed) events scheduled at this sample
            while ev_idx < self.events.len() && self.events[ev_idx].sample as usize == i {
                let e = self.events[ev_idx];
                for d in &mut self.devices {
                    if let Dsp::Inst(inst) = &mut d.dsp {
                        inst.handle(e.on, e.pitch, e.velocity);
                    }
                }
                ev_idx += 1;
            }

            // effect (return) tracks take their send bus as input; source tracks
            // sum their instruments plus any injected audio-clip signal.
            let (mut l, mut r) = if is_effect {
                input.map(|(a, b)| (a[i], b[i])).unwrap_or((0.0, 0.0))
            } else {
                let mut mono = 0.0;
                for d in &mut self.devices {
                    if let Dsp::Inst(inst) = &mut d.dsp {
                        mono += inst.next();
                    }
                }
                let (il, ir) = input.map(|(a, b)| (a[i], b[i])).unwrap_or((0.0, 0.0));
                (mono + il, mono + ir)
            };

            // audio-FX chain, in device order
            for d in &mut self.devices {
                if !d.enabled {
                    continue;
                }
                if let Dsp::Audio(fx) = &mut d.dsp {
                    let (a, b) = fx.process(l, r);
                    l = a;
                    r = b;
                }
            }

            buf_l[i] = l;
            buf_r[i] = r;
        }
    }

    /// Queue a live note event; it joins the next block's Note-FX pipeline.
    fn queue_live(&mut self, on: bool, pitch: u8, velocity: f32) {
        if self.live_events.len() < self.live_events.capacity() {
            self.live_events.push(NoteEvent { sample: 0, on, pitch, velocity });
        }
    }

    /// Render this track's audio clips into the injection buffers for one block.
    #[allow(clippy::too_many_arguments)]
    fn fill_audio(
        &self,
        buf_l: &mut [f32],
        buf_r: &mut [f32],
        playing: bool,
        block_start: f64,
        dt_beats: f64,
        _sr: f32,
        tempo: f64,
        frames: usize,
    ) {
        for i in 0..frames {
            buf_l[i] = 0.0;
            buf_r[i] = 0.0;
        }
        if !playing {
            return;
        }
        let sec_per_beat = 60.0 / tempo;
        for clip in &self.audio_clips {
            let end = clip.start + clip.duration;
            for i in 0..frames {
                let b = block_start + i as f64 * dt_beats;
                if b < clip.start || b >= end {
                    continue;
                }
                let sec = (b - clip.start) * sec_per_beat;
                let idx = sec * clip.src_sr as f64;
                let i0 = idx.floor() as usize;
                if i0 + 1 >= clip.data.len() {
                    continue;
                }
                let frac = (idx - i0 as f64) as f32;
                let s = (clip.data[i0] + (clip.data[i0 + 1] - clip.data[i0]) * frac) * clip.gain;
                buf_l[i] += s;
                buf_r[i] += s;
            }
        }
    }
}

impl EngineDevice {
    fn configure(&mut self) {
        let p = &self.eff_params;
        match &mut self.dsp {
            Dsp::Note(fx) => fx.configure(p),
            Dsp::Inst(i) => i.configure(p),
            Dsp::Audio(fx) => fx.configure(p),
        }
    }
}

#[inline]
fn lfo(shape: u8, phase: f32) -> f32 {
    match shape {
        1 => 1.0 - 4.0 * (phase - 0.5).abs(),     // triangle
        2 => phase * 2.0 - 1.0,                    // saw
        3 => if phase < 0.5 { 1.0 } else { -1.0 }, // square
        _ => crate::dmath::sin(phase * std::f32::consts::TAU),
    }
}

// Cheap xorshift so the Random modulator needs no allocation or external RNG.
fn xorshift(state: &mut u32) -> f32 {
    let mut x = if *state == 0 { 0x9E37_79B9 } else { *state };
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0 // -1..1
}

impl EngineMod {
    /// Current bipolar output (-1..1), except Macro which is unipolar (0..1).
    #[inline]
    fn evaluate(&self) -> f32 {
        match self.kind {
            ModKind::Lfo => lfo(self.shape, self.phase),
            ModKind::Steps => {
                if self.steps.is_empty() {
                    0.0
                } else {
                    let n = self.steps.len();
                    let idx = ((self.phase * n as f32) as usize).min(n - 1);
                    self.steps[idx] * 2.0 - 1.0
                }
            }
            ModKind::Random => self.rand_cur,
            ModKind::Macro => self.value, // unipolar control value
        }
    }

    #[inline]
    fn advance(&mut self, dt: f32) {
        match self.kind {
            ModKind::Lfo | ModKind::Steps => {
                let hz = 0.05 + self.rate * 8.0;
                self.phase = (self.phase + hz * dt).fract();
            }
            ModKind::Random => {
                let hz = 0.05 + self.rate * 8.0;
                let prev = self.phase;
                self.phase = (self.phase + hz * dt).fract();
                if self.phase < prev {
                    // new cycle: pick a fresh target
                    let mut seed = (self.rand_target.to_bits() ^ 0x1234_5678).wrapping_add(1);
                    self.rand_target = xorshift(&mut seed);
                }
                // smooth toward target (value = smoothing amount)
                let smooth = 0.001 + (1.0 - self.value) * 0.5;
                self.rand_cur += (self.rand_target - self.rand_cur) * smooth.min(1.0);
            }
            ModKind::Macro => {}
        }
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn push_event(
    events: &mut Vec<NoteEvent>,
    beat: f64,
    block_start: f64,
    spb: f64,
    frames: usize,
    on: bool,
    pitch: u8,
    velocity: f32,
) {
    let sample = (((beat - block_start) * spb).round() as i64).clamp(0, frames as i64 - 1) as u32;
    if events.len() < events.capacity() {
        events.push(NoteEvent { sample, on, pitch, velocity });
    }
}
