//! Small drawing helpers shared by the window: colours, icons, tooltips.

use std::borrow::Cow;

use audiobrew::theme::{Hex, Theme};
use gpui::{
    AnyView, App, AssetSource, Context, FontWeight, IntoElement, Render, SharedString, Svg, Window,
    div, prelude::*, px, rgb, rgba, svg,
};

/// A theme colour as a GPUI colour.
pub fn c(hex: Hex) -> gpui::Rgba {
    rgb(hex)
}

/// A theme colour with an alpha (0..1).
pub fn ca(hex: Hex, alpha: f32) -> gpui::Rgba {
    rgba((hex << 8) | ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32))
}

/// The icon SVGs in `assets/icons`, built into the binary.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/mute.svg" => include_bytes!("../../assets/icons/mute.svg"),
            "icons/headphones.svg" => include_bytes!("../../assets/icons/headphones.svg"),
            "icons/volume.svg" => include_bytes!("../../assets/icons/volume.svg"),
            "icons/pan.svg" => include_bytes!("../../assets/icons/pan.svg"),
            "icons/reset.svg" => include_bytes!("../../assets/icons/reset.svg"),
            "icons/trash.svg" => include_bytes!("../../assets/icons/trash.svg"),
            "icons/plus.svg" => include_bytes!("../../assets/icons/plus.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// A 14px monochrome icon in `color`.
pub fn icon(name: &'static str, color: Hex) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(14.))
        .flex_none()
        .text_color(c(color))
}

/// Hover help: a bold heading and a few plain lines. Drawn by us, so it looks the same on
/// every platform.
pub struct Tooltip {
    heading: SharedString,
    lines: Vec<SharedString>,
    theme: Theme,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .max_w(px(280.))
            .px(px(9.))
            .py(px(7.))
            .rounded(px(6.))
            .border_1()
            .border_color(c(t.tooltip_border))
            .bg(c(t.tooltip_bg))
            .text_color(c(t.tooltip_text))
            .text_size(px(11.))
            .line_height(px(16.))
            .shadow_md()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.heading.clone()),
            )
            .children(
                self.lines
                    .iter()
                    .map(|l| div().text_color(c(t.tooltip_muted)).child(l.clone())),
            )
    }
}

/// Builder for `.tooltip(...)`: the first line of `text` is the heading.
pub fn tip(theme: Theme, text: impl Into<String>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text: String = text.into();
    move |_window, cx| {
        let mut lines = text.lines().map(|l| SharedString::from(l.to_string()));
        let heading = lines.next().unwrap_or_default();
        let lines = lines.collect();
        cx.new(|_| Tooltip {
            heading,
            lines,
            theme,
        })
        .into()
    }
}
