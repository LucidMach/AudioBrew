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
        .map(|b| {
            if b.mute || (any_solo && !b.solo) {
                0.0
            } else {
                b.gain
            }
        })
        .collect()
}

/// The Mashup limiter: output never exceeds full scale (+/-1.0), so summed Tracks cannot clip.
/// A NaN sample becomes silence so it can never reach the output device.
pub fn limit(sample: f32) -> f32 {
    if sample.is_nan() {
        return 0.0;
    }
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
    fn a_shorter_track_is_silent_past_its_end() {
        let long = [0.5, 0.5, 0.5];
        let short = [0.25];
        let tracks = [
            TrackSignal {
                fader: 1.0,
                samples: &long,
            },
            TrackSignal {
                fader: 1.0,
                samples: &short,
            },
        ];

        assert_eq!(mix_down(&tracks), vec![0.75, 0.5, 0.5]);
    }

    #[test]
    fn loud_tracks_are_limited_instead_of_clipping_past_full_scale() {
        let a = [0.75, -0.75];
        let b = [0.75, -0.75];
        let tracks = [
            TrackSignal {
                fader: 1.0,
                samples: &a,
            },
            TrackSignal {
                fader: 1.0,
                samples: &b,
            },
        ];

        assert_eq!(mix_down(&tracks), vec![1.0, -1.0]);
    }

    #[test]
    fn no_tracks_make_an_empty_mashup() {
        assert_eq!(mix_down(&[]), Vec::<f32>::new());
    }

    #[test]
    fn mashup_is_the_faded_sum_of_the_tracks_then_limited() {
        let drums = [0.5, 0.5, -0.25];
        let vocals = [0.75, -0.25, -1.0];
        let tracks = [
            TrackSignal {
                fader: 1.0,
                samples: &drums,
            },
            TrackSignal {
                fader: 0.5,
                samples: &vocals,
            },
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
        let bands = [
            band(0.8, false, false),
            band(0.5, false, true),
            band(0.9, false, false),
        ];

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

    #[test]
    fn the_limiter_never_passes_nan_or_infinity_to_the_output() {
        assert_eq!(limit(f32::NAN), 0.0);
        assert_eq!(limit(f32::INFINITY), 1.0);
        assert_eq!(limit(f32::NEG_INFINITY), -1.0);
    }

    #[test]
    fn full_scale_passes_through_the_limiter_unchanged() {
        assert_eq!(limit(1.0), 1.0);
        assert_eq!(limit(-1.0), -1.0);
    }

    #[test]
    fn a_nan_sample_in_one_track_does_not_poison_the_mashup() {
        let bad = [f32::NAN, 0.25];
        let good = [0.5, 0.5];
        let tracks = [
            TrackSignal {
                fader: 1.0,
                samples: &bad,
            },
            TrackSignal {
                fader: 1.0,
                samples: &good,
            },
        ];
        assert!(mix_down(&tracks).iter().all(|s| s.is_finite()));
    }
}
