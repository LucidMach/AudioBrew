use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::engine::{Command, EngineHandle, Lane};
use crate::first_run::stretch_for;
use crate::grid::beats_to_secs;
use crate::import::{Stereo, import};
use crate::mix::BandState;
use crate::project::{LaneId, LoadedTrack, Project};
use crate::rerender::RenderJobs;
use crate::stretch::stretch;
use crate::timeline::{Placement, end_marker_secs};
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

/// Everything that decides what a lane's audio *is*. A change here needs a new render.
#[derive(Debug, Clone)]
struct RenderParams {
    source: String,
    factor: f64,
    pitch: i8,
    loop_region: Option<(f64, f64)>,
    crossovers: Vec<f64>,
}

impl RenderParams {
    fn same_audio(&self, other: &RenderParams) -> bool {
        self.source == other.source
            && (self.factor - other.factor).abs() <= 1e-9
            && self.pitch == other.pitch
            && self.loop_region == other.loop_region
            && self.crossovers == other.crossovers
    }
}

/// Everything the engine can change on a playing lane without new audio.
#[derive(Debug, Clone, PartialEq)]
struct MixParams {
    fader: f32,
    bands: Vec<BandState>,
}

/// The engine commands that turn mix settings `old` into `new` (same Band structure).
fn mix_diff(slot: usize, old: &MixParams, new: &MixParams) -> Vec<Command> {
    let mut commands = Vec::new();
    if old.fader != new.fader {
        commands.push(Command::SetFader {
            lane: slot,
            gain: new.fader,
        });
    }
    if old.bands.len() == new.bands.len() {
        for (band, (o, n)) in old.bands.iter().zip(&new.bands).enumerate() {
            if o.gain != n.gain {
                commands.push(Command::SetBandGain {
                    lane: slot,
                    band,
                    gain: n.gain,
                });
            }
            if o.mute != n.mute {
                commands.push(Command::SetBandMute {
                    lane: slot,
                    band,
                    mute: n.mute,
                });
            }
            if o.solo != n.solo {
                commands.push(Command::SetBandSolo {
                    lane: slot,
                    band,
                    solo: n.solo,
                });
            }
        }
    }
    commands
}

struct Done {
    lane: LaneId,
    generation: u64,
    /// The mix settings the new audio was built with.
    built_mix: MixParams,
    outcome: Result<(Box<Lane>, Arc<Stereo>), PipelineError>,
}

/// What the pipeline last asked the engine to do for one lane.
struct LaneState {
    /// The engine slot this lane plays in.
    slot: usize,
    render: RenderParams,
    /// What the Project wants now.
    mix: MixParams,
    /// What the engine's lane for this slot currently has.
    engine_mix: MixParams,
    offset_frames: i64,
    repeats: bool,
    /// The engine holds audio for this lane (possibly about to be replaced).
    loaded: bool,
}

struct Wanted {
    id: LaneId,
    render: RenderParams,
    mix: MixParams,
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
    looping: Option<bool>,
    audition: Option<LaneId>,
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
            looping: None,
            audition: None,
        }
    }

    /// For transport commands (play, pause, seek) and reading the Playhead.
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

    /// Solo audition: plays one lane alone through the same graph; `None` plays the whole
    /// Mashup again. A lane that is not loaded cannot be auditioned and is ignored.
    pub fn audition(&mut self, lane: Option<LaneId>) {
        match lane {
            None => {
                self.audition = None;
                self.queue(Command::Audition(None));
            }
            Some(id) => {
                let slot = self.lanes.get(&id).filter(|l| l.loaded).map(|l| l.slot);
                if let Some(slot) = slot {
                    self.audition = Some(id);
                    self.queue(Command::Audition(Some(slot)));
                }
            }
        }
        self.flush();
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

        for w in wanted {
            let needs_render = match self.lanes.get(&w.id) {
                None => true,
                Some(l) => !l.render.same_audio(&w.render),
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
        if self.looping != Some(project.loops_mashup()) {
            self.looping = Some(project.loops_mashup());
            self.queue(Command::SetLooping(project.loops_mashup()));
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
                    let slot = state.slot;
                    let (offset, repeats) = (state.offset_frames, state.repeats);
                    // The audio was built with the settings of its day; catch up to today's.
                    let catch_up = mix_diff(slot, &done.built_mix, &state.mix);
                    state.engine_mix = state.mix.clone();
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
                    for command in catch_up {
                        self.queue(command);
                    }
                    if self.audition == Some(done.lane) {
                        self.queue(Command::Audition(Some(slot)));
                    }
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
    fn plan(&self, project: &Project) -> (Vec<Wanted>, i64) {
        let mashup = project.mashup_bpm();
        let usable = |bpm: Option<f64>| bpm.filter(|b| b.is_finite() && *b > 0.0);
        let rows: Vec<(LaneId, &LoadedTrack, f64, f64)> = project
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
            .map(|(_, t, factor, offset)| {
                let source_len = t.loop_region.map_or(t.length_secs, |(s, e)| e - s);
                Placement {
                    length_secs: source_len / factor,
                    offset_secs: *offset,
                }
            })
            .collect();
        let end_secs = if rows.is_empty() {
            0.0
        } else {
            end_marker_secs(&placements, project.end_marker_secs())
        };
        let wanted = rows
            .iter()
            .zip(&placements)
            .map(|((id, t, factor, offset_secs), placement)| Wanted {
                id: *id,
                render: RenderParams {
                    source: t.source.clone(),
                    factor: *factor,
                    pitch: t.pitch_semitones,
                    loop_region: t.loop_region,
                    crossovers: t.bands.config.crossovers().to_vec(),
                },
                mix: MixParams {
                    fader: t.fader,
                    bands: t.bands.states.clone(),
                },
                offset_frames: secs_to_frames(*offset_secs, self.sample_rate as f64),
                repeats: t.loop_region.is_some()
                    || placement.offset_secs + placement.length_secs < end_secs - 1e-9,
            })
            .collect();
        (wanted, secs_to_frames(end_secs, self.sample_rate as f64))
    }

    fn start_render(&mut self, w: Wanted) {
        let (existing_slot, source_changed, loaded, engine_mix) = match self.lanes.get(&w.id) {
            Some(l) => (
                Some(l.slot),
                l.render.source != w.render.source,
                l.loaded,
                Some(l.engine_mix.clone()),
            ),
            None => (None, true, false, None),
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
        let engine_mix = engine_mix.unwrap_or_else(|| w.mix.clone());
        self.lanes.insert(
            w.id,
            LaneState {
                slot,
                render: w.render.clone(),
                mix: w.mix.clone(),
                engine_mix,
                offset_frames: w.offset_frames,
                repeats: w.repeats,
                loaded,
            },
        );

        let generation = self.jobs.start(w.id.0);
        let job = RenderJob {
            id: w.id,
            generation,
            cached: self.cache.get(&w.id).cloned(),
            loader: Arc::clone(&self.loader),
            sample_rate: self.sample_rate,
            render: w.render,
            mix: w.mix,
            offset: w.offset_frames,
            repeats: w.repeats,
            tx: self.results_tx.clone(),
        };
        self.executor.spawn(Box::new(move || job.run()));
    }

    /// The audio is unchanged; only where it plays, or how it is mixed, moved.
    fn update_placement(&mut self, w: Wanted) {
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
            commands.extend(mix_diff(state.slot, &state.engine_mix, &w.mix));
            state.engine_mix = w.mix.clone();
        }
        state.mix = w.mix;
        for c in commands {
            self.queue(c);
        }
    }

    fn remove_lane(&mut self, id: LaneId) {
        if self.audition == Some(id) {
            self.audition = None;
            self.queue(Command::Audition(None));
        }
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
    cached: Option<Arc<Stereo>>,
    loader: Arc<dyn Loader>,
    sample_rate: u32,
    render: RenderParams,
    mix: MixParams,
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
            built_mix: self.mix.clone(),
            outcome,
        });
    }

    fn render(&self) -> Result<(Box<Lane>, Arc<Stereo>), PipelineError> {
        let audio = match &self.cached {
            Some(audio) => Arc::clone(audio),
            None => Arc::new(
                self.loader
                    .load(&self.render.source, self.sample_rate)
                    .map_err(PipelineError::Load)?,
            ),
        };

        let region;
        let source: &Stereo = match self.render.loop_region {
            Some((start, end)) => {
                region = slice(&audio, start, end);
                &region
            }
            None => &audio,
        };

        let stretched;
        let playable: &Stereo = if (self.render.factor - 1.0).abs() < 1e-9 && self.render.pitch == 0
        {
            source
        } else {
            stretched = stretch(source, self.render.factor, self.render.pitch)
                .map_err(|e| PipelineError::Stretch(format!("{e:?}")))?;
            &stretched
        };

        let lane = Lane::with_settings(
            playable,
            &self.render.crossovers,
            self.offset,
            self.repeats,
            self.mix.fader,
            &self.mix.bands,
        )
        .map_err(|e| PipelineError::Lane(format!("{e:?}")))?;
        Ok((lane, audio))
    }
}

/// The part of `audio` between two times in seconds. An empty or backwards span gives all of it.
fn slice(audio: &Stereo, start_secs: f64, end_secs: f64) -> Stereo {
    let rate = audio.sample_rate as f64;
    let frames = audio.frames();
    let start = ((start_secs * rate).round() as usize).min(frames);
    let end = ((end_secs * rate).round() as usize).min(frames);
    if end <= start {
        return audio.clone();
    }
    Stereo {
        samples: audio.samples[start * 2..end * 2].to_vec(),
        sample_rate: audio.sample_rate,
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
            ..Default::default()
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

    fn tone(freq: f64, frames: usize, amplitude: f64) -> Stereo {
        let v = |i: usize| {
            (amplitude * (2.0 * std::f64::consts::PI * freq * i as f64 / RATE as f64).sin()) as f32
        };
        Stereo {
            samples: (0..frames).flat_map(|i| [v(i), v(i)]).collect(),
            sample_rate: RATE,
        }
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn frequency(x: &[f32]) -> f64 {
        let crossings = x
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        crossings as f64 / 2.0 / (x.len() as f64 / RATE as f64)
    }

    /// RMS of a unit-amplitude-0.5 sine.
    const HALF_SINE_RMS: f64 = 0.5 * std::f64::consts::FRAC_1_SQRT_2;

    #[test]
    fn the_fader_is_applied_and_changing_it_does_not_render_again() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.set_fader(0, 0.5).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert!(near(tail(&r.play(3_000)), 0.125));

        let spawned = r.exec.spawned();
        p.set_fader(0, 1.0).unwrap();
        r.pipeline.sync(&p);
        r.pipeline.pump();
        assert_eq!(r.exec.spawned(), spawned);
        assert!(near(tail(&r.play(3_000)), 0.25));
    }

    #[test]
    fn a_muted_band_is_silent_from_the_first_frame_and_unmuting_needs_no_render() {
        let mut r = rig(&[("t.wav", tone(1_000.0, 16_000, 0.5))]);
        let mut p = project(&[track("t.wav", Some(120.0), 1.0, 0.0)]);
        p.set_band_mute(0, 1, true).unwrap(); // the 300-4000 Band holds the 1 kHz tone
        r.pipeline.sync(&p);
        r.settle();
        // Built with the mute: no 20 ms burst of the muted Band at the start.
        let out = r.play(8_000);
        assert!(
            rms(&out[..2_000]) < 0.03 * HALF_SINE_RMS,
            "{}",
            rms(&out[..2_000])
        );

        let spawned = r.exec.spawned();
        p.set_band_mute(0, 1, false).unwrap();
        r.pipeline.sync(&p);
        r.pipeline.pump();
        assert_eq!(r.exec.spawned(), spawned);
        let out = r.play(8_000);
        assert!((rms(&out[4_000..]) - HALF_SINE_RMS).abs() < 0.03 * HALF_SINE_RMS);
    }

    #[test]
    fn soloing_and_gaining_a_band_reach_the_engine_without_rendering() {
        let mut r = rig(&[("t.wav", tone(1_000.0, 16_000, 0.5))]);
        let mut p = project(&[track("t.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p);
        r.settle();
        let spawned = r.exec.spawned();

        p.set_band_solo(0, 2, true).unwrap(); // solo the high Band: the 1 kHz tone vanishes
        r.pipeline.sync(&p);
        r.pipeline.pump();
        assert!(rms(&r.play(8_000)[4_000..]) < 0.03 * HALF_SINE_RMS);

        p.set_band_solo(0, 2, false).unwrap();
        p.set_band_gain(0, 1, 0.5).unwrap();
        r.pipeline.sync(&p);
        r.pipeline.pump();
        let level = rms(&r.play(8_000)[4_000..]);
        assert!(
            (level - 0.5 * HALF_SINE_RMS).abs() < 0.05 * HALF_SINE_RMS,
            "{level}"
        );
        assert_eq!(r.exec.spawned(), spawned);
    }

    #[test]
    fn changing_where_the_bands_split_renders_again_and_the_new_bands_follow_their_settings() {
        let mut r = rig(&[("t.wav", tone(2_000.0, 16_000, 0.5))]);
        let mut p = project(&[track("t.wav", Some(120.0), 1.0, 0.0)]);
        p.add_band(0, 1_000.0).unwrap(); // Bands: 20-300, 300-1000, 1000-4000, 4000-
        p.set_band_mute(0, 2, true).unwrap(); // the 1000-4000 Band holds the 2 kHz tone
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.exec.spawned(), 1);
        // 2 kHz is an octave from the 1 kHz and 4 kHz crossovers, so each neighbouring Band
        // leaks (1/2)^4 = 6% of it (24 dB/octave): a muted Band leaves at most about 12%.
        assert!(rms(&r.play(8_000)[4_000..]) < 0.15 * HALF_SINE_RMS);

        p.remove_band(0, 2).unwrap(); // the muted Band goes; its neighbour absorbs the range
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.exec.spawned(), 2);
        let level = rms(&r.play(8_000)[4_000..]);
        assert!(
            (level - HALF_SINE_RMS).abs() < 0.03 * HALF_SINE_RMS,
            "{level}"
        );
    }

    #[test]
    fn a_pitch_shift_renders_again_and_changes_the_pitch_not_the_length() {
        let mut r = rig(&[("t.wav", tone(440.0, 16_000, 0.5))]);
        let mut p = project(&[track("t.wav", Some(120.0), 1.0, 0.0)]);
        p.set_pitch(0, 6).unwrap(); // +6 semitones = x1.4142
        r.pipeline.sync(&p);
        r.settle();
        let out = r.play(16_000);
        let hz = frequency(&out[4_000..12_000]);
        assert!((hz - 440.0 * 2f64.powf(0.5)).abs() < 15.0, "{hz} Hz");
        assert_eq!(r.pipeline.handle().playhead(), 16_000);

        p.set_pitch(0, 0).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.exec.spawned(), 2);
        let hz = frequency(&r.play(16_000)[4_000..12_000]);
        assert!((hz - 440.0).abs() < 10.0, "{hz} Hz");
    }

    #[test]
    fn a_loop_region_repeats_only_that_span_to_fill_the_mashup() {
        // The first half of the file is 0.1, the second half 0.4. Loop the second half.
        let mut samples = vec![0.1f32; 16_000];
        samples.extend(vec![0.4f32; 16_000]);
        let file = Stereo {
            samples,
            sample_rate: RATE,
        };
        let mut r = rig(&[("a.wav", file), ("b.wav", constant(32_000, 0.0))]);
        let mut p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(120.0), 2.0, 0.0), // 2 s long: sets the Mashup length
        ]);
        p.set_loop_region(0, Some((0.5, 1.0))).unwrap();
        r.pipeline.sync(&p);
        r.settle();

        let out = r.play(32_000);
        assert_eq!(r.pipeline.handle().playhead(), 32_000);
        assert!(near(&out[3_000..3_100], 0.4)); // the first half (0.1) is never heard
        assert!(near(&out[20_000..20_100], 0.4)); // several passes later
        assert!(near(tail(&out), 0.4));
    }

    #[test]
    fn a_loop_region_shortens_the_tracks_own_length_for_the_default_end_marker() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.set_loop_region(0, Some((0.0, 0.5))).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        r.play(20_000);
        assert_eq!(r.pipeline.handle().playhead(), 8_000);
    }

    #[test]
    fn a_user_set_end_marker_overrides_the_longest_track_and_tracks_repeat_to_it() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.set_end_marker_beats(Some(4.0)).unwrap(); // 2 s at 120 BPM
        r.pipeline.sync(&p);
        r.settle();
        let out = r.play(40_000);
        assert_eq!(r.pipeline.handle().playhead(), 32_000);
        assert!(near(&out[31_900..32_000], 0.25)); // repeated all the way to the marker
        assert_eq!(out[32_000], 0.0);

        p.set_end_marker_beats(None).unwrap();
        r.pipeline.sync(&p);
        r.settle();
        r.play(40_000);
        assert_eq!(r.pipeline.handle().playhead(), 16_000);
    }

    #[test]
    fn the_whole_mashup_loop_toggle_wraps_the_playhead() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        p.set_loop_mashup(true);
        r.pipeline.sync(&p);
        r.settle();
        r.play(40_000);
        assert_eq!(r.pipeline.handle().playhead(), 40_000 % 16_000);

        p.set_loop_mashup(false);
        r.pipeline.sync(&p);
        r.pipeline.pump();
        r.play(40_000);
        assert_eq!(r.pipeline.handle().playhead(), 16_000);
    }

    #[test]
    fn auditioning_a_lane_plays_it_alone_and_none_brings_the_others_back() {
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.1)),
            ("b.wav", constant(16_000, 0.2)),
        ]);
        let p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(120.0), 1.0, 0.0),
        ]);
        r.pipeline.sync(&p);
        r.settle();
        let (a, b) = (p.lane_id(0).unwrap(), p.lane_id(1).unwrap());

        r.pipeline.audition(Some(b));
        assert!(near(tail(&r.play(3_000)), 0.2));
        r.pipeline.audition(Some(a));
        assert!(near(tail(&r.play(3_000)), 0.1));
        r.pipeline.audition(None);
        assert!(near(tail(&r.play(3_000)), 0.3));
    }

    #[test]
    fn removing_the_auditioned_lane_ends_the_audition() {
        let mut r = rig(&[
            ("a.wav", constant(16_000, 0.1)),
            ("b.wav", constant(16_000, 0.2)),
        ]);
        let mut p = project(&[
            track("a.wav", Some(120.0), 1.0, 0.0),
            track("b.wav", Some(120.0), 1.0, 0.0),
        ]);
        p.add_lane();
        r.pipeline.sync(&p);
        r.settle();
        r.pipeline.audition(Some(p.lane_id(0).unwrap()));
        r.play(100);

        p.remove_lane(0).unwrap();
        r.pipeline.sync(&p);
        // The remaining lane must not stay silenced by an audition of a lane that is gone.
        assert!(near(tail(&r.play(3_000)), 0.2));
    }

    #[test]
    fn auditioning_a_lane_that_is_not_loaded_changes_nothing() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.1))]);
        let p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p);
        r.settle();
        r.pipeline.audition(Some(p.lane_id(1).unwrap())); // lane 1 is empty
        assert!(near(tail(&r.play(3_000)), 0.1));
    }

    #[test]
    fn settings_survive_a_render_triggered_by_a_tempo_change() {
        let mut r = rig(&[("t.wav", tone(1_000.0, 16_000, 0.5))]);
        let mut p = project(&[track("t.wav", Some(120.0), 1.0, 0.0)]);
        p.set_fader(0, 0.5).unwrap();
        r.pipeline.sync(&p);
        r.settle();

        p.set_mashup_bpm(Some(100.0)).unwrap(); // re-stretches the Track
        r.pipeline.sync(&p);
        r.settle();
        assert_eq!(r.exec.spawned(), 2);

        let level = rms(&r.play(19_000)[6_000..]);
        assert!(
            (level - 0.5 * HALF_SINE_RMS).abs() < 0.06 * HALF_SINE_RMS,
            "{level}"
        );
    }

    #[test]
    fn a_setting_changed_while_its_render_is_in_flight_is_applied_when_it_lands() {
        let mut r = rig(&[("a.wav", constant(16_000, 0.25))]);
        let mut p = project(&[track("a.wav", Some(120.0), 1.0, 0.0)]);
        r.pipeline.sync(&p); // render queued, not run
        p.set_fader(0, 0.5).unwrap();
        p.set_end_marker_beats(Some(4.0)).unwrap();
        r.pipeline.sync(&p);
        r.settle(); // the render finishes only now

        assert!(near(tail(&r.play(3_000)), 0.125));
        assert_eq!(r.pipeline.handle().playhead(), 3_000);
    }
}
