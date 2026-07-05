//! Lock-free message protocol from the UI thread to the audio thread.
//!
//! Frequent, real-time-safe messages (notes, transport, parameter tweaks) carry
//! only small `Copy` payloads. Structural changes carry heap objects that were
//! *built on the UI thread* (so the audio thread never allocates); after the
//! audio thread swaps them in, the displaced objects are shipped back through
//! the garbage channel to be dropped by the UI thread.

use crate::engine::{EngineAutoPoint, EngineClip, EngineDevice, EngineTrack};

pub enum Command {
    // ---- transport ----
    Play,
    Stop,
    SetTempo(f64),
    SetLoop { enabled: bool, start: f64, end: f64 },
    SetLaunchQuant(f64),
    SetMetronome(bool),

    // ---- launcher / live ----
    LaunchClip { track: usize, scene: usize },
    LaunchScene(usize),
    StopTrack { track: usize },
    NoteOn { track: usize, note: u8, velocity: f32 },
    NoteOff { track: usize, note: u8 },

    // ---- mixer (hot, no alloc) ----
    SetTrackGain { track: usize, value: f32 },
    SetTrackPan { track: usize, value: f32 },
    SetTrackMute { track: usize, value: bool },
    SetTrackSolo { track: usize, value: bool },

    // ---- device params (hot, no alloc) ----
    SetParam { track: usize, device: usize, param: usize, value: f32 },
    SetDeviceEnabled { track: usize, device: usize, value: bool },
    SetModulator { track: usize, mod_index: usize, rate: f32, shape: u8 },
    /// Set a single Grid node parameter (hot path; topology edits use AddTrack).
    SetGridParam { track: usize, device: usize, node: usize, param: usize, value: f32 },

    // ---- structural (heap built on UI thread) ----
    AddTrack { slot: usize, track: Box<EngineTrack> },
    RemoveTrack { slot: usize },
    SetClip { track: usize, scene: usize, clip: Option<Box<EngineClip>> },
    AddDevice { track: usize, device: Box<EngineDevice> },
    RemoveDevice { track: usize, device: usize },
    SetModRoutes { track: usize, modulators: Box<crate::engine::EngineMods> },
    /// Replace a track's post-fader send levels (dest slot, level).
    SetSends { track: usize, sends: Box<Vec<(usize, f32)>> },
    /// Replace a track's volume-automation lane (sorted by beat).
    SetAutomation { track: usize, points: Box<Vec<EngineAutoPoint>> },
}

/// Displaced heap objects returned to the UI thread for dropping.
pub enum Garbage {
    Track(Box<EngineTrack>),
    Clip(Box<EngineClip>),
    Device(Box<EngineDevice>),
    Mods(Box<crate::engine::EngineMods>),
    Sends(Box<Vec<(usize, f32)>>),
    Auto(Box<Vec<EngineAutoPoint>>),
}
