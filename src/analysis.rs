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
            (lag, comb_score(&flux, lag) * tempo_prior(bpm))
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
/// Analysis hop in samples (about 3 ms at 44.1 kHz). Fine enough that a beat period
/// is not split ambiguously between two lags.
const HOP: usize = 128;

/// Half-width, in frames, of each moving-average pass (applied twice: a triangular blur).
const SMOOTH_RADIUS: usize = 3;

/// Moving average over `2 * radius + 1` frames, shrinking at the edges.
fn smooth(envelope: &[f64], radius: usize) -> Vec<f64> {
    (0..envelope.len())
        .map(|i| {
            let window = &envelope[i.saturating_sub(radius)..(i + radius + 1).min(envelope.len())];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect()
}

/// A very gentle preference for tempos near 120 BPM (log-Gaussian, 3 octaves wide), so a
/// pulse that fits both a tempo and its half or double picks the more common one.
fn tempo_prior(bpm: f64) -> f64 {
    let octaves_from_120 = (bpm / 120.0).log2();
    (-0.5 * (octaves_from_120 / 3.0).powi(2)).exp()
}

/// FFT window length in samples (about 46 ms at 44.1 kHz).
const FFT_SIZE: usize = 2048;
/// Compression applied to spectrum magnitudes before differencing, so quiet and loud
/// onsets count more evenly (as the ear hears them).
const LOG_GAMMA: f32 = 100.0;

/// Spectral flux: for every `hop` samples, how much the log-magnitude spectrum grew in any
/// frequency bin since the previous frame, summed over bins. Unlike plain energy, a hi-hat
/// over a steady bass line still registers, because only its own frequencies are compared.
fn onset_strength(samples: &[f32], hop: usize) -> Vec<f64> {
    use realfft::RealFftPlanner;

    if samples.len() < FFT_SIZE {
        return Vec::new();
    }
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
        .collect();
    let window_sum: f32 = window.iter().sum();

    let mut input = vec![0.0f32; FFT_SIZE];
    let mut spectrum = fft.make_output_vec();
    let mut previous = vec![0.0f32; spectrum.len()];
    let mut flux = Vec::with_capacity((samples.len() - FFT_SIZE) / hop + 1);

    for (frame, start) in (0..=samples.len() - FFT_SIZE).step_by(hop).enumerate() {
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = samples[start + i] * window[i];
        }
        fft.process(&mut input, &mut spectrum)
            .expect("buffer lengths match the planned FFT");

        let mut growth = 0.0f64;
        for (bin, value) in spectrum.iter().enumerate() {
            let level = (1.0 + LOG_GAMMA * value.norm() / window_sum).ln();
            if frame > 0 {
                growth += (level - previous[bin]).max(0.0) as f64;
            }
            previous[bin] = level;
        }
        flux.push(growth);
    }
    flux
}

/// How many whole beats of repetition a candidate beat period must show.
const COMB_BEATS: usize = 4;

/// Average strength with which the envelope repeats at 1, 2, 3 and 4 beat periods. A true beat
/// repeats at every multiple; an off-beat or dotted pulse (2:3 or 3:2 of the beat) does not.
fn comb_score(envelope: &[f64], lag: usize) -> f64 {
    let scores: Vec<f64> = (1..=COMB_BEATS)
        .map(|k| k * lag)
        .filter(|&l| l < envelope.len())
        .map(|l| autocorrelation(envelope, l) / (envelope.len() - l) as f64)
        .collect();
    scores.iter().sum::<f64>() / scores.len().max(1) as f64
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

            assert!(
                (estimate - bpm).abs() <= 1.0,
                "{bpm} BPM estimated as {estimate}"
            );
        }
    }

    #[test]
    fn a_180_bpm_click_track_is_estimated_as_180_or_its_half_time() {
        // 180 is the top of the search range and sits exactly an octave above 90.
        let samples = click_track(180.0, 12.0);

        let estimate = estimate_tempo(&samples, SAMPLE_RATE).expect("a steady pulse has a tempo");

        assert!(
            matches_up_to_octave(estimate, 180.0),
            "estimated {estimate} BPM"
        );
    }

    #[test]
    fn a_120_bpm_click_track_is_estimated_at_120_bpm() {
        let samples = click_track(120.0, 12.0);

        let estimate = estimate_tempo(&samples, SAMPLE_RATE).expect("a steady pulse has a tempo");

        assert!((estimate - 120.0).abs() <= 1.0, "estimated {estimate} BPM");
    }
}
