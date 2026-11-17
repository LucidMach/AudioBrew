/// How much faster (>1) or slower (<1) a Track must play to land on the Mashup BPM.
pub fn speed_factor(track_bpm: f64, mashup_bpm: f64) -> Option<f64> {
    if !(track_bpm.is_finite() && mashup_bpm.is_finite()) || track_bpm <= 0.0 || mashup_bpm <= 0.0 {
        return None;
    }
    Some(mashup_bpm / track_bpm)
}

/// Whether a speed factor is inside the range that still sounds acceptable (0.7x to 1.4x).
/// A tiny tolerance absorbs float rounding so a ratio that is 1.4x in decimal still passes.
pub fn within_stretch_limits(factor: f64) -> bool {
    const EPS: f64 = 1e-9;
    (0.7 - EPS..=1.4 + EPS).contains(&factor)
}

/// A correction the UI can offer when beat detection probably got half or double time.
#[derive(Debug, PartialEq, Eq)]
pub enum BpmFix {
    Double,
    Half,
}

/// Suggests halving or doubling the detected BPM, but only when the current
/// speed factor is out of limits and the correction brings it back in.
pub fn suggest_bpm_fix(track_bpm: f64, mashup_bpm: f64) -> Option<BpmFix> {
    if speed_factor(track_bpm, mashup_bpm).is_some_and(within_stretch_limits) {
        return None;
    }
    if speed_factor(track_bpm * 2.0, mashup_bpm).is_some_and(within_stretch_limits) {
        return Some(BpmFix::Double);
    }
    if speed_factor(track_bpm / 2.0, mashup_bpm).is_some_and(within_stretch_limits) {
        return Some(BpmFix::Half);
    }
    None
}

/// A pause longer than this between taps starts a fresh run (it is slower than 30 BPM).
const TAP_RESET_SECS: f64 = 2.0;

/// BPM from tap timestamps (seconds), averaged over the latest unbroken run of taps.
/// Needs at least two taps in that run; timestamps must be finite and strictly increasing.
pub fn tap_tempo_bpm(taps: &[f64]) -> Option<f64> {
    if taps.iter().any(|t| !t.is_finite()) || taps.windows(2).any(|w| w[1] <= w[0]) {
        return None;
    }
    let mut start = taps.len().checked_sub(1)?;
    while start > 0 && taps[start] - taps[start - 1] <= TAP_RESET_SECS {
        start -= 1;
    }
    let run = &taps[start..];
    if run.len() < 2 {
        return None;
    }
    let average_gap = (run[run.len() - 1] - run[0]) / (run.len() - 1) as f64;
    Some(60.0 / average_gap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_doubling_when_detected_tempo_is_half_the_mashup_tempo() {
        // 60 vs 120 needs 2.0x (out of limits); doubling 60 -> 120 needs 1.0x.
        assert_eq!(suggest_bpm_fix(60.0, 120.0), Some(BpmFix::Double));
    }

    #[test]
    fn suggests_halving_when_detected_tempo_is_double_the_mashup_tempo() {
        // 240 vs 120 needs 0.5x (out of limits); halving 240 -> 120 needs 1.0x.
        assert_eq!(suggest_bpm_fix(240.0, 120.0), Some(BpmFix::Half));
    }

    #[test]
    fn no_suggestion_when_the_stretch_is_already_fine() {
        assert_eq!(suggest_bpm_fix(110.0, 120.0), None);
    }

    #[test]
    fn stretch_beyond_0_7x_to_1_4x_is_out_of_limits() {
        assert!(within_stretch_limits(0.7));
        assert!(within_stretch_limits(1.4));
        assert!(!within_stretch_limits(1.41));
        assert!(!within_stretch_limits(0.69));
    }

    #[test]
    fn a_tempo_of_zero_or_less_has_no_speed_factor() {
        assert_eq!(speed_factor(0.0, 120.0), None);
        assert_eq!(speed_factor(-90.0, 120.0), None);
        assert_eq!(speed_factor(100.0, 0.0), None);
    }

    #[test]
    fn a_100_bpm_track_speeds_up_by_1_2_to_match_a_120_bpm_mashup() {
        assert_eq!(speed_factor(100.0, 120.0), Some(1.2));
    }

    #[test]
    fn non_finite_tempos_have_no_speed_factor() {
        assert_eq!(speed_factor(f64::NAN, 120.0), None);
        assert_eq!(speed_factor(100.0, f64::NAN), None);
        assert_eq!(speed_factor(f64::INFINITY, 120.0), None);
        assert_eq!(speed_factor(100.0, f64::INFINITY), None);
    }

    #[test]
    fn a_ratio_that_is_1_4x_within_rounding_is_in_limits() {
        // 100 * 1.4 = 140 in decimal, but 98.0 / 70.0 and 120.0 / (120.0 / 1.4) drift in binary floats.
        let track = 120.0 / 1.4; // 85.714285...
        let factor = speed_factor(track, 120.0).unwrap();
        assert!(within_stretch_limits(factor));
        // and the lower edge: a 0.7x ratio
        let slow = speed_factor(120.0 / 0.7, 120.0).unwrap();
        assert!(within_stretch_limits(slow));
    }

    #[test]
    fn nan_is_never_within_stretch_limits() {
        assert!(!within_stretch_limits(f64::NAN));
    }

    #[test]
    fn no_fix_is_suggested_for_unusable_tempos() {
        assert_eq!(suggest_bpm_fix(f64::NAN, 120.0), None);
        assert_eq!(suggest_bpm_fix(0.0, 120.0), None);
        assert_eq!(suggest_bpm_fix(-60.0, 120.0), None);
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn taps_half_a_second_apart_are_120_bpm() {
        assert!(close(tap_tempo_bpm(&[0.0, 0.5, 1.0, 1.5]).unwrap(), 120.0));
    }

    #[test]
    fn two_taps_one_second_apart_are_60_bpm() {
        assert!(close(tap_tempo_bpm(&[3.0, 4.0]).unwrap(), 60.0));
    }

    #[test]
    fn fewer_than_two_taps_give_no_tempo() {
        assert_eq!(tap_tempo_bpm(&[]), None);
        assert_eq!(tap_tempo_bpm(&[1.0]), None);
    }

    #[test]
    fn uneven_taps_are_averaged_over_the_whole_run() {
        // Four taps spanning 1.5 s with 3 gaps: 0.5 s average = 120 BPM, jitter and all.
        assert!(close(
            tap_tempo_bpm(&[0.0, 0.51, 0.99, 1.5]).unwrap(),
            120.0
        ));
    }

    #[test]
    fn a_long_pause_starts_a_fresh_run_of_taps() {
        // The first two taps are stale; the run after the 8.8 s gap is 0.6 s apart = 100 BPM.
        let taps = [0.0, 0.5, 9.3, 9.9, 10.5];
        assert!(close(tap_tempo_bpm(&taps).unwrap(), 100.0));
    }

    #[test]
    fn a_single_tap_after_a_pause_gives_no_tempo_yet() {
        assert_eq!(tap_tempo_bpm(&[0.0, 0.5, 9.0]), None);
    }

    #[test]
    fn taps_that_go_backwards_or_are_not_finite_give_no_tempo() {
        assert_eq!(tap_tempo_bpm(&[1.0, 0.5]), None);
        assert_eq!(tap_tempo_bpm(&[1.0, 1.0]), None);
        assert_eq!(tap_tempo_bpm(&[0.0, f64::NAN, 1.0]), None);
    }
}
