use crate::mix::{limit, limit_stereo};

/// One Track as the audio callback sees it: already stretched to the Mashup BPM,
/// with its Loop applied. Positions are sample frames on the Mashup timeline.
#[derive(Debug, Clone, Copy)]
pub struct RenderTrack<'a> {
    pub samples: &'a [f32],
    /// Where the first sample plays. May be negative (the Track began before the Mashup).
    pub offset: i64,
    /// Repeat from `offset` until the End marker instead of playing once.
    pub repeats: bool,
    pub fader: f32,
}

impl RenderTrack<'_> {
    /// The Track's contribution at Mashup position `pos`, fader applied.
    fn sample_at(&self, pos: i64) -> f32 {
        let fader = if self.fader.is_finite() && self.fader > 0.0 {
            self.fader
        } else {
            0.0
        };
        let since_start = pos - self.offset;
        if since_start < 0 || self.samples.is_empty() {
            return 0.0;
        }
        let len = self.samples.len() as i64;
        let index = if self.repeats {
            since_start % len
        } else {
            since_start
        };
        let raw = self.samples.get(index as usize).copied().unwrap_or(0.0);
        if raw.is_finite() { raw * fader } else { 0.0 }
    }
}

/// Fills `out` with the Mashup starting at frame `start`: every audible Track summed,
/// silent from the End marker onward, then limited. Does not allocate.
/// `audition: Some(i)` plays only Track `i` through the identical graph (Solo audition).
pub fn mix_into(
    out: &mut [f32],
    start: i64,
    end: i64,
    tracks: &[RenderTrack],
    audition: Option<usize>,
) {
    for (i, slot) in out.iter_mut().enumerate() {
        let pos = start + i as i64;
        *slot = if pos < 0 || pos >= end {
            0.0
        } else {
            let sum: f32 = tracks
                .iter()
                .enumerate()
                .filter(|(n, _)| audition.is_none_or(|solo| solo == *n))
                .map(|(_, t)| t.sample_at(pos))
                .sum();
            limit(sum)
        };
    }
}

/// One audio-callback block starting at Playhead frame `start`. With `looping`, a block that
/// reaches the End marker continues from frame 0 in the same block, so the seam has no gap.
/// Does not allocate.
pub fn mix_block(
    out: &mut [f32],
    start: i64,
    end: i64,
    looping: bool,
    tracks: &[RenderTrack],
    audition: Option<usize>,
) {
    if !looping || end <= 0 {
        mix_into(out, start, end, tracks, audition);
        return;
    }
    let mut pos = start.rem_euclid(end);
    let mut written = 0;
    while written < out.len() {
        let n = ((end - pos) as usize).min(out.len() - written);
        mix_into(&mut out[written..written + n], pos, end, tracks, audition);
        written += n;
        pos = 0;
    }
}

/// The whole Mashup from frame 0 up to the End marker.
pub fn render_mashup(tracks: &[RenderTrack], end: i64, audition: Option<usize>) -> Vec<f32> {
    let mut out = vec![0.0; end.max(0) as usize];
    mix_into(&mut out, 0, end, tracks, audition);
    out
}

/// One stereo Track as the audio callback sees it: planar, already stretched to the Mashup
/// BPM. Left and right share one Offset, repeat flag and fader.
#[derive(Debug, Clone, Copy)]
pub struct StereoTrack<'a> {
    pub left: &'a [f32],
    pub right: &'a [f32],
    pub offset: i64,
    pub repeats: bool,
    pub fader: f32,
}

/// One stereo audio-callback block starting at Playhead frame `start`: every audible Track
/// summed per channel, silent from the End marker on (or wrapped to frame 0 when `looping`),
/// then limited with linked channels. `audition: Some(i)` plays only Track `i`. Does not
/// allocate. If the output slices differ in length, the shorter one sets the block size.
pub fn mix_block_stereo(
    out_left: &mut [f32],
    out_right: &mut [f32],
    start: i64,
    end: i64,
    looping: bool,
    tracks: &[StereoTrack],
    audition: Option<usize>,
) {
    let frames = out_left.len().min(out_right.len());
    for i in 0..frames {
        let raw = start.saturating_add(i as i64);
        let pos = if looping && end > 0 {
            raw.rem_euclid(end)
        } else {
            raw
        };
        if pos < 0 || pos >= end {
            out_left[i] = 0.0;
            out_right[i] = 0.0;
            continue;
        }
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for (n, t) in tracks.iter().enumerate() {
            if audition.is_some_and(|solo| solo != n) {
                continue;
            }
            let channel = |samples| RenderTrack {
                samples,
                offset: t.offset,
                repeats: t.repeats,
                fader: t.fader,
            };
            l += channel(t.left).sample_at(pos);
            r += channel(t.right).sample_at(pos);
        }
        (out_left[i], out_right[i]) = limit_stereo(l, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: f32 = 0.5;
    const B: f32 = 0.25;
    const C: f32 = -0.125;

    fn track<'a>(samples: &'a [f32], offset: i64, repeats: bool) -> RenderTrack<'a> {
        RenderTrack {
            samples,
            offset,
            repeats,
            fader: 1.0,
        }
    }

    #[test]
    fn a_repeating_track_fills_the_mashup_from_its_offset_to_the_end_marker() {
        let s = [A, B, C];
        // Hand-written: silence for 2 samples, then the loop three times, cut at 10.
        assert_eq!(
            render_mashup(&[track(&s, 2, true)], 10, None),
            vec![0.0, 0.0, A, B, C, A, B, C, A, B]
        );
    }

    #[test]
    fn a_non_repeating_track_plays_once_then_is_silent() {
        let s = [A, B, C];
        assert_eq!(
            render_mashup(&[track(&s, 2, false)], 10, None),
            vec![0.0, 0.0, A, B, C, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn audio_past_the_end_marker_is_cut() {
        let s = [A, B, C];
        assert_eq!(render_mashup(&[track(&s, 0, false)], 2, None), vec![A, B]);
    }

    #[test]
    fn an_end_marker_before_the_offset_leaves_silence() {
        let s = [A, B, C];
        assert_eq!(
            render_mashup(&[track(&s, 5, true)], 3, None),
            vec![0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn an_end_marker_of_zero_or_less_is_an_empty_mashup() {
        let s = [A];
        assert!(render_mashup(&[track(&s, 0, true)], 0, None).is_empty());
        assert!(render_mashup(&[track(&s, 0, true)], -4, None).is_empty());
    }

    #[test]
    fn a_negative_offset_skips_the_start_of_the_first_pass_and_keeps_the_loop_phase() {
        let s = [A, B, C];
        // The track started one sample before the Mashup: we hear it from its second sample.
        assert_eq!(
            render_mashup(&[track(&s, -1, true)], 5, None),
            vec![B, C, A, B, C]
        );
        assert_eq!(
            render_mashup(&[track(&s, -1, false)], 5, None),
            vec![B, C, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn an_empty_track_is_silent_and_does_not_hang() {
        assert_eq!(
            render_mashup(&[track(&[], 0, true)], 3, None),
            vec![0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn tracks_are_summed_after_their_faders() {
        let (x, y) = ([0.5, 0.5], [0.25, -0.5]);
        let tracks = [
            track(&x, 0, false),
            RenderTrack {
                fader: 0.5,
                ..track(&y, 0, false)
            },
        ];
        // 0.5 + 0.125 | 0.5 - 0.25
        assert_eq!(render_mashup(&tracks, 2, None), vec![0.625, 0.25]);
    }

    #[test]
    fn the_sum_is_limited_to_full_scale() {
        let (x, y) = ([0.75], [0.75]);
        assert_eq!(
            render_mashup(&[track(&x, 0, false), track(&y, 0, false)], 1, None),
            vec![1.0]
        );
    }

    #[test]
    fn soloing_a_track_for_audition_plays_only_that_track_through_the_same_graph() {
        let (x, y) = ([0.5, 0.5, 0.5, 0.5], [0.25]);
        let tracks = [track(&x, 0, false), track(&y, 2, true)];

        assert_eq!(render_mashup(&tracks, 4, None), vec![0.5, 0.5, 0.75, 0.75]);
        assert_eq!(render_mashup(&tracks, 4, Some(0)), vec![0.5, 0.5, 0.5, 0.5]);
        // Track 1 keeps its Offset and its repeat while auditioned.
        assert_eq!(
            render_mashup(&tracks, 4, Some(1)),
            vec![0.0, 0.0, 0.25, 0.25]
        );
    }

    #[test]
    fn auditioning_a_track_equals_the_mashup_with_every_other_fader_at_zero() {
        let (x, y, z) = ([0.1, 0.2, 0.3], [0.4, -0.4], [0.05]);
        let live = [track(&x, 1, true), track(&y, 0, true), track(&z, 3, false)];
        let muted = [
            RenderTrack {
                fader: 0.0,
                ..track(&x, 1, true)
            },
            track(&y, 0, true),
            RenderTrack {
                fader: 0.0,
                ..track(&z, 3, false)
            },
        ];

        assert_eq!(
            render_mashup(&live, 9, Some(1)),
            render_mashup(&muted, 9, None)
        );
    }

    #[test]
    fn auditioning_a_track_that_does_not_exist_is_silence() {
        let s = [A];
        assert_eq!(
            render_mashup(&[track(&s, 0, true)], 2, Some(7)),
            vec![0.0, 0.0]
        );
    }

    #[test]
    fn a_useless_fader_silences_a_track_instead_of_corrupting_the_mix() {
        let s = [A, A];
        for bad in [f32::NAN, f32::INFINITY, -1.0] {
            let t = RenderTrack {
                fader: bad,
                ..track(&s, 0, false)
            };
            assert_eq!(render_mashup(&[t], 2, None), vec![0.0, 0.0], "fader {bad}");
        }
    }

    #[test]
    fn rendering_in_blocks_matches_rendering_in_one_go() {
        let s = [A, B, C, 0.0625, -0.5];
        let tracks = [track(&s, 3, true)];
        let whole = render_mashup(&tracks, 40, None);

        for block in [1usize, 7, 16, 40] {
            let mut out = vec![9.0; 40];
            for (n, chunk) in out.chunks_mut(block).enumerate() {
                mix_into(chunk, (n * block) as i64, 40, &tracks, None);
            }
            assert_eq!(out, whole, "block size {block}");
        }
    }

    #[test]
    fn a_block_that_starts_past_the_end_marker_is_silent() {
        let s = [A];
        let mut out = [9.0; 4];
        mix_into(&mut out, 10, 8, &[track(&s, 0, true)], None);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn a_nan_in_the_source_never_reaches_the_output() {
        let s = [f32::NAN, A];
        assert!(
            render_mashup(&[track(&s, 0, true)], 6, None)
                .iter()
                .all(|v| v.is_finite())
        );
    }

    fn stereo<'a>(
        left: &'a [f32],
        right: &'a [f32],
        offset: i64,
        repeats: bool,
    ) -> StereoTrack<'a> {
        StereoTrack {
            left,
            right,
            offset,
            repeats,
            fader: 1.0,
        }
    }

    fn render_stereo(
        tracks: &[StereoTrack],
        end: i64,
        looping: bool,
        audition: Option<usize>,
    ) -> (Vec<f32>, Vec<f32>) {
        let n = end.max(0) as usize;
        let (mut l, mut r) = (vec![9.0; n], vec![9.0; n]);
        mix_block_stereo(&mut l, &mut r, 0, end, looping, tracks, audition);
        (l, r)
    }

    #[test]
    fn stereo_channels_stay_independent() {
        let (l, r) = ([0.5, 0.25], [-0.25, 0.125]);
        let (out_l, out_r) = render_stereo(&[stereo(&l, &r, 0, false)], 2, false, None);
        assert_eq!(out_l, vec![0.5, 0.25]);
        assert_eq!(out_r, vec![-0.25, 0.125]);
    }

    #[test]
    fn stereo_offset_repeat_and_end_marker_match_the_mono_rules_on_each_channel() {
        let (l, r) = ([A, B, C], [C, B, A]);
        let (out_l, out_r) = render_stereo(&[stereo(&l, &r, 2, true)], 10, false, None);
        assert_eq!(out_l, vec![0.0, 0.0, A, B, C, A, B, C, A, B]);
        assert_eq!(out_r, vec![0.0, 0.0, C, B, A, C, B, A, C, B]);
    }

    #[test]
    fn stereo_tracks_sum_per_channel_after_their_faders() {
        let (xl, xr) = ([0.5], [0.25]);
        let (yl, yr) = ([0.25], [0.5]);
        let tracks = [
            stereo(&xl, &xr, 0, false),
            StereoTrack {
                fader: 0.5,
                ..stereo(&yl, &yr, 0, false)
            },
        ];
        let (l, r) = render_stereo(&tracks, 1, false, None);
        assert_eq!((l[0], r[0]), (0.625, 0.5));
    }

    #[test]
    fn the_stereo_mashup_is_limited_with_linked_channels() {
        // Sum is (1.5, 0.75): both scaled by 1/1.5 -> (1.0, 0.5), not (1.0, 0.75).
        let (xl, xr) = ([0.75], [0.25]);
        let (yl, yr) = ([0.75], [0.5]);
        let tracks = [stereo(&xl, &xr, 0, false), stereo(&yl, &yr, 0, false)];
        let (l, r) = render_stereo(&tracks, 1, false, None);
        assert!(
            (l[0] - 1.0).abs() < 1e-6 && (r[0] - 0.5).abs() < 1e-6,
            "{} {}",
            l[0],
            r[0]
        );
    }

    #[test]
    fn stereo_solo_audition_plays_only_that_track() {
        let (xl, xr) = ([0.5, 0.5], [0.1, 0.1]);
        let (yl, yr) = ([0.25], [0.05]);
        let tracks = [stereo(&xl, &xr, 0, false), stereo(&yl, &yr, 1, false)];
        let (l, r) = render_stereo(&tracks, 2, false, Some(1));
        assert_eq!(l, vec![0.0, 0.25]);
        assert_eq!(r, vec![0.0, 0.05]);
        assert_eq!(render_stereo(&tracks, 2, false, Some(7)).0, vec![0.0, 0.0]);
    }

    #[test]
    fn a_stereo_loop_wraps_across_blocks_without_a_gap() {
        let ramp: Vec<f32> = (0..10).map(|n| n as f32 * 0.1).collect();
        let neg: Vec<f32> = ramp.iter().map(|v| -v).collect();
        let tracks = [stereo(&ramp, &neg, 0, false)];
        let mut heard = Vec::new();
        for block in 0..7i64 {
            let (mut l, mut r) = ([9.0f32; 4], [9.0f32; 4]);
            mix_block_stereo(&mut l, &mut r, (block * 4) % 10, 10, true, &tracks, None);
            heard.extend(l.iter().zip(&r).map(|(a, b)| (*a, *b)));
        }
        for (n, (l, r)) in heard.iter().enumerate() {
            let want = (n % 10) as f32 * 0.1;
            assert!(
                (l - want).abs() < 1e-6 && (r + want).abs() < 1e-6,
                "frame {n}"
            );
        }
    }

    #[test]
    fn a_stereo_block_past_the_end_marker_without_looping_is_silent() {
        let (l, r) = ([A], [A]);
        let mut out_l = [9.0; 3];
        let mut out_r = [9.0; 3];
        mix_block_stereo(
            &mut out_l,
            &mut out_r,
            5,
            4,
            false,
            &[stereo(&l, &r, 0, true)],
            None,
        );
        assert_eq!((out_l, out_r), ([0.0; 3], [0.0; 3]));
    }

    #[test]
    fn non_finite_stereo_source_samples_never_reach_the_output() {
        let (l, r) = ([f32::NAN, A], [A, f32::INFINITY]);
        let (out_l, out_r) = render_stereo(&[stereo(&l, &r, 0, true)], 6, false, None);
        assert!(out_l.iter().chain(&out_r).all(|v| v.is_finite()));
    }
}
