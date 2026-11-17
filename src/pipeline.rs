use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::bands::BandConfig;
use crate::engine::{Command, EngineHandle, Lane};
use crate::first_run::stretch_for;
use crate::grid::beats_to_secs;
use crate::import::{Stereo, import};
use crate::project::{LaneId, Project};
use crate::rerender::RenderJobs;
use crate::stretch::stretch;
use crate::timeline::{Placement, mashup_length_secs};
use crate::transport::secs_to_frames;

pub type Job = Box<dyn FnOnce() + Send>;

/// Reads a source file and returns stereo audio at the device rate.
pub trait Loader: Send + Sync {
    fn load(&self, source: &str, device_rate: u32) -> Result<Stereo, String>;
}

/// Runs background work (decoding and stretching) off the UI thread.
pub trait Executor: Send + Sync {
    fn spawn(&self, job: Job);
}

/// Loads real files from disk through `import::import`.
pub struct FileLoader;

impl Loader for FileLoader {
    fn load(&self, source: &str, device_rate: u32) -> Result<Stereo, String> {
        let path = Path::new(source);
        let bytes = std::fs::read(path).map_err(|e| format!("{source}: {e}"))?;
        import(
            bytes,
            path.extension().and_then(|e| e.to_str()),
            device_rate,
        )
        .map_err(|e| format!("{source}: {e:?}"))
    }
}

/// One OS thread per job. Jobs are rare (a load or a tempo change), so this is enough.
pub struct ThreadExecutor;

impl Executor for ThreadExecutor {
    fn spawn(&self, job: Job) {
        std::thread::spawn(job);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    Load(String),
    Stretch(String),
    Lane(String),
    /// More Tracks than the engine has lanes.
    NoFreeSlot,
}

struct Done {
    lane: LaneId,
    generation: u64,
    outcome: Result<(Box<Lane>, Arc<Stereo>), PipelineError>,
}

/// What the pipeline last asked the engine to do for one lane.
struct LaneState {
    /// The engine slot this lane plays in.
    slot: usize,
    source: String,
    factor: f64,
    offset_frames: i64,
    repeats: bool,
    /// The engine holds audio for this lane (possibly about to be replaced).
    loaded: bool,
}

struct Wanted<'a> {
    id: LaneId,
    source: &'a str,
    factor: f64,
    offset_frames: i64,
    repeats: bool,
}

/// Turns Project states into engine work. Call `sync` with the current Project after every
/// change (undo and redo included) and `pump` regularly from the UI loop. Lanes are
/// identified by `LaneId`, never by index, so work in flight cannot land on the wrong lane.
pub struct Pipeline {
    handle: EngineHandle,
    sample_rate: u32,
    loader: Arc<dyn Loader>,
    executor: Arc<dyn Executor>,
    results_tx: Sender<Done>,
    results_rx: Receiver<Done>,
    jobs: RenderJobs,
    lanes: HashMap<LaneId, LaneState>,
    free_slots: Vec<usize>,
    /// Decoded audio per lane, so a tempo change re-stretches without re-reading the file.
    cache: HashMap<LaneId, Arc<Stereo>>,
    errors: HashMap<LaneId, PipelineError>,
    /// Commands the engine queue had no room for yet, in order.
    outbox: VecDeque<Command>,
    end_frames: Option<i64>,
}

impl Pipeline {
    pub fn new(
        handle: EngineHandle,
        sample_rate: u32,
        max_lanes: usize,
        loader: Arc<dyn Loader>,
        executor: Arc<dyn Executor>,
    ) -> Self {
        let (results_tx, results_rx) = mpsc::channel();
        Self {
            handle,
            sample_rate,
            loader,
            executor,
            results_tx,
            results_rx,
            jobs: RenderJobs::new(),
            lanes: HashMap::new(),
            free_slots: (0..max_lanes).collect(),
            cache: HashMap::new(),
            errors: HashMap::new(),
            outbox: VecDeque::new(),
            end_frames: None,
        }
    }

    /// For transport commands (play, pause, seek, loop) and reading the Playhead.
    pub fn handle(&self) -> &EngineHandle {
        &self.handle
    }

    pub fn handle_mut(&mut self) -> &mut EngineHandle {
        &mut self.handle
    }

    pub fn error(&self, lane: LaneId) -> Option<&PipelineError> {
        self.errors.get(&lane)
    }

    pub fn is_loaded(&self, lane: LaneId) -> bool {
        self.lanes.get(&lane).is_some_and(|l| l.loaded)
    }

    /// Brings the engine in line with `project`, doing only the work that changed.
    pub fn sync(&mut self, project: &Project) {
        let (wanted, end_frames) = self.plan(project);

        let present: HashSet<LaneId> = wanted.iter().map(|w| w.id).collect();
        let gone: Vec<LaneId> = self
            .lanes
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect();
        for id in gone {
            self.remove_lane(id);
        }
        self.errors.retain(|id, _| present.contains(id));
        self.cache.retain(|id, _| present.contains(id));

        for w in &wanted {
            let needs_render = match self.lanes.get(&w.id) {
                None => true,
                Some(l) => l.source != w.source || (l.factor - w.factor).abs() > 1e-9,
            };
            if needs_render {
                self.start_render(w);
            } else {
                self.update_placement(w);
            }
        }

        if self.end_frames != Some(end_frames) {
            self.end_frames = Some(end_frames);
            self.queue(Command::SetEnd(end_frames));
        }
        self.flush();
    }

    /// Applies finished background renders. Returns how many replaced a lane's audio.
    /// Results from superseded or cancelled renders are discarded.
    pub fn pump(&mut self) -> usize {
        let mut applied = 0;
        while let Ok(done) = self.results_rx.try_recv() {
            if !self.jobs.finish(done.lane.0, done.generation) {
                continue; // stale: a newer render started, or the lane is gone
            }
            let Some(state) = self.lanes.get_mut(&done.lane) else {
                continue;
            };
            match done.outcome {
                Ok((lane, audio)) => {
                    state.loaded = true;
                    let (slot, offset, repeats) = (state.slot, state.offset_frames, state.repeats);
                    self.cache.insert(done.lane, audio);
                    self.errors.remove(&done.lane);
                    self.queue(Command::LoadLane {
                        lane: slot,
                        track: lane,
                    });
                    self.queue(Command::SetOffset { lane: slot, offset });
                    self.queue(Command::SetRepeats {
                        lane: slot,
                        repeats,
                    });
                    applied += 1;
                }
                Err(e) => {
                    self.errors.insert(done.lane, e);
                }
            }
        }
        self.flush();
        self.handle.collect_garbage();
        applied
    }

    /// What every loaded Track should do at the current Mashup tempo, and where the Mashup ends.
    fn plan<'a>(&self, project: &'a Project) -> (Vec<Wanted<'a>>, i64) {
        let mashup = project.mashup_bpm();
        let usable = |bpm: Option<f64>| bpm.filter(|b| b.is_finite() && *b > 0.0);
        let rows: Vec<_> = project
            .tracks()
            .map(|(id, t)| {
                let factor = match (mashup, usable(t.bpm)) {
                    (Some(m), Some(b)) => stretch_for(b, m).0,
                    _ => 1.0,
                };
                let offset_secs = mashup.map_or(0.0, |m| beats_to_secs(t.offset_beats, m));
                (id, t, factor, offset_secs)
            })
            .collect();
        let placements: Vec<Placement> = rows
            .iter()
            .map(|(_, t, factor, offset)| Placement {
                length_secs: t.length_secs / factor,
                offset_secs: *offset,
            })
            .collect();
        let end_secs = mashup_length_secs(&placements);
        let wanted = rows
            .iter()
            .zip(&placements)
            .map(|((id, t, factor, offset_secs), placement)| Wanted {
                id: *id,
                source: &t.source,
                factor: *factor,
                offset_frames: secs_to_frames(*offset_secs, self.sample_rate as f64),
                repeats: placement.offset_secs + placement.length_secs < end_secs - 1e-9,
            })
            .collect();
        (wanted, secs_to_frames(end_secs, self.sample_rate as f64))
    }

    fn start_render(&mut self, w: &Wanted) {
        let (existing_slot, source_changed, loaded) = match self.lanes.get(&w.id) {
            Some(l) => (Some(l.slot), l.source != w.source, l.loaded),
            None => (None, true, false),
        };
        let slot = match existing_slot.or_else(|| self.alloc_slot()) {
            Some(slot) => slot,
            None => {
                self.errors.insert(w.id, PipelineError::NoFreeSlot);
                return;
            }
        };
        if source_changed {
            self.cache.remove(&w.id);
        }
        self.errors.remove(&w.id);
        self.lanes.insert(
            w.id,
            LaneState {
                slot,
                source: w.source.to_string(),
                factor: w.factor,
                offset_frames: w.offset_frames,
                repeats: w.repeats,
                loaded,
            },
        );

        let generation = self.jobs.start(w.id.0);
        let job = RenderJob {
            id: w.id,
            generation,
            source: w.source.to_string(),
            cached: self.cache.get(&w.id).cloned(),
            loader: Arc::clone(&self.loader),
            sample_rate: self.sample_rate,
            factor: w.factor,
            offset: w.offset_frames,
            repeats: w.repeats,
            tx: self.results_tx.clone(),
        };
        self.executor.spawn(Box::new(move || job.run()));
    }

    /// The audio is unchanged; only where it plays moved.
    fn update_placement(&mut self, w: &Wanted) {
        let Some(state) = self.lanes.get_mut(&w.id) else {
            return;
        };
        let mut commands = Vec::new();
        if state.offset_frames != w.offset_frames {
            state.offset_frames = w.offset_frames;
            commands.push(Command::SetOffset {
                lane: state.slot,
                offset: w.offset_frames,
            });
        }
        if state.repeats != w.repeats {
            state.repeats = w.repeats;
            commands.push(Command::SetRepeats {
                lane: state.slot,
                repeats: w.repeats,
            });
        }
        if state.loaded {
            for c in commands {
                self.queue(c);
            }
        }
    }

    fn remove_lane(&mut self, id: LaneId) {
        if let Some(state) = self.lanes.remove(&id) {
            if state.loaded {
                self.queue(Command::ClearLane(state.slot));
            }
            self.free_slots.push(state.slot);
            self.free_slots.sort_unstable();
        }
        self.jobs.cancel(id.0);
        self.cache.remove(&id);
        self.errors.remove(&id);
    }

    fn alloc_slot(&mut self) -> Option<usize> {
        (!self.free_slots.is_empty()).then(|| self.free_slots.remove(0))
    }

    fn queue(&mut self, command: Command) {
        self.outbox.push_back(command);
    }

    /// Sends queued commands in order until the engine's queue is full.
    fn flush(&mut self) {
        while let Some(command) = self.outbox.pop_front() {
            if let Err(command) = self.handle.send(command) {
                self.outbox.push_front(command);
                break;
            }
        }
    }
}

/// Everything a background render needs, moved onto the worker.
struct RenderJob {
    id: LaneId,
    generation: u64,
    source: String,
    cached: Option<Arc<Stereo>>,
    loader: Arc<dyn Loader>,
    sample_rate: u32,
    factor: f64,
    offset: i64,
    repeats: bool,
    tx: Sender<Done>,
}

impl RenderJob {
    fn run(self) {
        let outcome = self.render();
        let _ = self.tx.send(Done {
            lane: self.id,
            generation: self.generation,
            outcome,
        });
    }

    fn render(&self) -> Result<(Box<Lane>, Arc<Stereo>), PipelineError> {
        let audio = match &self.cached {
            Some(audio) => Arc::clone(audio),
            None => Arc::new(
                self.loader
                    .load(&self.source, self.sample_rate)
                    .map_err(PipelineError::Load)?,
            ),
        };
        let stretched;
        let playable: &Stereo = if (self.factor - 1.0).abs() < 1e-9 {
            &audio
        } else {
            stretched = stretch(&audio, self.factor, 0)
                .map_err(|e| PipelineError::Stretch(format!("{e:?}")))?;
            &stretched
        };
        let crossovers = BandConfig::default().crossovers().to_vec();
        let lane = Lane::new(playable, &crossovers, self.offset, self.repeats, 1.0)
            .map_err(|e| PipelineError::Lane(format!("{e:?}")))?;
        Ok((lane, audio))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Command, Engine};
    use crate::history::History;
    use crate::project::{LoadedTrack, Project};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const RATE: u32 = 16_000;

    fn constant(frames: usize, v: f32) -> Stereo {
        Stereo {
            samples: vec![v; frames * 2],
            sample_rate: RATE,
        }
    }

    /// Serves canned audio by name and counts how many times it was asked to decode.
    struct FakeLoader {
        files: HashMap<String, Stereo>,
        loads: AtomicUsize,
    }

    impl Loader for FakeLoader {
        fn load(&self, source: &str, _rate: u32) -> Result<Stereo, String> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            self.files
                .get(source)
                .cloned()
                .ok_or_else(|| format!("cannot read {source}"))
        }
    }

    /// Holds jobs until the test releases them, in any order.
    #[derive(Default)]
    struct Deferred {
        jobs: Mutex<Vec<Option<Job>>>,
        spawned: AtomicUsize,
    }

    impl Executor for Deferred {
        fn spawn(&self, job: Job) {
            self.spawned.fetch_add(1, Ordering::SeqCst);
            self.jobs.lock().unwrap().push(Some(job));
        }
    }

    impl Deferred {
        fn run(&self, index: usize) {
            let job = self.jobs.lock().unwrap()[index]
                .take()
                .expect("job already run");
            job();
        }

        fn run_all(&self) {
            let n = self.jobs.lock().unwrap().len();
            for i in 0..n {
                let job = self.jobs.lock().unwrap()[i].take();
                if let Some(job) = job {
                    job();
                }
            }
        }

        fn spawned(&self) -> usize {
            self.spawned.load(Ordering::SeqCst)
        }
    }

    struct Rig {
        engine: Engine,
        pipeline: Pipeline,
        loader: Arc<FakeLoader>,
        exec: Arc<Deferred>,
    }

    fn rig_with(max_lanes: usize, queue: usize, files: &[(&str, Stereo)]) -> Rig {
        let (engine, handle) = Engine::new(RATE, max_lanes, queue);
        let loader = Arc::new(FakeLoader {
            files: files
                .iter()
                .map(|(n, a)| (n.to_string(), a.clone()))
                .collect(),
            loads: AtomicUsize::new(0),
        });
        let exec = Arc::new(Deferred::default());
        let pipeline = Pipeline::new(handle, RATE, max_lanes, loader.clone(), exec.clone());
        Rig {
            engine,
            pipeline,
            loader,
            exec,
        }
    }

    fn rig(files: &[(&str, Stereo)]) -> Rig {
        rig_with(8, 64, files)
    }

    impl Rig {
        /// Runs every queued job and applies the results.
        fn settle(&mut self) -> usize {
            self.exec.run_all();
            self.pipeline.pump()
        }

        fn send(&mut self, c: Command) {
            assert!(self.pipeline.handle_mut().send(c).is_ok());
        }

        /// Restarts from frame 0, plays, and returns `frames` frames of the left channel.
        fn play(&mut self, frames: usize) -> Vec<f32> {
            self.send(Command::Seek(0));
            self.send(Command::Play);
            let mut out = vec![0.0f32; frames * 2];
            self.engine.process(&mut out);
            out.chunks(2).map(|f| f[0]).collect()
        }

        fn loads(&self) -> usize {
            self.loader.loads.load(Ordering::SeqCst)
        }
    }

    /// The last 100 frames: by then the Band splitter's start-up transient has died away.
    fn tail(out: &[f32]) -> &[f32] {
        &out[out.len() - 100..]
    }

    fn near(samples: &[f32], want: f32) -> bool {
        samples.iter().all(|v| (v - want).abs() < 1e-3)
    }

    /// The first frame with any sound in it.
    fn first_sound(out: &[f32]) -> Option<usize> {
        out.iter().position(|v| v.abs() > 1e-6)
    }

    fn track(source: &str, bpm: Option<f64>, length_secs: f64, offset_beats: f64) -> LoadedTrack {
        LoadedTrack {
            source: source.into(),
            bpm,
            key: None,
            length_secs,
            offset_beats,
        }
    }

    fn project(tracks: &[LoadedTrack]) -> Project {
        let mut p = Project::new();
        while p.lane_count() < tracks.len() {
            p.add_lane();
        }
        for (i, t) in tracks.iter().enumerate() {
            p.load_track(i, t.clone()).unwrap();
        }
        p
    }

    #[test]
    fn a_loaded_track_plays_once_its_background_render_has_finished() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);

        r.pipeline.sync(&p);
        assert!(r.play(100).iter().all(|v| *v == 0.0)); // still rendering

        assert_eq!(r.settle(), 1);
        assert!(near(tail(&r.play(3_000)), 0.25));
        assert!(r.pipeline.is_loaded(p.lane_id(0).unwrap()));
    }

    #[test]
    fn the_end_marker_is_where_the_longest_stretched_track_ends_and_shorter_ones_repeat() {
        // B is 100 BPM under a 120 BPM Mashup: 1.2x, so its 2 s become 2 / 1.2 = 1.6667 s.
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.25)),
            ("b.wav", constant(32_000, 0.0)),
        ]);
        r.pipeline.sync(&project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(100.0), 2.0, 0.0),
        ]));
        r.settle();

        let out = r.play(30_000);
        let end = 26_667; // round(1.6667 s * 16000)
        assert_eq!(r.pipeline.handle().playhead(), end);
        // A repeated all the way to the End marker, then silence.
        assert!(near(&out[end as usize - 100..end as usize], 0.25));
        assert_eq!(out[end as usize], 0.0);
    }

    #[test]
    fn an_offset_in_beats_becomes_a_delay_at_the_mashup_tempo() {
        // 0.5 beat at 120 BPM = 0.25 s = 4000 frames.
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        r.pipeline
            .sync(&project(&[track("a.wav", Some(120.0), 1.0, 0.5)]));
        r.settle();

        let out = r.play(8_000);
        assert_eq!(first_sound(&out), Some(4_000));
        assert!(near(tail(&out), 0.25));
    }

    #[test]
    fn changing_only_the_offset_moves_the_track_without_rendering_again() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.5)]);
        r.pipeline.sync(&p);
        r.settle();
        let spawned = r.exec.spawned();

        p.load_track(0, track("a.wav", Some(120.0), 1.0, 1.0))
            .unwrap(); // 1 beat = 8000 frames
        r.pipeline.sync(&p);
        r.pipeline.pump();

        assert_eq!(r.exec.spawned(), spawned);
        let out = r.play(12_000);
        assert_eq!(first_sound(&out), Some(8_000));
        assert!(near(tail(&out), 0.25));
    }

    #[test]
    fn changing_the_mashup_bpm_restretches_only_tracks_whose_speed_changed_and_reuses_decoded_audio()
     {
        // A has a tempo; B has none, so its speed never depends on the Mashup tempo.
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.0)),
            ("b.wav", constant(16_000, 0.0)),
        ]);
        let mut p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", None, 1.0, 0.0),
        ]);
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!((r.loads(), r.exec.spawned()), (2, 2));

        // Mashup 100 BPM: A slows to 100/120 = 0.8333x, so its 1 s becomes 1.2 s.
        p.set_mashup_bpm(Some(100.0)).unwrap();
        r.pipeline.sync(&p);
        r.settle();

        assert_eq!(r.exec.spawned(), 3); // only A was stretched again
        assert_eq!(r.loads(), 2); // the decoded audio was reused
        r.play(25_000);
        assert_eq!(r.pipeline.handle().playhead(), 19_200); // 1.2 s
    }

    #[test]
    fn a_tempo_change_that_leaves_one_tracks_speed_alone_still_restretches_the_other_that_moved() {
        // B is 100 BPM: under a 120 Mashup it runs 1.2x, under a 100 Mashup it runs 1.0x. Both move.
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.0)),
            ("b.wav", constant(16_000, 0.0)),
        ]);
        let mut p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(100.0), 1.0, 0.0),
        ]);
        r.pipeline.sync(&p);
        r.settle();
        p.set_mashup_bpm(Some(100.0)).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.exec.spawned(), 4);
        assert_eq!(r.loads(), 2);
    }

    #[test]
    fn a_changed_mashup_bpm_moves_every_offset_to_the_new_grid() {
        // B has no tempo (never stretched); A (120 BPM, silent) sets the default Mashup tempo.
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.0)),
            ("b.wav", constant(16_000, 0.25)),
        ]);
        let mut p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", None, 1.0, 2.0),
        ]);
        r.pipeline.sync(&p);
        r.settle();
        let out = r.play(20_000);
        assert_eq!(first_sound(&out), Some(16_000)); // 2 beats at 120 BPM = 1 s

        p.set_mashup_bpm(Some(60.0)).unwrap(); // 2 beats at 60 BPM = 2 s
        r.pipeline.sync(&p);
        r.settle();
        let out = r.play(40_000);
        // The first few hundred frames can still ring with the previous run's filter state,
        // so look past them for the real start.
        assert_eq!(first_sound(&out[500..]), Some(32_000 - 500));
        assert!(near(tail(&out), 0.25));
    }

    #[test]
    fn a_track_without_a_tempo_plays_at_its_own_speed() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", None, 1.0, 0.0)]);
        p.set_mashup_bpm(Some(100.0)).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        r.play(20_000);
        assert_eq!(r.pipeline.handle().playhead(), 16_000);
    }

    #[test]
    fn removing_a_lane_cancels_its_unfinished_render() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.add_lane();
        r.pipeline.sync(&p);
        p.remove_lane(0).unwrap();
        r.pipeline.sync(&p); // the render has not run yet

        assert_eq!(r.settle(), 0); // its result is thrown away
        assert!(r.play(100).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn removing_a_loaded_lane_silences_it() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.add_lane();
        let id = p.lane_id(0).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert!(near(tail(&r.play(3_000)), 0.25));

        p.remove_lane(0).unwrap();
        r.pipeline.sync(&p);
        assert!(!r.pipeline.is_loaded(id));
        assert!(r.play(10).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_stale_render_is_discarded_even_if_it_finishes_first() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.0))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p); // job 0
        p.set_mashup_bpm(Some(100.0)).unwrap();
        r.pipeline.sync(&p); // job 1 supersedes job 0

        r.exec.run(0);
        assert_eq!(r.pipeline.pump(), 0); // the old one finished first but is stale
        r.exec.run(1);
        assert_eq!(r.pipeline.pump(), 1);
    }

    #[test]
    fn a_stale_render_finishing_after_the_current_one_changes_nothing() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.0))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p);
        p.set_mashup_bpm(Some(100.0)).unwrap();
        r.pipeline.sync(&p);

        r.exec.run(1);
        r.exec.run(0);
        assert_eq!(r.pipeline.pump(), 1); // only the current one is applied
        assert_eq!(r.pipeline.pump(), 0);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_reported_and_does_not_stop_other_lanes() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let p = project(&[
            track("bad.wav", Some(120.0), 1.0, 0.0),
            track("a.wav", Some(120.0), 1.0, 0.0),
        ]);
        r.pipeline.sync(&p);
        r.settle();

        let bad = p.lane_id(0).unwrap();
        assert!(matches!(
            r.pipeline.error(bad),
            Some(PipelineError::Load(_))
        ));
        assert!(!r.pipeline.is_loaded(bad));
        assert!(near(tail(&r.play(3_000)), 0.25));
        assert_eq!(r.pipeline.error(p.lane_id(1).unwrap()), None);
    }

    #[test]
    fn fixing_the_source_clears_the_error() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("bad.wav", Some(120.0), 1.0, 0.0)]);
        let id = p.lane_id(0).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert!(r.pipeline.error(id).is_some());

        p.load_track(0, track("a.wav", Some(120.0), 1.0, 0.0))
            .unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.pipeline.error(id), None);
        assert!(near(tail(&r.play(3_000)), 0.25));
    }

    #[test]
    fn more_tracks_than_engine_slots_report_no_free_slot_for_the_extra() {
        let mut r = rig_with(
            1,
            64,
            &[
                ("a.wav", constant(16_000, 0.25)),
                ("b.wav", constant(16_000, 0.25)),
            ],
        );
        let p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(120.0), 1.0, 0.0),
        ]);
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(
            r.pipeline.error(p.lane_id(1).unwrap()),
            Some(&PipelineError::NoFreeSlot)
        );
        assert!(near(tail(&r.play(3_000)), 0.25)); // just the one
    }

    #[test]
    fn a_removed_lanes_engine_slot_is_reused() {
        let mut r = rig_with(
            1,
            64,
            &[
                ("a.wav", constant(16_000, 0.25)),
                ("b.wav", constant(16_000, 0.5)),
            ],
        );
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.add_lane();
        r.pipeline.sync(&p);
        r.settle();

        p.remove_lane(0).unwrap();
        p.load_track(0, track("b.wav", Some(120.0), 1.0, 0.0))
            .unwrap();
        r.pipeline.sync(&p);
        r.settle();

        assert!(near(tail(&r.play(3_000)), 0.5));
    }

    #[test]
    fn changing_a_lanes_source_decodes_the_new_file_and_replaces_the_audio() {
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.25)),
            ("b.wav", constant(16_000, 0.5)),
        ]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p);
        r.settle();
        assert!(near(tail(&r.play(3_000)), 0.25));

        p.load_track(0, track("b.wav", Some(120.0), 1.0, 0.0))
            .unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.loads(), 2);
        assert!(near(tail(&r.play(3_000)), 0.5));
    }

    #[test]
    fn undo_and_redo_through_history_bring_lanes_back_and_take_them_away() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut start = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        start.add_lane();
        let mut history = History::new(start);
        r.pipeline.sync(history.current());
        r.settle();

        let mut next = history.current().clone();
        next.remove_lane(0).unwrap();
        history.edit(next);
        r.pipeline.sync(history.current());
        assert!(r.play(10).iter().all(|v| *v == 0.0));

        history.undo();
        r.pipeline.sync(history.current());
        r.settle();
        assert!(near(tail(&r.play(3_000)), 0.25));

        history.redo();
        r.pipeline.sync(history.current());
        assert!(r.play(10).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn commands_are_never_lost_when_the_engine_queue_is_full() {
        // A queue of 4 cannot hold everything one sync wants to send.
        let mut r = rig_with(
            4,
            4,
            &[
                ("a.wav", constant(16_000, 0.1)),
                ("b.wav", constant(16_000, 0.2)),
                ("c.wav", constant(16_000, 0.3)),
            ],
        );
        r.pipeline.sync(&project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(120.0), 1.0, 0.0),
            track("c.wav", Some(120.0), 1.0, 0.0),
        ]));
        r.settle();
        // Drain the engine's queue a block at a time, letting the pipeline refill it.
        for _ in 0..20 {
            let mut out = [0.0f32; 8];
            r.engine.process(&mut out);
            r.pipeline.pump();
        }
        let out = r.play(3_000);
        assert!(near(tail(&out), 0.6), "{:?}", tail(&out));
    }

    #[test]
    fn an_empty_project_sends_nothing_harmful() {
        let mut r = rig(&[]);
        r.pipeline.sync(&Project::new());
        assert_eq!(r.settle(), 0);
        assert!(r.play(100).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn syncing_an_unchanged_project_again_does_no_work() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p);
        r.settle();
        let spawned = r.exec.spawned();
        r.pipeline.sync(&p);
        r.pipeline.sync(&p);
        assert_eq!(r.exec.spawned(), spawned);
        assert_eq!(r.settle(), 0);
    }
}
