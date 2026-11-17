# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Status

Pre-scaffold: no crate exists yet. "Product spec" and "Engine architecture" below are agreed design intent, not existing code. Update this file once the crate exists (real module layout, GPUI version, chosen crates). Work test-first: tests are written at seams the owner has confirmed (see "Testing").

Toolchain: rustup stable (`~/.cargo/bin`). The broken Homebrew `rust` formula was uninstalled because it shadowed rustup; do not reinstall it.

Crate layout so far: a library crate `audiobrew` (`src/lib.rs`) holding pure-logic modules, each with its tests inline: `timeline` (End marker, repeat seams), `tempo` (speed factor, stretch limits, half/double suggestion), `bands` (crossover-based Band config, max 6), `key` (Camelot compatibility), `mix` (Band mute/solo gains, limiter), `history` (undo/redo), `grid` (beats on the Mashup grid: one beat = 60/Mashup BPM seconds; Offset is stored in beats), `first_run` (zero-decision planner: Mashup BPM, auto halve/double, stretch cap with warning, bar-wrapped downbeat alignment, repeat flags, End marker), `project` (lanes, min two; Mashup BPM default; key reference; Offset in beats; the type History snapshots), `render` (allocation-free Offset/repeat/End-marker mix, Solo audition, limiter), `crossover` (LR4 `BandSplitter`; Bands sum flat), `transport` (shared Playhead: play/pause, loop wrap, End-marker clamp, seek), `ramp` (~20 ms linear parameter glide), `theme` (colour tokens, WCAG contrast, glow budget of 3, Band patterns/labels), `shortcuts` (pure keymap + help overlay entries; nothing fires while a text field is focused), `readout` (value formatters: BPM, dB, ms, stretch, semitones, Hz), `load` (file -> stereo-at-device-rate plan, load errors, drag-and-drop lane assignment), `rerender` (which Tracks need re-stretching; generation tracking that discards stale background results). `history` also has drag gestures (begin/update/commit/abort), `edit` (ignores no-ops) and a depth cap; `tempo` has tap tempo; `grid` has Loop region snapping; `key` has the Match-key semitone shift. GPUI and audio-device code will sit on top of it.

## Project

audioBrew is a Rust desktop application built on GPUI (the Zed UI framework) for making audio mashups. Design goal: the simplest mashup UI there is, which pros still love. Beginners see almost nothing to configure; pros get precision through progressive disclosure (Focus, an Advanced drawer, modifier keys, shortcuts).

Commands (standard Cargo; no custom tooling exists yet):
- `cargo run` — launch the app
- `cargo build` / `cargo check`
- `cargo test` / `cargo test <name>` — run all / a single test
- `cargo clippy` and `cargo fmt`

## Product spec

Domain vocabulary lives in `GLOSSARY.md`; use those terms in code. "Channel" means stereo L/R only. The UI uses plainer labels (Speed, Nudge, Repeat) for the same concepts.

1. **Zero-decision first run (the contract).** Dropping two files must auto-detect tempo, stretch, align downbeats, repeat the shorter Track to fill, and produce a playable Mashup, with every control optional. This is the highest-priority behaviour.
2. **Tracks.** Load via native file dialog or drag-and-drop. Two Track lanes are always visible (empty lanes read "Drop a file or click to load"); the user can add more and remove any, never below two. Each Track has a fader.
3. **Mashup lane.** Always pinned last, slightly deeper and brighter, showing the mixed output with faint ghosts of the Tracks. A limiter is on by default with no settings.
4. **Bands.** Per Track, default Low/Mid/High, each with gain, mute and solo. Each lane shows a compact strip of its Band gains; Focus expands the lane in place to show Band slabs whose top/bottom edges are draggable frequency limits, plus Add Band. Maximum 6 Bands. Never hardcode three.
5. **Timing.** Per-Track Offset (drag the waveform) and Loop region (beat-grid snapped). Each Track is time-stretched (pitch preserved) to one Mashup BPM, defaulting to the first Track's detected tempo and editable. Constant-BPM grid only. Stretch ratio is capped around 0.7-1.4x with a warning outside it. Detected BPM/downbeat are correctable: halve/double chip (shown only when detection looks doubtful), typed value, tap tempo, draggable downbeat, fine nudge, zoom.
6. **Key matching.** Detect each Track's key; show a compatibility badge (green/amber/red) relative to the first Track. A "Match key" pitch-shift (max about +/-6 semitones) lives in the Focus drawer.
7. **Playback.** One shared Playhead across all lanes. Each lane has a play button: a Track plays alone (Solo audition) through the identical graph as the Mashup; the Mashup lane plays everything. The Mashup's length is set by the **End marker**, which defaults to where the longest Track ends (offset included) and is user-draggable; repeating Tracks repeat until the End marker. Whole-Mashup loop toggle.
8. **Usability.** Undo/redo for everything, live value readouts on every control, keyboard shortcuts (Space, J/K/L, M/S) with a "?" overlay.
9. **Scope.** macOS first. Preview only in v1; WAV export is the first follow-up.
10. **Theme.** White neomorphic + skeuomorphic, modelled on Kevin Flynn's white Tron look. Rules: raised = pressable, inset = well/track; cyan glow means active/playing/soloed and nothing else, at most 3 glowing items on screen; text contrast at least 4.5:1 with a 1px hairline on every interactive control; Band slabs differ by pattern and label, not tint alone; optional high-contrast mode. All colours, shadows and glows are tokens in one Theme module.

## Engine architecture (design intent)

GPUI views only render state; audio work never runs on the UI thread or in views.
- Decode once with `symphonia` to f32, resample to device rate in a background thread. Output via `cpal`; the audio callback does no allocation or locking (atomics / lock-free ring for parameters; Playhead is an atomic frame counter).
- Pre-render each Track to a stretched buffer at the Mashup BPM with its loop applied; stretch once per Track, never per Band. Offset is a sample index. Re-render in the background when BPM or beat grid changes, crossfading from the old buffer.
- Split Bands live in the callback after the stretch (Linkwitz-Riley LR4 crossovers); ramp crossover and gain changes (about 20 ms) to avoid clicks.
- Solo audition is the Mashup graph with other Tracks muted, so the two cannot diverge.
- Crate candidates: `signalsmith-stretch` (stretch + pitch shift), `aubio-rs` (beat detection) or a custom tracker, `rubato`, `symphonia`, `cpal`. Licences checked on crates.io: `signalsmith-stretch` is MIT; `aubio-rs`/`aubio-lib` are GPL-3.0 and `rubberband` is GPL-2.0-or-later, so either would make the app GPL. Prefer `signalsmith-stretch`, and a custom or permissively licensed beat tracker over aubio, unless a GPL app is acceptable.

## Testing

Test-first, vertical slices, at owner-confirmed seams only. Pure-logic modules (beat-grid maths, band/crossover config, tempo-ratio planning, key compatibility, mix/limiter maths, timeline/offset/loop maths, undo history) must stay free of GPUI and audio-device dependencies so they are testable without hardware. Expected values come from independent sources (worked examples, known-good literals), never from recomputing the implementation.
