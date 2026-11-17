//! The audioBrew window, laid out like a light DaVinci Resolve: Media Pool, Mashup monitor and
//! Inspector above, a timeline below, and a page bar along the bottom.
//!
//! This file only draws. What is shown and where it goes is decided, and tested, in the library:
//! `lane_view`, `inspector_view`, `timeline_view`, `mashup_preview`, `waveform` and `theme`.

mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use audiobrew::badge::KeyBadge;
use audiobrew::ingest::{Ingestor, assign_dropped_paths};
use audiobrew::inspector_view::inspector_view;
use audiobrew::key::Compatibility;
use audiobrew::lane_view::{EMPTY_PROMPT, LaneView, lane_views};
use audiobrew::mashup_preview::{PreviewTrack, mashup_preview};
use audiobrew::pipeline::{FileLoader, ThreadExecutor};
use audiobrew::project::{LaneId, Project};
use audiobrew::readout::format_bpm;
use audiobrew::theme::{ClipColors, Theme};
use audiobrew::timeline_view::{ClipView, Geometry, Span, TimelineView, timeline_view};
use audiobrew::waveform::{peaks, visible_bars};
use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, ExternalPaths, FontWeight,
    PathPromptOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, relative, size,
};
use widgets::{bars, c, letter_button, panel, panel_header, slider, tag, value_box};

/// Sample rate the analysis assumes until the audio device is connected.
const ANALYSIS_RATE: u32 = 44_100;
/// Width of a timeline track header.
const HEADER_W: f32 = 230.0;
/// Height of a Track lane.
const LANE_H: f32 = 94.0;
/// Space before time 0 and after the End marker in a lane.
const PAD_LEFT: f64 = 40.0;
const PAD_RIGHT: f64 = 60.0;
/// Columns in the Mashup preview before it is resampled to the width it is drawn at.
const PREVIEW_COLUMNS: usize = 320;

struct AppView {
    project: Project,
    theme: Theme,
    ingest: Ingestor,
    /// Waveform picture of each loaded lane.
    waveforms: HashMap<LaneId, Vec<f32>>,
    /// The Track the Inspector shows.
    selected: usize,
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
            selected: 0,
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
        if let Some(last) = report.placed.last().and_then(|id| self.project.lane_index(*id)) {
            self.selected = last;
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

    fn pick_files(&mut self, target: Option<usize>, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                this.update(cx, |view, cx| view.drop_paths(&paths, target, cx)).ok();
            }
        })
        .detach();
    }

    fn picture(&self, lane: usize) -> Option<&Vec<f32>> {
        self.waveforms.get(&self.project.lane_id(lane)?)
    }

    // ---- title bar ---------------------------------------------------------------------

    fn title_bar(&self) -> Div {
        let t = self.theme;
        div()
            .flex_none()
            .h(px(38.))
            .px(px(14.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .bg(c(t.panel_header))
            .border_b_1()
            .border_color(c(t.border))
            .child(div().w(px(300.)).font_weight(FontWeight::BOLD).text_size(px(13.)).child("audioBrew"))
            .child(div().text_size(px(13.)).text_color(c(t.text_muted)).child("Untitled Mashup"))
            .child(
                div()
                    .w(px(300.))
                    .flex()
                    .justify_end()
                    .text_size(px(11.))
                    .text_color(c(t.text_muted))
                    .child(self.notice.clone().unwrap_or_default()),
            )
    }

    // ---- media pool --------------------------------------------------------------------

    fn media_pool(&self, views: &[LaneView], cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let loaded = views.iter().filter(|v| !v.is_empty()).count();
        let mut list = div().flex_1().p(px(8.)).flex().flex_col().gap(px(8.));
        for (i, view) in views.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
            let colors = t.clip_colors(i);
            let thumb = self
                .picture(i)
                .map_or_else(Vec::new, |p| visible_bars(p, 1.0, 24));
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .p(px(6.))
                    .rounded(px(5.))
                    .bg(c(t.content))
                    .border_1()
                    .border_color(c(t.border))
                    .child(
                        div()
                            .flex_none()
                            .w(px(78.))
                            .h(px(44.))
                            .rounded_sm()
                            .bg(c(colors.body))
                            .border_1()
                            .border_color(c(colors.wave))
                            .px(px(3.))
                            .py(px(4.))
                            .child(bars(thumb, colors.wave)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(view.file_name.clone().unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(c(t.text_muted))
                                    .child(format!(
                                        "{}{}",
                                        view.tempo.clone().unwrap_or_default(),
                                        view.key.as_ref().map_or(String::new(), |k| format!(" · {}", k.label))
                                    )),
                            )
                            .child(div().flex().child(tag(
                                format!("A{}", view.number),
                                colors.tag,
                                colors.tag_text,
                            ))),
                    ),
            );
        }
        let drop_zone = div()
            .id("media-drop")
            .flex_1()
            .min_h(px(70.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .rounded(px(6.))
            .border_2()
            .border_dashed()
            .border_color(c(t.hairline))
            .text_color(c(t.text_muted))
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.pick_files(None, cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _w, cx| {
                this.drop_paths(paths.paths(), None, cx)
            }))
            .child("Drop a song here")
            .child(div().text_size(px(11.)).child("or click to load"));

        panel(&t)
            .w(px(250.))
            .flex_none()
            .child(panel_header(&t, "MEDIA POOL", Some(format!("{loaded} songs"))))
            .child(list.child(drop_zone))
    }

    // ---- monitor -----------------------------------------------------------------------

    fn monitor(&self, loaded: usize, tv: &TimelineView, preview: &[f32]) -> Div {
        let t = self.theme;
        let bus = t.bus_colors();
        let right = match self.project.mashup_bpm() {
            Some(bpm) if loaded > 0 => format!("{loaded} tracks matched to {}", format_bpm(bpm)),
            _ => "no tracks yet".to_string(),
        };
        let screen = div()
            .flex_1()
            .m(px(8.))
            .relative()
            .rounded_sm()
            .bg(c(t.content))
            .border_1()
            .border_color(c(t.border))
            .overflow_hidden();
        let screen = if tv.end_secs > 0.0 {
            screen
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .px(px(12.))
                        .py(px(24.))
                        .child(bars(peaks(preview, 1, 190), bus.wave)),
                )
                .child(div().absolute().left(px(0.)).top(px(0.)).bottom(px(0.)).w(px(2.)).bg(c(t.playhead)))
        } else {
            screen.flex().items_center().justify_center().text_color(c(t.text_muted)).child(
                "Drop two songs to brew a mashup",
            )
        };
        let screen = screen
            .child(
                div()
                    .absolute()
                    .right(px(10.))
                    .top(px(8.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded_md()
                    .bg(c(t.panel))
                    .font_family("Menlo")
                    .text_size(px(20.))
                    .child("00:00.0"),
            );

        let dim = |label: &'static str| {
            div()
                .flex_none()
                .px(px(14.))
                .py(px(6.))
                .rounded_sm()
                .border_1()
                .border_color(c(t.hairline))
                .bg(c(t.content))
                .opacity(0.55)
                .child(label)
        };
        panel(&t)
            .flex_1()
            .child(panel_header(&t, "MASHUP", Some(right)))
            .child(screen)
            .child(
                div()
                    .flex_none()
                    .h(px(46.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .border_t_1()
                    .border_color(c(t.border))
                    .child(dim("Start"))
                    .child(dim("Play"))
                    .child(dim("Loop"))
                    .child(
                        div()
                            .ml(px(10.))
                            .text_size(px(11.))
                            .text_color(c(t.text_muted))
                            .child("Playback connects next"),
                    ),
            )
    }

    // ---- inspector ---------------------------------------------------------------------

    fn inspector(&self) -> Div {
        let t = self.theme;
        let Some(v) = inspector_view(&self.project, self.selected) else {
            return panel(&t)
                .w(px(340.))
                .flex_none()
                .child(panel_header(&t, "INSPECTOR", None))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(c(t.text_muted))
                        .child("Select a track to see its settings"),
                );
        };
        let row = |label: &str, content: Div| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(56.)).flex_none().child(label.to_string()))
                .child(content)
        };
        let section = |title: &str| {
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(t.text_muted))
                .mt(px(2.))
                .child(title.to_string())
        };

        let zones = v.bands.iter().enumerate().map(|(i, b)| {
            div()
                .absolute()
                .top(px(0.))
                .bottom(px(0.))
                .left(relative(b.from as f32))
                .w(relative((b.to - b.from) as f32))
                .bg(c(t.band_tint(i)))
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .top(px(4.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(b.name.clone()),
                )
                .child(
                    // the Band's gain as a line: higher is louder
                    div()
                        .absolute()
                        .left(px(0.))
                        .right(px(0.))
                        .top(relative((1.0 - b.gain_frac) as f32))
                        .h(px(2.))
                        .bg(c(t.text)),
                )
        });
        let splits = v.bands.iter().take(v.bands.len().saturating_sub(1)).map(|b| {
            div()
                .absolute()
                .top(px(0.))
                .bottom(px(0.))
                .left(relative(b.to as f32))
                .w(px(2.))
                .bg(c(t.text))
                .child(
                    div()
                        .absolute()
                        .top(px(44.))
                        .ml(px(-8.))
                        .w(px(16.))
                        .h(px(16.))
                        .rounded_full()
                        .bg(c(t.content))
                        .border_2()
                        .border_color(c(t.text)),
                )
        });
        let graph = div()
            .flex_none()
            .relative()
            .h(px(104.))
            .rounded_sm()
            .overflow_hidden()
            .border_1()
            .border_color(c(t.border))
            .bg(c(t.content))
            .children(zones)
            .children(splits);

        let band_rows = v.bands.iter().enumerate().map(|(i, b)| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_none()
                        .size(px(10.))
                        .rounded_sm()
                        .bg(c(t.band_tint(i)))
                        .border_1()
                        .border_color(c(t.hairline)),
                )
                .child(div().w(px(38.)).flex_none().font_weight(FontWeight::SEMIBOLD).child(b.name.clone()))
                .child(slider(&t, b.gain_frac))
                .child(div().w(px(54.)).flex_none().font_family("Menlo").text_size(px(11.)).child(b.gain_db.clone()))
                .child(letter_button(&t, "M", b.muted, t.status_bad))
                .child(letter_button(&t, "S", b.soloed, t.selection))
        });

        panel(&t)
            .w(px(340.))
            .flex_none()
            .child(panel_header(&t, "INSPECTOR", Some(format!("A{} · {}", v.lane_number, v.file_name))))
            .child(
                div()
                    .flex_1()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .overflow_hidden()
                    .child(section("TIMING"))
                    .child(row("Speed", value_box(&t, v.speed.clone())))
                    .child(row("Nudge", value_box(&t, v.nudge.clone())))
                    .child(section("LEVEL AND KEY"))
                    .child(row(
                        "Level",
                        div().flex_1().flex().flex_row().items_center().gap(px(8.)).child(slider(&t, v.level_frac)).child(value_box(&t, v.level.clone())),
                    ))
                    .child(row("Pitch", value_box(&t, v.pitch.clone())))
                    .child(row(
                        "Key",
                        div().flex().when_some(v.key.as_ref(), |d, k| d.child(key_tag(&t, k))),
                    ))
                    .child(section("BANDS"))
                    .child(graph)
                    .children(band_rows),
            )
    }

    // ---- timeline ----------------------------------------------------------------------

    fn timeline(
        &self,
        views: &[LaneView],
        tv: &TimelineView,
        preview: &[f32],
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let loaded = views.iter().filter(|v| !v.is_empty()).count();

        let ruler = div()
            .flex_none()
            .h(px(34.))
            .flex()
            .flex_row()
            .border_b_1()
            .border_color(c(t.border))
            .child(
                div()
                    .w(px(HEADER_W))
                    .flex_none()
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .bg(c(t.panel_header))
                    .border_r_1()
                    .border_color(c(t.border))
                    .font_family("Menlo")
                    .text_size(px(13.))
                    .child("00:00.0"),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .relative()
                    .overflow_hidden()
                    .bg(c(t.panel_header))
                    .children(tv.ticks.iter().map(|tick| {
                        div()
                            .absolute()
                            .left(px(tick.x as f32))
                            .bottom(px(0.))
                            .w(px(1.))
                            .h(px(if tick.major { 20. } else { 8. }))
                            .bg(c(t.text_muted))
                    }))
                    .children(tv.ticks.iter().filter_map(|tick| {
                        tick.label.as_ref().map(|label| {
                            div()
                                .absolute()
                                .left(px(tick.x as f32 + 3.0))
                                .top(px(2.))
                                .text_size(px(10.))
                                .child(label.clone())
                        })
                    }))
                    .when(tv.end_secs > 0.0, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left(px(tv.end_x as f32))
                                .top(px(0.))
                                .bottom(px(0.))
                                .w(px(2.))
                                .bg(c(t.text)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(tv.end_x as f32 + 3.0))
                                .top(px(10.))
                                .child(tag("END", t.text, t.content)),
                        )
                    }),
            );

        let mut lanes = div().flex_1().relative().flex().flex_col();
        for (i, view) in views.iter().enumerate() {
            lanes = lanes.child(self.lane_row(i, view, tv.lanes.get(i).and_then(|l| l.as_ref()), tv, cx));
        }
        lanes = lanes.child(self.bus_row(tv, preview));
        // the playhead, resting at the start until playback is connected
        lanes = lanes.child(
            div()
                .absolute()
                .left(px(HEADER_W + PAD_LEFT as f32))
                .top(px(0.))
                .bottom(px(0.))
                .w(px(2.))
                .bg(c(t.playhead)),
        );

        panel(&t)
            .flex_1()
            .mx(px(3.))
            .child(panel_header(
                &t,
                "TIMELINE",
                Some(format!("{loaded} track{}", if loaded == 1 { "" } else { "s" })),
            ))
            .child(ruler)
            .child(lanes)
    }

    fn lane_row(
        &self,
        index: usize,
        view: &LaneView,
        clips: Option<&ClipView>,
        tv: &TimelineView,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = self.theme;
        let colors = t.clip_colors(index);
        let selected = index == self.selected && !view.is_empty();
        let level = inspector_view(&self.project, index);

        let mut header = div()
            .id(("track-head", index))
            .relative()
            .w(px(HEADER_W))
            .flex_none()
            .h_full()
            .px(px(10.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(6.))
            .bg(c(t.panel_header))
            .border_r_1()
            .border_b_1()
            .border_color(c(t.border))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                this.selected = index;
                cx.notify();
            }));
        if selected {
            header = header.child(
                div().absolute().left(px(0.)).top(px(0.)).bottom(px(0.)).w(px(3.)).bg(c(t.selection)),
            );
        }
        header = header.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(7.))
                .child(tag(format!("A{}", view.number), colors.tag, colors.tag_text))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(view.file_name.clone().unwrap_or_else(|| "Empty".to_string())),
                ),
        );
        header = if view.is_empty() {
            header.child(div().text_size(px(11.)).text_color(c(t.text_muted)).child(EMPTY_PROMPT))
        } else {
            header
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_family("Menlo")
                                .text_size(px(11.))
                                .child(view.tempo.clone().unwrap_or_default()),
                        )
                        .when_some(view.key.as_ref(), |d, k| d.child(key_tag(&t, k))),
                )
                .when_some(level, |d, v| {
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(slider(&t, v.level_frac))
                            .child(div().w(px(56.)).font_family("Menlo").text_size(px(11.)).child(v.level)),
                    )
                })
        };

        let mut cell = div()
            .id(("lane", index))
            .flex_1()
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(c(t.content))
            .border_b_1()
            .border_color(c(t.border))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _w, cx| {
                this.drop_paths(paths.paths(), Some(index), cx)
            }))
            .children(tv.ticks.iter().filter(|tick| tick.major).map(|tick| {
                div().absolute().left(px(tick.x as f32)).top(px(0.)).bottom(px(0.)).w(px(1.)).bg(c(t.panel_header))
            }));

        match clips {
            Some(clips) => {
                let picture = self.picture(index).cloned().unwrap_or_default();
                let name = view.file_name.clone().unwrap_or_default();
                cell = cell.child(clip_box(&t, &colors, &clips.clip, &picture, clips.full_w, selected, false, Some(name)));
                for repeat in &clips.repeats {
                    cell = cell.child(clip_box(&t, &colors, repeat, &picture, clips.full_w, false, true, None));
                }
            }
            None => {
                cell = cell
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.pick_files(Some(index), cx)))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .m(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_dashed()
                            .border_color(c(t.hairline))
                            .text_color(c(t.text_muted))
                            .child(EMPTY_PROMPT),
                    );
            }
        }

        div().flex_none().h(px(LANE_H)).flex().flex_row().child(header).child(cell)
    }

    fn bus_row(&self, tv: &TimelineView, preview: &[f32]) -> Div {
        let t = self.theme;
        let bus = t.bus_colors();
        let width = (tv.end_x - PAD_LEFT).max(0.0);
        let lane = div()
            .flex_1()
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(c(t.content))
            .when(tv.end_secs > 0.0, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(PAD_LEFT as f32))
                        .w(px(width as f32))
                        .top(px(8.))
                        .bottom(px(8.))
                        .rounded(px(4.))
                        .bg(c(bus.body))
                        .border_1()
                        .border_color(c(bus.wave))
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .px(px(4.))
                                .py(px(14.))
                                .child(bars(peaks(preview, 1, ((width / 4.0) as usize).max(1)), bus.wave)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(6.))
                                .top(px(3.))
                                .text_size(px(10.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Mashup"),
                        ),
                )
            });
        div()
            .flex_1()
            .flex()
            .flex_row()
            .child(
                div()
                    .w(px(HEADER_W))
                    .flex_none()
                    .h_full()
                    .px(px(10.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(6.))
                    .bg(c(t.panel_header))
                    .border_r_1()
                    .border_color(c(t.border))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(7.))
                            .child(tag("BUS", bus.tag, bus.tag_text))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Mashup")),
                    )
                    .child(div().text_size(px(11.)).text_color(c(t.text_muted)).child("Limiter on")),
            )
            .child(lane)
    }

    // ---- page bar ----------------------------------------------------------------------

    fn page_bar(&self) -> Div {
        let t = self.theme;
        let page = |label: &'static str, active: bool| {
            div()
                .flex_none()
                .px(px(16.))
                .py(px(6.))
                .rounded_full()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .border_1()
                .border_color(c(t.hairline))
                .bg(c(if active { t.text } else { t.content }))
                .text_color(c(if active { t.content } else { t.text }))
                .when(!active, |d| d.opacity(0.55))
                .child(label)
        };
        div()
            .flex_none()
            .h(px(54.))
            .px(px(16.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .bg(c(t.panel_header))
            .border_t_1()
            .border_color(c(t.border))
            .child(div().w(px(300.)).font_weight(FontWeight::SEMIBOLD).child("audioBrew"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(page("Media", false))
                    .child(page("Mix", true))
                    .child(page("Export · soon", false)),
            )
            .child(
                div()
                    .w(px(300.))
                    .flex()
                    .justify_end()
                    .text_size(px(11.))
                    .text_color(c(t.text_muted))
                    .child("Drop a song anywhere to add a track"),
            )
    }
}

/// A clip on a lane: its body, the visible part of its waveform, and an optional name.
#[allow(clippy::too_many_arguments)]
fn clip_box(
    t: &Theme,
    colors: &ClipColors,
    span: &Span,
    picture: &[f32],
    full_w: f64,
    selected: bool,
    ghost: bool,
    name: Option<String>,
) -> Div {
    let fraction = if full_w > 0.0 { span.w / full_w } else { 1.0 };
    let count = ((span.w / 4.0) as usize).max(1);
    let mut clip = div()
        .absolute()
        .left(px(span.x as f32))
        .w(px(span.w as f32))
        .top(px(8.))
        .h(px(LANE_H - 16.))
        .rounded(px(4.))
        .overflow_hidden()
        .bg(c(colors.body))
        .border_1()
        .border_color(c(colors.wave))
        .child(
            div()
                .absolute()
                .inset_0()
                .px(px(4.))
                .py(px(16.))
                .child(bars(visible_bars(picture, fraction, count), colors.wave)),
        );
    if selected {
        clip = clip.border_2().border_color(c(t.selection));
    }
    if ghost {
        clip = clip.opacity(0.5).border_dashed();
    }
    match name {
        Some(name) => clip.child(
            div()
                .absolute()
                .left(px(6.))
                .top(px(3.))
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(name),
        ),
        None => clip,
    }
}

/// The Key tag: the Camelot code, with a word beside it so colour is never the only cue.
fn key_tag(t: &Theme, badge: &KeyBadge) -> Div {
    let (background, word) = match badge.compatibility {
        Some(Compatibility::Green) => (t.status_ok, " · fits"),
        Some(Compatibility::Amber) => (t.status_warn, " · risky"),
        Some(Compatibility::Red) => (t.status_bad, " · clashes"),
        None => (t.panel_header, ""),
    };
    let foreground = if badge.compatibility.is_some() { t.text_on_status } else { t.text };
    tag(format!("{}{}", badge.label, word), background, foreground)
        .border_1()
        .border_color(c(t.hairline))
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let width = f64::from(f32::from(window.viewport_size().width));
        let lane_width = (width - 8.0 - f64::from(HEADER_W)).max(300.0);
        let views = lane_views(&self.project);
        let tv = timeline_view(
            &self.project,
            Geometry { width: lane_width, pad_left: PAD_LEFT, pad_right: PAD_RIGHT },
        );

        // The Mashup preview, mixed from each loaded Track's waveform picture.
        let mut inputs = Vec::new();
        for (lane, clips) in tv.lanes.iter().enumerate() {
            if let (Some(clips), Some(track), Some(picture)) =
                (clips, self.project.track(lane), self.picture(lane))
            {
                inputs.push(PreviewTrack {
                    peaks: picture,
                    length_secs: clips.full_w / tv.px_per_sec,
                    offset_secs: (clips.clip.x - PAD_LEFT) / tv.px_per_sec,
                    gain: track.fader,
                });
            }
        }
        let preview = mashup_preview(&inputs, tv.end_secs, PREVIEW_COLUMNS);
        let loaded = views.iter().filter(|v| !v.is_empty()).count();

        div()
            .id("root")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(3.))
            .bg(c(t.gutter))
            .text_color(c(t.text))
            .text_size(px(12.))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _w, cx| {
                this.drop_paths(paths.paths(), None, cx)
            }))
            .child(self.title_bar())
            .child(
                div()
                    .flex_none()
                    .h(px(352.))
                    .mx(px(3.))
                    .flex()
                    .flex_row()
                    .gap(px(3.))
                    .child(self.media_pool(&views, cx))
                    .child(self.monitor(loaded, &tv, &preview))
                    .child(self.inspector()),
            )
            .child(self.timeline(&views, &tv, &preview, cx))
            .child(self.page_bar())
    }
}

pub fn run() {
    // Files named on the command line are loaded as if dropped on the window.
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1360.), px(880.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(AppView::new),
            )
            .unwrap();
        if !paths.is_empty() {
            window
                .update(cx, |view, _window, cx| view.drop_paths(&paths, None, cx))
                .ok();
        }
        cx.activate(true);
    });
}
