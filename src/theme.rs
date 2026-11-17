/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// WCAG relative luminance (0 = black, 1 = white).
pub fn luminance(c: Rgb) -> f64 {
    let lin = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Colours for one lane's clip: its body, the waveform drawn on it, and the small lane tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipColors {
    pub body: Rgb,
    pub wave: Rgb,
    pub tag: Rgb,
    pub tag_text: Rgb,
}

const WHITE: Rgb = Rgb(255, 255, 255);

/// One colour set per lane, cycling after four, so neighbouring Tracks are easy to tell apart
/// (green, blue, purple, teal). The Mashup bus has its own (orange).
const CLIP_PALETTE: [ClipColors; 4] = [
    ClipColors { body: Rgb(205, 231, 214), wave: Rgb(31, 122, 74), tag: Rgb(30, 110, 66), tag_text: WHITE },
    ClipColors { body: Rgb(207, 224, 246), wave: Rgb(36, 88, 166), tag: Rgb(36, 88, 166), tag_text: WHITE },
    ClipColors { body: Rgb(226, 215, 244), wave: Rgb(106, 58, 171), tag: Rgb(106, 58, 171), tag_text: WHITE },
    ClipColors { body: Rgb(200, 233, 232), wave: Rgb(13, 115, 119), tag: Rgb(13, 115, 119), tag_text: WHITE },
];
/// Light tints for the six possible Bands (amber, blue, rose, green, purple, teal).
const BAND_TINTS: [Rgb; 6] = [
    Rgb(251, 227, 176),
    Rgb(207, 224, 246),
    Rgb(244, 211, 218),
    Rgb(205, 231, 214),
    Rgb(226, 215, 244),
    Rgb(200, 233, 232),
];
const BUS_COLORS: ClipColors =
    ClipColors { body: Rgb(246, 222, 191), wave: Rgb(178, 85, 26), tag: Rgb(166, 77, 20), tag_text: WHITE };

/// Every colour the app uses. Views take colours from here and nowhere else.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Raised, pressable surfaces.
    pub surface: Rgb,
    /// Wells and tracks.
    pub inset: Rgb,
    /// The Mashup lane: slightly deeper than a Track lane.
    pub mashup_surface: Rgb,
    pub text: Rgb,
    pub text_muted: Rgb,
    /// The 1px outline on every interactive control.
    pub hairline: Rgb,
    /// Cyan: active, playing or soloed, and nothing else.
    pub glow: Rgb,
    pub text_on_glow: Rgb,
    /// Key badge backgrounds: compatible, workable, clashing. Always paired with a text label,
    /// so colour is never the only cue.
    pub status_ok: Rgb,
    pub status_warn: Rgb,
    pub status_bad: Rgb,
    pub text_on_status: Rgb,
    /// The window background showing between panels.
    pub gutter: Rgb,
    /// A panel's body and its header strip.
    pub panel: Rgb,
    pub panel_header: Rgb,
    /// The timeline and monitor drawing area inside a panel.
    pub content: Rgb,
    /// Quiet outline around panels and lanes (decorative: controls use `hairline`).
    pub border: Rgb,
    /// The playhead line.
    pub playhead: Rgb,
    /// Outline marking the selected Track.
    pub selection: Rgb,
}

impl Theme {
    pub fn standard() -> Self {
        Self {
            surface: Rgb(244, 246, 248),
            inset: Rgb(230, 234, 239),
            mashup_surface: Rgb(221, 236, 246),
            text: Rgb(17, 24, 39),
            text_muted: Rgb(75, 85, 99),
            hairline: Rgb(110, 120, 136),
            glow: Rgb(0, 194, 224),
            text_on_glow: Rgb(6, 32, 42),
            status_ok: Rgb(134, 239, 172),
            status_warn: Rgb(253, 224, 71),
            status_bad: Rgb(252, 165, 165),
            text_on_status: Rgb(17, 24, 39),
            gutter: Rgb(201, 204, 209),
            panel: Rgb(236, 237, 239),
            panel_header: Rgb(225, 227, 230),
            content: Rgb(246, 247, 248),
            border: Rgb(181, 185, 191),
            playhead: Rgb(229, 56, 59),
            selection: Rgb(232, 161, 0),
        }
    }

    pub fn high_contrast() -> Self {
        Self {
            surface: Rgb(255, 255, 255),
            inset: Rgb(235, 235, 235),
            mashup_surface: Rgb(225, 235, 245),
            text: Rgb(0, 0, 0),
            text_muted: Rgb(51, 51, 51),
            hairline: Rgb(0, 0, 0),
            glow: Rgb(0, 229, 255),
            text_on_glow: Rgb(0, 0, 0),
            status_ok: Rgb(134, 239, 172),
            status_warn: Rgb(253, 224, 71),
            status_bad: Rgb(252, 165, 165),
            text_on_status: Rgb(0, 0, 0),
            gutter: Rgb(201, 204, 209),
            panel: Rgb(240, 240, 240),
            panel_header: Rgb(228, 228, 228),
            content: Rgb(255, 255, 255),
            border: Rgb(120, 120, 120),
            playhead: Rgb(200, 0, 20),
            selection: Rgb(200, 130, 0),
        }
    }

    /// The colours for lane `lane`'s clip; the palette cycles after four lanes.
    pub fn clip_colors(&self, lane: usize) -> ClipColors {
        CLIP_PALETTE[lane % CLIP_PALETTE.len()]
    }

    /// The zone tint for Band `index` in the Bands graph, wrapping after six.
    pub fn band_tint(&self, index: usize) -> Rgb {
        BAND_TINTS[index % BAND_TINTS.len()]
    }

    /// The Mashup bus lane's colours.
    pub fn bus_colors(&self) -> ClipColors {
        BUS_COLORS
    }

    /// Every (name, foreground, background) pair that carries text.
    pub fn text_pairs(&self) -> Vec<(&'static str, Rgb, Rgb)> {
        vec![
            ("text on surface", self.text, self.surface),
            ("text on inset", self.text, self.inset),
            ("text on mashup", self.text, self.mashup_surface),
            ("muted on surface", self.text_muted, self.surface),
            ("muted on inset", self.text_muted, self.inset),
            ("muted on mashup", self.text_muted, self.mashup_surface),
            ("label on glow", self.text_on_glow, self.glow),
            ("label on ok", self.text_on_status, self.status_ok),
            ("label on warn", self.text_on_status, self.status_warn),
            ("label on bad", self.text_on_status, self.status_bad),
        ]
    }
}

/// Why something wants to glow, most deserving first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GlowKind {
    Playing,
    Solo,
    Other,
}

const MAX_GLOWING: usize = 3;

/// Which of the active items (by index) may glow: at most three, Playing first, then Solo,
/// then the rest, earlier items winning ties. The others must show "active" another way.
pub fn glowing(active: &[GlowKind]) -> Vec<usize> {
    let mut ranked: Vec<usize> = (0..active.len()).collect();
    ranked.sort_by_key(|&i| active[i]);
    ranked.truncate(MAX_GLOWING);
    ranked.sort_unstable();
    ranked
}

/// A fill pattern, so Bands differ by more than tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Solid,
    Stripes,
    Dots,
    Crosshatch,
    Waves,
    Checks,
}

pub fn band_pattern(index: usize) -> Option<Pattern> {
    [
        Pattern::Solid,
        Pattern::Stripes,
        Pattern::Dots,
        Pattern::Crosshatch,
        Pattern::Waves,
        Pattern::Checks,
    ]
    .get(index)
    .copied()
}

/// Low/Mid/High for the default three Bands, otherwise "Band N".
pub fn band_label(index: usize, count: usize) -> String {
    match (count, index) {
        (3, 0) => "Low".into(),
        (3, 1) => "Mid".into(),
        (3, 2) => "High".into(),
        _ => format!("Band {}", index + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_ratio_matches_known_wcag_values() {
        let (black, white) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(white, black) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
        // #767676 on white is the classic "just passes AA" grey: 4.54:1.
        assert!((contrast_ratio(Rgb(0x76, 0x76, 0x76), white) - 4.54).abs() < 0.01);
        // #777777 on white just fails: 4.48:1.
        assert!((contrast_ratio(Rgb(0x77, 0x77, 0x77), white) - 4.48).abs() < 0.01);
    }

    #[test]
    fn every_text_pair_in_the_standard_theme_meets_4_5_to_1() {
        for (name, fg, bg) in Theme::standard().text_pairs() {
            let ratio = contrast_ratio(fg, bg);
            assert!(ratio >= 4.5, "{name}: {ratio:.2}:1");
        }
    }

    #[test]
    fn the_high_contrast_theme_meets_7_to_1_for_text() {
        for (name, fg, bg) in Theme::high_contrast().text_pairs() {
            let ratio = contrast_ratio(fg, bg);
            assert!(ratio >= 7.0, "{name}: {ratio:.2}:1");
        }
    }


    #[test]
    fn every_panel_surface_carries_readable_text() {
        for (theme, min) in [(Theme::standard(), 4.5), (Theme::high_contrast(), 7.0)] {
            for (name, bg) in [
                ("panel", theme.panel),
                ("panel header", theme.panel_header),
                ("content", theme.content),
                ("gutter", theme.gutter),
            ] {
                for (what, fg) in [("text", theme.text), ("muted", theme.text_muted)] {
                    let ratio = contrast_ratio(fg, bg);
                    assert!(ratio >= min, "{what} on {name}: {ratio:.2}:1");
                }
            }
        }
    }

    #[test]
    fn every_clip_colour_set_reads_on_its_clip_and_its_tag() {
        for theme in [Theme::standard(), Theme::high_contrast()] {
            for lane in 0..8 {
                let c = theme.clip_colors(lane);
                // The waveform is a graphic: 3:1 against its clip body.
                assert!(contrast_ratio(c.wave, c.body) >= 3.0, "lane {lane} wave");
                assert!(contrast_ratio(c.tag_text, c.tag) >= 4.5, "lane {lane} tag");
            }
            let bus = theme.bus_colors();
            assert!(contrast_ratio(bus.wave, bus.body) >= 3.0, "bus wave");
            assert!(contrast_ratio(bus.tag_text, bus.tag) >= 4.5, "bus tag");
        }
    }

    #[test]
    fn neighbouring_lanes_get_different_clip_colours_and_the_palette_cycles() {
        let t = Theme::standard();
        assert_ne!(t.clip_colors(0).body, t.clip_colors(1).body);
        assert_ne!(t.clip_colors(1).body, t.clip_colors(2).body);
        assert_eq!(t.clip_colors(0), t.clip_colors(4), "the palette wraps after four lanes");
        assert_ne!(t.bus_colors().body, t.clip_colors(0).body);
    }

    #[test]
    fn the_playhead_and_selection_stand_out_from_every_clip_and_panel() {
        let t = Theme::standard();
        for bg in [t.content, t.panel, t.clip_colors(0).body, t.clip_colors(1).body, t.bus_colors().body] {
            assert!(contrast_ratio(t.playhead, bg) >= 3.0, "playhead on {bg:?}");
        }
        for bg in [t.content, t.panel] {
            assert!(contrast_ratio(t.selection, bg) >= 1.8, "selection outline is a highlight, not text");
        }
    }

    #[test]
    fn band_tints_are_distinct_and_carry_a_label_in_both_themes() {
        for theme in [Theme::standard(), Theme::high_contrast()] {
            let tints: Vec<Rgb> = (0..6).map(|i| theme.band_tint(i)).collect();
            for (i, a) in tints.iter().enumerate() {
                assert!(contrast_ratio(theme.text, *a) >= 4.5, "label on Band {i}");
                for b in &tints[i + 1..] {
                    assert_ne!(a, b, "two Bands share a tint");
                }
            }
            assert_eq!(theme.band_tint(6), theme.band_tint(0), "the tints wrap after six");
        }
    }

    #[test]
    fn hairlines_stand_out_from_every_surface_they_sit_on() {
        // WCAG 1.4.11 asks 3:1 for interface component boundaries.
        for (theme, min) in [(Theme::standard(), 3.0), (Theme::high_contrast(), 4.5)] {
            for bg in [theme.surface, theme.inset, theme.mashup_surface] {
                let ratio = contrast_ratio(theme.hairline, bg);
                assert!(ratio >= min, "hairline on {bg:?}: {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn the_mashup_lane_is_deeper_than_a_track_lane_but_not_darker_than_text() {
        let t = Theme::standard();
        assert_ne!(t.mashup_surface, t.surface);
        assert!(luminance(t.mashup_surface) < luminance(t.surface));
    }

    #[test]
    fn raised_and_inset_surfaces_are_distinct() {
        for t in [Theme::standard(), Theme::high_contrast()] {
            assert_ne!(t.surface, t.inset);
        }
    }

    #[test]
    fn at_most_three_items_glow_and_playing_beats_solo_beats_the_rest() {
        use GlowKind::*;
        // indices:   0      1     2     3      4
        let active = [Other, Solo, Playing, Solo, Solo];
        assert_eq!(glowing(&active), vec![1, 2, 3]);
    }

    #[test]
    fn ties_keep_the_earlier_item_and_fewer_than_three_all_glow() {
        use GlowKind::*;
        assert_eq!(glowing(&[Other, Other, Other, Other]), vec![0, 1, 2]);
        assert_eq!(glowing(&[Solo, Playing]), vec![0, 1]);
        assert_eq!(glowing(&[]), Vec::<usize>::new());
    }

    #[test]
    fn every_band_up_to_six_has_its_own_pattern_and_a_seventh_has_none() {
        let patterns: Vec<_> = (0..6).map(|i| band_pattern(i).unwrap()).collect();
        for (i, a) in patterns.iter().enumerate() {
            for b in &patterns[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(band_pattern(6), None);
    }

    #[test]
    fn three_bands_are_called_low_mid_high_and_other_counts_are_numbered() {
        assert_eq!(band_label(0, 3), "Low");
        assert_eq!(band_label(1, 3), "Mid");
        assert_eq!(band_label(2, 3), "High");
        assert_eq!(band_label(0, 4), "Band 1");
        assert_eq!(band_label(3, 4), "Band 4");
        assert_eq!(band_label(5, 6), "Band 6");
    }
}
