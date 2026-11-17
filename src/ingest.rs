use crate::detect::{detect, to_mono};
use crate::keydetect::detect_key;
use crate::first_run::{TrackInput, plan_first_run};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::pipeline::{Executor, Loader};
use crate::project::{LaneId, LoadedTrack, Project, ProjectError};

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
            key: detect_key(&to_mono(&audio), audio.sample_rate),
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


/// What a `poll` changed.
#[derive(Debug, Default, PartialEq)]
pub struct IngestReport {
    /// Lanes that now hold a Track (call `Pipeline::sync` when this is not empty).
    pub placed: Vec<LaneId>,
    /// Lanes whose file could not be loaded, with the reason.
    pub failed: Vec<(LaneId, String)>,
}

/// One analysed (or failed) file coming back from a background job.
struct Finished {
    batch: u64,
    lane: LaneId,
    result: Result<Analyzed, String>,
}

/// Files dropped together; the first run plans them as a group once every one has reported.
struct Batch {
    pending: usize,
    done: Vec<(LaneId, Result<Analyzed, String>)>,
}

/// Analyses dropped files off the UI thread and places them in the Project. Call `drop_files`
/// when the user drops files, then `poll` regularly from the UI loop. Lanes are identified by
/// `LaneId`, so a lane removed or re-dropped while analysis is running cannot receive the wrong
/// file: only the newest drop on a lane is placed.
pub struct Ingestor {
    loader: Arc<dyn Loader>,
    executor: Arc<dyn Executor>,
    device_rate: u32,
    results_tx: Sender<Finished>,
    results_rx: Receiver<Finished>,
    next_batch: u64,
    batches: HashMap<u64, Batch>,
    /// The batch holding the newest drop on each lane.
    newest: HashMap<LaneId, u64>,
}

impl Ingestor {
    pub fn new(loader: Arc<dyn Loader>, executor: Arc<dyn Executor>, device_rate: u32) -> Self {
        let (results_tx, results_rx) = mpsc::channel();
        Self {
            loader,
            executor,
            device_rate,
            results_tx,
            results_rx,
            next_batch: 0,
            batches: HashMap::new(),
            newest: HashMap::new(),
        }
    }

    /// Starts analysing `files` (lane and source path) in the background, as one first run.
    pub fn drop_files(&mut self, files: Vec<(LaneId, String)>) {
        if files.is_empty() {
            return;
        }
        let batch = self.next_batch;
        self.next_batch += 1;
        self.batches.insert(batch, Batch { pending: files.len(), done: Vec::new() });
        for (lane, source) in files {
            self.newest.insert(lane, batch);
            let loader = Arc::clone(&self.loader);
            let tx = self.results_tx.clone();
            let device_rate = self.device_rate;
            self.executor.spawn(Box::new(move || {
                let result = analyze_source(&*loader, &source, device_rate);
                let _ = tx.send(Finished { batch, lane, result });
            }));
        }
    }

    /// Collects finished analyses and, once a whole drop has reported, places its Tracks.
    pub fn poll(&mut self, project: &mut Project) -> IngestReport {
        let mut report = IngestReport::default();
        let finished: Vec<Finished> = self.results_rx.try_iter().collect();
        for Finished { batch, lane, result } in finished {
            let Some(entry) = self.batches.get_mut(&batch) else {
                continue;
            };
            entry.done.push((lane, result));
            entry.pending -= 1;
            if entry.pending == 0 {
                let done = self.batches.remove(&batch).map(|b| b.done).unwrap_or_default();
                self.finish_batch(batch, done, project, &mut report);
            }
        }
        report
    }

    /// Places a completed drop, skipping lanes that were removed or dropped on again meanwhile.
    fn finish_batch(
        &mut self,
        batch: u64,
        done: Vec<(LaneId, Result<Analyzed, String>)>,
        project: &mut Project,
        report: &mut IngestReport,
    ) {
        let mut ready: Vec<(usize, LaneId, Analyzed)> = Vec::new();
        for (lane, result) in done {
            if self.newest.get(&lane) != Some(&batch) {
                continue; // a newer drop on this lane replaced this one
            }
            let Some(index) = project.lane_index(lane) else {
                continue; // the lane was removed while the file was being analysed
            };
            match result {
                Ok(analyzed) => {
                    ready.retain(|(_, other, _)| *other != lane); // the last file for a lane wins
                    ready.push((index, lane, analyzed));
                }
                Err(reason) => report.failed.push((lane, reason)),
            }
        }
        self.newest.retain(|_, newest| *newest != batch);
        ready.sort_by_key(|(index, _, _)| *index);
        let lanes: Vec<LaneId> = ready.iter().map(|(_, lane, _)| *lane).collect();
        let files = ready.into_iter().map(|(index, _, analyzed)| (index, analyzed)).collect();
        if place_tracks(project, files).is_ok() {
            report.placed = lanes;
        }
    }
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
            self.0
                .get(source)
                .cloned()
                .ok_or_else(|| format!("{source}: not found"))
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
        Stereo {
            samples,
            sample_rate: RATE,
        }
    }

    fn loader(files: &[(&str, Stereo)]) -> FakeLoader {
        FakeLoader(
            files
                .iter()
                .map(|(n, s)| (n.to_string(), s.clone()))
                .collect(),
        )
    }

    /// Stereo audio of a C major I-IV-V-I progression (sine tones with harmonics), 2.5 s a chord.
    fn c_major_harmony() -> Stereo {
        let chords: [[i32; 3]; 4] = [[48, 52, 55], [53, 57, 60], [55, 59, 62], [48, 52, 55]];
        let mut mono = Vec::new();
        for chord in chords {
            let frames = (2.5 * RATE as f64) as usize;
            let mut block = vec![0.0f32; frames];
            for note in chord {
                let freq = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
                for harmonic in 1..=4 {
                    let amp = 0.15 / harmonic as f64;
                    for (i, sample) in block.iter_mut().enumerate() {
                        let t = i as f64 / RATE as f64;
                        *sample += (amp * (std::f64::consts::TAU * freq * harmonic as f64 * t).sin()) as f32;
                    }
                }
            }
            mono.extend(block);
        }
        Stereo { samples: mono.iter().flat_map(|&v| [v, v]).collect(), sample_rate: RATE }
    }

    #[test]
    fn analysing_a_source_fills_in_its_key_for_the_key_badge() {
        use crate::key::Key;
        let loader = loader(&[("pad.wav", c_major_harmony())]);

        let analyzed = analyze_source(&loader, "pad.wav", RATE).expect("the file loads");

        // C major is 8B on the Camelot wheel.
        assert_eq!(analyzed.track.key, Some(Key { number: 8, minor: false }));
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
        assert!(
            off.min(bar - off) <= 0.02,
            "downbeats are {:.0} ms apart",
            off.min(bar - off) * 1000.0
        );
    }


    use crate::pipeline::Job;
    use std::sync::Mutex;

    /// Holds background jobs until the test decides to run them.
    #[derive(Default)]
    struct ManualExecutor(Mutex<Vec<Job>>);

    impl Executor for ManualExecutor {
        fn spawn(&self, job: Job) {
            self.0.lock().unwrap().push(job);
        }
    }

    impl ManualExecutor {
        fn run_all(&self) {
            let jobs: Vec<Job> = self.0.lock().unwrap().drain(..).collect();
            for job in jobs {
                job();
            }
        }
    }

    fn ingestor(files: &[(&str, Stereo)]) -> (Ingestor, Arc<ManualExecutor>) {
        let executor = Arc::new(ManualExecutor::default());
        let loader: Arc<dyn Loader> = Arc::new(loader(files));
        (Ingestor::new(loader, executor.clone(), RATE), executor)
    }

    #[test]
    fn dropped_files_are_placed_once_their_background_analysis_finishes() {
        let (mut ingest, executor) = ingestor(&[
            ("a.wav", stereo_clicks(100.0, 16.0, 0.25)),
            ("b.wav", stereo_clicks(120.0, 16.0, 0.10)),
        ]);
        let mut project = Project::new();
        let (l0, l1) = (project.lane_id(0).unwrap(), project.lane_id(1).unwrap());

        ingest.drop_files(vec![(l0, "a.wav".into()), (l1, "b.wav".into())]);

        // Nothing is placed while the analysis is still running.
        assert_eq!(ingest.poll(&mut project), IngestReport::default());
        assert!(project.track(0).is_none() && project.track(1).is_none());

        executor.run_all();
        let report = ingest.poll(&mut project);

        assert_eq!(report.placed, vec![l0, l1]);
        assert!(report.failed.is_empty());
        let mashup = project.mashup_bpm().expect("tempo detected");
        assert!((mashup - 100.0).abs() <= 1.0, "mashup {mashup}");
        assert_eq!(project.track(1).unwrap().source, "b.wav");
    }

    #[test]
    fn a_file_that_fails_is_reported_and_the_others_are_still_placed() {
        let (mut ingest, executor) = ingestor(&[("b.wav", stereo_clicks(120.0, 12.0, 0.10))]);
        let mut project = Project::new();
        let (l0, l1) = (project.lane_id(0).unwrap(), project.lane_id(1).unwrap());

        ingest.drop_files(vec![(l0, "missing.wav".into()), (l1, "b.wav".into())]);
        executor.run_all();
        let report = ingest.poll(&mut project);

        assert_eq!(report.placed, vec![l1]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, l0);
        assert!(report.failed[0].1.contains("missing.wav"));
        assert!(project.track(0).is_none(), "the failed lane stays empty");
        assert_eq!(project.track(1).unwrap().source, "b.wav");
    }

    #[test]
    fn a_lane_removed_while_its_file_is_analysed_receives_nothing() {
        let (mut ingest, executor) = ingestor(&[
            ("a.wav", stereo_clicks(100.0, 12.0, 0.25)),
            ("c.wav", stereo_clicks(90.0, 12.0, 0.25)),
        ]);
        let mut project = Project::new();
        project.add_lane();
        let (l0, l2) = (project.lane_id(0).unwrap(), project.lane_id(2).unwrap());

        ingest.drop_files(vec![(l0, "a.wav".into()), (l2, "c.wav".into())]);
        project.remove_lane(2).unwrap(); // the user deletes lane 2 before analysis finishes
        executor.run_all();
        let report = ingest.poll(&mut project);

        assert_eq!(report.placed, vec![l0]);
        assert_eq!(project.lane_count(), 2);
        assert_eq!(project.track(0).unwrap().source, "a.wav");
        assert!(project.track(1).is_none());
    }

    #[test]
    fn only_the_newest_drop_on_a_lane_is_placed() {
        let (mut ingest, executor) = ingestor(&[
            ("old.wav", stereo_clicks(100.0, 12.0, 0.25)),
            ("new.wav", stereo_clicks(130.0, 12.0, 0.25)),
        ]);
        let mut project = Project::new();
        let l0 = project.lane_id(0).unwrap();

        ingest.drop_files(vec![(l0, "old.wav".into())]);
        ingest.drop_files(vec![(l0, "new.wav".into())]);
        executor.run_all();
        let report = ingest.poll(&mut project);

        assert_eq!(report.placed, vec![l0]);
        assert_eq!(project.track(0).unwrap().source, "new.wav");
    }

    #[test]
    fn nothing_is_pending_after_a_drop_has_been_placed() {
        let (mut ingest, executor) = ingestor(&[("a.wav", stereo_clicks(100.0, 12.0, 0.25))]);
        let mut project = Project::new();
        let l0 = project.lane_id(0).unwrap();
        ingest.drop_files(vec![(l0, "a.wav".into())]);
        executor.run_all();
        ingest.poll(&mut project);

        assert_eq!(ingest.poll(&mut project), IngestReport::default());
        assert!(ingest.batches.is_empty() && ingest.newest.is_empty());
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
        assert!(
            (analyzed.downbeat_secs - 0.25).abs() <= 0.015,
            "downbeat {}",
            analyzed.downbeat_secs
        );
        assert_eq!(analyzed.track.offset_beats, 0.0);
    }
}
