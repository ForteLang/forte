//! Helpers that translate high-level model edits into engine commands. The GUI
//! mutates its [`Project`] and calls these to keep the audio engine in step.

use crate::command::Command;
use crate::engine::{build_clip, build_device, build_mods, build_track, EngineHandle};
use crate::model::{Project, Track};

/// Push an entire project into a freshly-created engine.
pub fn full_sync(handle: &mut EngineHandle, project: &Project) {
    handle.send(Command::SetTempo(project.tempo));
    handle.send(Command::SetMaster(project.master));
    handle.send(Command::SetMasterChain(
        project.master_inserts.iter().map(|d| build_device(d, handle.sample_rate)).collect(),
    ));
    for t in &project.tracks {
        handle.send(Command::AddTrack { slot: t.id, track: build_track(t, handle.sample_rate) });
    }
}

/// Rebuild a single track's clips after a structural edit (e.g. note changes).
pub fn sync_clip(handle: &mut EngineHandle, track: &Track, scene: usize) {
    let clip = track.clips.get(scene).and_then(|c| c.as_ref()).map(build_clip);
    handle.send(Command::SetClip { track: track.id, scene, clip });
}

/// Replace a track wholesale (used after adding/removing devices).
pub fn sync_track(handle: &mut EngineHandle, track: &Track) {
    handle.send(Command::AddTrack { slot: track.id, track: build_track(track, handle.sample_rate) });
}

/// Re-send the device freshly built (used when adding a device).
pub fn add_device(handle: &mut EngineHandle, track: &Track, device_index: usize) {
    let dev = &track.devices[device_index];
    handle.send(Command::AddDevice { track: track.id, device: build_device(dev, handle.sample_rate) });
}

/// Push modulator routing after the user edits an LFO.
pub fn sync_mods(handle: &mut EngineHandle, track: &Track) {
    handle.send(Command::SetModRoutes { track: track.id, modulators: build_mods(track) });
}
