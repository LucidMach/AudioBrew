//! The whole chain on real files and real threads: decode -> resample -> stretch -> lane -> engine.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use audiobrew::engine::{Command, Engine};
use audiobrew::pipeline::{FileLoader, Pipeline, ThreadExecutor};
use audiobrew::project::{LoadedTrack, Project};

const RATE: u32 = 48_000;

fn write_wav(path: &Path, channels: u16, rate: u32, samples: &[i16]) {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let block = channels * 2;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * block as u32).to_le_bytes());
    out.extend(block.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).unwrap();
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pipeline_files");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn track(path: &Path, bpm: f64, length_secs: f64) -> LoadedTrack {
    LoadedTrack {
        source: path.to_string_lossy().into_owned(),
        bpm: Some(bpm),
        key: None,
        length_secs,
        offset_beats: 0.0,
        ..Default::default()
    }
}

#[test]
fn two_real_files_become_one_playable_mashup_with_the_right_pitch_and_length() {
    // A: 1 s of silence, mono, 48 kHz, 120 BPM. It sets the Mashup tempo and length.
    let anchor = scratch("anchor.wav");
    write_wav(&anchor, 1, RATE, &vec![0i16; RATE as usize]);
    // B: 1 s of a 440 Hz tone, stereo, 22.05 kHz, 100 BPM. It must be resampled to 48 kHz,
    // sped up 1.2x (to 0.8333 s) and repeated to fill the 1 s Mashup, still at 440 Hz.
    let tone = scratch("tone.wav");
    let samples: Vec<i16> = (0..22_050)
        .flat_map(|i| {
            let v = (0.5 * (2.0 * PI * 440.0 * i as f64 / 22_050.0).sin() * 32767.0) as i16;
            [v, v]
        })
        .collect();
    write_wav(&tone, 2, 22_050, &samples);

    let mut project = Project::new();
    project.load_track(0, track(&anchor, 120.0, 1.0)).unwrap();
    project.load_track(1, track(&tone, 100.0, 1.0)).unwrap();

    let (mut engine, handle) = Engine::new(RATE, 4, 64);
    let mut pipeline = Pipeline::new(
        handle,
        RATE,
        4,
        Arc::new(FileLoader),
        Arc::new(ThreadExecutor),
    );
    pipeline.sync(&project);

    // Wait for the background threads, applying results as they arrive.
    let (id_a, id_b) = (project.lane_id(0).unwrap(), project.lane_id(1).unwrap());
    let deadline = Instant::now() + Duration::from_secs(20);
    while !(pipeline.is_loaded(id_a) && pipeline.is_loaded(id_b)) {
        pipeline.pump();
        assert!(pipeline.error(id_a).is_none(), "{:?}", pipeline.error(id_a));
        assert!(pipeline.error(id_b).is_none(), "{:?}", pipeline.error(id_b));
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the renders"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    pipeline.pump(); // flush the final commands

    pipeline.handle_mut().send(Command::Play).ok();
    let mut out = vec![0.0f32; 2 * 60_000];
    engine.process(&mut out);
    let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();

    // The Mashup is 1 s long: 48_000 frames, then silence.
    assert_eq!(pipeline.handle().playhead(), 48_000);
    assert!(left[48_000..].iter().all(|v| *v == 0.0));

    // Pitch survived the resample and the 1.2x stretch: 440 Hz, measured mid-Mashup.
    let mid = &left[12_000..36_000];
    let crossings = mid
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    let hz = crossings as f64 / 2.0 / (mid.len() as f64 / RATE as f64);
    assert!((hz - 440.0).abs() < 8.0, "{hz} Hz");

    // The tone was repeated to the End marker: still sounding near the very end.
    let tail_rms = (left[47_000..48_000]
        .iter()
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        / 1000.0)
        .sqrt();
    assert!(tail_rms > 0.2, "tail rms {tail_rms}");
}

#[test]
fn a_missing_file_is_reported_on_its_lane_and_does_not_hang() {
    let mut project = Project::new();
    project
        .load_track(0, track(&scratch("does_not_exist.wav"), 120.0, 1.0))
        .unwrap();
    let id = project.lane_id(0).unwrap();

    let (_engine, handle) = Engine::new(RATE, 4, 64);
    let mut pipeline = Pipeline::new(
        handle,
        RATE,
        4,
        Arc::new(FileLoader),
        Arc::new(ThreadExecutor),
    );
    pipeline.sync(&project);

    let deadline = Instant::now() + Duration::from_secs(10);
    while pipeline.error(id).is_none() {
        pipeline.pump();
        assert!(Instant::now() < deadline, "no error was reported");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!pipeline.is_loaded(id));
}
