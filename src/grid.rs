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
}
