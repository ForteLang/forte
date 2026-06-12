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
use crate::dsp::effects::{Drive, Eq3, FdnReverb, StereoDelay};
use crate::dsp::filter::{FilterMode, Svf};
use crate::dsp::oscillator::Waveform;
use crate::dsp::synth::PolySynth;
use crate::dsp::voice::SynthParams;
use crate::model::{self, Device, DeviceKind, ModKind, Track, MAX_TRACKS};

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

enum Dsp {
    Synth(PolySynth),
    Filter { l: Svf, r: Svf, mode: FilterMode },
    Delay(StereoDelay),
    Reverb(FdnReverb),
    Eq { l: Eq3, r: Eq3 },
    Drive(Drive),
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

struct NoteEvent {
    sample: u32,
    on: bool,
    pitch: u8,
    velocity: f32,
}

pub struct EngineArrClip {
    start: f64,
    duration: f64,
    content_len: f64,
    notes: Vec<EngineNote>,
}

pub struct EngineTrack {
    devices: Vec<EngineDevice>,
    mods: Vec<EngineMod>,
    gain: f32,
    pan: f32,
    mute: bool,
    solo: bool,
    active_scene: i32,
    /// Quantized launch request awaiting the next quant boundary.
    pending_scene: Option<i32>,
    clips: Vec<Option<Box<EngineClip>>>,
    arranger: Vec<EngineArrClip>,
    pending_offs: Vec<(u8, f64)>,
    events: Vec<NoteEvent>,
}

// ---------------------------------------------------------------------------
// Builders (run on the UI thread — allocation allowed here)
// ---------------------------------------------------------------------------

fn map_cutoff(v: f32) -> f32 {
    30.0 * 600.0_f32.powf(v.clamp(0.0, 1.0))
}

fn build_dsp(kind: DeviceKind, sr: f32) -> Dsp {
    match kind {
        DeviceKind::Polymer => Dsp::Synth(PolySynth::new(sr)),
        DeviceKind::Filter => Dsp::Filter { l: Svf::new(sr), r: Svf::new(sr), mode: FilterMode::Lowpass },
        DeviceKind::Delay => Dsp::Delay(StereoDelay::new(sr)),
        DeviceKind::Reverb => Dsp::Reverb(FdnReverb::new(sr)),
        DeviceKind::Eq => Dsp::Eq { l: Eq3::new(sr), r: Eq3::new(sr) },
        DeviceKind::Drive => Dsp::Drive(Drive::new()),
    }
}

pub fn build_device(dev: &Device, sr: f32) -> Box<EngineDevice> {
    Box::new(EngineDevice {
        kind: dev.kind,
        enabled: dev.enabled,
        base_params: dev.params.clone(),
        eff_params: dev.params.clone(),
        dsp: build_dsp(dev.kind, sr),
    })
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
        active_scene: -1,
        pending_scene: None,
        clips,
        arranger: build_arranger(track),
        pending_offs: Vec::with_capacity(64),
        events: Vec::with_capacity(512),
    };
    Box::new(et)
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

    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
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
            scratch_l: vec![0.0; MAX_BLOCK],
            scratch_r: vec![0.0; MAX_BLOCK],
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
                self.position_beats = if self.loop_enabled { self.loop_start } else { 0.0 };
                for t in self.tracks.iter_mut().flatten() {
                    t.pending_offs.clear();
                    t.pending_scene = None;
                    for d in &mut t.devices {
                        if let Dsp::Synth(s) = &mut d.dsp {
                            s.all_notes_off();
                        }
                    }
                }
            }
            Command::SetTempo(bpm) => self.tempo = bpm.clamp(20.0, 300.0),
            Command::SetLoop { enabled, start, end } => {
                self.loop_enabled = enabled;
                self.loop_start = start.max(0.0);
                self.loop_end = end.max(start + 0.25);
            }
            Command::SetLaunchQuant(q) => self.launch_quant = q.max(0.0),
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
                    if let Some(s) = t.synth_mut() {
                        s.note_on(note, velocity);
                    }
                }
            }
            Command::NoteOff { track, note } => {
                if let Some(t) = self.track_mut(track) {
                    if let Some(s) = t.synth_mut() {
                        s.note_off(note);
                    }
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

        for slot in 0..self.tracks.len() {
            let Some(track) = self.tracks[slot].as_deref_mut() else { continue };

            // advance modulators (block rate) and bake effective params
            track.update_modulators(block_seconds);

            // schedule note events for this block
            track.schedule(self.playing, block_start, sched_end, spb, frames);

            // render this track into scratch
            track.render(self.sample_rate, &mut self.scratch_l, &mut self.scratch_r, frames);

            // mix into master with gain / pan / mute / solo
            let audible = track.solo || (!track.mute && !(any_solo && !track.solo));
            let g = if audible { track.gain * track.gain } else { 0.0 };
            let theta = (track.pan + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
            let (pl, pr) = (theta.cos(), theta.sin());

            let mut tpeak = 0.0f32;
            for i in 0..frames {
                let l = self.scratch_l[i] * g * pl;
                let r = self.scratch_r[i] * g * pr;
                out_l[i] += l;
                out_r[i] += r;
                tpeak = tpeak.max(l.abs()).max(r.abs());
            }

            self.shared.track_peak[slot].store(tpeak.to_bits(), Ordering::Relaxed);
            self.shared.active_scene[slot].store(
                if self.playing { track.active_scene } else { -1 },
                Ordering::Relaxed,
            );
        }

        // master soft limiter + peak meter
        for i in 0..frames {
            out_l[i] = out_l[i].clamp(-1.5, 1.5).tanh();
            out_r[i] = out_r[i].clamp(-1.5, 1.5).tanh();
            master_peak = master_peak.max(out_l[i].abs()).max(out_r[i].abs());
        }

        if self.playing {
            if self.loop_enabled && block_end >= self.loop_end {
                // wrap the global playhead; release voices so nothing hangs
                for t in self.tracks.iter_mut().flatten() {
                    t.pending_offs.clear();
                    for d in &mut t.devices {
                        if let Dsp::Synth(s) = &mut d.dsp {
                            s.all_notes_off();
                        }
                    }
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
    fn synth_mut(&mut self) -> Option<&mut PolySynth> {
        self.devices.iter_mut().find_map(|d| match &mut d.dsp {
            Dsp::Synth(s) => Some(s),
            _ => None,
        })
    }

    fn voice_count(&self) -> usize {
        self.devices
            .iter()
            .map(|d| match &d.dsp {
                Dsp::Synth(s) => s.active_voices(),
                _ => 0,
            })
            .sum()
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

    fn render(&mut self, _sr: f32, buf_l: &mut [f32], buf_r: &mut [f32], frames: usize) {
        let mut ev_idx = 0;
        for i in 0..frames {
            // fire any events scheduled at this sample
            while ev_idx < self.events.len() && self.events[ev_idx].sample as usize == i {
                let (on, pitch, velocity) = {
                    let e = &self.events[ev_idx];
                    (e.on, e.pitch, e.velocity)
                };
                if let Some(s) = self.synth_mut_inline() {
                    if on {
                        s.note_on(pitch, velocity);
                    } else {
                        s.note_off(pitch);
                    }
                }
                ev_idx += 1;
            }

            // instrument
            let mut mono = 0.0;
            for d in &mut self.devices {
                if let Dsp::Synth(s) = &mut d.dsp {
                    mono += s.next();
                }
            }
            let (mut l, mut r) = (mono, mono);

            // effect chain
            for d in &mut self.devices {
                if !d.enabled {
                    continue;
                }
                match &mut d.dsp {
                    Dsp::Synth(_) => {}
                    Dsp::Filter { l: fl, r: fr, mode } => {
                        l = fl.process(l, *mode);
                        r = fr.process(r, *mode);
                    }
                    Dsp::Eq { l: el, r: er } => {
                        l = el.process(l);
                        r = er.process(r);
                    }
                    Dsp::Drive(dr) => {
                        l = dr.process(l);
                        r = dr.process(r);
                    }
                    Dsp::Delay(dl) => {
                        let (a, b) = dl.process(l, r);
                        l = a;
                        r = b;
                    }
                    Dsp::Reverb(rv) => {
                        let (a, b) = rv.process(l, r);
                        l = a;
                        r = b;
                    }
                }
            }

            buf_l[i] = l;
            buf_r[i] = r;
        }
    }

    // Separate borrow helper so `render` can mutate synth while iterating events.
    fn synth_mut_inline(&mut self) -> Option<&mut PolySynth> {
        for d in &mut self.devices {
            if let Dsp::Synth(s) = &mut d.dsp {
                return Some(s);
            }
        }
        None
    }
}

impl EngineDevice {
    fn configure(&mut self) {
        let p = &self.eff_params;
        match &mut self.dsp {
            Dsp::Synth(s) => {
                s.params = SynthParams {
                    wave: Waveform::from_index(p[0] as u8),
                    cutoff: p[1],
                    resonance: p[2],
                    attack: p[3],
                    decay: p[4],
                    sustain: p[5],
                    release: p[6],
                    detune: p[7],
                    sub: p[8],
                    filter_env: p[9],
                };
            }
            Dsp::Filter { l, r, mode } => {
                *mode = FilterMode::from_index(p[0] as u8);
                let c = map_cutoff(p[1]);
                l.set(c, p[2]);
                r.set(c, p[2]);
            }
            Dsp::Delay(d) => {
                d.time = p[0];
                d.feedback = p[1];
                d.mix = p[2];
            }
            Dsp::Reverb(rv) => {
                rv.size = p[0];
                rv.decay = p[1];
                rv.mix = p[2];
            }
            Dsp::Eq { l, r } => {
                l.low_gain = p[0];
                l.mid_gain = p[1];
                l.high_gain = p[2];
                r.low_gain = p[0];
                r.mid_gain = p[1];
                r.high_gain = p[2];
            }
            Dsp::Drive(dr) => {
                dr.amount = p[0];
            }
        }
    }
}

#[inline]
fn lfo(shape: u8, phase: f32) -> f32 {
    match shape {
        1 => 1.0 - 4.0 * (phase - 0.5).abs(),     // triangle
        2 => phase * 2.0 - 1.0,                    // saw
        3 => if phase < 0.5 { 1.0 } else { -1.0 }, // square
        _ => (phase * std::f32::consts::TAU).sin(),
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
