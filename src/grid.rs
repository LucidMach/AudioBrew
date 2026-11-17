/// Beats live on the Mashup grid (after stretching), so one beat is `60 / mashup_bpm` seconds.
pub const BEATS_PER_BAR: f64 = 4.0;

pub fn secs_per_beat(bpm: f64) -> f64 {
    60.0 / bpm
}

pub fn beats_to_secs(beats: f64, bpm: f64) -> f64 {
    beats * secs_per_beat(bpm)
}

pub fn secs_to_beats(secs: f64, bpm: f64) -> f64 {
    secs / secs_per_beat(bpm)
}

/// Snaps a Loop region (source seconds on a Track) to the beat grid.
/// Edges may be given in either order; the region is at least one beat long and must
/// fit inside the Track. `None` for an unusable tempo, non-finite input, or no room.
pub fn snap_loop(a_secs: f64, b_secs: f64, bpm: f64, track_len_secs: f64) -> Option<(f64, f64)> {
    let usable = bpm.is_finite() && bpm > 0.0 && track_len_secs.is_finite();
    if !usable || !a_secs.is_finite() || !b_secs.is_finite() {
        return None;
    }
    let start = secs_to_beats(a_secs.min(b_secs), bpm).round().max(0.0);
    let mut end = secs_to_beats(a_secs.max(b_secs), bpm).round();
    if end - start < 1.0 {
        end = start + 1.0;
    }
    let (start_secs, end_secs) = (beats_to_secs(start, bpm), beats_to_secs(end, bpm));
    (end_secs <= track_len_secs + 1e-9).then_some((start_secs, end_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn one_beat_at_60_bpm_is_exactly_one_second() {
        assert_eq!(secs_per_beat(60.0), 1.0);
    }

    #[test]
    fn one_beat_at_120_bpm_is_half_a_second() {
        assert_eq!(secs_per_beat(120.0), 0.5);
    }

    #[test]
    fn four_beats_at_120_bpm_is_two_seconds() {
        assert_eq!(beats_to_secs(4.0, 120.0), 2.0);
    }

    #[test]
    fn the_same_two_beats_last_longer_at_a_slower_tempo() {
        assert_eq!(beats_to_secs(2.0, 120.0), 1.0);
        assert!(close(beats_to_secs(2.0, 100.0), 1.2));
    }

    #[test]
    fn two_seconds_at_100_bpm_is_three_and_a_third_beats() {
        assert!(close(secs_to_beats(2.0, 100.0), 10.0 / 3.0));
    }

    #[test]
    fn a_loop_region_snaps_both_edges_to_the_nearest_beat() {
        // 120 BPM: 1.12 s is 2.24 beats -> beat 2 (1.0 s); 3.9 s is 7.8 beats -> beat 8 (4.0 s).
        assert_eq!(snap_loop(1.12, 3.9, 120.0, 10.0), Some((1.0, 4.0)));
    }

    #[test]
    fn a_loop_region_is_at_least_one_beat_long() {
        assert_eq!(snap_loop(1.0, 1.1, 120.0, 10.0), Some((1.0, 1.5)));
    }

    #[test]
    fn a_loop_region_dragged_backwards_is_the_same_region() {
        assert_eq!(snap_loop(3.9, 1.12, 120.0, 10.0), Some((1.0, 4.0)));
    }

    #[test]
    fn a_loop_region_that_cannot_fit_inside_the_track_is_rejected() {
        // Snaps to 10.0 s on a 10 s Track: no room left for even one beat.
        assert_eq!(snap_loop(9.9, 10.0, 120.0, 10.0), None);
    }

    #[test]
    fn a_loop_region_cannot_start_before_the_track_does() {
        assert_eq!(snap_loop(-0.4, 2.0, 120.0, 10.0), Some((0.0, 2.0)));
    }

    #[test]
    fn a_loop_region_needs_a_usable_tempo_and_finite_edges() {
        assert_eq!(snap_loop(1.0, 3.0, 0.0, 10.0), None);
        assert_eq!(snap_loop(1.0, 3.0, f64::NAN, 10.0), None);
        assert_eq!(snap_loop(f64::NAN, 3.0, 120.0, 10.0), None);
        assert_eq!(snap_loop(1.0, f64::INFINITY, 120.0, 10.0), None);
    }
}
