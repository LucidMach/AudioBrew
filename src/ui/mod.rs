//! The audioBrew window: DAW-style Track headers beside a beat-grid timeline, matching the web
//! demo on the `demo` branch. This file only draws and routes input; the maths and rules
//! (fader taper, typed values, nudging, break points, track limits) live in the library.

mod widgets;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use audiobrew::mixer::{
    self, FADER_UNITY, PAN_NUDGE, PAN_NUDGE_FINE, VOLUME_NUDGE_DB, VOLUME_NUDGE_FINE_DB,
};
use audiobrew::theme::Theme;
use audiobrew::tracks::{MAX_TRACKS, Track, TrackId, Tracks, is_audio_file};
use gpui::{
    AnyElement, App, Application, Bounds, ClickEvent, Context, Div, ExternalPaths, FocusHandle,
    FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathPromptOptions, Pixels, ScrollWheelEvent, Stateful, Window, WindowAppearance, WindowBounds,
    WindowOptions, canvas, div, prelude::*, px, relative, size,
};
use widgets::{Assets, c, ca, icon, tip};

const HEADER_W: f32 = 300.0;
const TRACK_H: f32 = 100.0;
const RULER_H: f32 = 28.0;
const BARS: usize = 64;
const BAR_W: f32 = 96.0;
const BEAT_W: f32 = BAR_W / 4.0;
const CONTENT_W: f32 = BARS as f32 * BAR_W;
const READOUT_W: f32 = 52.0;
/// "R" label (8) + gap (6) + this = READOUT_W, so both readouts line up on the right.
const PAN_READOUT_W: f32 = 38.0;
/// Clicking within this distance of the thumb grabs it instead of jumping.
const THUMB_GRAB_PX: f32 = 7.0;

const DROP_PROMPT: &str = "CLICK TO CHOOSE AN AUDIO FILE, OR DRAG AND DROP ONE HERE";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Param {
    Volume,
    Pan,
}

type SliderKey = (TrackId, Param);

/// Element ids need a number; this packs a Track id and a slot together.
fn eid(name: &'static str, track: TrackId, slot: u32) -> (&'static str, u32) {
    (name, track.0 * 16 + slot)
}

impl Param {
    /// The slider fraction 0..1 for this parameter of `track`.
    fn get(self, track: &Track) -> f64 {
        match self {
            Param::Volume => track.fader,
            Param::Pan => (track.pan + 1.0) / 2.0,
        }
    }

    fn set(self, track: &mut Track, t: f64) {
        let t = t.clamp(0.0, 1.0);
        match self {
            Param::Volume => track.fader = t,
            Param::Pan => track.pan = t * 2.0 - 1.0,
        }
    }

    fn default_t(self) -> f64 {
        match self {
            Param::Volume => FADER_UNITY,
            Param::Pan => 0.5,
        }
    }

    fn detents(self) -> Vec<f64> {
        match self {
            Param::Volume => mixer::volume_detents(),
            Param::Pan => mixer::pan_detents(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Name,
    Volume,
    Pan,
}

/// A value being typed into a readout or the name.
struct Edit {
    track: TrackId,
    field: Field,
    text: String,
    /// The whole text is "selected": the next character replaces it.
    fresh: bool,
    /// Escape puts these back (nudges are applied live while typing).
    before_fader: f64,
    before_pan: f64,
}

struct SliderDrag {
    key: SliderKey,
    start_x: f32,
    start_t: f64,
    width: f32,
}

struct AppView {
    tracks: Tracks,
    edit: Option<Edit>,
    edit_focus: FocusHandle,
    drag: Option<SliderDrag>,
    /// Where each slider was last drawn, so a click can be turned into a value.
    slider_bounds: Rc<RefCell<HashMap<SliderKey, Bounds<Pixels>>>>,
    /// Width of the visible part of the timeline.
    lane_view_w: Rc<Cell<f32>>,
    /// How far the timeline is scrolled sideways.
    scroll_x: f32,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
            .detach();
        Self {
            tracks: Tracks::new(),
            edit: None,
            edit_focus: cx.focus_handle(),
            drag: None,
            slider_bounds: Rc::default(),
            lane_view_w: Rc::new(Cell::new(800.0)),
            scroll_x: 0.0,
        }
    }

    fn update_track(&mut self, id: TrackId, f: impl FnOnce(&mut Track), cx: &mut Context<Self>) {
        if let Some(track) = self.tracks.get_mut(id) {
            f(track);
            cx.notify();
        }
    }

    // ---- files -------------------------------------------------------------------------

    fn load_paths(&mut self, id: TrackId, paths: &[PathBuf], cx: &mut Context<Self>) {
        let name = paths
            .iter()
            .filter_map(|p| p.file_name()?.to_str())
            .find(|n| is_audio_file(n))
            .map(str::to_string);
        if let Some(name) = name {
            self.update_track(id, |t| t.file_name = Some(name), cx);
        }
    }

    fn pick_file(&mut self, id: TrackId, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                this.update(cx, |view, cx| view.load_paths(id, &paths, cx))
                    .ok();
            }
        })
        .detach();
    }

    // ---- sliders -----------------------------------------------------------------------

    fn slider_down(&mut self, key: SliderKey, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        self.tracks.select(key.0);
        let Some(bounds) = self.slider_bounds.borrow().get(&key).copied() else {
            return;
        };
        let Some(track) = self.tracks.get_mut(key.0) else {
            return;
        };
        let (param, width) = (key.1, f32::from(bounds.size.width).max(1.0));
        if ev.click_count >= 2 {
            param.set(track, param.default_t());
            self.drag = None;
            cx.notify();
            return;
        }
        let x = f32::from(ev.position.x - bounds.origin.x);
        let thumb = param.get(track) as f32 * width;
        if (x - thumb).abs() > THUMB_GRAB_PX {
            let t = (x / width) as f64;
            let t = if ev.modifiers.shift {
                t
            } else {
                mixer::snap(t, &param.detents(), width as f64)
            };
            param.set(track, t);
        }
        self.drag = Some(SliderDrag {
            key,
            start_x: f32::from(ev.position.x),
            start_t: param.get(track),
            width,
        });
        cx.notify();
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else { return };
        if ev.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let (key, width) = (drag.key, drag.width as f64);
        let dt = (f32::from(ev.position.x) - drag.start_x) as f64 / width;
        let t = if ev.modifiers.shift {
            drag.start_t + dt * 0.1
        } else {
            mixer::snap((drag.start_t + dt).clamp(0.0, 1.0), &key.1.detents(), width)
        };
        self.update_track(key.0, |track| key.1.set(track, t), cx);
    }

    // ---- typing ------------------------------------------------------------------------

    fn start_edit(
        &mut self,
        id: TrackId,
        field: Field,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_edit(cx);
        let Some(track) = self.tracks.get(id) else {
            return;
        };
        let text = match field {
            Field::Name => track.name.clone(),
            Field::Volume => mixer::format_db(mixer::fader_to_db(track.fader)),
            Field::Pan => mixer::format_pan(track.pan),
        };
        self.edit = Some(Edit {
            track: id,
            field,
            text,
            fresh: true,
            before_fader: track.fader,
            before_pan: track.pan,
        });
        window.focus(&self.edit_focus);
        cx.notify();
    }

    fn apply_text(&mut self, edit: &Edit) {
        let Some(track) = self.tracks.get_mut(edit.track) else {
            return;
        };
        match edit.field {
            Field::Name => {
                let name = edit.text.trim();
                if !name.is_empty() {
                    track.name = name.to_string();
                }
            }
            Field::Volume => {
                if let Some(db) = mixer::parse_db(&edit.text) {
                    track.fader = mixer::db_to_fader(db);
                }
            }
            Field::Pan => {
                if let Some(pan) = mixer::parse_pan(&edit.text) {
                    track.pan = pan;
                }
            }
        }
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.edit.take() {
            self.apply_text(&edit);
            cx.notify();
        }
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.edit.take() {
            self.update_track(
                edit.track,
                |t| {
                    t.fader = edit.before_fader;
                    t.pan = edit.before_pan;
                },
                cx,
            );
        }
    }

    fn nudge(&mut self, up: bool, fine: bool, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.edit else { return };
        let Some(track) = self.tracks.get_mut(edit.track) else {
            return;
        };
        let dir = if up { 1.0 } else { -1.0 };
        match edit.field {
            Field::Name => return,
            Field::Volume => {
                let current =
                    mixer::parse_db(&edit.text).unwrap_or(mixer::fader_to_db(track.fader));
                let step = if fine {
                    VOLUME_NUDGE_FINE_DB
                } else {
                    VOLUME_NUDGE_DB
                };
                let db = mixer::nudge_db(current, dir * step);
                track.fader = mixer::db_to_fader(db);
                edit.text = mixer::format_db(db);
            }
            Field::Pan => {
                let current = mixer::parse_pan(&edit.text).unwrap_or(track.pan);
                let step = if fine { PAN_NUDGE_FINE } else { PAN_NUDGE };
                track.pan = mixer::nudge_pan(current, dir * step);
                edit.text = mixer::format_pan(track.pan);
            }
        }
        edit.fresh = true;
        cx.notify();
    }

    fn edit_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        match ks.key.as_str() {
            "enter" => self.commit_edit(cx),
            "escape" => self.cancel_edit(cx),
            "up" => self.nudge(true, ks.modifiers.shift, cx),
            "down" => self.nudge(false, ks.modifiers.shift, cx),
            "backspace" => {
                if let Some(edit) = &mut self.edit {
                    if edit.fresh {
                        edit.text.clear();
                    } else {
                        edit.text.pop();
                    }
                    edit.fresh = false;
                    cx.notify();
                }
            }
            _ => {
                let typed = ks
                    .key_char
                    .as_deref()
                    .filter(|_| !ks.modifiers.platform && !ks.modifiers.control);
                if let (Some(ch), Some(edit)) = (typed, &mut self.edit) {
                    if edit.fresh {
                        edit.text.clear();
                        edit.fresh = false;
                    }
                    edit.text.push_str(ch);
                    cx.notify();
                }
            }
        }
        cx.stop_propagation();
    }

    fn is_editing(&self, id: TrackId, field: Field) -> bool {
        self.edit
            .as_ref()
            .is_some_and(|e| e.track == id && e.field == field)
    }

    /// The text field shown while typing: the value with a caret, or highlighted when the next
    /// keystroke will replace it.
    fn edit_field(&self, t: Theme, align_right: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let edit = self.edit.as_ref().unwrap();
        let text = div()
            .when(edit.fresh, |d| d.bg(ca(t.focus, 0.25)).rounded(px(2.)))
            .child(edit.text.clone());
        div()
            .id("edit-field")
            .track_focus(&self.edit_focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| this.edit_key(ev, cx)))
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _w, cx| this.commit_edit(cx)))
            .h(px(22.))
            .px(px(4.))
            .flex()
            .flex_row()
            .items_center()
            .when(align_right, |d| d.justify_end())
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded(px(4.))
            .border_1()
            .border_color(c(t.focus))
            .bg(c(t.bg))
            .child(text)
            .when(!edit.fresh, |d| {
                d.child(div().w(px(1.)).h(px(13.)).bg(c(t.text)))
            })
    }

    // ---- drawing -----------------------------------------------------------------------

    fn slider(
        &self,
        t: Theme,
        track: &Track,
        param: Param,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let key = (track.id, param);
        let value = param.get(track) as f32;
        let default = param.default_t() as f32;
        let (fill_from, fill_to) = match param {
            Param::Volume => (0.0, value),
            Param::Pan => (value.min(default), value.max(default)),
        };
        let accent = Theme::track_color(track.color);
        let bounds = self.slider_bounds.clone();
        let ticks = param.detents().into_iter().map(move |d| {
            let is_default = (d - param.default_t()).abs() < 1e-9;
            div()
                .absolute()
                .left(relative(d as f32))
                .top(px(if is_default { -4. } else { -2. }))
                .bottom(px(if is_default { -4. } else { -2. }))
                .w(px(1.))
                .bg(ca(t.text_muted, if is_default { 0.6 } else { 0.4 }))
        });
        let tooltip = match param {
            Param::Volume => {
                "Volume\nDrag to change. It catches on the marked points\nShift + drag: fine control, no catching\nDouble-click: back to 0 dB (original level)"
            }
            Param::Pan => {
                "Pan\nDrag toward L or R to move the sound to that speaker\nIt catches at centre and at 25L / 25R\nShift + drag: fine control, no catching\nDouble-click: back to centre"
            }
        };
        div()
            .id(eid("slider", track.id, param as u32))
            .relative()
            .flex_1()
            .min_w(px(60.))
            .h(px(18.))
            .cursor_pointer()
            .child(
                canvas(
                    move |b, _, _| {
                        bounds.borrow_mut().insert(key, b);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(7.))
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(c(t.control_hover))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(fill_from))
                            .w(relative(fill_to - fill_from))
                            .rounded(px(2.))
                            .bg(c(accent)),
                    )
                    .children(ticks),
            )
            .child(
                div()
                    .absolute()
                    .top(px(1.))
                    .left(relative(value))
                    .ml(px(-6.))
                    .w(px(12.))
                    .h(px(16.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(c(t.border))
                    .bg(c(t.surface))
                    .shadow_sm(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _w, cx| this.slider_down(key, ev, cx)),
            )
            .tooltip(tip(t, tooltip))
    }

    /// A readout that turns into a text field on click.
    fn readout(&self, t: Theme, track: &Track, field: Field, cx: &mut Context<Self>) -> AnyElement {
        let width = if field == Field::Pan {
            PAN_READOUT_W
        } else {
            READOUT_W
        };
        if self.is_editing(track.id, field) {
            let unit = (field == Field::Volume).then(|| div().flex_none().child("dB"));
            return div()
                .flex_none()
                .w(px(width))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(3.))
                .text_size(px(11.))
                .child(self.edit_field(t, true, cx).flex_1().min_w_0())
                .children(unit)
                .into_any_element();
        }
        let id = track.id;
        let (text, tooltip) = match field {
            Field::Volume => {
                let db = mixer::format_db(mixer::fader_to_db(track.fader));
                (
                    format!("{db} dB"),
                    format!(
                        "Volume: {db} dB\nClick to type a value, e.g. -6 or +3 (-inf for silence)\nWhile typing: ↑ / ↓ nudge by {VOLUME_NUDGE_DB} dB\nShift + ↑ / ↓ nudge by {VOLUME_NUDGE_FINE_DB} dB\nEnter to set · Esc to cancel"
                    ),
                )
            }
            _ => {
                let pan = mixer::format_pan(track.pan);
                (
                    pan.clone(),
                    format!(
                        "Pan: {pan}\nClick to type a value, e.g. 20L, 15R or 0 for centre (50 is fully to one side)\nWhile typing: ↑ / ↓ nudge by {PAN_NUDGE}\nShift + ↑ / ↓ nudge by {PAN_NUDGE_FINE}\nEnter to set · Esc to cancel"
                    ),
                )
            }
        };
        div()
            .id(eid("readout", id, field as u32))
            .flex_none()
            .w(px(width))
            .h(px(20.))
            .px(px(3.))
            .flex()
            .items_center()
            .justify_end()
            .whitespace_nowrap()
            .rounded(px(4.))
            .border_1()
            .border_color(gpui::transparent_black())
            .text_size(px(11.))
            .text_color(c(t.text_muted))
            .cursor_text()
            .hover(|s| s.border_color(c(t.border)).text_color(c(t.text)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.start_edit(id, field, window, cx)
            }))
            .tooltip(tip(t, tooltip))
            .child(text)
            .into_any_element()
    }

    fn icon_button(
        &self,
        t: Theme,
        id: (&'static str, u32),
        name: &'static str,
        dim: bool,
        hover_color: u32,
        tooltip: String,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .flex_none()
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .border_color(gpui::transparent_black())
            .when(dim, |d| d.opacity(0.35))
            .when(!dim, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.border_color(c(t.border)).bg(c(t.control)))
            })
            .child(icon(name, if dim { t.text_muted } else { hover_color }))
            .tooltip(tip(t, tooltip))
    }

    fn toggle(
        &self,
        t: Theme,
        id: (&'static str, u32),
        name: &'static str,
        on: bool,
        (on_bg, on_fg): (u32, u32),
        tooltip: &'static str,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .flex_none()
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .cursor_pointer()
            .border_color(c(if on { on_bg } else { t.border }))
            .bg(c(if on { on_bg } else { t.control }))
            .when(!on, |d| d.hover(move |s| s.bg(c(t.control_hover))))
            .child(icon(name, if on { on_fg } else { t.text_muted }))
            .tooltip(tip(t, tooltip))
    }

    fn header(
        &self,
        t: Theme,
        index: usize,
        track: &Track,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = track.id;
        let selected = self.tracks.selected() == id;
        let silent = self.tracks.is_silent(id);
        let at_defaults = track.is_at_defaults();

        let name: AnyElement = if self.is_editing(id, Field::Name) {
            self.edit_field(t, false, cx)
                .flex_1()
                .min_w_0()
                .font_weight(FontWeight::SEMIBOLD)
                .into_any_element()
        } else {
            div()
                .id(eid("name", id, 0))
                .flex_1()
                .min_w_0()
                .h(px(22.))
                .px(px(6.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(gpui::transparent_black())
                .hover(|s| s.border_color(c(t.border)))
                .font_weight(FontWeight::SEMIBOLD)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(track.name.clone())
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    if ev.click_count() >= 2 {
                        this.start_edit(id, Field::Name, window, cx)
                    }
                }))
                .tooltip(tip(t, format!("{}\nDouble-click to rename", track.name)))
                .into_any_element()
        };

        let reset = self
            .icon_button(
                t,
                eid("reset", id, 0),
                "reset",
                at_defaults,
                t.text_muted,
                if at_defaults {
                    "Reset track\nNothing to reset: already at 0 dB, centred, not muted or soloed".into()
                } else {
                    "Reset track\nVolume back to 0 dB, pan to centre, mute and solo off\nThe name and audio are kept".into()
                },
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.update_track(id, Track::reset, cx)));

        let delete = self.tracks.can_delete(id).then(|| {
            self.icon_button(
                t,
                eid("delete", id, 0),
                "trash",
                false,
                t.text_muted,
                "Delete track\nRemoves this track and its audio from the mix\nThe first two tracks can't be deleted".into(),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                if this.edit.as_ref().is_some_and(|e| e.track == id) {
                    this.edit = None;
                }
                this.tracks.delete(id);
                cx.notify();
            }))
        });

        let mute = self
            .toggle(
                t,
                eid("mute", id, 0),
                "mute",
                track.muted,
                (t.mute, t.mute_text),
                if track.muted {
                    "Muted\nThis track is silent. Click to hear it again"
                } else {
                    "Mute\nSilence this track. Click again to unmute"
                },
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                this.update_track(id, |t| t.muted = !t.muted, cx)
            }));
        let solo = self
            .toggle(
                t,
                eid("solo", id, 0),
                "headphones",
                track.soloed,
                (t.solo, t.solo_text),
                if track.soloed {
                    "Soloed\nOnly soloed tracks play. Click to stop soloing"
                } else {
                    "Solo\nPlay only this track (and any other soloed tracks)\nClick again to hear everything"
                },
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.update_track(id, |t| t.soloed = !t.soloed, cx)));

        let row = || div().flex().flex_row().items_center().gap(px(6.));
        let label_icon = |eid_: (&'static str, u32), name: &'static str, text: &'static str| {
            div()
                .id(eid_)
                .flex_none()
                .flex()
                .child(icon(name, t.text_muted))
                .tooltip(tip(t, text))
        };
        let pan_end = |eid_: (&'static str, u32), label: &'static str, text: &'static str| {
            div()
                .id(eid_)
                .flex_none()
                .w(px(8.))
                .flex()
                .justify_center()
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(c(t.text_muted))
                .child(label)
                .tooltip(tip(t, text))
        };

        div()
            .id(eid("header", id, 0))
            .flex_none()
            .w(px(HEADER_W))
            .h_full()
            .flex()
            .flex_row()
            .bg(c(if selected { t.surface_2 } else { t.surface }))
            .border_r_1()
            .border_color(c(t.border))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _w, cx| {
                    this.tracks.select(id);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(5.))
                    .h_full()
                    .bg(c(Theme::track_color(track.color)))
                    .when(silent, |d| d.opacity(0.35)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        row()
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(14.))
                                    .text_color(c(t.text_muted))
                                    .child(format!("{}", index + 1)),
                            )
                            .child(name)
                            .child(reset)
                            .children(delete)
                            .child(div().flex().flex_row().gap(px(3.)).child(mute).child(solo)),
                    )
                    .child(
                        row()
                            .child(label_icon(
                                eid("vol-icon", id, 0),
                                "volume",
                                "Volume\nHow loud this track is in the mix",
                            ))
                            .child(self.slider(t, track, Param::Volume, cx))
                            .child(self.readout(t, track, Field::Volume, cx)),
                    )
                    .child(
                        row()
                            .child(label_icon(
                                eid("pan-icon", id, 0),
                                "pan",
                                "Pan\nWhere this track sits between the left and right speakers",
                            ))
                            .child(pan_end(eid("pan-l", id, 0), "L", "Left speaker"))
                            .child(self.slider(t, track, Param::Pan, cx))
                            .child(pan_end(eid("pan-r", id, 0), "R", "Right speaker"))
                            .child(self.readout(t, track, Field::Pan, cx)),
                    ),
            )
            .child(meter(t))
    }

    /// Beat-grid lines for the visible part of the timeline.
    fn grid_lines(&self, t: Theme) -> impl Iterator<Item = Div> {
        let first = (self.scroll_x / BEAT_W).floor() as usize;
        let last =
            (((self.scroll_x + self.lane_view_w.get()) / BEAT_W).ceil() as usize).min(BARS * 4);
        let scroll = self.scroll_x;
        (first..=last).map(move |beat| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(beat as f32 * BEAT_W - scroll))
                .w(px(1.))
                .bg(c(if beat % 4 == 0 {
                    t.grid_bar
                } else {
                    t.grid_beat
                }))
        })
    }

    fn lane(&self, t: Theme, track: &Track, cx: &mut Context<Self>) -> Div {
        let id = track.id;
        let selected = self.tracks.selected() == id;
        let silent = self.tracks.is_silent(id);
        let content: AnyElement = match &track.file_name {
            Some(name) => div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .font_weight(FontWeight::SEMIBOLD)
                .child(name.clone())
                .into_any_element(),
            None => div()
                .id(eid("drop", id, 0))
                .absolute()
                .inset(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .px(px(12.))
                .rounded(px(6.))
                .border_1()
                .border_dashed()
                .border_color(c(t.grid_bar))
                .bg(ca(t.lane, 0.7))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(c(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.border_color(c(t.text_muted)).text_color(c(t.text)))
                .drag_over::<ExternalPaths>(move |s, _, _, _| {
                    s.bg(c(t.lane_selected)).border_color(c(t.focus)).border_2()
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.pick_file(id, cx)))
                .on_drop(cx.listener(move |this, paths: &ExternalPaths, _w, cx| {
                    this.load_paths(id, paths.paths(), cx)
                }))
                .tooltip(tip(
                    t,
                    "Load audio into this track\nClick to browse your files\nOr drag an audio file from your computer onto this track\nWAV, AIFF, MP3, M4A, FLAC, OGG",
                ))
                .child(DROP_PROMPT)
                .into_any_element(),
        };
        div()
            .flex_1()
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(c(if selected { t.lane_selected } else { t.lane }))
            .when(silent, |d| d.opacity(0.55))
            .children(self.grid_lines(t))
            .child(content)
    }

    fn ruler(&self, t: Theme) -> Div {
        let lane_w = self.lane_view_w.clone();
        let first = (self.scroll_x / BAR_W).floor() as usize;
        let last = (((self.scroll_x + lane_w.get()) / BAR_W).ceil() as usize).min(BARS - 1);
        let scroll = self.scroll_x;
        let bars = (first..=last).map(move |bar| {
            let left = bar as f32 * BAR_W - scroll;
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(left))
                .w(px(BAR_W))
                .border_l_1()
                .border_color(c(t.grid_bar))
                .child(
                    div()
                        .absolute()
                        .top(px(5.))
                        .left(px(5.))
                        .text_size(px(11.))
                        .text_color(c(t.text_muted))
                        .child(format!("{}", bar + 1)),
                )
                .children((1..4).map(move |beat| {
                    div()
                        .absolute()
                        .bottom_0()
                        .h(px(6.))
                        .left(px(beat as f32 * BEAT_W))
                        .w(px(1.))
                        .bg(c(t.border))
                }))
        });
        div()
            .flex_none()
            .h(px(RULER_H))
            .flex()
            .flex_row()
            .bg(c(t.surface))
            .border_b_1()
            .border_color(c(t.border))
            .child(
                div()
                    .flex_none()
                    .w(px(HEADER_W))
                    .h_full()
                    .border_r_1()
                    .border_color(c(t.border)),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .relative()
                    .overflow_hidden()
                    .child(
                        canvas(
                            move |b, _, _| lane_w.set(f32::from(b.size.width)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .children(bars),
            )
    }

    fn add_track_row(&self, t: Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("add-track")
            .flex_none()
            .w_full()
            .h(px(40.))
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .bg(c(t.surface))
            .border_b_1()
            .border_color(c(t.border))
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(c(t.text_muted))
            .cursor_pointer()
            .hover(|s| s.bg(c(t.surface_2)).text_color(c(t.text)))
            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                this.tracks.add();
                cx.notify();
            }))
            .tooltip(tip(
                t,
                format!("Add track\nAdds an empty track below the last one (up to {MAX_TRACKS})\nTracks you add can be deleted again"),
            ))
            .child(icon("plus", t.text_muted))
            .child("ADD TRACK")
    }

    fn scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(16.));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        let dx = if ev.modifiers.shift && dx == 0.0 {
            dy
        } else {
            dx
        };
        if dx == 0.0 {
            return;
        }
        let max = (CONTENT_W - self.lane_view_w.get()).max(0.0);
        self.scroll_x = (self.scroll_x - dx).clamp(0.0, max);
        cx.notify();
    }
}

/// Stereo level meter; silent until audio plays.
fn meter(t: Theme) -> Div {
    let bar = || {
        div()
            .w(px(4.))
            .h_full()
            .rounded(px(2.))
            .bg(c(t.control_hover))
    };
    div()
        .flex_none()
        .flex()
        .flex_row()
        .gap(px(2.))
        .py(px(8.))
        .pr(px(8.))
        .child(bar())
        .child(bar())
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::DARK,
            _ => Theme::LIGHT,
        };
        let tracks: Vec<Track> = self.tracks.iter().cloned().collect();
        let rows = tracks.iter().enumerate().map(|(i, track)| {
            div()
                .flex_none()
                .h(px(TRACK_H))
                .flex()
                .flex_row()
                .border_b_1()
                .border_color(c(t.border))
                .child(self.header(t, i, track, cx))
                .child(self.lane(t, track, cx))
        });
        let rows: Vec<Div> = rows.collect();
        let add = self.tracks.can_add().then(|| self.add_track_row(t, cx));

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c(t.bg))
            .text_color(c(t.text))
            .text_size(px(12.))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _w, cx| this.mouse_move(ev, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _w, _cx| this.drag = None),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _w, cx| this.scroll(ev, cx)))
            .child(self.ruler(t))
            .child(
                div()
                    .id("tracks")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .bg(c(t.lane))
                    .children(rows)
                    .children(add),
            )
    }
}

pub fn run() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| AppView::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
