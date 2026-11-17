/// One Band's controls on a Track.
#[derive(Debug, Clone, Copy)]
pub struct BandState {
    pub gain: f32,
    pub mute: bool,
    pub solo: bool,
}

/// The gain each Band actually contributes once mute and solo are applied.
pub fn effective_gains(bands: &[BandState]) -> Vec<f32> {
    let any_solo = bands.iter().any(|b| b.solo);
    bands
        .iter()
        .map(|b| if b.mute || (any_solo && !b.solo) { 0.0 } else { b.gain })
        .collect()
}

/// The Mashup limiter: output never exceeds full scale (+/-1.0), so summed Tracks cannot clip.
pub fn limit(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

/// One Track's already-processed samples and its fader level.
pub struct TrackSignal<'a> {
    pub fader: f32,
    pub samples: &'a [f32],
}

/// Sums every Track (after its fader) into the Mashup, then limits the result.
/// A Track shorter than the longest one is silent past its end.
pub fn mix_down(tracks: &[TrackSignal]) -> Vec<f32> {
    let length = tracks.iter().map(|t| t.samples.len()).max().unwrap_or(0);
    (0..length)
        .map(|i| {
            let sum: f32 = tracks
                .iter()
                .map(|t| t.samples.get(i).copied().unwrap_or(0.0) * t.fader)
                .sum();
            limit(sum)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mashup_is_the_faded_sum_of_the_tracks_then_limited() {
        let drums = [0.5, 0.5, -0.25];
        let vocals = [0.75, -0.25, -1.0];
        let tracks = [
            TrackSignal { fader: 1.0, samples: &drums },
            TrackSignal { fader: 0.5, samples: &vocals },
        ];

        // 0.5 + 0.375 = 0.875 | 0.5 - 0.125 = 0.375 | -0.25 - 0.5 = -0.75
        assert_eq!(mix_down(&tracks), vec![0.875, 0.375, -0.75]);
    }

    #[test]
    fn the_limiter_keeps_the_sum_within_full_scale() {
        assert_eq!(limit(1.5), 1.0);
        assert_eq!(limit(-1.5), -1.0);
        assert_eq!(limit(0.5), 0.5);
    }

    fn band(gain: f32, mute: bool, solo: bool) -> BandState {
        BandState { gain, mute, solo }
    }

    #[test]
    fn soloing_a_band_silences_the_others() {
        let bands = [band(0.8, false, false), band(0.5, false, true), band(0.9, false, false)];

        assert_eq!(effective_gains(&bands), vec![0.0, 0.5, 0.0]);
    }

    #[test]
    fn mute_wins_over_solo_on_the_same_band() {
        let bands = [band(0.8, false, false), band(0.5, true, true)];

        assert_eq!(effective_gains(&bands), vec![0.0, 0.0]);
    }

    #[test]
    fn a_muted_band_contributes_nothing() {
        let bands = [band(0.8, false, false), band(0.5, true, false)];

        assert_eq!(effective_gains(&bands), vec![0.8, 0.0]);
    }
}
