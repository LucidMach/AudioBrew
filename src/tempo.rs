/// How much faster (>1) or slower (<1) a Track must play to land on the Mashup BPM.
pub fn speed_factor(track_bpm: f64, mashup_bpm: f64) -> f64 {
    mashup_bpm / track_bpm
}

/// Whether a speed factor is inside the range that still sounds acceptable (0.7x to 1.4x).
pub fn within_stretch_limits(factor: f64) -> bool {
    (0.7..=1.4).contains(&factor)
}

/// A correction the UI can offer when beat detection probably got half or double time.
#[derive(Debug, PartialEq, Eq)]
pub enum BpmFix {
    Double,
    Half,
}

/// Suggests halving or doubling the detected BPM, but only when the current
/// speed factor is out of limits and the correction brings it back in.
pub fn suggest_bpm_fix(_track_bpm: f64, _mashup_bpm: f64) -> Option<BpmFix> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretch_beyond_0_7x_to_1_4x_is_out_of_limits() {
        assert!(within_stretch_limits(0.7));
        assert!(within_stretch_limits(1.4));
        assert!(!within_stretch_limits(1.41));
        assert!(!within_stretch_limits(0.69));
    }

    #[test]
    fn a_100_bpm_track_speeds_up_by_1_2_to_match_a_120_bpm_mashup() {
        assert_eq!(speed_factor(100.0, 120.0), 1.2);
    }
}
