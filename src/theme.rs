//! Colour tokens for light and dark, matching the web demo (`demo` branch, ui/src/index.css).

/// A colour as 0xRRGGBB.
pub type Hex = u32;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg: Hex,
    pub surface: Hex,
    pub surface_2: Hex,
    pub lane: Hex,
    pub lane_selected: Hex,
    pub border: Hex,
    pub grid_bar: Hex,
    pub grid_beat: Hex,
    pub control: Hex,
    pub control_hover: Hex,
    pub text: Hex,
    pub text_muted: Hex,
    pub mute: Hex,
    pub mute_text: Hex,
    pub solo: Hex,
    pub solo_text: Hex,
    pub meter: Hex,
    pub focus: Hex,
    pub danger: Hex,
    pub tooltip_bg: Hex,
    pub tooltip_border: Hex,
    pub tooltip_text: Hex,
    pub tooltip_muted: Hex,
}

/// One colour per Track (at most six Tracks).
pub const TRACK_COLORS: [Hex; 6] = [0x4f8cff, 0xff7a45, 0x36c08e, 0xc26bff, 0xffbf3c, 0xff5c8a];

impl Theme {
    pub const LIGHT: Theme = Theme {
        bg: 0xf4f4f5,
        surface: 0xffffff,
        surface_2: 0xfafafa,
        lane: 0xf7f7f8,
        lane_selected: 0xeef3ff,
        border: 0xe4e4e7,
        grid_bar: 0xdcdce0,
        grid_beat: 0xececef,
        control: 0xf0f0f2,
        control_hover: 0xe6e6e9,
        text: 0x18181b,
        text_muted: 0x6b6b73,
        mute: 0xf0a020,
        mute_text: 0x1a1200,
        solo: 0x2fb36b,
        solo_text: 0x00170b,
        meter: 0x2fb36b,
        focus: 0x4f8cff,
        danger: 0xe5484d,
        tooltip_bg: 0x18181b,
        tooltip_border: 0x18181b,
        tooltip_text: 0xfafafa,
        tooltip_muted: 0xc4c4cc,
    };

    pub const DARK: Theme = Theme {
        bg: 0x0f0f11,
        surface: 0x1a1a1d,
        surface_2: 0x202024,
        lane: 0x141416,
        lane_selected: 0x1a2233,
        border: 0x2a2a2e,
        grid_bar: 0x2e2e33,
        grid_beat: 0x1f1f23,
        control: 0x26262b,
        control_hover: 0x303036,
        text: 0xf4f4f5,
        text_muted: 0x9a9aa3,
        tooltip_bg: 0x2a2a30,
        tooltip_border: 0x3a3a41,
        ..Theme::LIGHT
    };

    pub fn track_color(index: usize) -> Hex {
        TRACK_COLORS[index % TRACK_COLORS.len()]
    }
}
