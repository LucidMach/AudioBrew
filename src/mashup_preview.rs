/// One Track's contribution to the Mashup preview.
#[derive(Debug, Clone, Copy)]
pub struct PreviewTrack<'a> {
    /// The Track's waveform picture: peak levels evenly across its whole length.
    pub peaks: &'a [f32],
    /// How long the Track plays on the Mashup, after stretching, in seconds.
    pub length_secs: f64,
    /// Where it starts on the Mashup, in seconds. Before this it is silent.
    pub offset_secs: f64,
    /// Its fader, 0.0 and up.
    pub gain: f32,
}

/// A waveform picture of the whole Mashup: `columns` peak levels over `end_secs`. Each Track
/// repeats from its start to fill the time up to `end_secs`, is scaled by its fader, and the
/// Tracks are summed and limited to 1.0, matching how the Mashup is mixed.
pub fn mashup_preview(tracks: &[PreviewTrack], end_secs: f64, columns: usize) -> Vec<f32> {
    if !(end_secs.is_finite() && end_secs > 0.0) {
        return vec![0.0; columns];
    }
    let dt = end_secs / columns.max(1) as f64;
    (0..columns)
        .map(|column| {
            let (t0, t1) = (column as f64 * dt, (column + 1) as f64 * dt);
            let sum: f32 = tracks.iter().map(|t| loudest(t, t0, t1) * t.gain.max(0.0)).sum();
            sum.min(1.0)
        })
        .collect()
}

/// The loudest peak of `track` between Mashup times `t0` and `t1`, or 0.0 before it starts.
/// The Track repeats from its start, so a span can wrap past its end.
fn loudest(track: &PreviewTrack, t0: f64, t1: f64) -> f32 {
    let usable = track.length_secs.is_finite()
        && track.length_secs > 0.0
        && track.offset_secs.is_finite()
        && !track.peaks.is_empty();
    if !usable || t1 <= track.offset_secs {
        return 0.0;
    }
    let from = t0.max(track.offset_secs);
    let local0 = (from - track.offset_secs).rem_euclid(track.length_secs);
    let local1 = local0 + (t1 - from);
    let n = track.peaks.len();
    // Peak index range covering [a, b) seconds of the Track, a < b <= length.
    let range = |a: f64, b: f64| {
        let first = ((a / track.length_secs * n as f64).floor() as usize).min(n - 1);
        let last = (((b / track.length_secs * n as f64) - 1e-9).ceil() as usize)
            .saturating_sub(1)
            .clamp(first, n - 1);
        track.peaks[first..=last]
            .iter()
            .filter(|p| p.is_finite())
            .fold(0.0f32, |m, p| m.max(p.abs()))
    };
    if local1 <= track.length_secs {
        range(local0, local1)
    } else {
        range(local0, track.length_secs).max(range(0.0, local1 - track.length_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(peaks: &[f32], length_secs: f64, offset_secs: f64, gain: f32) -> PreviewTrack<'_> {
        PreviewTrack { peaks, length_secs, offset_secs, gain }
    }

    #[test]
    fn a_shorter_track_repeats_to_fill_the_whole_mashup() {
        let peaks = [0.5; 8];

        let preview = mashup_preview(&[track(&peaks, 4.0, 0.0, 1.0)], 8.0, 8);

        assert_eq!(preview, vec![0.5; 8]);
    }

    #[test]
    fn a_track_is_silent_before_its_offset() {
        let peaks = [0.5; 8];

        let preview = mashup_preview(&[track(&peaks, 4.0, 4.0, 1.0)], 8.0, 8);

        assert_eq!(preview, vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn tracks_are_summed_scaled_by_their_faders_and_limited_to_one() {
        let (a, b) = ([0.5; 4], [0.25; 4]);
        let summed = mashup_preview(&[track(&a, 4.0, 0.0, 1.0), track(&b, 4.0, 0.0, 1.0)], 4.0, 2);
        assert_eq!(summed, vec![0.75, 0.75]);

        let (loud1, loud2) = ([0.8; 4], [0.8; 4]);
        let limited = mashup_preview(&[track(&loud1, 4.0, 0.0, 1.0), track(&loud2, 4.0, 0.0, 1.0)], 4.0, 2);
        assert_eq!(limited, vec![1.0, 1.0]);

        let quiet = mashup_preview(&[track(&loud1, 4.0, 0.0, 0.5)], 4.0, 2);
        assert_eq!(quiet, vec![0.4, 0.4]);
    }

    #[test]
    fn a_column_covering_several_peaks_shows_the_loudest_not_the_average() {
        // Two columns over four peaks: each column spans two of them.
        let peaks = [0.1, 0.9, 0.2, 0.7];

        let preview = mashup_preview(&[track(&peaks, 4.0, 0.0, 1.0)], 4.0, 2);

        assert_eq!(preview, vec![0.9, 0.7]);
    }

    #[test]
    fn nothing_to_draw_is_safe() {
        assert_eq!(mashup_preview(&[], 8.0, 4), vec![0.0; 4]);
        assert_eq!(mashup_preview(&[track(&[0.5], 4.0, 0.0, 1.0)], 0.0, 4), vec![0.0; 4]);
        assert_eq!(mashup_preview(&[track(&[], 4.0, 0.0, 1.0)], 8.0, 3), vec![0.0; 3]);
        assert_eq!(mashup_preview(&[track(&[0.5], 0.0, 0.0, 1.0)], 8.0, 3), vec![0.0; 3]);
        assert_eq!(mashup_preview(&[track(&[0.5], 4.0, 0.0, 1.0)], 8.0, 0), Vec::<f32>::new());
    }
}
