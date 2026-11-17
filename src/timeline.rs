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

#[cfg(test)]
mod tests {
    use super::*;

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
