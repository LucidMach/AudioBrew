use std::collections::HashMap;

/// Everything that changes a Track's stretched buffer. Offset, fader and Band mute/solo are
/// deliberately absent: changing them never needs a re-render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StretchParams {
    pub factor: f64,
    pub pitch_semitones: i8,
    pub loop_region: Option<(f64, f64)>,
}

pub fn needs_rerender(old: &StretchParams, new: &StretchParams) -> bool {
    (old.factor - new.factor).abs() > 1e-9
        || old.pitch_semitones != new.pitch_semitones
        || old.loop_region != new.loop_region
}

/// Lanes (by index) that need a fresh stretched buffer: loaded now, and either new or changed.
pub fn tracks_to_rerender(
    old: &[Option<StretchParams>],
    new: &[Option<StretchParams>],
) -> Vec<usize> {
    new.iter()
        .enumerate()
        .filter_map(|(lane, n)| {
            let n = n.as_ref()?;
            match old.get(lane).copied().flatten() {
                Some(o) if !needs_rerender(&o, n) => None,
                _ => Some(lane),
            }
        })
        .collect()
}

/// Tracks background re-renders so a stale result (the BPM changed again, undo ran, the lane
/// was removed) is thrown away instead of overwriting newer audio.
#[derive(Debug, Default)]
pub struct RenderJobs {
    next_generation: u64,
    latest: HashMap<usize, u64>,
}

impl RenderJobs {
    pub fn new() -> Self {
        Self {
            next_generation: 1,
            latest: HashMap::new(),
        }
    }

    /// Starts a render for `lane`; any earlier one for that lane becomes stale.
    pub fn start(&mut self, lane: usize) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        self.latest.insert(lane, generation);
        generation
    }

    /// Makes any in-flight render for `lane` stale.
    pub fn cancel(&mut self, lane: usize) {
        self.latest.remove(&lane);
    }

    /// True if this result is the latest for its lane and should be applied (once).
    pub fn finish(&mut self, lane: usize, generation: u64) -> bool {
        if self.latest.get(&lane) == Some(&generation) {
            self.latest.remove(&lane);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(factor: f64) -> StretchParams {
        StretchParams {
            factor,
            pitch_semitones: 0,
            loop_region: None,
        }
    }

    #[test]
    fn identical_params_need_no_rerender() {
        assert!(!needs_rerender(&params(1.2), &params(1.2)));
    }

    #[test]
    fn a_change_in_speed_pitch_or_loop_needs_a_rerender() {
        assert!(needs_rerender(&params(1.2), &params(1.25)));
        assert!(needs_rerender(
            &params(1.0),
            &StretchParams {
                pitch_semitones: 2,
                ..params(1.0)
            }
        ));
        assert!(needs_rerender(
            &params(1.0),
            &StretchParams {
                loop_region: Some((0.0, 4.0)),
                ..params(1.0)
            }
        ));
        assert!(needs_rerender(
            &StretchParams {
                loop_region: Some((0.0, 4.0)),
                ..params(1.0)
            },
            &StretchParams {
                loop_region: Some((0.0, 8.0)),
                ..params(1.0)
            }
        ));
    }

    #[test]
    fn rounding_noise_in_the_speed_factor_is_not_a_change() {
        assert!(!needs_rerender(&params(1.2), &params(1.2 + 1e-12)));
    }

    #[test]
    fn only_the_tracks_whose_stretch_changed_are_rerendered() {
        let old = [
            Some(params(1.0)),
            Some(params(1.2)),
            None,
            Some(params(0.9)),
        ];
        let new = [
            Some(params(1.0)),
            Some(params(1.5)),
            None,
            Some(params(0.9)),
        ];
        assert_eq!(tracks_to_rerender(&old, &new), vec![1]);
    }

    #[test]
    fn a_newly_loaded_track_is_rendered_and_a_cleared_one_is_not() {
        let old = [Some(params(1.0)), Some(params(1.0))];
        let new = [None, Some(params(1.0)), Some(params(1.1))];
        assert_eq!(tracks_to_rerender(&old, &new), vec![2]);
        let loaded = [Some(params(1.0)), None];
        let now = [Some(params(1.0)), Some(params(0.8))];
        assert_eq!(tracks_to_rerender(&loaded, &now), vec![1]);
    }

    #[test]
    fn changing_the_mashup_bpm_rerenders_every_track_whose_factor_moves() {
        let old = [Some(params(1.0)), Some(params(1.2))];
        let new = [Some(params(0.9)), Some(params(1.08))];
        assert_eq!(tracks_to_rerender(&old, &new), vec![0, 1]);
    }

    #[test]
    fn a_result_from_the_latest_request_is_accepted_once() {
        let mut jobs = RenderJobs::new();
        let g = jobs.start(0);
        assert!(jobs.finish(0, g));
        assert!(!jobs.finish(0, g)); // not applied twice
    }

    #[test]
    fn a_stale_result_is_discarded_when_a_newer_request_started() {
        let mut jobs = RenderJobs::new();
        let old = jobs.start(0);
        let new = jobs.start(0); // e.g. undo changed the BPM while the first stretch was running
        assert!(!jobs.finish(0, old));
        assert!(jobs.finish(0, new));
    }

    #[test]
    fn a_cancelled_lane_discards_its_in_flight_result() {
        let mut jobs = RenderJobs::new();
        let g = jobs.start(2);
        jobs.cancel(2); // the lane was removed
        assert!(!jobs.finish(2, g));
    }

    #[test]
    fn generations_are_never_reused_after_a_cancel() {
        let mut jobs = RenderJobs::new();
        let first = jobs.start(0);
        jobs.cancel(0);
        let second = jobs.start(0);
        assert_ne!(first, second);
        assert!(!jobs.finish(0, first));
        assert!(jobs.finish(0, second));
    }

    #[test]
    fn lanes_are_tracked_independently() {
        let mut jobs = RenderJobs::new();
        let a = jobs.start(0);
        let b = jobs.start(1);
        assert!(jobs.finish(1, b));
        assert!(jobs.finish(0, a));
    }

    #[test]
    fn a_result_for_a_lane_that_never_started_is_discarded() {
        let mut jobs = RenderJobs::new();
        assert!(!jobs.finish(5, 1));
    }
}
