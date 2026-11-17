use crate::bands::{BandConfig, BandError};
use crate::grid::beats_to_secs;
use crate::key::Key;
use crate::mix::BandState;

/// The fewest lanes a Project ever has (CLAUDE.md: two Track lanes are always visible).
pub const MIN_LANES: usize = 2;

/// The largest fader or Band gain: +6 dB.
pub const MAX_GAIN: f32 = 2.0;
/// The most a Track can be pitch-shifted to match keys.
pub const MAX_PITCH_SEMITONES: i8 = 6;

/// A Track's Bands: where they split, and each Band's gain, mute and solo. The two always
/// have the same number of Bands; `Project` keeps them in step.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackBands {
    pub config: BandConfig,
    pub states: Vec<BandState>,
}

impl Default for TrackBands {
    fn default() -> Self {
        let config = BandConfig::default();
        let states = vec![
            BandState {
                gain: 1.0,
                mute: false,
                solo: false
            };
            config.ranges().len()
        ];
        Self { config, states }
    }
}

/// A Track loaded into a lane, with what detection found and how the user has shaped it.
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
    /// The Track's overall volume, 0.0 to `MAX_GAIN`.
    pub fader: f32,
    pub bands: TrackBands,
    /// Match-key pitch shift, within `MAX_PITCH_SEMITONES` either way.
    pub pitch_semitones: i8,
    /// A span of the source, in source seconds, that repeats to fill the Mashup.
    pub loop_region: Option<(f64, f64)>,
}

impl Default for LoadedTrack {
    fn default() -> Self {
        Self {
            source: String::new(),
            bpm: None,
            key: None,
            length_secs: 0.0,
            offset_beats: 0.0,
            fader: 1.0,
            bands: TrackBands::default(),
            pitch_semitones: 0,
            loop_region: None,
        }
    }
}

impl LoadedTrack {
    /// A Track with default mix settings: fader at unity, Low/Mid/High Bands, no pitch shift,
    /// no Loop region, no Offset.
    pub fn new(source: impl Into<String>, bpm: Option<f64>, length_secs: f64) -> Self {
        Self {
            source: source.into(),
            bpm,
            length_secs,
            ..Self::default()
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProjectError {
    MinLanes,
    NoSuchLane,
    /// The lane exists but has no Track loaded.
    NoTrack,
    InvalidBpm,
    /// A setting outside its allowed range, or not a finite number.
    InvalidValue,
    Band(BandError),
}

/// A lane's identity. Unlike a lane's index it never shifts when another lane is removed,
/// and it is never reused, so background work can be matched to the right lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LaneId(pub u64);

/// Everything undo/redo snapshots: the lanes and the Mashup-level settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    lanes: Vec<Option<LoadedTrack>>,
    /// Parallel to `lanes`.
    ids: Vec<LaneId>,
    next_id: u64,
    mashup_bpm_override: Option<f64>,
    /// The user-dragged End marker, in beats; `None` means "where the longest Track ends".
    end_marker_beats: Option<f64>,
    loop_mashup: bool,
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
            ids: (0..MIN_LANES as u64).map(LaneId).collect(),
            next_id: MIN_LANES as u64,
            mashup_bpm_override: None,
            end_marker_beats: None,
            loop_mashup: false,
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
        self.ids.push(LaneId(self.next_id));
        self.next_id += 1;
    }

    pub fn lane_id(&self, index: usize) -> Option<LaneId> {
        self.ids.get(index).copied()
    }

    pub fn lane_index(&self, id: LaneId) -> Option<usize> {
        self.ids.iter().position(|&i| i == id)
    }

    /// The loaded Tracks with their lane ids, in lane order.
    pub fn tracks(&self) -> impl Iterator<Item = (LaneId, &LoadedTrack)> {
        self.ids
            .iter()
            .zip(&self.lanes)
            .filter_map(|(&id, lane)| Some((id, lane.as_ref()?)))
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
        self.ids.remove(lane);
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

    fn track_mut(&mut self, lane: usize) -> Result<&mut LoadedTrack, ProjectError> {
        self.lanes
            .get_mut(lane)
            .ok_or(ProjectError::NoSuchLane)?
            .as_mut()
            .ok_or(ProjectError::NoTrack)
    }

    pub fn set_fader(&mut self, lane: usize, gain: f32) -> Result<(), ProjectError> {
        let track = self.track_mut(lane)?;
        track.fader = checked_gain(gain)?;
        Ok(())
    }

    pub fn set_band_gain(
        &mut self,
        lane: usize,
        band: usize,
        gain: f32,
    ) -> Result<(), ProjectError> {
        let gain = checked_gain(gain)?;
        band_state(self.track_mut(lane)?, band)?.gain = gain;
        Ok(())
    }

    pub fn set_band_mute(
        &mut self,
        lane: usize,
        band: usize,
        mute: bool,
    ) -> Result<(), ProjectError> {
        band_state(self.track_mut(lane)?, band)?.mute = mute;
        Ok(())
    }

    pub fn set_band_solo(
        &mut self,
        lane: usize,
        band: usize,
        solo: bool,
    ) -> Result<(), ProjectError> {
        band_state(self.track_mut(lane)?, band)?.solo = solo;
        Ok(())
    }

    /// Splits the Band containing `hz`; the new upper Band starts with the split Band's settings.
    pub fn add_band(&mut self, lane: usize, hz: f64) -> Result<(), ProjectError> {
        let bands = &mut self.track_mut(lane)?.bands;
        let split = bands.config.band_containing(hz);
        bands.config.add_band(hz).map_err(ProjectError::Band)?;
        let inherited = bands.states[split];
        bands.states.insert(split + 1, inherited);
        Ok(())
    }

    /// Moves the boundary between Band `index` and Band `index + 1`; settings stay put.
    pub fn move_crossover(
        &mut self,
        lane: usize,
        index: usize,
        hz: f64,
    ) -> Result<(), ProjectError> {
        let bands = &mut self.track_mut(lane)?.bands;
        bands
            .config
            .move_crossover(index, hz)
            .map_err(ProjectError::Band)
    }

    /// Removes a Band and its settings; the neighbouring Band absorbs its frequency range.
    pub fn remove_band(&mut self, lane: usize, band: usize) -> Result<(), ProjectError> {
        let bands = &mut self.track_mut(lane)?.bands;
        bands.config.remove_band(band).map_err(ProjectError::Band)?;
        bands.states.remove(band);
        Ok(())
    }

    pub fn set_pitch(&mut self, lane: usize, semitones: i8) -> Result<(), ProjectError> {
        let track = self.track_mut(lane)?;
        if semitones.abs() > MAX_PITCH_SEMITONES {
            return Err(ProjectError::InvalidValue);
        }
        track.pitch_semitones = semitones;
        Ok(())
    }

    /// `None` clears the Loop region. A region must lie inside the Track and have some length.
    pub fn set_loop_region(
        &mut self,
        lane: usize,
        region: Option<(f64, f64)>,
    ) -> Result<(), ProjectError> {
        let track = self.track_mut(lane)?;
        if let Some((start, end)) = region {
            let valid = start.is_finite()
                && end.is_finite()
                && start >= 0.0
                && start < end
                && end <= track.length_secs;
            if !valid {
                return Err(ProjectError::InvalidValue);
            }
        }
        track.loop_region = region;
        Ok(())
    }

    pub fn end_marker_beats(&self) -> Option<f64> {
        self.end_marker_beats
    }

    /// `None` goes back to "where the longest Track ends".
    pub fn set_end_marker_beats(&mut self, beats: Option<f64>) -> Result<(), ProjectError> {
        if beats.is_some_and(|b| !b.is_finite() || b < 0.0) {
            return Err(ProjectError::InvalidValue);
        }
        self.end_marker_beats = beats;
        Ok(())
    }

    /// The user-set End marker in Mashup seconds; needs a Mashup BPM.
    pub fn end_marker_secs(&self) -> Option<f64> {
        Some(beats_to_secs(self.end_marker_beats?, self.mashup_bpm()?))
    }

    pub fn loops_mashup(&self) -> bool {
        self.loop_mashup
    }

    pub fn set_loop_mashup(&mut self, looping: bool) {
        self.loop_mashup = looping;
    }

    /// A Track's Offset in Mashup seconds; needs a loaded Track and a Mashup BPM.
    pub fn offset_secs(&self, lane: usize) -> Option<f64> {
        Some(beats_to_secs(
            self.track(lane)?.offset_beats,
            self.mashup_bpm()?,
        ))
    }
}

fn checked_gain(gain: f32) -> Result<f32, ProjectError> {
    if gain.is_finite() && (0.0..=MAX_GAIN).contains(&gain) {
        Ok(gain)
    } else {
        Err(ProjectError::InvalidValue)
    }
}

fn band_state(track: &mut LoadedTrack, band: usize) -> Result<&mut BandState, ProjectError> {
    track
        .bands
        .states
        .get_mut(band)
        .ok_or(ProjectError::Band(BandError::NoSuchBand))
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
            ..Default::default()
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

    #[test]
    fn every_lane_has_its_own_id() {
        let mut p = Project::new();
        p.add_lane();
        let ids: Vec<_> = (0..3).map(|i| p.lane_id(i).unwrap()).collect();
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
        assert_eq!(p.lane_id(3), None);
    }

    #[test]
    fn a_lanes_id_survives_other_lanes_being_removed() {
        let mut p = Project::new();
        p.add_lane();
        let (first, third) = (p.lane_id(0).unwrap(), p.lane_id(2).unwrap());

        p.remove_lane(0).unwrap();

        // The old third lane is now at index 1 but is still the same lane.
        assert_eq!(p.lane_id(1), Some(third));
        assert_ne!(p.lane_id(0), Some(first));
        assert_eq!(p.lane_index(third), Some(1));
        assert_eq!(p.lane_index(first), None);
    }

    #[test]
    fn a_removed_lanes_id_is_never_handed_to_a_new_lane() {
        let mut p = Project::new();
        p.add_lane();
        let removed = p.lane_id(2).unwrap();
        p.remove_lane(2).unwrap();
        p.add_lane();
        assert_ne!(p.lane_id(2), Some(removed));
    }

    #[test]
    fn clearing_or_reloading_a_track_keeps_its_lane_id() {
        let mut p = Project::new();
        let id = p.lane_id(0).unwrap();
        p.load_track(0, track(Some(120.0))).unwrap();
        p.load_track(0, track(Some(90.0))).unwrap();
        p.clear_track(0).unwrap();
        assert_eq!(p.lane_id(0), Some(id));
    }

    #[test]
    fn tracks_lists_only_loaded_lanes_with_their_ids() {
        let mut p = Project::new();
        p.add_lane();
        p.load_track(2, track(Some(100.0))).unwrap();
        let listed: Vec<_> = p.tracks().map(|(id, t)| (id, t.bpm)).collect();
        assert_eq!(listed, vec![(p.lane_id(2).unwrap(), Some(100.0))]);
    }

    #[test]
    fn undo_through_history_restores_the_same_lane_ids() {
        let mut history = crate::history::History::new(Project::new());
        let before = history.current().lane_id(1);
        let mut next = history.current().clone();
        next.add_lane();
        next.remove_lane(0).unwrap();
        history.edit(next);
        history.undo();
        assert_eq!(history.current().lane_id(1), before);
    }

    fn with_track() -> Project {
        let mut p = Project::new();
        p.load_track(0, track(Some(120.0))).unwrap(); // length 8 s
        p
    }

    #[test]
    fn a_new_track_starts_with_default_mix_settings() {
        let t = LoadedTrack::new("a.wav", Some(120.0), 8.0);
        assert_eq!((t.fader, t.pitch_semitones, t.loop_region), (1.0, 0, None));
        assert_eq!(t.bands.config.ranges().len(), 3);
        assert_eq!(t.bands.states.len(), 3);
        assert!(
            t.bands
                .states
                .iter()
                .all(|b| b.gain == 1.0 && !b.mute && !b.solo)
        );
        let p = Project::new();
        assert_eq!((p.end_marker_beats(), p.loops_mashup()), (None, false));
    }

    #[test]
    fn the_fader_takes_zero_to_two_and_nothing_else() {
        let mut p = with_track();
        for ok in [0.0, 0.5, 2.0] {
            assert_eq!(p.set_fader(0, ok), Ok(()));
            assert_eq!(p.track(0).unwrap().fader, ok);
        }
        for bad in [-0.1, 2.1, f32::NAN, f32::INFINITY] {
            assert_eq!(p.set_fader(0, bad), Err(ProjectError::InvalidValue));
        }
        assert_eq!(p.track(0).unwrap().fader, 2.0); // unchanged by the failures
    }

    #[test]
    fn per_track_settings_need_a_loaded_track_in_an_existing_lane() {
        let mut p = Project::new(); // both lanes empty
        assert_eq!(p.set_fader(0, 0.5), Err(ProjectError::NoTrack));
        assert_eq!(p.set_fader(9, 0.5), Err(ProjectError::NoSuchLane));
        assert_eq!(p.set_pitch(0, 1), Err(ProjectError::NoTrack));
        assert_eq!(p.add_band(0, 1_000.0), Err(ProjectError::NoTrack));
        assert_eq!(p.set_loop_region(0, None), Err(ProjectError::NoTrack));
    }

    #[test]
    fn band_gain_mute_and_solo_are_set_per_band() {
        let mut p = with_track();
        p.set_band_gain(0, 1, 0.5).unwrap();
        p.set_band_mute(0, 0, true).unwrap();
        p.set_band_solo(0, 2, true).unwrap();
        let s = &p.track(0).unwrap().bands.states;
        assert_eq!((s[1].gain, s[0].mute, s[2].solo), (0.5, true, true));
        assert_eq!((s[0].gain, s[1].mute, s[2].mute), (1.0, false, false));
    }

    #[test]
    fn band_settings_reject_a_missing_band_and_bad_gains() {
        let mut p = with_track();
        let nobody = Err(ProjectError::Band(crate::bands::BandError::NoSuchBand));
        assert_eq!(p.set_band_gain(0, 3, 0.5), nobody);
        assert_eq!(p.set_band_mute(0, 3, true), nobody);
        assert_eq!(p.set_band_solo(0, 3, true), nobody);
        for bad in [-1.0, 2.5, f32::NAN] {
            assert_eq!(p.set_band_gain(0, 0, bad), Err(ProjectError::InvalidValue));
        }
    }

    #[test]
    fn splitting_a_band_gives_the_new_band_the_split_bands_settings() {
        let mut p = with_track();
        p.set_band_mute(0, 1, true).unwrap();
        p.set_band_gain(0, 1, 0.5).unwrap();

        p.add_band(0, 1_000.0).unwrap(); // inside the 300-4000 Band

        let t = p.track(0).unwrap();
        assert_eq!(t.bands.config.ranges().len(), 4);
        assert_eq!(t.bands.states.len(), 4);
        assert!(t.bands.states[1].mute && t.bands.states[2].mute);
        assert_eq!((t.bands.states[1].gain, t.bands.states[2].gain), (0.5, 0.5));
        assert!(!t.bands.states[0].mute && !t.bands.states[3].mute);
    }

    #[test]
    fn a_rejected_band_change_leaves_settings_and_ranges_in_step() {
        let mut p = with_track();
        p.add_band(0, 500.0).unwrap();
        p.add_band(0, 600.0).unwrap();
        p.add_band(0, 700.0).unwrap(); // now 6 Bands
        let before = p.clone();
        assert!(matches!(p.add_band(0, 800.0), Err(ProjectError::Band(_))));
        assert_eq!(p, before);
        // At the cap the cap is what is reported, even for a nonsense frequency.
        assert_eq!(
            p.add_band(0, f64::NAN),
            Err(ProjectError::Band(crate::bands::BandError::TooManyBands))
        );
        assert_eq!(p, before);

        // Below the cap a nonsense frequency is reported as such, and nothing changes.
        let mut q = with_track();
        let untouched = q.clone();
        assert_eq!(
            q.add_band(0, f64::NAN),
            Err(ProjectError::Band(
                crate::bands::BandError::InvalidCrossover
            ))
        );
        assert_eq!(q, untouched);
    }

    #[test]
    fn moving_a_crossover_keeps_every_bands_settings() {
        let mut p = with_track();
        p.set_band_solo(0, 1, true).unwrap();
        p.move_crossover(0, 0, 250.0).unwrap();
        let t = p.track(0).unwrap();
        assert_eq!(t.bands.config.ranges()[0], (20.0, 250.0));
        assert!(t.bands.states[1].solo);
        assert!(p.move_crossover(0, 5, 250.0).is_err());
    }

    #[test]
    fn removing_a_band_removes_its_settings_and_leaves_its_neighbours_alone() {
        let mut p = with_track();
        p.set_band_gain(0, 0, 0.25).unwrap();
        p.set_band_gain(0, 1, 0.5).unwrap();
        p.set_band_gain(0, 2, 0.75).unwrap();

        p.remove_band(0, 1).unwrap();

        let t = p.track(0).unwrap();
        let gains: Vec<f32> = t.bands.states.iter().map(|b| b.gain).collect();
        assert_eq!(gains, vec![0.25, 0.75]);
        assert_eq!(
            t.bands.config.ranges(),
            vec![(20.0, 4_000.0), (4_000.0, 20_000.0)]
        );
    }

    #[test]
    fn the_last_band_cannot_be_removed() {
        let mut p = with_track();
        p.remove_band(0, 0).unwrap();
        p.remove_band(0, 0).unwrap();
        let before = p.clone();
        assert_eq!(
            p.remove_band(0, 0),
            Err(ProjectError::Band(crate::bands::BandError::LastBand))
        );
        assert_eq!(p, before);
    }

    #[test]
    fn pitch_shift_is_limited_to_six_semitones_either_way() {
        let mut p = with_track();
        for ok in [-6, 0, 6] {
            assert_eq!(p.set_pitch(0, ok), Ok(()));
        }
        assert_eq!(p.set_pitch(0, 7), Err(ProjectError::InvalidValue));
        assert_eq!(p.set_pitch(0, -7), Err(ProjectError::InvalidValue));
        assert_eq!(p.track(0).unwrap().pitch_semitones, 6);
    }

    #[test]
    fn a_loop_region_must_lie_inside_the_track() {
        let mut p = with_track(); // 8 s long
        assert_eq!(p.set_loop_region(0, Some((2.0, 4.0))), Ok(()));
        assert_eq!(p.track(0).unwrap().loop_region, Some((2.0, 4.0)));
        assert_eq!(p.set_loop_region(0, Some((0.0, 8.0))), Ok(()));
        for bad in [
            (4.0, 2.0),
            (2.0, 2.0),
            (-1.0, 2.0),
            (2.0, 8.5),
            (f64::NAN, 2.0),
            (0.0, f64::INFINITY),
        ] {
            assert_eq!(
                p.set_loop_region(0, Some(bad)),
                Err(ProjectError::InvalidValue),
                "{bad:?}"
            );
        }
        assert_eq!(p.track(0).unwrap().loop_region, Some((0.0, 8.0)));
        assert_eq!(p.set_loop_region(0, None), Ok(()));
        assert_eq!(p.track(0).unwrap().loop_region, None);
    }

    #[test]
    fn the_end_marker_is_stored_in_beats_and_converted_at_the_mashup_tempo() {
        let mut p = with_track(); // Mashup BPM 120
        assert_eq!(p.end_marker_secs(), None);
        p.set_end_marker_beats(Some(8.0)).unwrap();
        assert_eq!(p.end_marker_secs(), Some(4.0));
        p.set_mashup_bpm(Some(60.0)).unwrap();
        assert_eq!(p.end_marker_secs(), Some(8.0)); // same beats, longer in seconds
        p.set_end_marker_beats(None).unwrap();
        assert_eq!(p.end_marker_secs(), None);
    }

    #[test]
    fn a_bad_end_marker_is_rejected_and_needs_a_tempo_to_become_seconds() {
        let mut p = with_track();
        for bad in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                p.set_end_marker_beats(Some(bad)),
                Err(ProjectError::InvalidValue)
            );
        }
        assert_eq!(p.end_marker_beats(), None);
        let mut empty = Project::new();
        empty.set_end_marker_beats(Some(8.0)).unwrap();
        assert_eq!(empty.end_marker_secs(), None); // no Mashup tempo yet
    }

    #[test]
    fn the_whole_mashup_loop_toggle_is_stored() {
        let mut p = Project::new();
        p.set_loop_mashup(true);
        assert!(p.loops_mashup());
        p.set_loop_mashup(false);
        assert!(!p.loops_mashup());
    }

    #[test]
    fn every_new_setting_is_undoable_through_history() {
        let mut h = crate::history::History::new(with_track());
        type Step = Box<dyn Fn(&mut Project)>;
        let steps: Vec<Step> = vec![
            Box::new(|p| p.set_fader(0, 0.5).unwrap()),
            Box::new(|p| p.set_band_mute(0, 1, true).unwrap()),
            Box::new(|p| p.add_band(0, 1_000.0).unwrap()),
            Box::new(|p| p.set_pitch(0, 3).unwrap()),
            Box::new(|p| p.set_loop_region(0, Some((1.0, 3.0))).unwrap()),
            Box::new(|p| p.set_end_marker_beats(Some(16.0)).unwrap()),
            Box::new(|p| p.set_loop_mashup(true)),
        ];
        let original = h.current().clone();
        for step in &steps {
            let mut next = h.current().clone();
            step(&mut next);
            assert_ne!(&next, h.current());
            h.edit(next);
        }
        for _ in &steps {
            h.undo();
        }
        assert_eq!(h.current(), &original);
    }
}
