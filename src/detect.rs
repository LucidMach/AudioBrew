use crate::analysis::track_beats;
use crate::first_run::TrackInput;
use crate::import::Stereo;

/// Runs tempo and beat detection on a loaded file and returns what `first_run` needs.
/// With no detectable pulse, `bpm` is `None` and the downbeat is at the start.
pub fn detect(audio: &Stereo) -> TrackInput {
    let mono: Vec<f32> = audio
        .samples
        .chunks_exact(2)
        .map(|frame| 0.5 * (frame[0] + frame[1]))
        .collect();
    let length_secs = mono.len() as f64 / audio.sample_rate.max(1) as f64;

    match track_beats(&mono, audio.sample_rate) {
        Some(beats) => TrackInput {
            bpm: Some(beats.bpm),
            length_secs,
            downbeat_secs: beats.times_secs.get(beats.downbeat_offset).copied().unwrap_or(0.0),
        },
        None => TrackInput { bpm: None, length_secs, downbeat_secs: 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

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
        Stereo { samples, sample_rate: RATE }
    }

    #[test]
    fn detection_reports_tempo_length_and_downbeat_of_a_stereo_file() {
        let audio = stereo_clicks(120.0, 12.0, 0.25);

        let input = detect(&audio);

        let bpm = input.bpm.expect("a steady pulse has a tempo");
        assert!((bpm - 120.0).abs() <= 1.0, "bpm {bpm}");
        assert!((input.length_secs - 12.0).abs() < 1e-6, "length {}", input.length_secs);
        assert!((input.downbeat_secs - 0.25).abs() <= 0.015, "downbeat {}", input.downbeat_secs);
    }
}
