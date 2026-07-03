//! Loopback calibration (SRS-REC-004): find where a known probe signal
//! appears inside a recording via normalized cross-correlation. Browsers
//! cannot report end-to-end audio latency truthfully (research report §3.2),
//! so measuring it is the only honest option.

/// Best lag (in samples) of `probe` inside `rec`, with a 0..1 confidence
/// (the normalized correlation peak). None if the signal is degenerate or
/// the peak is too weak to trust (< 0.25 — e.g. the mic never heard the
/// probe).
pub fn estimate_delay(probe: &[f32], rec: &[f32]) -> Option<(usize, f32)> {
    if probe.is_empty() || rec.len() <= probe.len() {
        return None;
    }
    let probe_norm: f32 = probe.iter().map(|s| s * s).sum::<f32>().sqrt();
    if probe_norm < 1e-6 {
        return None;
    }

    // sliding window energy for normalization, updated incrementally
    let m = probe.len();
    let mut win_energy: f32 = rec[..m].iter().map(|s| s * s).sum();
    let mut best_lag = 0usize;
    let mut best = f32::MIN;
    for lag in 0..=(rec.len() - m) {
        let win_norm = win_energy.max(0.0).sqrt();
        if win_norm > 1e-6 {
            let dot: f32 = probe.iter().zip(&rec[lag..lag + m]).map(|(a, b)| a * b).sum();
            let corr = dot / (probe_norm * win_norm);
            if corr > best {
                best = corr;
                best_lag = lag;
            }
        }
        if lag + m < rec.len() {
            win_energy += rec[lag + m] * rec[lag + m] - rec[lag] * rec[lag];
        }
    }
    if best < 0.25 {
        return None;
    }
    Some((best_lag, best.min(1.0)))
}

/// The calibration probe: a short Hann-windowed linear chirp (300→3500 Hz).
/// Chirps autocorrelate sharply, so the lag estimate is sample-accurate.
pub fn chirp(rate: f32, seconds: f32) -> Vec<f32> {
    let n = (rate * seconds) as usize;
    let (f0, f1) = (300.0f32, 3500.0f32);
    (0..n)
        .map(|i| {
            let t = i as f32 / rate;
            let dur = n as f32 / rate;
            let phase =
                std::f32::consts::TAU * (f0 * t + (f1 - f0) * t * t / (2.0 * dur));
            let hann = 0.5 - 0.5 * dawcore::dmath::cos(std::f32::consts::TAU * i as f32 / n as f32);
            dawcore::dmath::sin(phase) * hann * 0.8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_exact_lag_under_noise() {
        let rate = 48_000.0;
        let probe = chirp(rate, 0.1);
        let lag = 4321usize;
        let mut rec = vec![0.0f32; 48_000];
        // attenuated echo of the probe + deterministic noise
        for (i, &s) in probe.iter().enumerate() {
            rec[lag + i] += s * 0.3;
        }
        let mut seed = 0x1234_5678u32;
        for s in rec.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *s += ((seed as f32 / u32::MAX as f32) - 0.5) * 0.05;
        }
        let (found, conf) = estimate_delay(&probe, &rec).expect("must find the probe");
        assert_eq!(found, lag);
        assert!(conf > 0.5, "confidence {conf}");
    }

    #[test]
    fn unrelated_signal_yields_none() {
        let rate = 48_000.0;
        let probe = chirp(rate, 0.1);
        // a steady tone (like Chromium's fake mic) is not the probe
        let rec: Vec<f32> = (0..48_000)
            .map(|i| dawcore::dmath::sin(i as f32 * 440.0 * std::f32::consts::TAU / rate) * 0.5)
            .collect();
        assert!(estimate_delay(&probe, &rec).is_none());
    }

    #[test]
    fn silence_yields_none() {
        let probe = chirp(48_000.0, 0.1);
        assert!(estimate_delay(&probe, &vec![0.0; 48_000]).is_none());
    }
}
