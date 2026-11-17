/// Estimates the tempo in BPM of mono `samples`, or `None` if there is no usable pulse.
pub fn estimate_tempo(samples: &[f32], sample_rate: u32) -> Option<f64> {
    // Smoothing widens each onset into a small bump, so two beats that are not an exact
    // whole number of frames apart still line up when autocorrelated.
    let flux = smooth(
        &smooth(&onset_strength(samples, HOP), SMOOTH_RADIUS),
        SMOOTH_RADIUS,
    );
    let frames_per_sec = sample_rate as f64 / HOP as f64;
    let lag_min = (60.0 / MAX_BPM * frames_per_sec).floor() as usize;
    let lag_max = (60.0 / MIN_BPM * frames_per_sec).ceil() as usize;
    if flux.len() <= lag_max + 1 {
        return None;
    }

    let (best_lag, best_score) = (lag_min..=lag_max)
        .map(|lag| {
            let bpm = 60.0 * frames_per_sec / lag as f64;
            (lag, autocorrelation(&flux, lag) * tempo_prior(bpm))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if best_score <= 0.0 {
        return None;
    }
    Some(60.0 * frames_per_sec / best_lag as f64)
}

/// Tempo search range. Anything outside it is folded in by the caller as half/double time.
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 180.0;
/// Analysis frame size in samples (about 1.5 ms at 44.1 kHz). Fine enough that a beat period
/// is not split ambiguously between two lags.
const HOP: usize = 64;

/// Half-width, in frames, of each moving-average pass (applied twice: a triangular blur).
const SMOOTH_RADIUS: usize = 6;

/// Moving average over `2 * radius + 1` frames, shrinking at the edges.
fn smooth(envelope: &[f64], radius: usize) -> Vec<f64> {
    (0..envelope.len())
        .map(|i| {
            let window = &envelope[i.saturating_sub(radius)..(i + radius + 1).min(envelope.len())];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect()
}

/// A gentle preference for tempos near 120 BPM (log-Gaussian, 1.4 octaves wide), so a
/// pulse that fits both a tempo and its half or double picks the more common one.
fn tempo_prior(bpm: f64) -> f64 {
    let octaves_from_120 = (bpm / 120.0).log2();
    (-0.5 * (octaves_from_120 / 1.4).powi(2)).exp()
}

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

    /// True if `estimate` is within 1 BPM of `truth`, or of exactly half or double it.
    /// Half and double time are the same pulse, so beat trackers cannot always tell them apart.
    fn matches_up_to_octave(estimate: f64, truth: f64) -> bool {
        [truth, truth / 2.0, truth * 2.0]
            .iter()
            .any(|t| (estimate - t).abs() <= 1.0)
    }

    #[test]
    fn click_tracks_across_the_tempo_range_are_estimated_within_one_bpm() {
        for bpm in [70.0, 90.0, 100.0, 140.0, 160.0, 175.0] {
            let samples = click_track(bpm, 12.0);

            let estimate = estimate_tempo(&samples, SAMPLE_RATE)
                .unwrap_or_else(|| panic!("{bpm} BPM click track has a tempo"));

            assert!((estimate - bpm).abs() <= 1.0, "{bpm} BPM estimated as {estimate}");
        }
    }

    #[test]
    fn a_180_bpm_click_track_is_estimated_as_180_or_its_half_time() {
        // 180 is the top of the search range and sits exactly an octave above 90.
        let samples = click_track(180.0, 12.0);

        let estimate = estimate_tempo(&samples, SAMPLE_RATE).expect("a steady pulse has a tempo");

        assert!(matches_up_to_octave(estimate, 180.0), "estimated {estimate} BPM");
    }

    #[test]
    fn a_120_bpm_click_track_is_estimated_at_120_bpm() {
        let samples = click_track(120.0, 12.0);

        let estimate = estimate_tempo(&samples, SAMPLE_RATE).expect("a steady pulse has a tempo");

        assert!((estimate - 120.0).abs() <= 1.0, "estimated {estimate} BPM");
    }
}
