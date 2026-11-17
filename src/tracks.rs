//! The list of Tracks: at least two, at most six, each with its own palette colour.

use crate::mixer::FADER_UNITY;

/// The first MIN_TRACKS tracks are always there and cannot be deleted.
pub const MIN_TRACKS: usize = 2;
/// Most tracks a mix can have; one palette colour each.
pub const MAX_TRACKS: usize = 6;

/// Stable identity of a Track; survives other Tracks being deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    /// Index into the theme's track palette.
    pub color: usize,
    /// Fader position 0..1 (see `mixer::fader_to_db`).
    pub fader: f64,
    /// -1 (left) .. 1 (right).
    pub pan: f64,
    pub muted: bool,
    pub soloed: bool,
    /// Name of the chosen audio file; not decoded yet.
    pub file_name: Option<String>,
}

impl Track {
    /// True when Reset would change nothing.
    pub fn is_at_defaults(&self) -> bool {
        self.fader == FADER_UNITY && self.pan == 0.0 && !self.muted && !self.soloed
    }

    /// Volume to 0 dB, pan to centre, mute and solo off. Name, colour and file are kept.
    pub fn reset(&mut self) {
        self.fader = FADER_UNITY;
        self.pan = 0.0;
        self.muted = false;
        self.soloed = false;
    }
}

#[derive(Clone, Debug)]
pub struct Tracks {
    list: Vec<Track>,
    next_id: u32,
    selected: TrackId,
}

impl Default for Tracks {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracks {
    /// Two empty Tracks, the first selected.
    pub fn new() -> Self {
        let mut tracks = Self {
            list: Vec::new(),
            next_id: 1,
            selected: TrackId(1),
        };
        for _ in 0..MIN_TRACKS {
            tracks.push();
        }
        tracks
    }

    fn push(&mut self) -> TrackId {
        let id = TrackId(self.next_id);
        self.next_id += 1;
        let color = (0..MAX_TRACKS)
            .find(|c| self.list.iter().all(|t| t.color != *c))
            .unwrap_or(0);
        self.list.push(Track {
            id,
            name: format!("Track {}", id.0),
            color,
            fader: FADER_UNITY,
            pan: 0.0,
            muted: false,
            soloed: false,
            file_name: None,
        });
        id
    }

    pub fn iter(&self) -> impl Iterator<Item = &Track> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, id: TrackId) -> Option<&Track> {
        self.list.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    pub fn selected(&self) -> TrackId {
        self.selected
    }

    pub fn select(&mut self, id: TrackId) {
        if self.get(id).is_some() {
            self.selected = id;
        }
    }

    pub fn can_add(&self) -> bool {
        self.list.len() < MAX_TRACKS
    }

    /// Add an empty Track at the bottom and select it. None at the limit.
    pub fn add(&mut self) -> Option<TrackId> {
        if !self.can_add() {
            return None;
        }
        let id = self.push();
        self.selected = id;
        Some(id)
    }

    /// Tracks after the first MIN_TRACKS may be deleted.
    pub fn can_delete(&self, id: TrackId) -> bool {
        self.list
            .iter()
            .position(|t| t.id == id)
            .is_some_and(|i| i >= MIN_TRACKS)
    }

    /// Delete a Track; if it was selected, the Track above it becomes selected.
    pub fn delete(&mut self, id: TrackId) -> bool {
        if !self.can_delete(id) {
            return false;
        }
        let i = self.list.iter().position(|t| t.id == id).unwrap();
        self.list.remove(i);
        if self.selected == id {
            self.selected = self.list[i - 1].id;
        }
        true
    }

    /// A Track is silent when muted, or when another Track is soloed and it is not.
    pub fn is_silent(&self, id: TrackId) -> bool {
        let any_solo = self.list.iter().any(|t| t.soloed);
        self.get(id)
            .is_some_and(|t| t.muted || (any_solo && !t.soloed))
    }
}

/// File extensions offered by the picker and accepted on drop.
pub const AUDIO_EXTENSIONS: [&str; 10] = [
    "wav", "aif", "aiff", "mp3", "m4a", "aac", "flac", "ogg", "opus", "webm",
];

pub fn is_audio_file(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_two_tracks_first_selected() {
        let t = Tracks::new();
        assert_eq!(t.len(), 2);
        assert_eq!(t.selected(), TrackId(1));
        let names: Vec<_> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Track 1", "Track 2"]);
    }

    #[test]
    fn at_most_six_tracks() {
        let mut t = Tracks::new();
        for _ in 0..4 {
            assert!(t.add().is_some());
        }
        assert_eq!(t.len(), 6);
        assert!(!t.can_add());
        assert_eq!(t.add(), None);
    }

    #[test]
    fn first_two_tracks_cannot_be_deleted() {
        let mut t = Tracks::new();
        let third = t.add().unwrap();
        assert!(!t.delete(TrackId(1)));
        assert!(!t.delete(TrackId(2)));
        assert!(t.delete(third));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn a_track_that_moves_up_to_third_place_stays_deletable() {
        let mut t = Tracks::new();
        let third = t.add().unwrap();
        let fourth = t.add().unwrap();
        t.delete(third);
        assert!(t.can_delete(fourth));
    }

    #[test]
    fn deleting_the_selected_track_selects_the_one_above() {
        let mut t = Tracks::new();
        let third = t.add().unwrap();
        let fourth = t.add().unwrap();
        t.select(fourth);
        t.delete(fourth);
        assert_eq!(t.selected(), third);
    }

    #[test]
    fn every_track_has_its_own_colour_even_after_deletes() {
        let mut t = Tracks::new();
        let a = t.add().unwrap();
        for _ in 0..3 {
            t.add();
        }
        t.delete(a);
        t.add();
        let mut colors: Vec<_> = t.iter().map(|t| t.color).collect();
        colors.sort();
        assert_eq!(colors, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn reset_keeps_name_and_file() {
        let mut t = Tracks::new();
        let track = t.get_mut(TrackId(1)).unwrap();
        track.fader = 0.3;
        track.pan = -0.5;
        track.muted = true;
        track.name = "Vocals".into();
        track.file_name = Some("a.wav".into());
        assert!(!track.is_at_defaults());
        track.reset();
        assert!(track.is_at_defaults());
        assert_eq!(track.name, "Vocals");
        assert_eq!(track.file_name.as_deref(), Some("a.wav"));
    }

    #[test]
    fn solo_silences_the_other_tracks() {
        let mut t = Tracks::new();
        t.get_mut(TrackId(1)).unwrap().soloed = true;
        assert!(!t.is_silent(TrackId(1)));
        assert!(t.is_silent(TrackId(2)));
    }

    #[test]
    fn audio_files_by_extension() {
        assert!(is_audio_file("song.WAV"));
        assert!(is_audio_file("a.b.flac"));
        assert!(!is_audio_file("notes.txt"));
        assert!(!is_audio_file("wav"));
    }
}
