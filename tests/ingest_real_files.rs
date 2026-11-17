//! The whole call path on real audio: decode WAV files from disk with `FileLoader`, detect
//! tempo and downbeat, and place the Tracks. Uses the gitignored Freesound loops in
//! `testdata/freesound-loops/` (manifest.csv lists their human-annotated BPM) and skips when
//! they are absent.

use std::path::Path;

use audiobrew::ingest::{analyze_source, place_tracks};
use audiobrew::pipeline::FileLoader;
use audiobrew::project::Project;

const DIR: &str = "testdata/freesound-loops";

#[test]
fn two_real_loops_are_decoded_analysed_and_placed_with_their_tempos() {
    // 360687.wav is annotated 100 BPM and 9360.wav 120 BPM.
    let (a, b) = (format!("{DIR}/360687.wav"), format!("{DIR}/9360.wav"));
    if !Path::new(&a).exists() || !Path::new(&b).exists() {
        eprintln!("skipping: {DIR} audio not present");
        return;
    }

    let files = vec![
        (0, analyze_source(&FileLoader, &a, 44_100).expect("first loop loads")),
        (1, analyze_source(&FileLoader, &b, 44_100).expect("second loop loads")),
    ];
    let mut project = Project::new();
    place_tracks(&mut project, files).expect("both lanes exist");

    let within_4_percent = |got: f64, want: f64| (got - want).abs() <= 0.04 * want;
    let first = project.track(0).unwrap();
    let second = project.track(1).unwrap();
    assert!(within_4_percent(first.bpm.unwrap(), 100.0), "first {:?}", first.bpm);
    eprintln!("placed: first {:.1} BPM, second {:.1} BPM, mashup {:.1} BPM, offset {:.2} beats", first.bpm.unwrap(), second.bpm.unwrap(), project.mashup_bpm().unwrap(), second.offset_beats);
    assert!(within_4_percent(second.bpm.unwrap(), 120.0), "second {:?}", second.bpm);
    // The Mashup runs at the first loop's tempo, and the second is stretched to it.
    assert!(within_4_percent(project.mashup_bpm().unwrap(), 100.0));
    assert!(first.length_secs > 1.0 && second.length_secs > 1.0);
}
