/// A Track placed on the timeline: how long it plays and how far it is shifted
/// from the start of the Mashup (its Offset).
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub length_secs: f64,
    pub offset_secs: f64,
}

/// The Mashup ends where the last audible Track ends.
pub fn mashup_length_secs(tracks: &[Placement]) -> f64 {
    tracks
        .iter()
        .map(|t| t.offset_secs + t.length_secs)
        .fold(0.0, f64::max)
}

/// Times (in Mashup seconds) where a repeating Track starts a new pass,
/// up to but not including the end marker.
pub fn repeat_seams_secs(track: Placement, end_marker_secs: f64) -> Vec<f64> {
    if track.length_secs <= 0.0 {
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
    fn repeating_track_starts_a_new_pass_every_length_until_the_end_marker() {
        // A 4s drum loop at offset 0 under a Mashup that ends at 10s
        // plays 2.5 times: new passes begin at 4s and 8s.
        let drums = Placement { length_secs: 4.0, offset_secs: 0.0 };

        assert_eq!(repeat_seams_secs(drums, 10.0), vec![4.0, 8.0]);
    }

    #[test]
    fn mashup_ends_where_the_last_offset_track_ends() {
        // Track 1: 10s long, starts at 0s -> ends at 10s.
        // Track 2: 8s long, nudged to start at 5s -> ends at 13s.
        let tracks = [
            Placement { length_secs: 10.0, offset_secs: 0.0 },
            Placement { length_secs: 8.0, offset_secs: 5.0 },
        ];

        assert_eq!(mashup_length_secs(&tracks), 13.0);
    }
}
