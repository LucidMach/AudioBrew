use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use rtrb::{Consumer, Producer, PushError, RingBuffer};

use crate::bands::MAX_BANDS;
use crate::crossover::{BandSplitter, SplitterError};
use crate::import::Stereo;
use crate::mix::{BandState, effective_gains_into, limit_stereo};
use crate::ramp::{Ramp, ramp_len};
use crate::render::RenderTrack;
use crate::transport::Transport;

/// Every control change glides over this long, so nothing clicks.
pub const RAMP_MS: f64 = 20.0;

/// One Track as the audio callback plays it: stretched stereo audio, its two Band splitters,
/// and ramped Band gains, fader and audition level. Built off the audio thread.
#[derive(Debug)]
pub struct Lane {
    sample_rate: u32,
    left: Vec<f32>,
    right: Vec<f32>,
    offset: i64,
    repeats: bool,
    splitter_left: BandSplitter,
    splitter_right: BandSplitter,
    bands: Vec<BandState>,
    band_gain: Vec<Ramp>,
    fader: Ramp,
    audible: Ramp,
}

impl Lane {
    /// `audio` must already be stretched to the Mashup BPM and at the engine's sample rate.
    /// `crossovers` define the Bands (at most `MAX_BANDS`, so five crossovers). Every Band
    /// starts open at unity gain.
    pub fn new(
        audio: &Stereo,
        crossovers: &[f64],
        offset: i64,
        repeats: bool,
        fader: f32,
    ) -> Result<Box<Lane>, SplitterError> {
        let open = BandState {
            gain: 1.0,
            mute: false,
            solo: false,
        };
        let bands = vec![open; crossovers.len() + 1];
        Self::with_settings(audio, crossovers, offset, repeats, fader, &bands)
    }

    /// Like `new`, but each Band starts at the given gain, mute and solo with no ramp, so a
    /// muted Band is silent from the first frame. `bands` must have one entry per Band.
    pub fn with_settings(
        audio: &Stereo,
        crossovers: &[f64],
        offset: i64,
        repeats: bool,
        fader: f32,
        bands: &[BandState],
    ) -> Result<Box<Lane>, SplitterError> {
        if crossovers.len() >= MAX_BANDS {
            return Err(SplitterError::InvalidCrossover);
        }
        let splitter = BandSplitter::new(audio.sample_rate as f64, crossovers)?;
        let count = splitter.band_count();
        if bands.len() != count {
            return Err(SplitterError::InvalidCrossover);
        }
        let frames = audio.frames();
        let (mut left, mut right) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
        for frame in audio.samples.chunks_exact(2) {
            left.push(frame[0]);
            right.push(frame[1]);
        }
        let mut gains = vec![0.0f32; count];
        effective_gains_into(bands, &mut gains);
        Ok(Box::new(Lane {
            sample_rate: audio.sample_rate,
            left,
            right,
            offset,
            repeats,
            splitter_left: splitter.clone(),
            splitter_right: splitter,
            bands: bands.to_vec(),
            band_gain: gains.into_iter().map(Ramp::new).collect(),
            fader: Ramp::new(if fader.is_finite() {
                fader.max(0.0)
            } else {
                1.0
            }),
            audible: Ramp::new(1.0),
        }))
    }

    /// Points every Band's gain ramp at what mute/solo/gain now ask for.
    fn retarget_bands(&mut self, ramp: usize) {
        let mut targets = [0.0f32; MAX_BANDS];
        let n = self.bands.len();
        effective_gains_into(&self.bands, &mut targets[..n]);
        for (ramp_state, target) in self.band_gain.iter_mut().zip(targets) {
            ramp_state.set_target(target, ramp);
        }
    }

    /// One stereo frame at Mashup position `pos`: split into Bands, apply the ramped Band
    /// gains, then the ramped fader and audition level. Does not allocate.
    fn render(&mut self, pos: i64) -> (f32, f32) {
        let source = |samples| RenderTrack {
            samples,
            offset: self.offset,
            repeats: self.repeats,
            fader: 1.0,
        };
        let (l_in, r_in) = (
            source(&self.left).sample_at(pos),
            source(&self.right).sample_at(pos),
        );

        let n = self.bands.len();
        let (mut bands_l, mut bands_r) = ([0.0f32; MAX_BANDS], [0.0f32; MAX_BANDS]);
        self.splitter_left.process(l_in, &mut bands_l[..n]);
        self.splitter_right.process(r_in, &mut bands_r[..n]);

        let (mut l, mut r) = (0.0f32, 0.0f32);
        for b in 0..n {
            let gain = self.band_gain[b].next_value();
            l += bands_l[b] * gain;
            r += bands_r[b] * gain;
        }
        let level = self.fader.next_value() * self.audible.next_value();
        (l * level, r * level)
    }
}

/// Everything the UI can ask the audio thread to do. Sent over a wait-free ring buffer.
#[derive(Debug)]
pub enum Command {
    LoadLane {
        lane: usize,
        track: Box<Lane>,
    },
    ClearLane(usize),
    SetFader {
        lane: usize,
        gain: f32,
    },
    SetBandGain {
        lane: usize,
        band: usize,
        gain: f32,
    },
    SetBandMute {
        lane: usize,
        band: usize,
        mute: bool,
    },
    SetBandSolo {
        lane: usize,
        band: usize,
        solo: bool,
    },
    SetOffset {
        lane: usize,
        offset: i64,
    },
    SetRepeats {
        lane: usize,
        repeats: bool,
    },
    /// Play one lane alone through the same graph; `None` plays the whole Mashup.
    Audition(Option<usize>),
    Play,
    Pause,
    Seek(i64),
    SetLooping(bool),
    SetEnd(i64),
}

/// The audio-thread half: call `process` from the output callback. It never allocates, locks,
/// or frees: replaced lanes go back to the UI thread through the garbage ring.
pub struct Engine {
    sample_rate: u32,
    ramp: usize,
    lanes: Vec<Option<Box<Lane>>>,
    transport: Transport,
    audition: Option<usize>,
    commands: Consumer<Command>,
    garbage: Producer<Box<Lane>>,
    playhead: Arc<AtomicI64>,
}

/// The UI-thread half.
pub struct EngineHandle {
    commands: Producer<Command>,
    garbage: Consumer<Box<Lane>>,
    playhead: Arc<AtomicI64>,
}

impl Engine {
    /// `max_lanes` lanes, and room for `queue_capacity` pending commands.
    pub fn new(
        sample_rate: u32,
        max_lanes: usize,
        queue_capacity: usize,
    ) -> (Engine, EngineHandle) {
        let (command_tx, command_rx) = RingBuffer::new(queue_capacity);
        let (garbage_tx, garbage_rx) = RingBuffer::new(queue_capacity + max_lanes);
        let playhead = Arc::new(AtomicI64::new(0));
        let engine = Engine {
            sample_rate,
            ramp: ramp_len(sample_rate as f64, RAMP_MS),
            lanes: (0..max_lanes).map(|_| None).collect(),
            transport: Transport::new(0),
            audition: None,
            commands: command_rx,
            garbage: garbage_tx,
            playhead: Arc::clone(&playhead),
        };
        let handle = EngineHandle {
            commands: command_tx,
            garbage: garbage_rx,
            playhead,
        };
        (engine, handle)
    }

    /// Fills an interleaved stereo buffer. A trailing odd sample is left silent.
    pub fn process(&mut self, out: &mut [f32]) {
        for _ in 0..self.commands.slots() {
            match self.commands.pop() {
                Ok(command) => self.apply(command),
                Err(_) => break,
            }
        }
        for frame in out.chunks_exact_mut(2) {
            let (mut l, mut r) = (0.0f32, 0.0f32);
            if self.transport.is_playing() {
                let pos = self.transport.position();
                for lane in self.lanes.iter_mut().flatten() {
                    let (lane_l, lane_r) = lane.render(pos);
                    l += lane_l;
                    r += lane_r;
                }
                self.transport.advance(1);
            }
            (frame[0], frame[1]) = limit_stereo(l, r);
        }
        if out.len() % 2 == 1 {
            let last = out.len() - 1;
            out[last] = 0.0;
        }
        self.playhead
            .store(self.transport.position(), Ordering::Relaxed);
    }

    fn apply(&mut self, command: Command) {
        let ramp = self.ramp;
        match command {
            Command::LoadLane { lane, mut track } => {
                if lane < self.lanes.len() && track.sample_rate == self.sample_rate {
                    let level = self.audible_target(lane);
                    track.audible.set_target(level, 0);
                    if let Some(old) = self.lanes[lane].replace(track) {
                        self.discard(old);
                    }
                } else {
                    self.discard(track);
                }
            }
            Command::ClearLane(lane) => {
                if let Some(old) = self.lanes.get_mut(lane).and_then(Option::take) {
                    self.discard(old);
                }
            }
            Command::SetFader { lane, gain } => {
                if let Some(l) = self.lane_mut(lane)
                    && gain.is_finite()
                {
                    l.fader.set_target(gain.max(0.0), ramp);
                }
            }
            Command::SetBandGain { lane, band, gain } => {
                if let Some(l) = self.lane_mut(lane)
                    && let Some(b) = l.bands.get_mut(band)
                    && gain.is_finite()
                {
                    b.gain = gain.max(0.0);
                    l.retarget_bands(ramp);
                }
            }
            Command::SetBandMute { lane, band, mute } => {
                if let Some(l) = self.lane_mut(lane)
                    && let Some(b) = l.bands.get_mut(band)
                {
                    b.mute = mute;
                    l.retarget_bands(ramp);
                }
            }
            Command::SetBandSolo { lane, band, solo } => {
                if let Some(l) = self.lane_mut(lane)
                    && let Some(b) = l.bands.get_mut(band)
                {
                    b.solo = solo;
                    l.retarget_bands(ramp);
                }
            }
            Command::SetOffset { lane, offset } => {
                if let Some(l) = self.lane_mut(lane) {
                    l.offset = offset;
                }
            }
            Command::SetRepeats { lane, repeats } => {
                if let Some(l) = self.lane_mut(lane) {
                    l.repeats = repeats;
                }
            }
            Command::Audition(which) => {
                self.audition = which;
                for index in 0..self.lanes.len() {
                    let level = self.audible_target(index);
                    if let Some(l) = self.lanes[index].as_mut() {
                        l.audible.set_target(level, ramp);
                    }
                }
            }
            Command::Play => self.transport.play(),
            Command::Pause => self.transport.pause(),
            Command::Seek(frame) => self.transport.seek(frame),
            Command::SetLooping(looping) => self.transport.set_looping(looping),
            Command::SetEnd(end) => self.transport.set_end(end),
        }
    }

    fn lane_mut(&mut self, lane: usize) -> Option<&mut Lane> {
        self.lanes.get_mut(lane)?.as_deref_mut()
    }

    fn audible_target(&self, lane: usize) -> f32 {
        if self.audition.is_none_or(|solo| solo == lane) {
            1.0
        } else {
            0.0
        }
    }

    /// Sends a lane back to the UI thread to be freed there. If the ring is somehow full the
    /// lane is leaked rather than freed on the audio thread.
    fn discard(&mut self, lane: Box<Lane>) {
        if let Err(PushError::Full(lane)) = self.garbage.push(lane) {
            std::mem::forget(lane);
        }
    }
}

impl EngineHandle {
    /// Queues a command. A full queue hands the command back instead of blocking.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands.push(command).map_err(|PushError::Full(c)| c)
    }

    /// The Playhead in frames, as of the last block.
    pub fn playhead(&self) -> i64 {
        self.playhead.load(Ordering::Relaxed)
    }

    /// Frees lanes the audio thread has finished with; returns how many.
    pub fn collect_garbage(&mut self) -> usize {
        let mut freed = 0;
        while self.garbage.pop().is_ok() {
            freed += 1;
        }
        freed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::Stereo;
    use std::f64::consts::PI;

    const RATE: u32 = 48_000;
    /// A 20 ms ramp at 48 kHz.
    const RAMP: usize = 960;

    fn constant(frames: usize, left: f32, right: f32) -> Stereo {
        Stereo {
            samples: (0..frames).flat_map(|_| [left, right]).collect(),
            sample_rate: RATE,
        }
    }

    fn tone(freq: f64, frames: usize, amplitude: f64) -> Stereo {
        let s = |i: usize| (amplitude * (2.0 * PI * freq * i as f64 / RATE as f64).sin()) as f32;
        Stereo {
            samples: (0..frames).flat_map(|i| [s(i), s(i)]).collect(),
            sample_rate: RATE,
        }
    }

    fn lane(audio: &Stereo, crossovers: &[f64], offset: i64, fader: f32) -> Box<Lane> {
        Lane::new(audio, crossovers, offset, true, fader).unwrap()
    }

    fn engine() -> (Engine, EngineHandle) {
        Engine::new(RATE, 4, 64)
    }

    fn send(handle: &mut EngineHandle, command: Command) {
        assert!(handle.send(command).is_ok(), "queue full");
    }

    /// Processes `frames` frames in blocks of `block` and returns the interleaved output.
    fn run(engine: &mut Engine, frames: usize, block: usize) -> Vec<f32> {
        let mut all = Vec::new();
        let mut done = 0;
        while done < frames {
            let n = block.min(frames - done);
            let mut out = vec![9.0f32; n * 2];
            engine.process(&mut out);
            all.extend(out);
            done += n;
        }
        all
    }

    fn left(out: &[f32]) -> Vec<f32> {
        out.chunks(2).map(|f| f[0]).collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn it_is_silent_with_nothing_loaded_and_while_paused() {
        let (mut e, mut h) = engine();
        assert!(run(&mut e, 256, 64).iter().all(|v| *v == 0.0));

        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        assert!(run(&mut e, 256, 64).iter().all(|v| *v == 0.0)); // loaded but not playing
    }

    #[test]
    fn a_playing_single_band_track_comes_out_at_unity() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.25, -0.5), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let out = run(&mut e, 100, 32);
        assert!(out.chunks(2).all(|f| f == [0.25, -0.5]));
    }

    #[test]
    fn the_fader_scales_the_track() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.25, 0.25), &[], 0, 0.5),
            },
        );
        send(&mut h, Command::Play);
        assert!(run(&mut e, 10, 10).iter().all(|v| *v == 0.125));
    }

    #[test]
    fn a_fader_change_glides_linearly_over_20_ms() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(10_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(10_000, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        send(&mut h, Command::SetFader { lane: 0, gain: 0.0 });
        let out = left(&run(&mut e, RAMP + 10, 64));
        for (i, v) in out.iter().take(RAMP).enumerate() {
            let want = 0.25 * (1.0 - (i + 1) as f32 / RAMP as f32);
            assert!((v - want).abs() < 1e-5, "frame {i}: {v} vs {want}");
        }
        assert!(out[RAMP..].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn muting_a_band_silences_a_tone_in_it_after_the_ramp() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(96_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&tone(1_000.0, 96_000, 0.5), &[300.0, 4_000.0], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let before = left(&run(&mut e, 24_000, 128));
        let want = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (rms(&before[12_000..]) - want).abs() < 0.02 * want,
            "{}",
            rms(&before[12_000..])
        );

        send(
            &mut h,
            Command::SetBandMute {
                lane: 0,
                band: 1,
                mute: true,
            },
        );
        let after = left(&run(&mut e, 24_000, 128));
        assert!(
            rms(&after[12_000..]) < 0.03 * want,
            "{}",
            rms(&after[12_000..])
        );
    }

    #[test]
    fn muting_a_band_does_not_click() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(96_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&tone(1_000.0, 96_000, 0.5), &[300.0, 4_000.0], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        run(&mut e, 12_000, 128);
        send(
            &mut h,
            Command::SetBandMute {
                lane: 0,
                band: 1,
                mute: true,
            },
        );
        let out = left(&run(&mut e, 4_000, 128));
        // A 1 kHz tone at 0.5 amplitude steps at most 0.5 * 2*pi*1000/48000 = 0.0654 per sample.
        // An instant mute would jump by up to 0.5.
        let biggest_step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(biggest_step < 0.1, "step {biggest_step}");
    }

    #[test]
    fn soloing_the_high_band_drops_a_low_tone() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(96_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&tone(100.0, 96_000, 0.5), &[300.0, 4_000.0], 0, 1.0),
            },
        );
        send(
            &mut h,
            Command::SetBandSolo {
                lane: 0,
                band: 2,
                solo: true,
            },
        );
        send(&mut h, Command::Play);
        let out = left(&run(&mut e, 48_000, 128));
        assert!(rms(&out[24_000..]) < 0.01 * 0.5, "{}", rms(&out[24_000..]));
    }

    #[test]
    fn a_band_gain_command_scales_that_band() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(96_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&tone(1_000.0, 96_000, 0.5), &[300.0, 4_000.0], 0, 1.0),
            },
        );
        send(
            &mut h,
            Command::SetBandGain {
                lane: 0,
                band: 1,
                gain: 0.5,
            },
        );
        send(&mut h, Command::Play);
        let out = left(&run(&mut e, 48_000, 128));
        let want = 0.5 * 0.5 * std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (rms(&out[24_000..]) - want).abs() < 0.04 * want,
            "{}",
            rms(&out[24_000..])
        );
    }

    #[test]
    fn the_playhead_counts_frames_and_stops_at_the_end_marker() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        run(&mut e, 600, 100);
        assert_eq!(h.playhead(), 600);
        let out = run(&mut e, 600, 100);
        assert_eq!(h.playhead(), 1_000);
        assert!(out[400 * 2..].iter().all(|v| *v == 0.0)); // silent after frame 1000
        assert!(out[..400 * 2].iter().all(|v| *v == 0.25));
    }

    #[test]
    fn looping_wraps_the_playhead_and_keeps_playing() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(100));
        send(&mut h, Command::SetLooping(true));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(100, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let out = run(&mut e, 250, 64);
        assert_eq!(h.playhead(), 50);
        assert!(out.iter().all(|v| *v == 0.25));
    }

    #[test]
    fn seek_moves_the_playhead_and_pause_freezes_it() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        send(&mut h, Command::Seek(40));
        run(&mut e, 8, 8);
        assert_eq!(h.playhead(), 40);
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        run(&mut e, 10, 10);
        assert_eq!(h.playhead(), 50);
        send(&mut h, Command::Pause);
        run(&mut e, 10, 10);
        assert_eq!(h.playhead(), 50);
    }

    #[test]
    fn an_offset_delays_a_track() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.25, 0.25), &[], 10, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let out = left(&run(&mut e, 20, 20));
        assert!(out[..10].iter().all(|v| *v == 0.0));
        assert!(out[10..].iter().all(|v| *v == 0.25));
    }

    #[test]
    fn auditioning_a_lane_fades_the_others_out_and_back_in() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(10_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(10_000, 0.1, 0.1), &[], 0, 1.0),
            },
        );
        send(
            &mut h,
            Command::LoadLane {
                lane: 1,
                track: lane(&constant(10_000, 0.2, 0.2), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let both = left(&run(&mut e, 10, 10));
        assert!(both.iter().all(|v| (v - 0.3).abs() < 1e-6));

        send(&mut h, Command::Audition(Some(1)));
        let out = left(&run(&mut e, RAMP + 10, 64));
        assert!(out[RAMP..].iter().all(|v| (v - 0.2).abs() < 1e-6));
        assert!(out.windows(2).all(|w| w[1] <= w[0] + 1e-6)); // glides down, no jump

        send(&mut h, Command::Audition(None));
        let back = left(&run(&mut e, RAMP + 10, 64));
        assert!(back[RAMP..].iter().all(|v| (v - 0.3).abs() < 1e-6));
    }

    #[test]
    fn a_loud_sum_is_limited_with_linked_channels() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(1_000, 0.75, 0.25), &[], 0, 1.0),
            },
        );
        send(
            &mut h,
            Command::LoadLane {
                lane: 1,
                track: lane(&constant(1_000, 0.75, 0.25), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let out = run(&mut e, 10, 10);
        // (1.5, 0.5) scaled by 1/1.5 is (1.0, 0.3333)
        assert!(
            out.chunks(2)
                .all(|f| (f[0] - 1.0).abs() < 1e-6 && (f[1] - 1.0 / 3.0).abs() < 1e-6)
        );
    }

    #[test]
    fn replacing_or_clearing_a_lane_hands_the_old_one_back_instead_of_dropping_it() {
        let (mut e, mut h) = engine();
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(100, 0.1, 0.1), &[], 0, 1.0),
            },
        );
        run(&mut e, 8, 8);
        assert_eq!(h.collect_garbage(), 0);
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(100, 0.2, 0.2), &[], 0, 1.0),
            },
        );
        run(&mut e, 8, 8);
        assert_eq!(h.collect_garbage(), 1);
        send(&mut h, Command::ClearLane(0));
        run(&mut e, 8, 8);
        assert_eq!(h.collect_garbage(), 1);
    }

    #[test]
    fn a_lane_beyond_the_engines_capacity_is_refused_and_handed_back() {
        let (mut e, mut h) = engine(); // 4 lanes
        send(
            &mut h,
            Command::LoadLane {
                lane: 9,
                track: lane(&constant(100, 0.1, 0.1), &[], 0, 1.0),
            },
        );
        run(&mut e, 8, 8);
        assert_eq!(h.collect_garbage(), 1);
    }

    #[test]
    fn a_full_command_queue_refuses_instead_of_blocking() {
        let (_e, mut h) = Engine::new(RATE, 2, 4);
        for _ in 0..4 {
            assert!(h.send(Command::Play).is_ok());
        }
        assert!(matches!(h.send(Command::Pause), Err(Command::Pause)));
    }

    #[test]
    fn commands_for_lanes_that_do_not_exist_are_ignored() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetFader { lane: 3, gain: 0.5 });
        send(
            &mut h,
            Command::SetBandMute {
                lane: 99,
                band: 0,
                mute: true,
            },
        );
        send(&mut h, Command::SetOffset { lane: 2, offset: 5 });
        send(&mut h, Command::Play);
        assert!(run(&mut e, 32, 32).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_band_that_does_not_exist_is_ignored() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(100));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(100, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(
            &mut h,
            Command::SetBandMute {
                lane: 0,
                band: 5,
                mute: true,
            },
        );
        send(&mut h, Command::Play);
        assert!(run(&mut e, 10, 10).iter().all(|v| *v == 0.25));
    }

    #[test]
    fn the_block_size_does_not_change_the_output() {
        let build = || {
            let (e, mut h) = engine();
            send(&mut h, Command::SetEnd(20_000));
            send(
                &mut h,
                Command::LoadLane {
                    lane: 0,
                    track: lane(&tone(700.0, 20_000, 0.5), &[300.0, 4_000.0], 0, 0.8),
                },
            );
            send(&mut h, Command::Play);
            send(
                &mut h,
                Command::SetBandMute {
                    lane: 0,
                    band: 0,
                    mute: true,
                },
            );
            (e, h)
        };
        let (mut a, _ha) = build();
        let (mut b, _hb) = build();
        assert_eq!(run(&mut a, 4_096, 4_096), run(&mut b, 4_096, 64));
    }

    #[test]
    fn a_non_finite_fader_command_is_ignored() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(100));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(100, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(
            &mut h,
            Command::SetFader {
                lane: 0,
                gain: f32::NAN,
            },
        );
        send(&mut h, Command::Play);
        assert!(run(&mut e, 10, 10).iter().all(|v| *v == 0.25));
    }

    #[test]
    fn an_odd_length_output_buffer_leaves_its_last_sample_silent() {
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(100));
        send(
            &mut h,
            Command::LoadLane {
                lane: 0,
                track: lane(&constant(100, 0.25, 0.25), &[], 0, 1.0),
            },
        );
        send(&mut h, Command::Play);
        let mut out = [9.0f32; 5];
        e.process(&mut out);
        assert_eq!(out, [0.25, 0.25, 0.25, 0.25, 0.0]);
    }

    #[test]
    fn a_lane_with_unusable_crossovers_is_an_error_not_a_panic() {
        let audio = constant(100, 0.1, 0.1);
        assert!(Lane::new(&audio, &[4_000.0, 300.0], 0, true, 1.0).is_err());
    }

    #[test]
    fn a_lane_built_with_settings_starts_at_them_with_no_ramp() {
        use crate::mix::BandState;
        let open = BandState {
            gain: 1.0,
            mute: false,
            solo: false,
        };
        let muted = BandState { mute: true, ..open };
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(96_000));
        let track = Lane::with_settings(
            &tone(1_000.0, 96_000, 0.5),
            &[300.0, 4_000.0],
            0,
            true,
            0.5,
            &[open, muted, open],
        )
        .unwrap();
        send(&mut h, Command::LoadLane { lane: 0, track });
        send(&mut h, Command::Play);
        // From the very first frame the mid Band is silent: a 20 ms ramp would let it through.
        let out = left(&run(&mut e, 2_000, 128));
        assert!(
            rms(&out) < 0.03 * 0.5 * std::f64::consts::FRAC_1_SQRT_2 * 0.5,
            "{}",
            rms(&out)
        );
    }

    #[test]
    fn a_lane_built_with_settings_applies_the_fader_at_once() {
        use crate::mix::BandState;
        let open = BandState {
            gain: 1.0,
            mute: false,
            solo: false,
        };
        let (mut e, mut h) = engine();
        send(&mut h, Command::SetEnd(1_000));
        let track =
            Lane::with_settings(&constant(1_000, 0.25, 0.25), &[], 0, true, 0.5, &[open]).unwrap();
        send(&mut h, Command::LoadLane { lane: 0, track });
        send(&mut h, Command::Play);
        assert!(run(&mut e, 10, 10).iter().all(|v| *v == 0.125));
    }

    #[test]
    fn settings_for_the_wrong_number_of_bands_are_an_error() {
        use crate::mix::BandState;
        let open = BandState {
            gain: 1.0,
            mute: false,
            solo: false,
        };
        let audio = constant(100, 0.1, 0.1);
        assert!(Lane::with_settings(&audio, &[300.0], 0, true, 1.0, &[open]).is_err());
        assert!(Lane::with_settings(&audio, &[], 0, true, 1.0, &[open, open]).is_err());
    }
}
