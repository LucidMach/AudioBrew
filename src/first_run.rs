use crate::grid::{BEATS_PER_BAR, beats_to_secs, secs_to_beats};
use crate::tempo::{BpmFix, speed_factor, suggest_bpm_fix, within_stretch_limits};

/// What detection found in one loaded file. Lengths and times are in source seconds.
pub struct TrackInput {
    pub bpm: Option<f64>,
    pub length_secs: f64,
    pub downbeat_secs: f64,
}

/// How one Track is placed in the first Mashup.
#[derive(Debug)]
pub struct TrackPlan {
    /// Speed factor after any auto-applied halve/double and the stretch cap.
    pub factor: f64,
    /// The halve/double that was applied automatically, if any.
    pub fix: Option<BpmFix>,
    /// True when the Track could not be brought into the stretch limits, or has no tempo.
    pub warn: bool,
    /// Where the Track starts, in beats on the Mashup grid, never negative.
    pub offset_beats: f64,
    /// True when the Track ends before the End marker and so repeats to fill.
    pub repeats: bool,
}

#[derive(Debug)]
pub struct FirstRunPlan {
    /// `None` when no Track has a detected tempo.
    pub mashup_bpm: Option<f64>,
    pub tracks: Vec<TrackPlan>,
    pub end_secs: f64,
}

/// Turns freshly loaded Tracks into a playable Mashup with no user decisions.
pub fn plan_first_run(tracks: &[TrackInput]) -> Option<FirstRunPlan> {
    let first = tracks.first()?;
    let usable = |bpm: Option<f64>| bpm.filter(|b| b.is_finite() && *b > 0.0);
    let mashup_bpm = tracks.iter().find_map(|t| usable(t.bpm));

    let stretches: Vec<(f64, Option<BpmFix>, bool)> = tracks
        .iter()
        .map(|t| match (mashup_bpm, usable(t.bpm)) {
            (Some(mashup), Some(bpm)) => stretch_for(bpm, mashup),
            (Some(_), None) => (1.0, None, true),
            (None, _) => (1.0, None, false),
        })
        .collect();

    // Downbeat of the first Track on the Mashup timeline: every other Track lines up with it.
    let reference_downbeat = first.downbeat_secs / stretches[0].0;

    let mut offsets_secs = Vec::with_capacity(tracks.len());
    let mut offsets_beats = Vec::with_capacity(tracks.len());
    for (i, (t, (factor, _, _))) in tracks.iter().zip(&stretches).enumerate() {
        let beats = match mashup_bpm {
            Some(mashup) if i > 0 => {
                let early = reference_downbeat - t.downbeat_secs / factor;
                bar_wrapped(secs_to_beats(early, mashup))
            }
            _ => 0.0,
        };
        offsets_beats.push(beats);
        offsets_secs.push(mashup_bpm.map_or(0.0, |mashup| beats_to_secs(beats, mashup)));
    }

    let ends: Vec<f64> = tracks
        .iter()
        .zip(&stretches)
        .zip(&offsets_secs)
        .map(|((t, (factor, _, _)), offset)| offset + t.length_secs / factor)
        .collect();
    let end_secs = ends.iter().copied().fold(0.0, f64::max);

    let plans = stretches
        .into_iter()
        .zip(offsets_beats)
        .zip(ends)
        .map(|(((factor, fix, warn), offset_beats), end)| TrackPlan {
            factor,
            fix,
            warn,
            offset_beats,
            repeats: end < end_secs - 1e-9,
        })
        .collect();

    Some(FirstRunPlan {
        mashup_bpm,
        tracks: plans,
        end_secs,
    })
}

/// Speed factor for one Track: auto-applies halve/double when it rescues the stretch,
/// otherwise caps at the stretch limit and flags a warning.
pub fn stretch_for(bpm: f64, mashup: f64) -> (f64, Option<BpmFix>, bool) {
    let fix = suggest_bpm_fix(bpm, mashup);
    let effective = match fix {
        Some(BpmFix::Double) => bpm * 2.0,
        Some(BpmFix::Half) => bpm / 2.0,
        None => bpm,
    };
    let factor = speed_factor(effective, mashup).unwrap_or(1.0);
    if within_stretch_limits(factor) {
        (factor, fix, false)
    } else {
        (factor.clamp(0.7, 1.4), fix, true)
    }
}

/// Moves an offset by whole bars into 0..one bar, which keeps downbeats aligned.
fn bar_wrapped(beats: f64) -> f64 {
    let wrapped = beats.rem_euclid(BEATS_PER_BAR);
    if BEATS_PER_BAR - wrapped < 1e-9 {
        0.0
    } else {
        wrapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn input(bpm: Option<f64>, length_secs: f64, downbeat_secs: f64) -> TrackInput {
        TrackInput {
            bpm,
            length_secs,
            downbeat_secs,
        }
    }

    #[test]
    fn no_tracks_means_no_plan() {
        assert!(plan_first_run(&[]).is_none());
    }

    #[test]
    fn the_mashup_bpm_is_the_first_tracks_tempo_and_it_is_not_stretched() {
        let plan = plan_first_run(&[input(Some(120.0), 8.0, 0.5)]).unwrap();

        assert_eq!(plan.mashup_bpm, Some(120.0));
        assert_eq!(plan.tracks[0].factor, 1.0);
        assert_eq!(plan.tracks[0].offset_beats, 0.0);
        assert!(!plan.tracks[0].repeats);
        assert!(!plan.tracks[0].warn);
        assert_eq!(plan.end_secs, 8.0);
    }

    #[test]
    fn a_100_bpm_track_is_sped_up_aligned_and_repeated_under_a_120_bpm_track() {
        // A: 120 BPM, 8 s, downbeat 0.5 s. B: 100 BPM, 4 s, downbeat 0.3 s.
        // B plays 1.2x faster, so its downbeat lands at 0.3 / 1.2 = 0.25 s.
        // To meet A's downbeat at 0.5 s it starts 0.25 s late = 0.5 beat at 120 BPM.
        let plan =
            plan_first_run(&[input(Some(120.0), 8.0, 0.5), input(Some(100.0), 4.0, 0.3)]).unwrap();

        assert_eq!(plan.mashup_bpm, Some(120.0));
        let b = &plan.tracks[1];
        assert!(close(b.factor, 1.2));
        assert!(close(b.offset_beats, 0.5));
        assert!(b.repeats);
        assert!(!b.warn);
        assert_eq!(b.fix, None);
        assert!(!plan.tracks[0].repeats);
        assert_eq!(plan.end_secs, 8.0);
    }

    #[test]
    fn a_track_that_would_start_before_zero_is_pushed_forward_by_whole_bars() {
        // B's downbeat lands at 1.2 / 1.2 = 1.0 s but A's is at 0 s, so B would start 2 beats early.
        // One bar later (+4 beats) keeps the downbeats aligned.
        let plan =
            plan_first_run(&[input(Some(120.0), 8.0, 0.0), input(Some(100.0), 4.0, 1.2)]).unwrap();

        assert!(close(plan.tracks[1].offset_beats, 2.0));
    }

    #[test]
    fn an_out_of_range_tempo_is_halved_or_doubled_automatically() {
        // 140 first, then 60: 60 -> 140 is 2.33x (too far), but doubling 60 to 120 needs only 1.1667x.
        let plan =
            plan_first_run(&[input(Some(140.0), 8.0, 0.0), input(Some(60.0), 8.0, 0.0)]).unwrap();

        let b = &plan.tracks[1];
        assert_eq!(b.fix, Some(BpmFix::Double));
        assert!(close(b.factor, 140.0 / 120.0));
        assert!(!b.warn);
    }

    #[test]
    fn a_tempo_that_no_fix_can_save_is_capped_at_the_stretch_limit_with_a_warning() {
        // 40 vs 140: 3.5x. Doubling gives 1.75x, still too far. Cap at 1.4x and warn.
        let plan =
            plan_first_run(&[input(Some(140.0), 8.0, 0.0), input(Some(40.0), 8.0, 0.0)]).unwrap();

        let b = &plan.tracks[1];
        assert_eq!(b.fix, None);
        assert_eq!(b.factor, 1.4);
        assert!(b.warn);
    }

    #[test]
    fn the_mashup_bpm_comes_from_the_first_track_that_has_a_tempo() {
        let plan = plan_first_run(&[input(None, 8.0, 0.0), input(Some(100.0), 8.0, 0.0)]).unwrap();

        assert_eq!(plan.mashup_bpm, Some(100.0));
        assert_eq!(plan.tracks[0].factor, 1.0);
        assert!(plan.tracks[0].warn);
        assert!(!plan.tracks[1].warn);
    }

    #[test]
    fn with_no_tempo_anywhere_nothing_is_stretched_and_it_is_still_playable() {
        let plan = plan_first_run(&[input(None, 8.0, 0.0), input(None, 4.0, 0.0)]).unwrap();

        assert_eq!(plan.mashup_bpm, None);
        assert!(
            plan.tracks
                .iter()
                .all(|t| t.factor == 1.0 && t.offset_beats == 0.0)
        );
        assert_eq!(plan.end_secs, 8.0);
        assert!(plan.tracks[1].repeats);
    }

    #[test]
    fn the_end_marker_is_where_the_longest_stretched_track_ends() {
        // B is 10 s at 100 BPM -> 1.2x -> 10 / 1.2 = 8.333 s, longer than A's 8 s.
        let plan =
            plan_first_run(&[input(Some(120.0), 8.0, 0.0), input(Some(100.0), 10.0, 0.0)]).unwrap();

        assert!(close(plan.end_secs, 10.0 / 1.2));
        assert!(plan.tracks[0].repeats);
        assert!(!plan.tracks[1].repeats);
    }

    #[test]
    fn stretch_for_is_available_on_its_own_for_one_track() {
        assert_eq!(stretch_for(100.0, 120.0), (1.2, None, false));
        let (factor, fix, warn) = stretch_for(60.0, 140.0);
        assert!(close(factor, 140.0 / 120.0));
        assert_eq!((fix, warn), (Some(BpmFix::Double), false));
    }
}
