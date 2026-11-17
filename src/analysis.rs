/// Estimates the tempo in BPM of mono `samples`, or `None` if there is no usable pulse.
pub fn estimate_tempo(samples: &[f32], sample_rate: u32) -> Option<f64> {
    let flux = onset_strength(samples, HOP);
    let frames_per_sec = sample_rate as f64 / HOP as f64;
    let lag_min = (60.0 / MAX_BPM * frames_per_sec).floor() as usize;
    let lag_max = (60.0 / MIN_BPM * frames_per_sec).ceil() as usize;
    if flux.len() <= lag_max + 1 {
        return None;
    }

    let (best_lag, best_score) = (lag_min..=lag_max)
        .map(|lag| (lag, autocorrelation(&flux, lag)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if best_score <= 0.0 {
        return None;
    }
    Some(60.0 * frames_per_sec / best_lag as f64)
}

/// Tempo search range. Anything outside it is folded in by the caller as half/double time.
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 180.0;
/// Analysis frame size in samples (about 6 ms at 44.1 kHz).
const HOP: usize = 256;

/// How much new energy arrives in each `hop`-sized block: the half-wave rectified
/// increase in block energy. Peaks mark drum hits and note starts.
fn onset_strength(samples: &[f32], hop: usize) -> Vec<f64> {
    let energy: Vec<f64> = samples
        .chunks(hop)
        .map(|block| block.iter().map(|&s| (s as f64) * (s as f64)).sum())
        .collect();
    energy
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).max(0.0))
        .collect()
}

/// How strongly the envelope repeats itself `lag` frames later.
fn autocorrelation(envelope: &[f64], lag: usize) -> f64 {
    envelope
        .iter()
        .zip(&envelope[lag..])
        .map(|(a, b)| a * b)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 44_100;

    /// Silence with a short decaying click on every beat, so the true tempo is exactly `bpm`.
    fn click_track(bpm: f64, secs: f64) -> Vec<f32> {
        let mut samples = vec![0.0; (secs * SAMPLE_RATE as f64) as usize];
        let beat_secs = 60.0 / bpm;
        let click_len = (0.01 * SAMPLE_RATE as f64) as usize; // 10 ms
        let mut beat = 0.0;
        while beat < secs {
            let start = (beat * SAMPLE_RATE as f64) as usize;
            for i in 0..click_len.min(samples.len().saturating_sub(start)) {
                let decay = 1.0 - i as f32 / click_len as f32;
                samples[start + i] = if i % 2 == 0 { decay } else { -decay };
            }
            beat += beat_secs;
        }
        samples
    }

    #[test]
    fn a_120_bpm_click_track_is_estimated_at_120_bpm() {
        let samples = click_track(120.0, 12.0);

        let estimate = estimate_tempo(&samples, SAMPLE_RATE).expect("a steady pulse has a tempo");

        assert!((estimate - 120.0).abs() <= 1.0, "estimated {estimate} BPM");
    }
}
