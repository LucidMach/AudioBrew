/// How much faster (>1) or slower (<1) a Track must play to land on the Mashup BPM.
pub fn speed_factor(track_bpm: f64, mashup_bpm: f64) -> f64 {
    mashup_bpm / track_bpm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_100_bpm_track_speeds_up_by_1_2_to_match_a_120_bpm_mashup() {
        assert_eq!(speed_factor(100.0, 120.0), 1.2);
    }
}
