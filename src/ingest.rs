use crate::detect::detect;
use crate::first_run::{TrackInput, plan_first_run};
use crate::pipeline::Loader;
use crate::project::{LoadedTrack, Project, ProjectError};

/// A file that has been loaded and analysed, ready to be placed in a lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Analyzed {
    pub track: LoadedTrack,
    /// Where beat 1 falls in the source, in seconds (what `first_run` aligns on).
    pub downbeat_secs: f64,
}

/// Loads `source` through `loader` and runs tempo and beat detection on it.
pub fn analyze_source(
    loader: &dyn Loader,
    source: &str,
    device_rate: u32,
) -> Result<Analyzed, String> {
    let audio = loader.load(source, device_rate)?;
    let found = detect(&audio);
    Ok(Analyzed {
        track: LoadedTrack {
            source: source.to_string(),
            bpm: found.bpm,
            key: None,
            length_secs: found.length_secs,
            offset_beats: 0.0,
            ..Default::default()
        },
        downbeat_secs: found.downbeat_secs,
    })
}

/// The zero-decision first run: puts analysed files in their lanes with the planned offsets,
/// so the Mashup is tempo matched and downbeat aligned with no user input.
pub fn place_tracks(
    project: &mut Project,
    mut files: Vec<(usize, Analyzed)>,
) -> Result<(), ProjectError> {
    if files.iter().any(|(lane, _)| *lane >= project.lane_count()) {
        return Err(ProjectError::NoSuchLane); // reject before changing anything
    }
    files.sort_by_key(|(lane, _)| *lane);
    let inputs: Vec<TrackInput> = files
        .iter()
        .map(|(_, a)| TrackInput {
            bpm: a.track.bpm,
            length_secs: a.track.length_secs,
            downbeat_secs: a.downbeat_secs,
        })
        .collect();
    let Some(plan) = plan_first_run(&inputs) else {
        return Ok(()); // nothing was dropped
    };
    for ((lane, mut analyzed), planned) in files.into_iter().zip(&plan.tracks) {
        analyzed.track.offset_beats = planned.offset_beats;
        project.load_track(lane, analyzed.track)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::Stereo;
    use std::collections::HashMap;

    const RATE: u32 = 44_100;

    /// Hands out in-memory audio instead of reading files.
    struct FakeLoader(HashMap<String, Stereo>);

    impl Loader for FakeLoader {
        fn load(&self, source: &str, _device_rate: u32) -> Result<Stereo, String> {
            self.0.get(source).cloned().ok_or_else(|| format!("{source}: not found"))
        }
    }

    /// Stereo audio with a short decaying click every beat, the first at `first_click_secs`.
    fn stereo_clicks(bpm: f64, secs: f64, first_click_secs: f64) -> Stereo {
        let frames = (secs * RATE as f64) as usize;
        let mut samples = vec![0.0f32; frames * 2];
        let click_len = (0.01 * RATE as f64) as usize;
        let mut beat = first_click_secs;
        while beat < secs {
            let start = (beat * RATE as f64) as usize;
            for i in 0..click_len.min(frames.saturating_sub(start)) {
                let v = (1.0 - i as f32 / click_len as f32) * if i % 2 == 0 { 1.0 } else { -1.0 };
                samples[(start + i) * 2] = v;
                samples[(start + i) * 2 + 1] = v;
            }
            beat += 60.0 / bpm;
        }
        Stereo { samples, sample_rate: RATE }
    }

    fn loader(files: &[(&str, Stereo)]) -> FakeLoader {
        FakeLoader(files.iter().map(|(n, s)| (n.to_string(), s.clone())).collect())
    }

    #[test]
    fn a_file_that_cannot_be_loaded_is_an_error_naming_it() {
        let loader = loader(&[]);

        let err = analyze_source(&loader, "missing.wav", RATE).unwrap_err();

        assert!(err.contains("missing.wav"), "{err}");
    }

    #[test]
    fn dropping_two_files_places_a_tempo_matched_downbeat_aligned_mashup_in_the_project() {
        use crate::first_run::stretch_for;

        // Track 1: 100 BPM, first click 0.25 s. Track 2: 120 BPM, first click 0.10 s.
        let loader = loader(&[
            ("a.wav", stereo_clicks(100.0, 16.0, 0.25)),
            ("b.wav", stereo_clicks(120.0, 16.0, 0.10)),
        ]);
        let files = vec![
            (0, analyze_source(&loader, "a.wav", RATE).unwrap()),
            (1, analyze_source(&loader, "b.wav", RATE).unwrap()),
        ];
        let mut project = Project::new();

        place_tracks(&mut project, files).expect("both lanes exist");

        let mashup = project.mashup_bpm().expect("a tempo was detected");
        assert!((mashup - 100.0).abs() <= 1.0, "mashup bpm {mashup}");
        assert_eq!(project.track(0).unwrap().source, "a.wav");
        assert_eq!(project.track(0).unwrap().offset_beats, 0.0);
        let second = project.track(1).expect("lane 1 is loaded");
        assert_eq!(second.source, "b.wav");

        // Track 2's first click (true time 0.10 s) lands on one of Track 1's bar lines.
        let factor = stretch_for(second.bpm.unwrap(), mashup).0;
        let landed = project.offset_secs(1).unwrap() + 0.10 / factor;
        let bar = 4.0 * 60.0 / 100.0;
        let off = (landed - 0.25).rem_euclid(bar);
        assert!(off.min(bar - off) <= 0.02, "downbeats are {:.0} ms apart", off.min(bar - off) * 1000.0);
    }

    #[test]
    fn a_missing_lane_rejects_the_whole_drop_without_placing_anything() {
        let loader = loader(&[("a.wav", stereo_clicks(100.0, 12.0, 0.25))]);
        let a = analyze_source(&loader, "a.wav", RATE).unwrap();
        let mut project = Project::new();
        let before = project.clone();

        let result = place_tracks(&mut project, vec![(0, a.clone()), (7, a)]);

        assert_eq!(result, Err(ProjectError::NoSuchLane));
        assert_eq!(project, before);
    }

    #[test]
    fn dropping_nothing_changes_nothing() {
        let mut project = Project::new();
        let before = project.clone();

        assert_eq!(place_tracks(&mut project, Vec::new()), Ok(()));
        assert_eq!(project, before);
    }

    #[test]
    fn analysing_a_source_fills_in_its_tempo_length_and_downbeat() {
        let loader = loader(&[("drums.wav", stereo_clicks(120.0, 12.0, 0.25))]);

        let analyzed = analyze_source(&loader, "drums.wav", RATE).expect("the file loads");

        assert_eq!(analyzed.track.source, "drums.wav");
        let bpm = analyzed.track.bpm.expect("a steady pulse has a tempo");
        assert!((bpm - 120.0).abs() <= 1.0, "bpm {bpm}");
        assert!((analyzed.track.length_secs - 12.0).abs() < 1e-6);
        assert!((analyzed.downbeat_secs - 0.25).abs() <= 0.015, "downbeat {}", analyzed.downbeat_secs);
        assert_eq!(analyzed.track.offset_beats, 0.0);
    }
}
