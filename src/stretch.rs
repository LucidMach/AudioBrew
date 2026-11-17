use signalsmith_stretch::Stretch;

use crate::import::Stereo;

#[derive(Debug, PartialEq, Eq)]
pub enum StretchError {
    /// The speed factor must be a positive, finite number.
    InvalidFactor,
    InvalidSampleRate,
}

/// Time-stretches a Track to the Mashup tempo, keeping its pitch (unless `pitch_semitones`
/// asks for a shift). `factor` is the speed factor: above 1 plays faster and gets shorter,
/// so the output has `frames / factor` frames. Done once per Track, never per Band.
pub fn stretch(input: &Stereo, factor: f64, pitch_semitones: i8) -> Result<Stereo, StretchError> {
    if !factor.is_finite() || factor <= 0.0 {
        return Err(StretchError::InvalidFactor);
    }
    if input.sample_rate == 0 {
        return Err(StretchError::InvalidSampleRate);
    }
    let frames = input.frames();
    if frames == 0 {
        return Ok(Stereo {
            samples: Vec::new(),
            sample_rate: input.sample_rate,
        });
    }
    let out_frames = (frames as f64 / factor).round() as usize;

    let clean = |v: &f32| if v.is_finite() { *v } else { 0.0 };
    let source: Vec<f32> = input.samples[..frames * 2].iter().map(clean).collect();
    let mut output = vec![0.0f32; out_frames * 2];

    let mut engine = Stretch::preset_default(2, input.sample_rate);
    if pitch_semitones != 0 {
        engine.set_transpose_factor_semitones(pitch_semitones as f32, None);
    }
    engine.exact(&source, &mut output);

    let samples = output.iter().map(clean).collect();
    Ok(Stereo {
        samples,
        sample_rate: input.sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const RATE: u32 = 48_000;

    /// Stereo with `left` and `right` generated per frame index.
    fn stereo(frames: usize, left: impl Fn(usize) -> f32, right: impl Fn(usize) -> f32) -> Stereo {
        let samples = (0..frames).flat_map(|i| [left(i), right(i)]).collect();
        Stereo {
            samples,
            sample_rate: RATE,
        }
    }

    fn sine(freq: f64, i: usize) -> f32 {
        (0.5 * (2.0 * PI * freq * i as f64 / RATE as f64).sin()) as f32
    }

    fn left(s: &Stereo) -> Vec<f32> {
        s.samples.chunks(2).map(|f| f[0]).collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// Frequency of a sine-like signal from its zero crossings, in Hz.
    fn frequency(x: &[f32]) -> f64 {
        let crossings = x
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        crossings as f64 / 2.0 / (x.len() as f64 / RATE as f64)
    }

    /// The middle half of a signal, away from the stretcher's start-up and tail.
    fn middle(x: &[f32]) -> &[f32] {
        &x[x.len() / 4..x.len() * 3 / 4]
    }

    #[test]
    fn the_output_length_is_the_input_length_divided_by_the_speed_factor() {
        let input = stereo(48_000, |i| sine(1_000.0, i), |i| sine(1_000.0, i));
        assert_eq!(stretch(&input, 1.2, 0).unwrap().frames(), 40_000); // 48000 / 1.2
        assert_eq!(stretch(&input, 0.5, 0).unwrap().frames(), 96_000);
        assert_eq!(stretch(&input, 1.0, 0).unwrap().frames(), 48_000);
        assert_eq!(stretch(&input, 1.0, 0).unwrap().sample_rate, RATE);
    }

    #[test]
    fn the_stretched_length_matches_the_timelines_stretched_placement() {
        use crate::timeline::Placement;
        let input = stereo(48_000, |i| sine(440.0, i), |i| sine(440.0, i));
        let out = stretch(&input, 1.25, 0).unwrap();
        let expected = Placement {
            length_secs: 1.0,
            offset_secs: 0.0,
        }
        .stretched(1.25)
        .length_secs;
        assert_eq!(out.frames(), (expected * RATE as f64).round() as usize);
    }

    #[test]
    fn stretching_preserves_pitch_and_level() {
        let input = stereo(96_000, |i| sine(1_000.0, i), |i| sine(1_000.0, i));
        for factor in [0.8, 1.25, 1.4] {
            let out = left(&stretch(&input, factor, 0).unwrap());
            let mid = middle(&out);
            assert!(
                (frequency(mid) - 1_000.0).abs() < 15.0,
                "{factor}x: {} Hz",
                frequency(mid)
            );
            let want = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
            assert!(
                (rms(mid) - want).abs() < 0.03 * want,
                "{factor}x: rms {}",
                rms(mid)
            );
        }
    }

    #[test]
    fn a_pitch_shift_of_twelve_semitones_doubles_the_frequency_without_changing_length() {
        let input = stereo(96_000, |i| sine(500.0, i), |i| sine(500.0, i));
        let out = stretch(&input, 1.0, 12).unwrap();
        assert_eq!(out.frames(), 96_000);
        let mid_left = left(&out);
        let mid = middle(&mid_left);
        assert!(
            (frequency(mid) - 1_000.0).abs() < 20.0,
            "{} Hz",
            frequency(mid)
        );
    }

    #[test]
    fn a_pitch_shift_of_minus_twelve_semitones_halves_the_frequency() {
        let input = stereo(96_000, |i| sine(1_000.0, i), |i| sine(1_000.0, i));
        let out = left(&stretch(&input, 1.0, -12).unwrap());
        assert!((frequency(middle(&out)) - 500.0).abs() < 15.0);
    }

    /// Times (seconds) of clicks: where a 5 ms energy window first rises above a threshold,
    /// ignoring anything within 0.25 s of the previous click.
    fn click_times(x: &[f32]) -> Vec<f64> {
        let win = (RATE as f64 * 0.005) as usize;
        let energy: Vec<f64> = x
            .chunks(win)
            .map(|c| c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>())
            .collect();
        let peak = energy.iter().cloned().fold(0.0, f64::max);
        let mut times: Vec<f64> = Vec::new();
        for (i, e) in energy.iter().enumerate() {
            let t = (i * win) as f64 / RATE as f64;
            if *e > 0.1 * peak && times.last().is_none_or(|last| t - last > 0.25) {
                times.push(t);
            }
        }
        times
    }

    fn click_track(bpm: f64, secs: f64) -> Stereo {
        let frames = (secs * RATE as f64) as usize;
        let beat = 60.0 / bpm * RATE as f64;
        let click = |i: usize| {
            let since = i as f64 % beat;
            if since < 240.0 {
                (0.9 * (1.0 - since / 240.0) * (2.0 * PI * 1_500.0 * since / RATE as f64).sin())
                    as f32
            } else {
                0.0
            }
        };
        stereo(frames, click, click)
    }

    #[test]
    fn a_100_bpm_click_track_stretched_by_1_2_lands_on_a_120_bpm_grid() {
        let out = left(&stretch(&click_track(100.0, 8.0), 1.2, 0).unwrap());
        let times = click_times(&out);
        // 120 BPM: a click every 0.5 s. Skip the first (start-up) and last (tail) clicks.
        assert!(times.len() >= 10, "{times:?}");
        for (k, t) in times.iter().enumerate().skip(1).take(times.len() - 2) {
            let want = k as f64 * 0.5;
            assert!((t - want).abs() < 0.03, "click {k} at {t}, wanted {want}");
        }
    }

    #[test]
    fn a_slowed_click_track_lands_on_the_slower_grid() {
        // 120 BPM slowed by 0.8x is 96 BPM: a click every 0.625 s.
        let out = left(&stretch(&click_track(120.0, 8.0), 0.8, 0).unwrap());
        let times = click_times(&out);
        assert!(times.len() >= 9, "{times:?}");
        for (k, t) in times.iter().enumerate().skip(1).take(times.len() - 2) {
            let want = k as f64 * 0.625;
            assert!((t - want).abs() < 0.03, "click {k} at {t}, wanted {want}");
        }
    }

    #[test]
    fn the_two_channels_stay_independent() {
        let input = stereo(96_000, |i| sine(800.0, i), |_| 0.0);
        let out = stretch(&input, 1.2, 0).unwrap();
        let right: Vec<f32> = out.samples.chunks(2).map(|f| f[1]).collect();
        assert!(
            rms(middle(&right)) < 0.01,
            "right leaked {}",
            rms(middle(&right))
        );
        assert!(rms(middle(&left(&out))) > 0.3);
    }

    #[test]
    fn a_factor_that_is_not_a_positive_finite_number_is_an_error() {
        let input = stereo(1_000, |_| 0.1, |_| 0.1);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                stretch(&input, bad, 0),
                Err(StretchError::InvalidFactor),
                "{bad}"
            );
        }
    }

    #[test]
    fn empty_audio_stays_empty() {
        let empty = Stereo {
            samples: vec![],
            sample_rate: RATE,
        };
        assert_eq!(stretch(&empty, 1.2, 0).unwrap().frames(), 0);
    }

    #[test]
    fn non_finite_input_samples_never_reach_the_output() {
        let mut input = stereo(48_000, |i| sine(440.0, i), |i| sine(440.0, i));
        input.samples[1_000] = f32::NAN;
        input.samples[1_001] = f32::INFINITY;
        let out = stretch(&input, 1.2, 0).unwrap();
        assert!(out.samples.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn a_clip_shorter_than_the_stretchers_window_still_has_the_right_length() {
        let input = stereo(100, |_| 0.25, |_| 0.25);
        let out = stretch(&input, 1.25, 0).unwrap();
        assert_eq!(out.frames(), 80);
        assert!(out.samples.iter().all(|v| v.is_finite()));
    }
}
