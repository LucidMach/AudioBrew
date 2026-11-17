/// Peak level (0.0 to 1.0+) of each of `buckets` equal slices of interleaved audio with
/// `channels` channels, taking the loudest channel. Always returns exactly `buckets` values,
/// so a lane can draw one bar per pixel column; slices with no audio are 0.0.
pub fn peaks(interleaved: &[f32], channels: usize, buckets: usize) -> Vec<f32> {
    if channels == 0 {
        return vec![0.0; buckets];
    }
    let frames = interleaved.len() / channels;
    (0..buckets)
        .map(|bucket| {
            let start = bucket * frames / buckets;
            let end = (bucket + 1) * frames / buckets;
            interleaved[start * channels..end * channels]
                .iter()
                .filter(|s| s.is_finite())
                .fold(0.0f32, |peak, s| peak.max(s.abs()))
        })
        .collect()
}

/// The first `fraction` (0.0 to 1.0) of a waveform `picture`, resampled to exactly `bars` bars.
/// A clip cut off by the End marker shows only the part that still plays.
pub fn visible_bars(picture: &[f32], fraction: f64, bars: usize) -> Vec<f32> {
    if !fraction.is_finite() || fraction <= 0.0 {
        return vec![0.0; bars];
    }
    let keep = ((picture.len() as f64 * fraction.min(1.0)).ceil() as usize).min(picture.len());
    peaks(&picture[..keep], 1, bars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_of_the_picture_is_resampled_to_the_requested_bars() {
        let picture = [0.1, 0.9, 0.2, 0.7];

        assert_eq!(visible_bars(&picture, 1.0, 2), vec![0.9, 0.7]);
        assert_eq!(visible_bars(&picture, 0.5, 1), vec![0.9], "only the first two peaks remain");
        assert_eq!(visible_bars(&picture, 0.25, 1), vec![0.1]);
    }

    #[test]
    fn nothing_visible_or_nothing_to_show_gives_silent_bars() {
        assert_eq!(visible_bars(&[0.5, 0.5], 0.0, 3), vec![0.0; 3]);
        assert_eq!(visible_bars(&[], 1.0, 3), vec![0.0; 3]);
        assert_eq!(visible_bars(&[0.5], f64::NAN, 2), vec![0.0; 2]);
        assert_eq!(visible_bars(&[0.5], 1.0, 0), Vec::<f32>::new());
    }

    #[test]
    fn each_bucket_is_the_loudest_sample_in_its_slice() {
        // 8 mono frames in 2 buckets: [0.1, -0.5, 0.2, 0.0] and [0.9, -0.3, 0.0, 0.4].
        let audio = [0.1, -0.5, 0.2, 0.0, 0.9, -0.3, 0.0, 0.4];

        assert_eq!(peaks(&audio, 1, 2), vec![0.5, 0.9]);
    }

    #[test]
    fn stereo_takes_the_louder_channel_of_each_frame() {
        // Frames (L, R): (0.2, -0.6) (0.1, 0.0) | (-0.8, 0.3) (0.0, 0.5)
        let audio = [0.2, -0.6, 0.1, 0.0, -0.8, 0.3, 0.0, 0.5];

        assert_eq!(peaks(&audio, 2, 2), vec![0.6, 0.8]);
    }

    #[test]
    fn it_always_returns_one_value_per_bucket() {
        // Fewer frames than buckets: the buckets with no frame are silent.
        let audio = [0.5, -0.25];

        let result = peaks(&audio, 1, 5);

        assert_eq!(result.len(), 5);
        assert_eq!(result.iter().filter(|&&v| v > 0.0).count(), 2);
        assert_eq!(result.iter().cloned().fold(0.0, f32::max), 0.5);
    }

    #[test]
    fn no_audio_or_no_buckets_is_safe() {
        assert_eq!(peaks(&[], 2, 4), vec![0.0; 4]);
        assert_eq!(peaks(&[0.3, 0.3], 2, 0), Vec::<f32>::new());
        assert_eq!(peaks(&[0.3], 0, 3), vec![0.0; 3], "zero channels is not a divide by zero");
    }

    #[test]
    fn a_bad_sample_does_not_poison_the_picture() {
        let audio = [f32::NAN, 0.25, f32::INFINITY, -0.5];

        let result = peaks(&audio, 1, 2);

        assert!(result.iter().all(|v| v.is_finite()), "{result:?}");
        assert_eq!(result[0], 0.25);
    }
}
