//! Small building blocks shared by the panels. They only draw: every value, position and
//! colour is decided by the library (`lane_view`, `inspector_view`, `timeline_view`, `theme`).

use audiobrew::theme::{Rgb, Theme};
use gpui::{Div, FontWeight, SharedString, div, prelude::*, px, relative, rgb};

/// A theme colour as a GPUI colour.
pub fn c(v: Rgb) -> gpui::Rgba {
    rgb(((v.0 as u32) << 16) | ((v.1 as u32) << 8) | v.2 as u32)
}

/// A small filled label, such as a lane tag "A1".
pub fn tag(text: impl Into<SharedString>, background: Rgb, foreground: Rgb) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded_sm()
        .bg(c(background))
        .text_color(c(foreground))
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// A read-only slider: a track with a thumb at `frac` (0.0 to 1.0) of its width.
pub fn slider(t: &Theme, frac: f64) -> Div {
    div()
        .flex_1()
        .min_w(px(20.))
        .h(px(3.))
        .rounded_sm()
        .bg(c(t.border))
        .relative()
        .child(
            div()
                .absolute()
                .top(px(-4.))
                .left(relative(frac.clamp(0.0, 1.0) as f32))
                .ml(px(-5.))
                .w(px(10.))
                .h(px(11.))
                .rounded_sm()
                .bg(c(t.content))
                .border_1()
                .border_color(c(t.hairline)),
        )
}

/// A boxed numeric readout in the monospace face.
pub fn value_box(t: &Theme, text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(5.))
        .py(px(2.))
        .rounded_sm()
        .bg(c(t.content))
        .border_1()
        .border_color(c(t.border))
        .font_family("Menlo")
        .text_size(px(11.))
        .child(text.into())
}

/// A panel's header strip: a small-caps title on the left and optional text on the right.
pub fn panel_header(t: &Theme, title: &str, right: Option<String>) -> Div {
    div()
        .flex_none()
        .h(px(28.))
        .px(px(10.))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .bg(c(t.panel_header))
        .border_b_1()
        .border_color(c(t.border))
        .text_size(px(11.))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
        .when_some(right, |this, text| {
            this.child(div().text_color(c(t.text_muted)).child(text))
        })
}

/// An empty panel with Resolve's thin outline.
pub fn panel(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(4.))
        .bg(c(t.panel))
        .border_1()
        .border_color(c(t.border))
}

/// A row of waveform bars, one per level (0.0 to 1.0), centred vertically.
pub fn bars(levels: Vec<f32>, color: Rgb) -> Div {
    div()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.))
        .children(levels.into_iter().map(move |level| {
            div()
                .flex_1()
                .h(relative(level.clamp(0.04, 1.0)))
                .rounded_sm()
                .bg(c(color))
        }))
}

/// A small square button face with a letter (S, M), lit when `on`.
pub fn letter_button(t: &Theme, letter: &str, on: bool, lit: Rgb) -> Div {
    div()
        .flex_none()
        .w(px(22.))
        .h(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(c(t.hairline))
        .bg(c(if on { lit } else { t.content }))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(letter.to_string())
}
