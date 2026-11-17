use crate::key::{Compatibility, compatibility};
use crate::project::Project;

/// What a lane's Key badge shows.
#[derive(Debug, PartialEq, Eq)]
pub struct KeyBadge {
    /// The Camelot code, such as "8A" (minor) or "8B" (major).
    pub label: String,
    /// Green, amber or red relative to the first keyed Track. `None` on that reference Track
    /// itself, which only shows its label.
    pub compatibility: Option<Compatibility>,
}

/// The badge for `lane`, or `None` if the lane is empty or its Track has no detected key.
pub fn key_badge(project: &Project, lane: usize) -> Option<KeyBadge> {
    let key = project.track(lane)?.key?;
    let reference_lane = (0..project.lane_count())
        .find(|&i| project.track(i).is_some_and(|t| t.key.is_some()));
    let compatibility = match (reference_lane, project.key_reference()) {
        (Some(reference), Some(reference_key)) if reference != lane => {
            Some(compatibility(reference_key, key))
        }
        _ => None,
    };
    Some(KeyBadge {
        label: format!("{}{}", key.number, if key.minor { 'A' } else { 'B' }),
        compatibility,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Key;
    use crate::project::LoadedTrack;

    fn track(key: Option<Key>) -> LoadedTrack {
        LoadedTrack {
            source: "x.wav".into(),
            bpm: Some(120.0),
            key,
            length_secs: 10.0,
            offset_beats: 0.0,
            ..Default::default()
        }
    }

    fn keyed(number: u8, minor: bool) -> Option<Key> {
        Key::new(number, minor)
    }

    #[test]
    fn the_reference_track_shows_its_camelot_code_without_a_colour() {
        let mut p = Project::new();
        p.load_track(0, track(keyed(8, true))).unwrap();

        assert_eq!(
            key_badge(&p, 0),
            Some(KeyBadge { label: "8A".into(), compatibility: None })
        );
    }

    #[test]
    fn a_later_track_is_coloured_by_how_well_it_mixes_with_the_reference() {
        let mut p = Project::new();
        p.add_lane();
        p.add_lane();
        p.load_track(0, track(keyed(8, true))).unwrap(); // 8A, the reference
        p.load_track(1, track(keyed(9, true))).unwrap(); // 9A: next to it on the wheel
        p.load_track(2, track(keyed(10, true))).unwrap(); // 10A: two steps
        p.load_track(3, track(keyed(3, false))).unwrap(); // 3B: far away

        let colour = |lane| key_badge(&p, lane).unwrap().compatibility;
        assert_eq!(colour(1), Some(Compatibility::Green));
        assert_eq!(colour(2), Some(Compatibility::Amber));
        assert_eq!(colour(3), Some(Compatibility::Red));
        assert_eq!(key_badge(&p, 3).unwrap().label, "3B");
    }

    #[test]
    fn lanes_without_a_key_show_no_badge() {
        let mut p = Project::new();
        p.load_track(1, track(None)).unwrap(); // loaded, but no key detected

        assert_eq!(key_badge(&p, 0), None, "empty lane");
        assert_eq!(key_badge(&p, 1), None, "Track without a key");
        assert_eq!(key_badge(&p, 9), None, "lane that does not exist");
    }

    #[test]
    fn the_reference_is_the_first_track_that_has_a_key_not_just_the_first_track() {
        let mut p = Project::new();
        p.add_lane();
        p.load_track(0, track(None)).unwrap();
        p.load_track(1, track(keyed(5, false))).unwrap(); // the first keyed Track: 5B
        p.load_track(2, track(keyed(6, false))).unwrap();

        assert_eq!(key_badge(&p, 1).unwrap().compatibility, None);
        assert_eq!(key_badge(&p, 2).unwrap().compatibility, Some(Compatibility::Green));
    }

    #[test]
    fn removing_the_reference_track_hands_the_reference_to_the_next_keyed_track() {
        let mut p = Project::new();
        p.load_track(0, track(keyed(8, true))).unwrap();
        p.load_track(1, track(keyed(9, true))).unwrap();
        assert_eq!(key_badge(&p, 1).unwrap().compatibility, Some(Compatibility::Green));

        p.clear_track(0).unwrap();

        assert_eq!(key_badge(&p, 1).unwrap().compatibility, None);
    }
}
