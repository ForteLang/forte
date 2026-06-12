//! cpal output backend. Owns the real-time stream; the [`Engine`] is moved into
//! the audio callback and the [`EngineHandle`] is returned to the UI thread.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use dawcore::engine::{Engine, EngineHandle};

const MAX_FRAMES: usize = 8192;

pub struct Audio {
    pub handle: EngineHandle,
    // Keep the stream alive for the lifetime of the app.
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: f32,
}

pub fn start() -> Result<Audio, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "no output device available".to_string())?;
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

    Ok(Audio { handle, _stream: stream, device_name, sample_rate })
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
