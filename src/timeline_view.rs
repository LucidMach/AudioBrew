use crate::first_run::stretch_for;
use crate::project::Project;
use crate::timeline::{Placement, mashup_length_secs, repeat_seams_secs};

/// The drawing area for the timeline lanes, in pixels.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    /// Total width of a lane's drawing area.
    pub width: f64,
    /// Empty space before time 0 and after the End marker.
    pub pad_left: f64,
    pub pad_right: f64,
}

/// A horizontal span in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub x: f64,
    pub w: f64,
}

/// Where a Track's clip sits, and the faded copies it repeats as until the End marker.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipView {
    pub clip: Span,
    /// The clip's width if the End marker did not cut it off, so a waveform can show just the
    /// visible fraction.
    pub full_w: f64,
    pub repeats: Vec<Span>,
}

/// One beat line on the ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub x: f64,
    /// The first beat of a bar.
    pub major: bool,
    /// The bar number, shown on some major ticks.
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineView {
    pub px_per_sec: f64,
    pub end_secs: f64,
    pub end_x: f64,
    /// One entry per lane; `None` for an empty lane.
    pub lanes: Vec<Option<ClipView>>,
    pub ticks: Vec<Tick>,
}

/// Lays the Project's Tracks out on a timeline that fits `geo.width`, with the End marker
/// (or, by default, the end of the longest Track) near the right edge.
pub fn timeline_view(project: &Project, geo: Geometry) -> TimelineView {
    let mashup = project.mashup_bpm();
    // Each loaded Track as it plays on the Mashup grid: stretched to the Mashup BPM, then offset.
    let placements: Vec<Option<Placement>> = (0..project.lane_count())
        .map(|lane| {
            let track = project.track(lane)?;
            let factor = match (track.bpm, mashup) {
                (Some(bpm), Some(mashup)) => stretch_for(bpm, mashup).0,
                _ => 1.0,
            };
            Some(Placement {
                length_secs: track.length_secs / factor,
                offset_secs: project.offset_secs(lane).unwrap_or(0.0),
            })
        })
        .collect();
    let loaded: Vec<Placement> = placements.iter().flatten().copied().collect();
    let end_secs = project.end_marker_secs().unwrap_or_else(|| mashup_length_secs(&loaded));
    let usable = geo.width - geo.pad_left - geo.pad_right;

    if !(end_secs > 0.0 && usable > 0.0) {
        return TimelineView {
            px_per_sec: 0.0,
            end_secs: 0.0,
            end_x: geo.pad_left,
            lanes: vec![None; placements.len()],
            ticks: Vec::new(),
        };
    }
    let px_per_sec = usable / end_secs;
    let at = |secs: f64| geo.pad_left + secs * px_per_sec;

    let lanes = placements
        .iter()
        .map(|placement| {
            let p = (*placement)?;
            let visible = (p.offset_secs + p.length_secs).min(end_secs) - p.offset_secs;
            let repeats = repeat_seams_secs(p, end_secs)
                .into_iter()
                .map(|seam| Span {
                    x: at(seam),
                    w: p.length_secs.min(end_secs - seam).max(0.0) * px_per_sec,
                })
                .collect();
            Some(ClipView {
                clip: Span { x: at(p.offset_secs), w: visible.max(0.0) * px_per_sec },
                full_w: p.length_secs * px_per_sec,
                repeats,
            })
        })
        .collect();

    TimelineView {
        px_per_sec,
        end_secs,
        end_x: at(end_secs),
        lanes,
        ticks: ruler_ticks(mashup, px_per_sec, geo),
    }
}

/// A tick per beat from time 0 to the right edge, with every bar numbered when there is room
/// and every 2nd, 4th, 8th... bar when there is not.
fn ruler_ticks(mashup_bpm: Option<f64>, px_per_sec: f64, geo: Geometry) -> Vec<Tick> {
    let Some(bpm) = mashup_bpm.filter(|b| b.is_finite() && *b > 0.0) else {
        return Vec::new();
    };
    const MIN_LABEL_GAP_PX: f64 = 40.0;
    const MAX_TICKS: usize = 4000;
    let beat = 60.0 / bpm;
    let bar_px = 4.0 * beat * px_per_sec;
    let mut every = 1usize;
    while bar_px * (every as f64) < MIN_LABEL_GAP_PX {
        every *= 2;
    }
    let last_secs = (geo.width - geo.pad_left) / px_per_sec;
    (0..MAX_TICKS)
        .take_while(|&i| i as f64 * beat <= last_secs + 1e-9)
        .map(|i| {
            let major = i % 4 == 0;
            let bar = i / 4;
            Tick {
                x: geo.pad_left + i as f64 * beat * px_per_sec,
                major,
                label: (major && bar % every == 0).then(|| (bar + 1).to_string()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::LoadedTrack;

    fn project(tracks: &[(Option<f64>, f64, f64)]) -> Project {
        // (bpm, length in source seconds, offset in beats)
        let mut p = Project::new();
        for (lane, (bpm, len, offset)) in tracks.iter().enumerate() {
            if lane >= p.lane_count() {
                p.add_lane();
            }
            p.load_track(
                lane,
                LoadedTrack { offset_beats: *offset, ..LoadedTrack::new(format!("{lane}.wav"), *bpm, *len) },
            )
            .unwrap();
        }
        p
    }

    fn geo(width: f64, pad: f64) -> Geometry {
        Geometry { width, pad_left: pad, pad_right: pad }
    }

    #[test]
    fn clips_are_laid_out_to_scale_and_the_shorter_one_repeats_to_the_end() {
        // 120 BPM both, so no stretch. Lengths 10 s and 8 s. 1000 px for 10 s = 100 px/s.
        let p = project(&[(Some(120.0), 10.0, 0.0), (Some(120.0), 8.0, 0.0)]);

        let v = timeline_view(&p, geo(1100.0, 50.0));

        assert_eq!(v.px_per_sec, 100.0);
        assert_eq!(v.end_x, 1050.0);
        let first = v.lanes[0].as_ref().unwrap();
        assert_eq!(first.clip, Span { x: 50.0, w: 1000.0 });
        assert_eq!(first.full_w, 1000.0);
        assert!(first.repeats.is_empty());
        let second = v.lanes[1].as_ref().unwrap();
        assert_eq!(second.clip, Span { x: 50.0, w: 800.0 });
        assert_eq!(second.repeats, vec![Span { x: 850.0, w: 200.0 }]);
    }

    #[test]
    fn an_offset_moves_the_clip_and_its_repeat_with_it() {
        // 2 beats at 120 BPM = 1 s = 100 px.
        let p = project(&[(Some(120.0), 10.0, 0.0), (Some(120.0), 8.0, 2.0)]);

        let v = timeline_view(&p, geo(1100.0, 50.0));

        let second = v.lanes[1].as_ref().unwrap();
        assert_eq!(second.clip, Span { x: 150.0, w: 800.0 });
        // The clip ends at 9 s; one more second of the next pass fits before the End marker.
        assert_eq!(second.repeats, vec![Span { x: 950.0, w: 100.0 }]);
    }

    #[test]
    fn a_track_at_another_tempo_is_drawn_at_its_stretched_length() {
        // Mashup 120 BPM (first Track). A 100 BPM Track plays 1.2x faster: 12 s becomes 10 s.
        let p = project(&[(Some(120.0), 10.0, 0.0), (Some(100.0), 12.0, 0.0)]);

        let v = timeline_view(&p, geo(1100.0, 50.0));

        assert_eq!(v.lanes[1].as_ref().unwrap().clip.w, 1000.0);
    }

    #[test]
    fn the_ruler_has_a_tick_per_beat_and_numbers_each_bar() {
        // 120 BPM: a beat every 0.5 s. 8 s over 800 px = 100 px/s, so a beat every 50 px.
        let p = project(&[(Some(120.0), 8.0, 0.0)]);

        let v = timeline_view(&p, geo(850.0, 25.0));

        assert_eq!(v.px_per_sec, 100.0);
        assert_eq!(v.ticks[0], Tick { x: 25.0, major: true, label: Some("1".into()) });
        assert_eq!(v.ticks[1], Tick { x: 75.0, major: false, label: None });
        assert_eq!(v.ticks[4], Tick { x: 225.0, major: true, label: Some("2".into()) });
        let labelled: Vec<_> = v.ticks.iter().filter_map(|t| t.label.as_deref()).collect();
        assert_eq!(labelled, vec!["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn bar_numbers_are_thinned_when_bars_would_be_too_close_to_read() {
        // 60 s over 600 px = 10 px/s; a bar (2 s at 120 BPM) is only 20 px wide.
        let p = project(&[(Some(120.0), 60.0, 0.0)]);

        let v = timeline_view(&p, geo(640.0, 20.0));

        let labelled: Vec<_> = v.ticks.iter().filter_map(|t| t.label.as_deref()).take(4).collect();
        assert_eq!(labelled, vec!["1", "3", "5", "7"], "every second bar");
    }

    #[test]
    fn a_dragged_end_marker_shortens_the_timeline_and_trims_clips() {
        // End marker at 8 beats = 4 s at 120 BPM. 1000 px over 4 s = 250 px/s.
        let mut p = project(&[(Some(120.0), 10.0, 0.0)]);
        p.set_end_marker_beats(Some(8.0)).unwrap();

        let v = timeline_view(&p, geo(1100.0, 50.0));

        assert_eq!(v.px_per_sec, 250.0);
        assert_eq!(v.end_x, 1050.0);
        let clip = v.lanes[0].as_ref().unwrap();
        assert_eq!(clip.clip, Span { x: 50.0, w: 1000.0 });
        // Untrimmed it would be 10 s = 2500 px wide; only the first 40% of it is visible.
        assert_eq!(clip.full_w, 2500.0);
    }

    #[test]
    fn an_empty_project_has_empty_lanes_and_no_ruler() {
        let v = timeline_view(&Project::new(), geo(1100.0, 50.0));

        assert_eq!(v.lanes, vec![None, None]);
        assert!(v.ticks.is_empty());
        assert_eq!(v.px_per_sec, 0.0);
    }
}
