//! `dawcore` — the real-time audio engine and DSP for a Bitwig-style DAW.
//!
//! The crate is deliberately free of any GUI or hardware dependency so it can be
//! unit-tested with offline rendering and reused under different front-ends.
//!
//! - [`dsp`]      band-limited oscillators, ADSR, SVF filter, synth voices, effects
//! - [`model`]    plain-data project model (the GUI's source of truth)
//! - [`command`]  lock-free UI → audio message protocol
//! - [`engine`]   the audio-thread engine: scheduler, mixer, modulators
//! - [`sync`]     helpers to mirror model edits into the engine

pub mod bounce;
pub mod command;
pub mod dsp;
pub mod engine;
pub mod model;
pub mod samples;
pub mod sync;

pub use engine::{Engine, EngineHandle, Shared};
pub use model::Project;
