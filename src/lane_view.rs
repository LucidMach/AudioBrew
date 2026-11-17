use crate::badge::{KeyBadge, key_badge};
use crate::project::Project;
use crate::readout::format_bpm;

/// The text an empty lane shows (CLAUDE.md, Tracks).
pub const EMPTY_PROMPT: &str = "Drop a file or click to load";

/// Everything a Track lane's header shows, ready to draw. Pure data: the GPUI views only lay
/// these values out, so what is shown can be tested without a window.
#[derive(Debug, PartialEq, Eq)]
pub struct LaneView {
    /// 1-based position, shown as the lane's number.
    pub number: usize,
    /// The loaded file's name without its folder; `None` for an empty lane.
    pub file_name: Option<String>,
    /// "120.0 BPM", or an em dash when the tempo could not be detected; `None` when empty.
    pub tempo: Option<String>,
    pub key: Option<KeyBadge>,
}

impl LaneView {
    pub fn is_empty(&self) -> bool {
        self.file_name.is_none()
    }
}

/// One `LaneView` per lane in the Project, in lane order.
pub fn lane_views(project: &Project) -> Vec<LaneView> {
    (0..project.lane_count())
        .map(|lane| {
            let track = project.track(lane);
            LaneView {
                number: lane + 1,
                file_name: track.map(|t| file_name(&t.source)),
                tempo: track.map(|t| {
                    // An undetected tempo shows a dash rather than a made-up number.
                    t.bpm.map_or_else(|| format_bpm(f64::NAN), format_bpm)
                }),
                key: key_badge(project, lane),
            }
        })
        .collect()
}

/// The last component of a path, or the whole string if it has none.
fn file_name(source: &str) -> String {
    std::path::Path::new(source)
        .file_name()
        .map_or_else(|| source.to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Key;
    use crate::project::LoadedTrack;

    fn track(source: &str, bpm: Option<f64>, key: Option<Key>) -> LoadedTrack {
        LoadedTrack {
            source: source.into(),
            bpm,
            key,
            length_secs: 10.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_new_project_shows_two_numbered_empty_lanes() {
        let views = lane_views(&Project::new());

        assert_eq!(views.len(), 2);
        assert_eq!((views[0].number, views[1].number), (1, 2));
        assert!(views.iter().all(|v| v.is_empty()));
        assert!(views.iter().all(|v| v.tempo.is_none() && v.key.is_none()));
    }

    #[test]
    fn a_loaded_track_shows_its_file_name_tempo_and_key() {
        let mut project = Project::new();
        project
            .load_track(0, track("/music/set/drums.wav", Some(120.0), Key::new(8, true)))
            .unwrap();

        let views = lane_views(&project);

        assert_eq!(views[0].file_name.as_deref(), Some("drums.wav"));
        assert_eq!(views[0].tempo.as_deref(), Some("120.0 BPM"));
        assert_eq!(views[0].key.as_ref().map(|k| k.label.as_str()), Some("8A"));
        assert!(!views[0].is_empty());
        assert!(views[1].is_empty(), "the other lane stays empty");
    }

    #[test]
    fn a_track_whose_tempo_was_not_found_shows_a_dash_not_a_made_up_number() {
        let mut project = Project::new();
        project.load_track(1, track("pad.wav", None, None)).unwrap();

        let views = lane_views(&project);

        assert_eq!(views[1].tempo.as_deref(), Some("—"));
        assert_eq!(views[1].key, None);
        assert_eq!(views[1].file_name.as_deref(), Some("pad.wav"));
    }

    #[test]
    fn extra_lanes_keep_their_order_and_numbers() {
        let mut project = Project::new();
        project.add_lane();
        project.load_track(2, track("c.wav", Some(90.0), None)).unwrap();

        let views = lane_views(&project);

        assert_eq!(views.len(), 3);
        assert_eq!(views[2].number, 3);
        assert_eq!(views[2].tempo.as_deref(), Some("90.0 BPM"));
    }
}
