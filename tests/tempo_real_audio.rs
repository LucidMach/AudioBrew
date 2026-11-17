//! Scores the tempo estimator against real loops with human-annotated BPM
//! (the Freesound Loop Dataset subset listed in `testdata/freesound-loops/manifest.csv`).
//! The audio is gitignored, so the test skips itself when the files are missing.

use std::path::Path;

use audiobrew::analysis::estimate_tempo;

const DIR: &str = "testdata/freesound-loops";

struct Loop {
    file: String,
    bpm: f64,
}

fn manifest() -> Vec<Loop> {
    let text = std::fs::read_to_string(Path::new(DIR).join("manifest.csv")).unwrap_or_default();
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split(',');
            Some(Loop {
                file: cols.next()?.to_string(),
                bpm: cols.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// Reads a WAV as mono f32 by averaging channels.
fn read_mono(path: &Path) -> Option<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / scale)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
    };
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Some((mono, spec.sample_rate))
}

/// Within 4% of the annotated tempo (the standard "accuracy 1" tolerance).
fn exact(estimate: f64, truth: f64) -> bool {
    (estimate - truth).abs() <= 0.04 * truth
}

/// Exact, or the same pulse at half, double, a third or triple time ("accuracy 2").
fn up_to_octave(estimate: f64, truth: f64) -> bool {
    [1.0, 0.5, 2.0, 1.0 / 3.0, 3.0]
        .iter()
        .any(|k| exact(estimate, truth * k))
}

/// Returns `(exact_rate, octave_rate)` over the loops, or `None` if the audio is not present.
fn score_real_loops() -> Option<(f64, f64)> {
    let loops = manifest();
    if loops.is_empty() || !Path::new(DIR).join(&loops[0].file).exists() {
        eprintln!("skipping: {DIR} audio not present");
        return None;
    }

    let (mut exact_hits, mut octave_hits, mut total) = (0, 0, 0);
    for l in &loops {
        let Some((samples, rate)) = read_mono(&Path::new(DIR).join(&l.file)) else {
            continue;
        };
        total += 1;
        let estimate = estimate_tempo(&samples, rate);
        let (e, o) = match estimate {
            Some(t) => (exact(t, l.bpm), up_to_octave(t, l.bpm)),
            None => (false, false),
        };
        exact_hits += e as usize;
        octave_hits += o as usize;
        eprintln!(
            "{:>12} truth {:>5.1}  estimate {:>7.2?}  exact={e} octave={o}",
            l.file, l.bpm, estimate
        );
    }

    let exact_rate = exact_hits as f64 / total as f64;
    let octave_rate = octave_hits as f64 / total as f64;
    eprintln!(
        "exact {exact_hits}/{total} = {exact_rate:.2}, octave {octave_hits}/{total} = {octave_rate:.2}"
    );
    assert!(total >= 30, "expected to read most loops, read {total}");
    Some((exact_rate, octave_rate))
}

/// Guards the level reached so far (62% exact, 93% allowing half/double time).
#[test]
fn estimator_keeps_its_accuracy_on_real_loops() {
    let Some((exact_rate, octave_rate)) = score_real_loops() else {
        return;
    };
    assert!(
        exact_rate >= 0.60,
        "within 4% on only {exact_rate:.2} of loops"
    );
    assert!(
        octave_rate >= 0.90,
        "within an octave on only {octave_rate:.2} of loops"
    );
}

/// The goal. Remaining misses are mostly half-time readings of fast loops (kick-snare patterns
/// look identical at the beat and at half the beat), which tuning has not resolved.
#[test]
#[ignore = "goal not yet met: 70% within 4%"]
fn estimator_reaches_70_percent_exact_on_real_loops() {
    let Some((exact_rate, _)) = score_real_loops() else {
        return;
    };
    assert!(
        exact_rate >= 0.70,
        "within 4% on only {exact_rate:.2} of loops"
    );
}

/// Distance from `t` to the nearest multiple of `period`, in seconds.
fn distance_to_grid(t: f64, period: f64) -> f64 {
    let r = t.rem_euclid(period);
    r.min(period - r)
}

/// "Well cut" loops start on beat 1 of a bar, so on loops whose tempo is found, the first beat
/// should sit on the grid origin and the downbeat should be on a bar line that starts at 0.
/// Returns `(beat_rate, downbeat_rate, evaluated)` or `None` if the audio is not present.
fn score_beat_grid() -> Option<(f64, f64, usize)> {
    let loops = manifest();
    if loops.is_empty() || !Path::new(DIR).join(&loops[0].file).exists() {
        eprintln!("skipping: {DIR} audio not present");
        return None;
    }
    let (mut beat_ok, mut down_ok, mut evaluated) = (0, 0, 0);
    for l in &loops {
        let Some((samples, rate)) = read_mono(&Path::new(DIR).join(&l.file)) else {
            continue;
        };
        let Some(beats) = audiobrew::analysis::track_beats(&samples, rate) else {
            continue;
        };
        if !exact(beats.bpm, l.bpm) || beats.times_secs.len() <= beats.downbeat_offset {
            continue; // only judge the grid where the tempo itself was right
        }
        evaluated += 1;
        let beat = 60.0 / beats.bpm;
        let first_beat_err = distance_to_grid(beats.times_secs[0], beat);
        let downbeat_err = distance_to_grid(beats.times_secs[beats.downbeat_offset], 4.0 * beat);
        let (b, d) = (first_beat_err <= 0.15 * beat, downbeat_err <= 0.15 * beat);
        beat_ok += b as usize;
        down_ok += d as usize;
        eprintln!(
            "{:>12} bpm {:>6.1}  first beat off {:>5.0} ms  downbeat off {:>5.0} ms  beat={b} down={d}",
            l.file,
            beats.bpm,
            first_beat_err * 1000.0,
            downbeat_err * 1000.0
        );
    }
    let n = evaluated.max(1) as f64;
    eprintln!("beat grid {beat_ok}/{evaluated}, downbeat {down_ok}/{evaluated}");
    Some((beat_ok as f64 / n, down_ok as f64 / n, evaluated))
}

#[test]
fn beat_grid_lines_up_with_the_start_of_well_cut_loops() {
    let Some((beat_rate, downbeat_rate, evaluated)) = score_beat_grid() else {
        return;
    };
    assert!(
        evaluated >= 15,
        "only {evaluated} loops had the right tempo to judge"
    );
    assert!(
        beat_rate >= 0.7,
        "first beat on the grid for only {beat_rate:.2} of loops"
    );
    assert!(
        downbeat_rate >= 0.5,
        "downbeat on a bar line for only {downbeat_rate:.2} of loops"
    );
}
