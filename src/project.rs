use crate::grid::beats_to_secs;
use crate::key::Key;

/// The fewest lanes a Project ever has (CLAUDE.md: two Track lanes are always visible).
pub const MIN_LANES: usize = 2;

/// A Track loaded into a lane, with what detection found.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTrack {
    /// The audio file this Track was loaded from; saved projects refer to it by this path.
    pub source: String,
    pub bpm: Option<f64>,
    pub key: Option<Key>,
    /// Length in source seconds.
    pub length_secs: f64,
    /// Offset in beats on the Mashup grid, so it survives Mashup BPM edits.
    pub offset_beats: f64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProjectError {
    MinLanes,
    NoSuchLane,
    InvalidBpm,
}

/// Everything undo/redo snapshots: the lanes and the Mashup-level settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    lanes: Vec<Option<LoadedTrack>>,
    mashup_bpm_override: Option<f64>,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    pub fn new() -> Self {
        Self {
            lanes: vec![None; MIN_LANES],
            mashup_bpm_override: None,
        }
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    pub fn track(&self, lane: usize) -> Option<&LoadedTrack> {
        self.lanes.get(lane)?.as_ref()
    }

    pub fn add_lane(&mut self) {
        self.lanes.push(None);
    }

    /// Removes a lane and its Track. Never goes below two lanes.
    pub fn remove_lane(&mut self, lane: usize) -> Result<(), ProjectError> {
        if lane >= self.lanes.len() {
            return Err(ProjectError::NoSuchLane);
        }
        if self.lanes.len() <= MIN_LANES {
            return Err(ProjectError::MinLanes);
        }
        self.lanes.remove(lane);
        Ok(())
    }

    /// Puts a Track in a lane, replacing any Track already there.
    pub fn load_track(&mut self, lane: usize, track: LoadedTrack) -> Result<(), ProjectError> {
        *self.lanes.get_mut(lane).ok_or(ProjectError::NoSuchLane)? = Some(track);
        Ok(())
    }

    /// Empties a lane but keeps the lane.
    pub fn clear_track(&mut self, lane: usize) -> Result<(), ProjectError> {
        *self.lanes.get_mut(lane).ok_or(ProjectError::NoSuchLane)? = None;
        Ok(())
    }

    /// The typed Mashup BPM, or by default the first loaded Track's detected tempo.
    pub fn mashup_bpm(&self) -> Option<f64> {
        self.mashup_bpm_override
            .or_else(|| self.lanes.iter().flatten().find_map(|t| t.bpm))
    }

    /// `None` goes back to the default. Anything else must be positive and finite.
    pub fn set_mashup_bpm(&mut self, bpm: Option<f64>) -> Result<(), ProjectError> {
        if bpm.is_some_and(|b| !b.is_finite() || b <= 0.0) {
            return Err(ProjectError::InvalidBpm);
        }
        self.mashup_bpm_override = bpm;
        Ok(())
    }

    /// The key every other Track's badge is compared with: the first loaded Track that has one.
    pub fn key_reference(&self) -> Option<Key> {
        self.lanes.iter().flatten().find_map(|t| t.key)
    }

    /// A Track's Offset in Mashup seconds; needs a loaded Track and a Mashup BPM.
    pub fn offset_secs(&self, lane: usize) -> Option<f64> {
        Some(beats_to_secs(
            self.track(lane)?.offset_beats,
            self.mashup_bpm()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use crate::key::Key;

    fn track(bpm: Option<f64>) -> LoadedTrack {
        LoadedTrack {
            source: "a.wav".into(),
            bpm,
            key: None,
            length_secs: 8.0,
            offset_beats: 0.0,
        }
    }

    fn keyed(number: u8) -> LoadedTrack {
        LoadedTrack {
            key: Key::new(number, true),
            ..track(None)
        }
    }

    #[test]
    fn a_new_project_has_two_empty_lanes() {
        let p = Project::new();
        assert_eq!(p.lane_count(), 2);
        assert!(p.track(0).is_none() && p.track(1).is_none());
    }

    #[test]
    fn lanes_can_be_added() {
        let mut p = Project::new();
        p.add_lane();
        assert_eq!(p.lane_count(), 3);
    }

    #[test]
    fn removing_a_lane_never_goes_below_two() {
        let mut p = Project::new();
        p.load_track(0, track(Some(120.0))).unwrap();
        let before = p.clone();

        assert_eq!(p.remove_lane(0), Err(ProjectError::MinLanes));
        assert_eq!(p, before);

        p.add_lane();
        assert_eq!(p.remove_lane(2), Ok(()));
        assert_eq!(p.lane_count(), 2);
    }

    #[test]
    fn a_lane_that_does_not_exist_is_an_error() {
        let mut p = Project::new();
        assert_eq!(p.remove_lane(9), Err(ProjectError::NoSuchLane));
        assert_eq!(p.load_track(9, track(None)), Err(ProjectError::NoSuchLane));
        assert_eq!(p.clear_track(9), Err(ProjectError::NoSuchLane));
    }

    #[test]
    fn clearing_a_track_empties_its_lane_without_removing_it() {
        let mut p = Project::new();
        p.load_track(0, track(Some(120.0))).unwrap();
        p.clear_track(0).unwrap();
        assert!(p.track(0).is_none());
        assert_eq!(p.lane_count(), 2);
    }

    #[test]
    fn loading_into_an_occupied_lane_replaces_the_track() {
        let mut p = Project::new();
        p.load_track(0, track(Some(120.0))).unwrap();
        p.load_track(0, track(Some(90.0))).unwrap();
        assert_eq!(p.track(0).unwrap().bpm, Some(90.0));
    }

    #[test]
    fn the_mashup_bpm_defaults_to_the_first_loaded_tracks_tempo() {
        let mut p = Project::new();
        assert_eq!(p.mashup_bpm(), None);
        p.load_track(1, track(Some(100.0))).unwrap(); // lane 0 still empty
        assert_eq!(p.mashup_bpm(), Some(100.0));
        p.load_track(0, track(Some(120.0))).unwrap();
        assert_eq!(p.mashup_bpm(), Some(120.0));
    }

    #[test]
    fn a_first_track_without_a_tempo_defers_to_the_next_that_has_one() {
        let mut p = Project::new();
        p.load_track(0, track(None)).unwrap();
        p.load_track(1, track(Some(100.0))).unwrap();
        assert_eq!(p.mashup_bpm(), Some(100.0));
    }

    #[test]
    fn removing_the_first_track_hands_the_mashup_bpm_to_the_next() {
        let mut p = Project::new();
        p.add_lane();
        p.load_track(0, track(Some(120.0))).unwrap();
        p.load_track(1, track(Some(100.0))).unwrap();

        p.remove_lane(0).unwrap();

        assert_eq!(p.mashup_bpm(), Some(100.0));
    }

    #[test]
    fn a_typed_mashup_bpm_beats_the_default_and_survives_removals() {
        let mut p = Project::new();
        p.add_lane();
        p.load_track(0, track(Some(120.0))).unwrap();
        p.set_mashup_bpm(Some(95.0)).unwrap();
        p.remove_lane(0).unwrap();
        assert_eq!(p.mashup_bpm(), Some(95.0));

        p.set_mashup_bpm(None).unwrap();
        assert_eq!(p.mashup_bpm(), None);
    }

    #[test]
    fn a_mashup_bpm_must_be_a_positive_finite_number() {
        let mut p = Project::new();
        for bad in [0.0, -120.0, f64::NAN, f64::INFINITY] {
            assert_eq!(p.set_mashup_bpm(Some(bad)), Err(ProjectError::InvalidBpm));
        }
        assert_eq!(p.mashup_bpm(), None);
    }

    #[test]
    fn the_key_reference_is_the_first_loaded_tracks_key() {
        let mut p = Project::new();
        p.add_lane();
        assert_eq!(p.key_reference(), None);
        p.load_track(1, keyed(8)).unwrap();
        p.load_track(2, keyed(3)).unwrap();
        assert_eq!(p.key_reference(), Key::new(8, true));

        p.clear_track(1).unwrap();
        assert_eq!(p.key_reference(), Key::new(3, true));
    }

    #[test]
    fn an_offset_stored_in_beats_keeps_its_place_when_the_mashup_bpm_changes() {
        let mut p = Project::new();
        p.load_track(
            0,
            LoadedTrack {
                offset_beats: 2.0,
                ..track(Some(120.0))
            },
        )
        .unwrap();
        assert_eq!(p.offset_secs(0), Some(1.0));

        p.set_mashup_bpm(Some(100.0)).unwrap();

        assert_eq!(p.track(0).unwrap().offset_beats, 2.0);
        assert!((p.offset_secs(0).unwrap() - 1.2).abs() < 1e-9);
    }

    #[test]
    fn an_offset_in_seconds_needs_a_mashup_bpm_and_a_track() {
        let mut p = Project::new();
        assert_eq!(p.offset_secs(0), None);
        p.load_track(0, track(None)).unwrap();
        assert_eq!(p.offset_secs(0), None);
    }

    #[test]
    fn undoing_a_lane_removal_through_history_brings_the_track_back() {
        let mut p = Project::new();
        p.add_lane();
        p.load_track(2, track(Some(90.0))).unwrap();
        let mut history = History::new(p);

        let mut next = history.current().clone();
        next.remove_lane(2).unwrap();
        history.edit(next);
        assert_eq!(history.current().lane_count(), 2);

        history.undo();
        assert_eq!(history.current().lane_count(), 3);
        assert_eq!(history.current().track(2).unwrap().bpm, Some(90.0));
    }
}
