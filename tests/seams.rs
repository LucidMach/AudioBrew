//! Tests that cross module boundaries, using only the public API.

use audiobrew::crossover::BandSplitter;
use audiobrew::first_run::{TrackInput, plan_first_run};
use audiobrew::grid::beats_to_secs;
use audiobrew::history::History;
use audiobrew::mix::{BandState, effective_gains, limit};
use audiobrew::project::{LoadedTrack, Project};
use audiobrew::render::{RenderTrack, mix_block, render_mashup};
use audiobrew::transport::{Transport, secs_to_frames};

const FS: f64 = 48_000.0;

/// RMS of a unit sine at `freq` after it is split into Bands, scaled by the Band gains the
/// mute/solo rules give, summed and limited. The input sine's own RMS is 1/sqrt 2.
fn rms_through_bands(crossovers: &[f64], bands: &[BandState], freq: f64) -> f64 {
    let mut split = BandSplitter::new(FS, crossovers).unwrap();
    let gains = effective_gains(bands);
    let mut out = vec![0.0f32; split.band_count()];
    let total = FS as usize;
    let mut sq = 0.0;
    for i in 0..total {
        let x = (2.0 * std::f64::consts::PI * freq * i as f64 / FS).sin() as f32;
        split.process(x, &mut out);
        let mixed = limit(out.iter().zip(&gains).map(|(b, g)| b * g).sum());
        if i >= total / 2 {
            sq += (mixed as f64).powi(2);
        }
    }
    (sq / (total / 2) as f64).sqrt()
}

const SINE_RMS: f64 = std::f64::consts::FRAC_1_SQRT_2;

fn open(gain: f32) -> BandState {
    BandState {
        gain,
        mute: false,
        solo: false,
    }
}

#[test]
fn muting_the_mid_band_removes_a_mid_range_tone() {
    let mut bands = [open(1.0), open(1.0), open(1.0)];
    bands[1].mute = true;
    // 1 kHz sits in 300 Hz..4 kHz; LR4 leaks (0.3)^4 and (0.25)^4 from its neighbours.
    assert!(rms_through_bands(&[300.0, 4_000.0], &bands, 1_000.0) < 0.03 * SINE_RMS);
}

#[test]
fn muting_the_low_band_leaves_a_mid_range_tone_intact() {
    let mut bands = [open(1.0), open(1.0), open(1.0)];
    bands[0].mute = true;
    assert!(rms_through_bands(&[300.0, 4_000.0], &bands, 1_000.0) > 0.97 * SINE_RMS);
}

#[test]
fn soloing_the_high_band_keeps_the_treble_and_drops_the_bass() {
    let mut bands = [open(1.0), open(1.0), open(1.0)];
    bands[2].solo = true;
    let xo = [300.0, 4_000.0];
    assert!(rms_through_bands(&xo, &bands, 10_000.0) > 0.97 * SINE_RMS);
    assert!(rms_through_bands(&xo, &bands, 100.0) < 0.01 * SINE_RMS);
}

#[test]
fn a_full_scale_tone_through_every_band_never_exceeds_full_scale() {
    let mut split = BandSplitter::new(FS, &[100.0, 300.0, 1_000.0, 3_000.0, 8_000.0]).unwrap();
    let gains = effective_gains(&[open(1.0); 6]);
    let mut out = [0.0f32; 6];
    for i in 0..48_000 {
        let x = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / FS).sin() as f32;
        split.process(x, &mut out);
        let v = limit(out.iter().zip(&gains).map(|(b, g)| b * g).sum());
        assert!(v.is_finite() && v.abs() <= 1.0);
    }
}

#[test]
fn the_first_run_plan_drives_the_renderer() {
    // Control rate of 100 frames per second keeps the buffers small.
    let rate = 100.0;
    let plan = plan_first_run(&[
        TrackInput {
            bpm: Some(120.0),
            length_secs: 8.0,
            downbeat_secs: 0.5,
        },
        TrackInput {
            bpm: Some(100.0),
            length_secs: 4.0,
            downbeat_secs: 0.3,
        },
    ])
    .unwrap();
    let bpm = plan.mashup_bpm.unwrap();
    let end = secs_to_frames(plan.end_secs, rate);
    assert_eq!(end, 800);

    // Buffers are already stretched: A is 800 frames, B is 4 s / 1.2 = 333 frames.
    let a = vec![0.25f32; 800];
    let b = vec![0.125f32; 333];
    let offset_b = secs_to_frames(beats_to_secs(plan.tracks[1].offset_beats, bpm), rate);
    assert_eq!(offset_b, 25); // 0.5 beat at 120 BPM = 0.25 s

    let tracks = [
        RenderTrack {
            samples: &a,
            offset: 0,
            repeats: plan.tracks[0].repeats,
            fader: 1.0,
        },
        RenderTrack {
            samples: &b,
            offset: offset_b,
            repeats: plan.tracks[1].repeats,
            fader: 1.0,
        },
    ];
    let mix = render_mashup(&tracks, end, None);

    assert_eq!(mix[24], 0.25); // B has not started yet
    assert_eq!(mix[25], 0.375); // B's downbeat meets A's
    assert_eq!(mix[799], 0.375); // B repeated all the way to the End marker
}

#[test]
fn a_looping_transport_drives_the_renderer_without_a_gap_at_the_seam() {
    // A ramp 0.0, 0.1 .. 0.9 on a 10-frame Mashup; blocks of 4 straddle the End marker.
    let ramp: Vec<f32> = (0..10).map(|n| n as f32 * 0.1).collect();
    let tracks = [RenderTrack {
        samples: &ramp,
        offset: 0,
        repeats: false,
        fader: 1.0,
    }];
    let mut transport = Transport::new(10);
    transport.set_looping(true);
    transport.play();

    let mut heard = Vec::new();
    for _ in 0..7 {
        let mut block = [9.0f32; 4];
        mix_block(&mut block, transport.position(), 10, true, &tracks, None);
        heard.extend_from_slice(&block);
        transport.advance(4);
    }

    for (n, v) in heard.iter().enumerate() {
        let want = (n % 10) as f32 * 0.1;
        assert!((v - want).abs() < 1e-6, "frame {n}: {v} vs {want}");
    }
}

#[test]
fn without_looping_a_block_past_the_end_marker_is_silent_after_it() {
    let ramp: Vec<f32> = (0..10).map(|n| n as f32 * 0.1 + 0.1).collect();
    let tracks = [RenderTrack {
        samples: &ramp,
        offset: 0,
        repeats: false,
        fader: 1.0,
    }];
    let mut block = [9.0f32; 4];
    mix_block(&mut block, 8, 10, false, &tracks, None);
    assert!((block[0] - 0.9).abs() < 1e-6 && (block[1] - 1.0).abs() < 1e-6);
    assert_eq!(&block[2..], &[0.0, 0.0]);
}

#[test]
fn dragging_an_offset_through_history_is_one_undo_step() {
    let mut project = Project::new();
    project
        .load_track(
            0,
            LoadedTrack {
                source: "a.wav".into(),
                bpm: Some(120.0),
                key: None,
                length_secs: 8.0,
                offset_beats: 0.0,
            },
        )
        .unwrap();
    let mut history = History::new(project);

    history.begin_gesture();
    for step in 1..=60 {
        let mut next = history.current().clone();
        let mut track = next.track(0).unwrap().clone();
        track.offset_beats = step as f64 * 0.05;
        next.load_track(0, track).unwrap();
        history.update(next);
    }
    history.commit();

    assert!((history.current().track(0).unwrap().offset_beats - 3.0).abs() < 1e-9);
    history.undo();
    assert_eq!(history.current().track(0).unwrap().offset_beats, 0.0);
    assert!(!history.can_undo());
}

#[test]
fn soloing_a_band_on_one_track_does_not_silence_the_other_track() {
    // Band solo is scoped to its own Track: each Track has its own Band states.
    let xo = [300.0, 4_000.0];
    let mut track_a = [open(1.0), open(1.0), open(1.0)];
    track_a[2].solo = true; // A: only the high Band
    let track_b = [open(1.0), open(1.0), open(1.0)]; // B: untouched

    // A 100 Hz tone is in the low Band: silenced on A, fully audible on B.
    assert!(rms_through_bands(&xo, &track_a, 100.0) < 0.01 * SINE_RMS);
    assert!(rms_through_bands(&xo, &track_b, 100.0) > 0.97 * SINE_RMS);
}
