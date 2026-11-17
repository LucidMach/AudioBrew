use crate::analysis::track_beats;
use crate::first_run::TrackInput;
use crate::import::Stereo;

/// Averages the left and right channels into one, which is what the analysers work on.
pub fn to_mono(audio: &Stereo) -> Vec<f32> {
    audio
        .samples
        .chunks_exact(2)
        .map(|frame| 0.5 * (frame[0] + frame[1]))
        .collect()
}

/// Runs tempo and beat detection on a loaded file and returns what `first_run` needs.
/// With no detectable pulse, `bpm` is `None` and the downbeat is at the start.
pub fn detect(audio: &Stereo) -> TrackInput {
    let mono = to_mono(audio);
    let length_secs = mono.len() as f64 / audio.sample_rate.max(1) as f64;

    match track_beats(&mono, audio.sample_rate) {
        Some(beats) => TrackInput {
            bpm: Some(beats.bpm),
            length_secs,
            downbeat_secs: beats
                .times_secs
                .get(beats.downbeat_offset)
                .copied()
                .unwrap_or(0.0),
        },
        None => TrackInput {
            bpm: None,
            length_secs,
            downbeat_secs: 0.0,
        },
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
        Stereo {
            samples,
            sample_rate: RATE,
        }
    }

    #[test]
    fn silence_has_no_tempo_so_the_track_is_flagged_not_guessed() {
        let silence = Stereo {
            samples: vec![0.0; 2 * RATE as usize * 8],
            sample_rate: RATE,
        };

        let input = detect(&silence);

        assert_eq!(input.bpm, None);
        assert_eq!(input.downbeat_secs, 0.0);
        assert!((input.length_secs - 8.0).abs() < 1e-6);
    }

    #[test]
    fn two_dropped_files_become_a_tempo_matched_downbeat_aligned_mashup() {
        use crate::first_run::plan_first_run;
        use crate::grid::beats_to_secs;

        // Track 1: 100 BPM, first click at 0.25 s. Track 2: 120 BPM, first click at 0.10 s.
        let tracks = [
            stereo_clicks(100.0, 16.0, 0.25),
            stereo_clicks(120.0, 16.0, 0.10),
        ];

        let inputs: Vec<_> = tracks.iter().map(detect).collect();
        let plan = plan_first_run(&inputs).expect("two files make a plan");

        // The Mashup takes the first Track's tempo; the second slows by 100/120.
        let mashup = plan.mashup_bpm.expect("a tempo was detected");
        assert!((mashup - 100.0).abs() <= 1.0, "mashup bpm {mashup}");
        assert!(
            (plan.tracks[1].factor - 100.0 / 120.0).abs() < 0.02,
            "factor {}",
            plan.tracks[1].factor
        );
        assert!(!plan.tracks[1].warn);

        // After stretching and offsetting, Track 2's first click (true time 0.10 s) must land on
        // one of Track 1's bar lines (its first click is at 0.25 s; a bar is 4 beats = 2.4 s).
        let landed =
            beats_to_secs(plan.tracks[1].offset_beats, mashup) + 0.10 / plan.tracks[1].factor;
        let bar = 4.0 * 60.0 / 100.0;
        let off = (landed - 0.25).rem_euclid(bar);
        assert!(
            off.min(bar - off) <= 0.02,
            "downbeats are {:.0} ms apart",
            off.min(bar - off) * 1000.0
        );
    }

    #[test]
    fn detection_reports_tempo_length_and_downbeat_of_a_stereo_file() {
        let audio = stereo_clicks(120.0, 12.0, 0.25);

        let input = detect(&audio);

        let bpm = input.bpm.expect("a steady pulse has a tempo");
        assert!((bpm - 120.0).abs() <= 1.0, "bpm {bpm}");
        assert!(
            (input.length_secs - 12.0).abs() < 1e-6,
            "length {}",
            input.length_secs
        );
        assert!(
            (input.downbeat_secs - 0.25).abs() <= 0.015,
            "downbeat {}",
            input.downbeat_secs
        );
    }
}
