//! The audioBrew window. Views only lay out values that `audiobrew` (the library) has already
//! decided and tested: lane titles and readouts come from `lane_view`, drops from
//! `ingest::assign_dropped_paths`, colours from `theme::Theme`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use audiobrew::ingest::{Ingestor, assign_dropped_paths};
use audiobrew::key::Compatibility;
use audiobrew::lane_view::{EMPTY_PROMPT, LaneView, lane_views};
use audiobrew::pipeline::{FileLoader, ThreadExecutor};
use audiobrew::project::{LaneId, Project};
use audiobrew::readout::format_bpm;
use audiobrew::theme::{Rgb, Theme};
use audiobrew::waveform::peaks;
use gpui::{
    App, Application, Bounds, BoxShadow, ClickEvent, Context, ExternalPaths, FontWeight,
    PathPromptOptions, Window, WindowBounds, WindowOptions, div, hsla, point, prelude::*, px,
    relative, rgb, size,
};

/// Sample rate the analysis assumes until the audio device is connected.
const ANALYSIS_RATE: u32 = 44_100;
/// How many waveform bars a lane draws.
const WAVEFORM_BARS: usize = 180;

fn color(c: Rgb) -> gpui::Rgba {
    rgb(((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32)
}

/// The neomorphic raised look: a light shadow up-left and a soft dark one down-right.
fn raised() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0.0, 0.0, 1.0, 0.95),
            offset: point(px(-5.), px(-5.)),
            blur_radius: px(10.),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: hsla(0.58, 0.18, 0.55, 0.38),
            offset: point(px(5.), px(5.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
        },
    ]
}

struct AppView {
    project: Project,
    theme: Theme,
    ingest: Ingestor,
    /// Waveform picture of each loaded lane.
    waveforms: HashMap<LaneId, Vec<f32>>,
    /// The latest thing worth telling the user, such as a file that would not load.
    notice: Option<String>,
}

impl AppView {
    fn new(cx: &mut Context<Self>) -> Self {
        // Collect finished background analyses a few times a second.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if this.update(cx, |view, cx| view.poll(cx)).is_err() {
                    break; // the window is gone
                }
            }
        })
        .detach();

        Self {
            project: Project::new(),
            theme: Theme::standard(),
            ingest: Ingestor::new(Arc::new(FileLoader), Arc::new(ThreadExecutor), ANALYSIS_RATE),
            waveforms: HashMap::new(),
            notice: None,
        }
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let report = self.ingest.poll(&mut self.project);
        if report.placed.is_empty() && report.failed.is_empty() {
            return;
        }
        for (lane, picture) in report.peaks {
            self.waveforms.insert(lane, picture);
        }
        self.notice = report.failed.first().map(|(_, reason)| reason.clone());
        cx.notify();
    }

    fn drop_paths(&mut self, paths: &[PathBuf], target: Option<usize>, cx: &mut Context<Self>) {
        let jobs = assign_dropped_paths(&mut self.project, paths, target);
        if jobs.is_empty() {
            self.notice = Some("That wasn't an audio file we can open.".into());
        } else {
            self.notice = None;
            self.ingest.drop_files(jobs);
        }
        cx.notify();
    }

    fn pick_files(&mut self, lane: usize, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                this.update(cx, |view, cx| view.drop_paths(&paths, Some(lane), cx)).ok();
            }
        })
        .detach();
    }

    fn header(&self) -> impl IntoElement {
        let t = self.theme;
        let speed = self.project.mashup_bpm().map_or_else(|| "—".to_string(), format_bpm);
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_5()
            .h(px(64.))
            .rounded_2xl()
            .bg(color(t.surface))
            .border_1()
            .border_color(color(t.hairline))
            .shadow(raised())
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child("audioBrew"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(color(t.text_muted))
                    .child("Speed")
                    .child(div().text_color(color(t.text)).font_weight(FontWeight::SEMIBOLD).child(speed)),
            )
    }

    fn key_badge(&self, view: &LaneView) -> Option<gpui::AnyElement> {
        let t = self.theme;
        let badge = view.key.as_ref()?;
        let (background, word) = match badge.compatibility {
            Some(Compatibility::Green) => (t.status_ok, " · fits"),
            Some(Compatibility::Amber) => (t.status_warn, " · risky"),
            Some(Compatibility::Red) => (t.status_bad, " · clashes"),
            None => (t.inset, ""),
        };
        Some(
            div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .border_1()
                .border_color(color(t.hairline))
                .bg(color(background))
                .text_sm()
                .text_color(color(if badge.compatibility.is_some() { t.text_on_status } else { t.text }))
                .child(format!("Key {}{}", badge.label, word))
                .into_any_element(),
        )
    }

    fn lane_row(&self, index: usize, view: &LaneView, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let lane_id = self.project.lane_id(index);
        let picture = lane_id.and_then(|id| self.waveforms.get(&id));

        let header = div()
            .w(px(240.))
            .flex_none()
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .p_3()
            .rounded_2xl()
            .bg(color(t.surface))
            .border_1()
            .border_color(color(t.hairline))
            .shadow(raised())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .size_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(color(t.inset))
                            .border_1()
                            .border_color(color(t.hairline))
                            .text_sm()
                            .child(view.number.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(view.file_name.clone().unwrap_or_else(|| "Empty".to_string())),
                    ),
            )
            .when_some(view.tempo.clone(), |this, tempo| {
                this.child(div().text_sm().text_color(color(t.text_muted)).child(tempo))
            })
            .when_some(self.key_badge(view), |this, badge| this.child(div().flex().child(badge)));

        let well = div()
            .id(("lane-well", index))
            .flex_1()
            .h_full()
            .rounded_2xl()
            .bg(color(t.inset))
            .border_1()
            .border_color(color(t.hairline))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| this.pick_files(index, cx)))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _window, cx| {
                this.drop_paths(paths.paths(), Some(index), cx)
            }))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .map(|this| match picture {
                Some(picture) if !view.is_empty() => this.child(self.waveform(picture)),
                _ => this.child(
                    div()
                        .text_color(color(t.text_muted))
                        .child(if view.is_empty() { EMPTY_PROMPT } else { "Analysing…" }),
                ),
            });

        div()
            .id(("lane", index))
            .flex()
            .flex_row()
            .gap_4()
            .h(px(112.))
            .child(header)
            .child(well)
    }

    fn waveform(&self, picture: &[f32]) -> impl IntoElement {
        let t = self.theme;
        let bars = peaks(picture, 1, WAVEFORM_BARS);
        div()
            .size_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(1.))
            .px_3()
            .children(bars.into_iter().map(move |level| {
                div()
                    .flex_1()
                    .h(relative(level.clamp(0.03, 1.0)))
                    .rounded_sm()
                    .bg(color(t.text_muted))
            }))
    }

    fn mashup_row(&self) -> impl IntoElement {
        let t = self.theme;
        let loaded = (0..self.project.lane_count())
            .filter(|&i| self.project.track(i).is_some())
            .count();
        let message = match loaded {
            0 => "Drop two songs to brew a mashup",
            1 => "Add a second song to start the mashup",
            _ => "Mashup preview and playback aren't connected yet",
        };
        div()
            .flex()
            .flex_row()
            .gap_4()
            .h(px(132.))
            .child(
                div()
                    .w(px(240.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .p_3()
                    .rounded_2xl()
                    .bg(color(t.mashup_surface))
                    .border_1()
                    .border_color(color(t.hairline))
                    .shadow(raised())
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child("Mashup"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(color(t.text_muted))
                            .child(format!("{loaded} track{} loaded", if loaded == 1 { "" } else { "s" })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_2xl()
                    .bg(color(t.mashup_surface))
                    .border_1()
                    .border_color(color(t.hairline))
                    .text_color(color(t.text_muted))
                    .child(message),
            )
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let views = lane_views(&self.project);
        let lanes: Vec<_> = views
            .iter()
            .enumerate()
            .map(|(i, v)| self.lane_row(i, v, cx).into_any_element())
            .collect();

        div()
            .id("root")
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_5()
            .bg(color(t.surface))
            .text_color(color(t.text))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                this.drop_paths(paths.paths(), None, cx)
            }))
            .child(self.header())
            .children(lanes)
            .child(self.mashup_row())
            .when_some(self.notice.clone(), |this, notice| {
                this.child(div().text_sm().text_color(color(t.text_muted)).child(notice))
            })
    }
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(AppView::new),
        )
        .unwrap();
        cx.activate(true);
    });
}

