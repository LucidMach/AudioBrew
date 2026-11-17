//! Scores key detection against real loops whose key was labelled by human annotators
//! (the Freesound Loop Dataset subset in `testdata/freesound-keys/manifest.csv`).
//! The audio is gitignored, so the test skips itself when the files are missing.

use std::path::Path;

use audiobrew::keydetect::detect_key;

const DIR: &str = "testdata/freesound-keys";

struct Loop {
    file: String,
    tonic_pc: usize,
    minor: bool,
}

fn manifest() -> Vec<Loop> {
    let text = std::fs::read_to_string(Path::new(DIR).join("manifest.csv")).unwrap_or_default();
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split(',');
            Some(Loop {
                file: cols.next()?.to_string(),
                tonic_pc: cols.next()?.parse().ok()?,
                minor: cols.next()? == "1",
            })
        })
        .collect()
}

fn read_mono(path: &Path) -> Option<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 / scale).collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
    };
    let mono = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Some((mono, spec.sample_rate))
}

/// The tonic (0 = C) of a Camelot key. Inverse of `keydetect::camelot`, written out here from
/// the wheel's own rule so the test does not lean on the code under test: C major is 8B, each
/// step of a fifth moves one number, and a minor key shares its number with the major key
/// three semitones above its tonic.
fn tonic_of(number: u8, minor: bool) -> usize {
    let major_tonic = (0..12).find(|pc| (8 - 1 + pc * 7 % 12) % 12 + 1 == number as usize).unwrap();
    if minor { (major_tonic + 9) % 12 } else { major_tonic }
}

#[derive(Default, Debug)]
struct Tally {
    exact: usize,
    fifth: usize,
    relative: usize,
    parallel: usize,
    other: usize,
    none: usize,
}

fn score() -> Option<(Tally, usize)> {
    let loops = manifest();
    if loops.is_empty() || !Path::new(DIR).join(&loops[0].file).exists() {
        eprintln!("skipping: {DIR} audio not present");
        return None;
    }
    let mut t = Tally::default();
    let mut total = 0;
    for l in &loops {
        let Some((samples, rate)) = read_mono(&Path::new(DIR).join(&l.file)) else {
            continue;
        };
        total += 1;
        let Some(found) = detect_key(&samples, rate) else {
            t.none += 1;
            continue;
        };
        let (pc, minor) = (tonic_of(found.number, found.minor), found.minor);
        let up_fifth = (l.tonic_pc + 7) % 12;
        let down_fifth = (l.tonic_pc + 5) % 12;
        let relative = if l.minor { (l.tonic_pc + 3) % 12 } else { (l.tonic_pc + 9) % 12 };
        let class = if pc == l.tonic_pc && minor == l.minor {
            t.exact += 1;
            "exact"
        } else if minor == l.minor && (pc == up_fifth || pc == down_fifth) {
            t.fifth += 1;
            "fifth"
        } else if minor != l.minor && pc == relative {
            t.relative += 1;
            "relative"
        } else if minor != l.minor && pc == l.tonic_pc {
            t.parallel += 1;
            "parallel"
        } else {
            t.other += 1;
            "other"
        };
        eprintln!(
            "{:>11} truth pc {:>2} {}  detected pc {:>2} {}  {class}",
            l.file, l.tonic_pc, if l.minor { "min" } else { "maj" }, pc, if minor { "min" } else { "maj" }
        );
    }
    eprintln!("{t:?} of {total}");
    Some((t, total))
}

/// Guards the level reached so far: 54% exact, 78% exact-or-neighbouring.
#[test]
fn key_detection_keeps_its_accuracy_on_real_loops() {
    let Some((t, total)) = score() else { return };
    assert!(total >= 40, "expected to read most loops, read {total}");
    let exact = t.exact as f64 / total as f64;
    assert!(exact >= 0.5, "exact key on only {exact:.2} of loops: {t:?}");
    // A fifth or relative-key error still lands on a Camelot-neighbouring key.
    let close = (t.exact + t.fifth + t.relative) as f64 / total as f64;
    assert!(close >= 0.7, "exact, fifth or relative on only {close:.2} of loops: {t:?}");
}
