//! cpal output backend. Owns the real-time stream; the [`Engine`] is moved into
//! the audio callback and the [`EngineHandle`] is returned to the UI thread.
//!
//! If no audio hardware is available the app still runs: a *null backend* drives
//! the engine on a paced thread so the transport, meters and sequencer behave
//! exactly as they would with real output — there is just no sound.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use dawcore::engine::{Engine, EngineHandle};

const MAX_FRAMES: usize = 8192;

enum Backend {
    /// Held only to keep the cpal stream alive for the app's lifetime.
    Stream(#[allow(dead_code)] cpal::Stream),
    Null(Arc<AtomicBool>), // stop flag for the paced thread
}

pub struct Audio {
    pub handle: EngineHandle,
    _backend: Backend,
    pub device_name: String,
    pub sample_rate: f32,
    /// True when running on the silent fallback (no audio device).
    pub silent: bool,
}

impl Drop for Audio {
    fn drop(&mut self) {
        if let Backend::Null(stop) = &self._backend {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// Start audio. Always succeeds: falls back to a silent driver if no device or
/// stream can be opened, so the app is usable regardless of hardware.
pub fn start() -> Audio {
    match try_real() {
        Ok(audio) => audio,
        Err(e) => {
            eprintln!("audio: {e} — running silent null backend");
            null_backend(e)
        }
    }
}

fn try_real() -> Result<Audio, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no output device".to_string())?;
    let device_name = device.name().unwrap_or_else(|_| "Unknown".into());
    let supported = device
        .default_output_config()
        .map_err(|e| format!("default output config: {e}"))?;

    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;
    let config: cpal::StreamConfig = supported.config();

    let (engine, handle) = Engine::new(sample_rate);

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, engine, channels),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, engine, channels),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, engine, channels),
        other => return Err(format!("unsupported sample format: {other:?}")),
    }
    .map_err(|e| format!("build stream: {e}"))?;

    stream.play().map_err(|e| format!("stream play: {e}"))?;

    Ok(Audio {
        handle,
        _backend: Backend::Stream(stream),
        device_name,
        sample_rate,
        silent: false,
    })
}

/// Drive the engine on a paced thread, discarding output.
fn null_backend(reason: String) -> Audio {
    let sample_rate = 48_000.0f32;
    let (mut engine, handle) = Engine::new(sample_rate);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    std::thread::Builder::new()
        .name("null-audio".into())
        .spawn(move || {
            const BLOCK: usize = 256;
            let mut l = vec![0.0f32; BLOCK];
            let mut r = vec![0.0f32; BLOCK];
            let block_dur = Duration::from_secs_f64(BLOCK as f64 / sample_rate as f64);
            let mut next = Instant::now();
            while !stop_thread.load(Ordering::Relaxed) {
                engine.process(&mut l, &mut r, BLOCK);
                next += block_dur;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now; // fell behind; resync
                }
            }
        })
        .expect("spawn null-audio thread");

    Audio {
        handle,
        _backend: Backend::Null(stop),
        device_name: format!("Silent ({reason})"),
        sample_rate,
        silent: true,
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut engine: Engine,
    channels: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let mut l = vec![0.0f32; MAX_FRAMES];
    let mut r = vec![0.0f32; MAX_FRAMES];
    let err_fn = |err| eprintln!("audio stream error: {err}");

    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = (data.len() / channels).min(MAX_FRAMES);
            engine.process(&mut l, &mut r, frames);
            for (i, frame) in data.chunks_mut(channels).enumerate() {
                let lv = if i < frames { l[i] } else { 0.0 };
                let rv = if i < frames { r[i] } else { 0.0 };
                for (ch, sample) in frame.iter_mut().enumerate() {
                    let v = match ch {
                        0 => lv,
                        1 => rv,
                        _ => 0.0,
                    };
                    *sample = T::from_sample(v);
                }
            }
        },
        err_fn,
        None,
    )
}
