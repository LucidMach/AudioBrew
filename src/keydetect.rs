use crate::key::Key;

/// The Camelot key for a tonic (`pitch_class`: 0 = C, 1 = C#, ... 11 = B) and mode.
pub fn camelot(pitch_class: usize, minor: bool) -> Key {
    // A minor key shares its Camelot number with its relative major, three semitones up.
    let major_tonic = if minor {
        (pitch_class + 3) % 12
    } else {
        pitch_class % 12
    };
    // C major is 8B, and each step of a fifth (7 semitones) moves one number round the wheel.
    let number = (8 - 1 + major_tonic * 7 % 12) % 12 + 1;
    Key {
        number: number as u8,
        minor,
    }
}

/// Estimates the musical key of mono `samples`, or `None` if there is no tonal content.
pub fn detect_key(samples: &[f32], sample_rate: u32) -> Option<Key> {
    let chroma = chromagram(samples, sample_rate)?;
    let best = (0..12)
        .flat_map(|tonic| [(tonic, false), (tonic, true)])
        .map(|(tonic, minor)| {
            let profile = if minor {
                &MINOR_PROFILE
            } else {
                &MAJOR_PROFILE
            };
            // The profile is written for tonic C; rotate it so index `tonic` is the tonic.
            let rotated: Vec<f64> = (0..12).map(|pc| profile[(pc + 12 - tonic) % 12]).collect();
            ((tonic, minor), correlation(&chroma, &rotated))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let ((tonic, minor), score) = best;
    (score > 0.0).then(|| camelot(tonic, minor))
}

/// Krumhansl-Kessler key profiles: how strongly each pitch class, counted from the tonic,
/// is heard in a major or minor key (probe-tone ratings, Krumhansl & Kessler 1982).
const MAJOR_PROFILE: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// FFT window for pitch analysis: long (about 370 ms) so low notes resolve into the right
/// semitone, since a semitone near 100 Hz is only about 6 Hz wide.
const KEY_FFT_SIZE: usize = 16_384;
const KEY_HOP: usize = 8_192;
/// Spectral magnitudes are raised to this power before summing, so a few loud partials do not
/// outweigh many quiet ones (1.0 = no compression).
const MAGNITUDE_EXPONENT: f64 = 2.0;
/// Frequency range, in Hz, whose energy counts towards pitch classes.
const PITCH_RANGE_HZ: (f64, f64) = (65.0, 2_100.0);

/// Tuning offsets tried, in semitones (0.2 = 20 cents sharp of A = 440 Hz).
const TUNING_OFFSETS: [f64; 5] = [-0.4, -0.2, 0.0, 0.2, 0.4];

/// Total spectral magnitude in each of the 12 pitch classes (0 = C), or `None` if the audio is
/// too short or silent. The audio may be tuned a little off A = 440 Hz, so the tuning offset that
/// concentrates the energy into the fewest pitch classes is used.
fn chromagram(samples: &[f32], sample_rate: u32) -> Option<[f64; 12]> {
    use realfft::RealFftPlanner;

    if samples.len() < KEY_FFT_SIZE {
        return None;
    }
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(KEY_FFT_SIZE);
    let window: Vec<f32> = (0..KEY_FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / KEY_FFT_SIZE as f32).cos())
        .collect();
    let mut input = vec![0.0f32; KEY_FFT_SIZE];
    let mut spectrum = fft.make_output_vec();

    // Energy in each FFT bin, summed over all frames.
    let mut bin_energy = vec![0.0f64; spectrum.len()];
    for start in (0..=samples.len() - KEY_FFT_SIZE).step_by(KEY_HOP) {
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = samples[start + i] * window[i];
        }
        fft.process(&mut input, &mut spectrum)
            .expect("buffer lengths match the planned FFT");
        for (bin, value) in spectrum.iter().enumerate() {
            bin_energy[bin] += (value.norm() as f64).powf(MAGNITUDE_EXPONENT);
        }
    }

    let bin_hz = sample_rate as f64 / KEY_FFT_SIZE as f64;
    let fold = |tuning: f64| {
        let mut chroma = [0.0f64; 12];
        for (bin, energy) in bin_energy.iter().enumerate() {
            let hz = bin as f64 * bin_hz;
            if (PITCH_RANGE_HZ.0..=PITCH_RANGE_HZ.1).contains(&hz) {
                let midi = 69.0 + 12.0 * (hz / 440.0).log2() - tuning;
                chroma[(midi.round() as i64).rem_euclid(12) as usize] += energy;
            }
        }
        chroma
    };
    // Concentration: the sum of squares is largest when the energy sits in few pitch classes.
    let concentration = |c: &[f64; 12]| {
        let total: f64 = c.iter().sum();
        if total <= 0.0 { 0.0 } else { c.iter().map(|v| (v / total).powi(2)).sum::<f64>() }
    };
    let best = TUNING_OFFSETS
        .iter()
        .map(|&t| fold(t))
        .max_by(|a, b| concentration(a).total_cmp(&concentration(b)))?;
    (best.iter().sum::<f64>() > 1e-6).then_some(best)
}

/// Pearson correlation of two equal-length vectors (0 if either is constant).
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        cov += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    if va <= 0.0 || vb <= 0.0 {
        0.0
    } else {
        cov / (va * vb).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(number: u8, minor: bool) -> Key {
        Key { number, minor }
    }

    const RATE: u32 = 44_100;

    /// A chord of sine tones with a few harmonics, `secs` long, for MIDI notes `notes`.
    fn chord(notes: &[i32], secs: f64) -> Vec<f32> {
        let frames = (secs * RATE as f64) as usize;
        let mut out = vec![0.0f32; frames];
        for &note in notes {
            let freq = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
            for harmonic in 1..=4 {
                let amp = 0.15 / harmonic as f64;
                for (i, sample) in out.iter_mut().enumerate() {
                    let t = i as f64 / RATE as f64;
                    *sample +=
                        (amp * (std::f64::consts::TAU * freq * harmonic as f64 * t).sin()) as f32;
                }
            }
        }
        out
    }

    /// Four chords (I-IV-V-I, or i-iv-V-i) in the key of `tonic` (0 = C), 2.5 s each.
    fn progression(tonic: i32, minor: bool) -> Vec<f32> {
        let root = 48 + tonic; // octave 3
        let third = if minor { 3 } else { 4 };
        let sixth = if minor { 8 } else { 9 }; // fourth chord's third: iv has a flat 6th in minor
        let triads: [[i32; 3]; 4] = [
            [0, third, 7],
            [5, sixth, 12],
            [7, 11, 14], // V, major in both modes
            [0, third, 7],
        ];
        triads
            .iter()
            .flat_map(|t| chord(&t.map(|n| root + n), 2.5))
            .collect()
    }

    #[test]
    fn every_major_and_minor_key_is_recognised_from_its_progression() {
        let mut wrong = Vec::new();
        for tonic in 0..12 {
            for minor in [false, true] {
                let samples = progression(tonic, minor);
                let want = camelot(tonic as usize, minor);
                let got = detect_key(&samples, RATE);
                if got != Some(want) {
                    wrong.push(format!(
                        "tonic {tonic} minor={minor}: wanted {want:?}, got {got:?}"
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of 24 keys wrong:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn silence_and_audio_too_short_to_analyse_have_no_key() {
        assert_eq!(detect_key(&vec![0.0; RATE as usize * 5], RATE), None);
        assert_eq!(detect_key(&chord(&[60, 64, 67], 0.1), RATE), None);
        assert_eq!(detect_key(&[], RATE), None);
    }

    #[test]
    fn an_a_minor_progression_is_8a() {
        let samples = progression(9, true);

        assert_eq!(detect_key(&samples, RATE), Some(key(8, true)));
    }

    #[test]
    fn a_c_major_progression_is_8b() {
        let samples = progression(0, false);

        assert_eq!(detect_key(&samples, RATE), Some(key(8, false)));
    }

    #[test]
    fn pitch_classes_map_onto_the_camelot_wheel() {
        // Known pairs from the Camelot wheel (pitch class: C=0, C#=1, D=2, D#=3, E=4, F=5,
        // F#=6, G=7, G#=8, A=9, A#=10, B=11).
        let known = [
            (9, true, 8, true),    // A minor  = 8A
            (0, false, 8, false),  // C major  = 8B (relative of A minor)
            (4, true, 9, true),    // E minor  = 9A
            (7, false, 9, false),  // G major  = 9B
            (6, true, 11, true),   // F# minor = 11A
            (8, true, 1, true),    // G# minor = 1A
            (11, false, 1, false), // B major  = 1B
            (3, false, 5, false),  // Eb major = 5B
            (0, true, 5, true),    // C minor  = 5A
            (5, false, 7, false),  // F major  = 7B
            (1, true, 12, true),   // C# minor = 12A
            (4, false, 12, false), // E major  = 12B
        ];
        for (pc, minor, number, wheel_minor) in known {
            assert_eq!(
                camelot(pc, minor),
                key(number, wheel_minor),
                "pitch class {pc} minor={minor}"
            );
        }
    }
}
