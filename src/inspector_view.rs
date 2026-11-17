use crate::badge::{KeyBadge, key_badge};
use crate::bands::{MAX_HZ, MIN_HZ};
use crate::first_run::stretch_for;
use crate::project::{MAX_GAIN, Project};
use crate::readout::{format_db, format_hz, format_offset_ms, format_semitones, format_stretch};
use crate::theme::{Pattern, band_label, band_pattern};

/// One Band as the Inspector shows it.
#[derive(Debug, PartialEq)]
pub struct BandView {
    pub name: String,
    /// "20 Hz – 300 Hz".
    pub range: String,
    /// Where the Band starts and ends on the 20 Hz to 20 kHz log axis, 0.0 to 1.0.
    pub from: f64,
    pub to: f64,
    pub gain_db: String,
    /// The gain's slider position, 0.0 to 1.0 (unity is the middle).
    pub gain_frac: f64,
    pub muted: bool,
    pub soloed: bool,
    /// How the Band's zone is filled, so Bands differ by more than colour.
    pub pattern: Option<Pattern>,
}

/// Everything the Inspector shows for the selected Track.
#[derive(Debug, PartialEq)]
pub struct InspectorView {
    pub lane_number: usize,
    pub file_name: String,
    /// The stretch to the Mashup BPM, such as "0.79×".
    pub speed: String,
    /// The Offset, such as "+500.0 ms".
    pub nudge: String,
    pub level: String,
    pub level_frac: f64,
    pub pitch: String,
    pub key: Option<KeyBadge>,
    pub bands: Vec<BandView>,
}

/// The Inspector for `lane`, or `None` if the lane is empty or does not exist.
pub fn inspector_view(project: &Project, lane: usize) -> Option<InspectorView> {
    let track = project.track(lane)?;
    let factor = match (track.bpm, project.mashup_bpm()) {
        (Some(bpm), Some(mashup)) => stretch_for(bpm, mashup).0,
        _ => 1.0,
    };
    let ranges = track.bands.config.ranges();
    let count = ranges.len();
    let axis = |hz: f64| ((hz / MIN_HZ).log10() / (MAX_HZ / MIN_HZ).log10()).clamp(0.0, 1.0);
    let slider = |gain: f32| (gain / MAX_GAIN).clamp(0.0, 1.0) as f64;

    let bands = ranges
        .iter()
        .zip(&track.bands.states)
        .enumerate()
        .map(|(i, (&(low, high), state))| BandView {
            name: band_label(i, count),
            range: format!("{} – {}", format_hz(low), format_hz(high)),
            from: axis(low),
            to: axis(high),
            gain_db: format_db(state.gain),
            gain_frac: slider(state.gain),
            muted: state.mute,
            soloed: state.solo,
            pattern: band_pattern(i),
        })
        .collect();

    Some(InspectorView {
        lane_number: lane + 1,
        file_name: std::path::Path::new(&track.source)
            .file_name()
            .map_or_else(|| track.source.clone(), |n| n.to_string_lossy().into_owned()),
        speed: format_stretch(factor),
        nudge: format_offset_ms(project.offset_secs(lane).unwrap_or(0.0)),
        level: format_db(track.fader),
        level_frac: slider(track.fader),
        pitch: format_semitones(track.pitch_semitones),
        key: key_badge(project, lane),
        bands,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Key;
    use crate::project::LoadedTrack;

    fn project_with(tracks: &[(Option<f64>, Option<Key>)]) -> Project {
        let mut p = Project::new();
        for (lane, (bpm, key)) in tracks.iter().enumerate() {
            p.load_track(
                lane,
                LoadedTrack { key: *key, ..LoadedTrack::new(format!("/m/track{lane}.wav"), *bpm, 10.0) },
            )
            .unwrap();
        }
        p
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.002
    }

    #[test]
    fn an_empty_or_missing_lane_has_no_inspector() {
        let p = project_with(&[(Some(120.0), None)]);

        assert_eq!(inspector_view(&p, 1), None);
        assert_eq!(inspector_view(&p, 9), None);
    }

    #[test]
    fn a_fresh_track_shows_unity_readouts_and_three_unmuted_bands() {
        let p = project_with(&[(Some(120.0), Key::new(8, true))]);

        let v = inspector_view(&p, 0).unwrap();

        assert_eq!((v.lane_number, v.file_name.as_str()), (1, "track0.wav"));
        assert_eq!(v.speed, "1.00×");
        assert_eq!(v.nudge, "0.0 ms");
        assert_eq!(v.level, "0.0 dB");
        assert_eq!(v.level_frac, 0.5);
        assert_eq!(v.pitch, "0 st");
        assert_eq!(v.key.as_ref().map(|k| k.label.as_str()), Some("8A"));
        let names: Vec<_> = v.bands.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["Low", "Mid", "High"]);
        assert!(v.bands.iter().all(|b| !b.muted && !b.soloed && b.gain_db == "0.0 dB" && b.gain_frac == 0.5));
    }

    #[test]
    fn bands_sit_side_by_side_on_the_log_frequency_axis() {
        let p = project_with(&[(Some(120.0), None)]);

        let bands = inspector_view(&p, 0).unwrap().bands;

        // log10(300/20)/3 = 0.392 and log10(4000/20)/3 = 0.767.
        assert_eq!(bands[0].from, 0.0);
        assert!(close(bands[0].to, 0.392) && close(bands[1].from, 0.392));
        assert!(close(bands[1].to, 0.767) && close(bands[2].from, 0.767));
        assert_eq!(bands[2].to, 1.0);
        assert_eq!(bands[0].range, "20 Hz – 300 Hz");
        assert_eq!(bands[2].range, "4.00 kHz – 20.0 kHz");
        let patterns: Vec<_> = bands.iter().map(|b| b.pattern).collect();
        assert_eq!(patterns, vec![Some(Pattern::Solid), Some(Pattern::Stripes), Some(Pattern::Dots)]);
    }

    #[test]
    fn the_speed_is_this_tracks_stretch_to_the_mashup_tempo() {
        // The Mashup runs at the first Track's 120 BPM, so a 96 BPM Track plays 1.25x faster.
        let p = project_with(&[(Some(120.0), None), (Some(96.0), None)]);

        assert_eq!(inspector_view(&p, 1).unwrap().speed, "1.25×");
    }

    #[test]
    fn edits_show_up_in_the_readouts() {
        let mut p = project_with(&[(Some(120.0), None)]);
        p.set_fader(0, 0.5).unwrap();
        p.set_pitch(0, 2).unwrap();
        p.set_band_gain(0, 2, 0.25).unwrap();
        p.set_band_mute(0, 1, true).unwrap();
        p.set_band_solo(0, 0, true).unwrap();

        let v = inspector_view(&p, 0).unwrap();

        assert_eq!(v.level, "-6.0 dB");
        assert_eq!(v.level_frac, 0.25);
        assert_eq!(v.pitch, "+2 st");
        assert_eq!(v.bands[2].gain_db, "-12.0 dB");
        assert!(v.bands[1].muted && !v.bands[0].muted);
        assert!(v.bands[0].soloed);
    }
}
