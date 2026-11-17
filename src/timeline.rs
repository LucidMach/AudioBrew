/// A Track placed on the timeline: how long it plays and how far it is shifted
/// from the start of the Mashup (its Offset).
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub length_secs: f64,
    pub offset_secs: f64,
}

impl Placement {
    /// The Placement after speeding the Track up (>1) or slowing it down (<1).
    /// The Offset is a position on the Mashup timeline, so only the length changes.
    /// An unusable factor leaves the Placement unchanged.
    pub fn stretched(self, factor: f64) -> Placement {
        if !factor.is_finite() || factor <= 0.0 {
            return self;
        }
        Placement {
            length_secs: self.length_secs / factor,
            ..self
        }
    }
}

/// The Mashup ends where the last audible Track ends.
pub fn mashup_length_secs(tracks: &[Placement]) -> f64 {
    tracks
        .iter()
        .map(|t| t.offset_secs + t.length_secs)
        .fold(0.0, f64::max)
}

/// Where the Mashup ends: the user's End marker if they dragged it,
/// otherwise where the longest Track ends.
pub fn end_marker_secs(tracks: &[Placement], user_set_secs: Option<f64>) -> f64 {
    match user_set_secs {
        Some(secs) if secs.is_finite() => secs.max(0.0),
        _ => mashup_length_secs(tracks),
    }
}

/// Times (in Mashup seconds) where a repeating Track starts a new pass,
/// up to but not including the end marker.
pub fn repeat_seams_secs(track: Placement, end_marker_secs: f64) -> Vec<f64> {
    let all_finite = track.length_secs.is_finite()
        && track.offset_secs.is_finite()
        && end_marker_secs.is_finite();
    if !all_finite || track.length_secs <= 0.0 {
        return Vec::new();
    }
    (1..)
        .map(|pass| track.offset_secs + pass as f64 * track.length_secs)
        .take_while(|&seam| seam < end_marker_secs)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_length_track_never_repeats() {
        // Guards against an infinite list of seams at the same instant.
        let empty = Placement {
            length_secs: 0.0,
            offset_secs: 2.0,
        };

        assert_eq!(repeat_seams_secs(empty, 10.0), Vec::<f64>::new());
    }

    #[test]
    fn dragged_end_marker_overrides_the_longest_track_default() {
        let tracks = [
            Placement {
                length_secs: 10.0,
                offset_secs: 0.0,
            },
            Placement {
                length_secs: 8.0,
                offset_secs: 5.0,
            },
        ];

        assert_eq!(end_marker_secs(&tracks, None), 13.0);
        assert_eq!(end_marker_secs(&tracks, Some(6.0)), 6.0);
    }

    #[test]
    fn repeating_track_starts_a_new_pass_every_length_until_the_end_marker() {
        // A 4s drum loop at offset 0 under a Mashup that ends at 10s
        // plays 2.5 times: new passes begin at 4s and 8s.
        let drums = Placement {
            length_secs: 4.0,
            offset_secs: 0.0,
        };

        assert_eq!(repeat_seams_secs(drums, 10.0), vec![4.0, 8.0]);
    }

    #[test]
    fn mashup_ends_where_the_last_offset_track_ends() {
        // Track 1: 10s long, starts at 0s -> ends at 10s.
        // Track 2: 8s long, nudged to start at 5s -> ends at 13s.
        let tracks = [
            Placement {
                length_secs: 10.0,
                offset_secs: 0.0,
            },
            Placement {
                length_secs: 8.0,
                offset_secs: 5.0,
            },
        ];

        assert_eq!(mashup_length_secs(&tracks), 13.0);
    }

    fn at(length_secs: f64, offset_secs: f64) -> Placement {
        Placement {
            length_secs,
            offset_secs,
        }
    }

    #[test]
    fn a_stretched_track_is_shorter_when_sped_up_and_longer_when_slowed() {
        // 10 s at 1.25x plays in 8 s; 10 s at 0.5x takes 20 s.
        assert_eq!(at(10.0, 3.0).stretched(1.25).length_secs, 8.0);
        assert_eq!(at(10.0, 3.0).stretched(0.5).length_secs, 20.0);
        assert_eq!(at(10.0, 3.0).stretched(1.25).offset_secs, 3.0);
    }

    #[test]
    fn stretching_by_a_useless_factor_leaves_the_track_alone() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(at(10.0, 0.0).stretched(bad).length_secs, 10.0);
        }
    }

    #[test]
    fn no_tracks_make_a_zero_length_mashup() {
        assert_eq!(mashup_length_secs(&[]), 0.0);
    }

    #[test]
    fn a_track_shifted_left_of_zero_ends_earlier_and_its_seams_follow() {
        // 10 s long starting at -2 s: ends at 8 s; its next pass would start at 8 s.
        assert_eq!(mashup_length_secs(&[at(10.0, -2.0)]), 8.0);
        assert_eq!(repeat_seams_secs(at(10.0, -2.0), 20.0), vec![8.0, 18.0]);
    }

    #[test]
    fn a_seam_exactly_on_the_end_marker_is_not_a_new_pass() {
        assert_eq!(repeat_seams_secs(at(5.0, 0.0), 10.0), vec![5.0]);
    }

    #[test]
    fn a_track_starting_after_the_end_marker_has_no_seams() {
        assert_eq!(repeat_seams_secs(at(2.0, 12.0), 10.0), Vec::<f64>::new());
    }

    #[test]
    fn a_short_loop_does_not_gain_a_spurious_seam_from_float_drift() {
        // 0.1 s passes under a 1.0 s End marker: seams at 0.1 .. 0.9 (nine), none at 1.0.
        assert_eq!(repeat_seams_secs(at(0.1, 0.0), 1.0).len(), 9);
    }

    #[test]
    fn non_finite_inputs_make_no_seams_instead_of_hanging() {
        assert!(repeat_seams_secs(at(f64::NAN, 0.0), 10.0).is_empty());
        assert!(repeat_seams_secs(at(4.0, f64::NAN), 10.0).is_empty());
        assert!(repeat_seams_secs(at(4.0, 0.0), f64::NAN).is_empty());
        assert!(repeat_seams_secs(at(f64::INFINITY, 0.0), 10.0).is_empty());
        assert!(repeat_seams_secs(at(4.0, 0.0), f64::INFINITY).is_empty());
    }

    #[test]
    fn a_user_end_marker_that_is_nonsense_falls_back_or_clamps() {
        let tracks = [at(10.0, 0.0)];
        assert_eq!(end_marker_secs(&tracks, Some(f64::NAN)), 10.0);
        assert_eq!(end_marker_secs(&tracks, Some(f64::INFINITY)), 10.0);
        assert_eq!(end_marker_secs(&tracks, Some(-3.0)), 0.0);
        assert_eq!(end_marker_secs(&tracks, Some(0.0)), 0.0);
    }
}
