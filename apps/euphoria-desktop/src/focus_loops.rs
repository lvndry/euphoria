// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loops in the focus view: a length of beats or bars from the beat at or
//! before the playhead, a stretch drawn on the waveform, and loops saved
//! with the track. The loop plays through the player itself, so tempo, key
//! and stems apply to it as to the rest of the track.

use std::path::{Path, PathBuf};

use euphoria_core::SavedLoop;
use gpui::{Context, Modifiers, Pixels, px};

use crate::app::EuphoriaApp;
use crate::library::Track;
use crate::playback::SHORTEST_LOOP_SECONDS;

/// Beats in a bar of the grids Euphoria detects and reads.
const BEATS_PER_BAR: f64 = 4.0;

/// The tempo a track without a beat grid or a tempo is measured at, so
/// its loop buttons still give loops of a usual length.
const UNKNOWN_TEMPO_BPM: f64 = 120.0;

/// The lengths the loop buttons offer, in beats: half a beat, a beat, and
/// one to sixteen bars.
pub(crate) const LOOP_BEATS: [f64; 7] = [0.5, 1.0, 4.0, 8.0, 16.0, 32.0, 64.0];

/// The length the L key loops.
pub(crate) const KEY_LOOP_BEATS: f64 = 16.0;

/// The shortest and longest a loop can be halved or doubled to, in beats.
const SHORTEST_BEATS: f64 = 1.0 / 32.0;
const LONGEST_BEATS: f64 = 512.0;

/// How far the pointer must move after pressing the waveform with Shift
/// before the press draws a loop.
const DRAW_THRESHOLD: f32 = 3.0;

/// The label of a loop length on its button: "½" and "1" for beats, then
/// bars.
pub(crate) fn length_label(beats: f64) -> String {
    if beats < 1.0 {
        "\u{bd}".to_string()
    } else if beats < BEATS_PER_BAR {
        format!("{beats}")
    } else {
        format!("{}", beats / BEATS_PER_BAR)
    }
}

/// How a length is said in words, as "half a beat", "1 beat" or "4 bars".
pub(crate) fn length_words(beats: f64) -> String {
    if beats < 1.0 {
        return "half a beat".to_string();
    }
    if beats < BEATS_PER_BAR {
        return if beats == 1.0 {
            "1 beat".to_string()
        } else {
            format!("{beats} beats")
        };
    }
    let bars = beats / BEATS_PER_BAR;
    if bars == 1.0 {
        "1 bar".to_string()
    } else if bars.fract() == 0.0 {
        format!("{bars} bars")
    } else {
        format!("{beats} beats")
    }
}

/// The beats a loop is measured on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LoopGrid {
    /// A beat of the grid, in seconds from the start of the track.
    pub(crate) first_beat_seconds: f64,
    pub(crate) beat_seconds: f64,
    /// Whether loops snap to these beats: true for a beat grid, false for a
    /// tempo alone, which only gives loops their length.
    pub(crate) snaps: bool,
}

impl LoopGrid {
    /// The beats of `track`: its beat grid, or, without one, its tempo, or
    /// a usual tempo, which measure lengths but place no beats.
    pub(crate) fn of(track: &Track) -> Self {
        if let Some(grid) = track.beat_grid()
            && grid.bpm.is_finite()
            && grid.bpm > 0.0
        {
            return Self {
                first_beat_seconds: grid.first_downbeat_seconds,
                beat_seconds: 60.0 / grid.bpm,
                snaps: true,
            };
        }
        let bpm = track
            .bpm()
            .map(f64::from)
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
            .unwrap_or(UNKNOWN_TEMPO_BPM);
        Self {
            first_beat_seconds: 0.0,
            beat_seconds: 60.0 / bpm,
            snaps: false,
        }
    }

    /// The beat at or before `seconds`, or `seconds` itself without a grid.
    pub(crate) fn beat_at_or_before(&self, seconds: f64) -> f64 {
        if !self.snaps {
            return seconds.max(0.0);
        }
        let beats = ((seconds - self.first_beat_seconds) / self.beat_seconds + 1e-9).floor();
        let beat = self.first_beat_seconds + beats * self.beat_seconds;
        if beat < 0.0 { seconds.max(0.0) } else { beat }
    }

    /// The beat nearest `seconds`, or `seconds` itself without a grid.
    pub(crate) fn nearest_beat(&self, seconds: f64) -> f64 {
        if !self.snaps {
            return seconds.max(0.0);
        }
        let beats = ((seconds - self.first_beat_seconds) / self.beat_seconds).round();
        let beat = self.first_beat_seconds + beats * self.beat_seconds;
        if beat < 0.0 {
            beat + self.beat_seconds
        } else {
            beat
        }
    }
}

/// A loop of `beats` beats from the beat at or before `position`, ending
/// within a track `duration` seconds long, or `None` when no such loop
/// fits.
pub(crate) fn beat_loop(
    grid: &LoopGrid,
    position: f64,
    beats: f64,
    duration: f64,
) -> Option<(f64, f64)> {
    let start = grid.beat_at_or_before(position);
    let end = (start + beats * grid.beat_seconds).min(duration);
    (end - start >= SHORTEST_LOOP_SECONDS).then_some((start, end))
}

/// The loop `start..end` made `factor` times as long from the same start,
/// or `None` past the shortest or longest loop or the end of the track.
pub(crate) fn resized(
    grid: &LoopGrid,
    (start, end): (f64, f64),
    factor: f64,
    duration: f64,
) -> Option<(f64, f64)> {
    let length = (end - start) * factor;
    let beats = length / grid.beat_seconds;
    let fits = length >= SHORTEST_LOOP_SECONDS
        && (SHORTEST_BEATS - 1e-9..=LONGEST_BEATS + 1e-9).contains(&beats)
        && start + length <= duration + 1e-9;
    fits.then_some((start, (start + length).min(duration)))
}

/// The loop drawn from `from` to `to`, either way round, each edge on the
/// nearest beat unless `is_free` or the track has no grid. A drawn loop is
/// at least a beat long on a grid.
pub(crate) fn drawn(
    grid: &LoopGrid,
    from: f64,
    to: f64,
    is_free: bool,
    duration: f64,
) -> Option<(f64, f64)> {
    let (first, last) = if from <= to { (from, to) } else { (to, from) };
    let (mut start, mut end) = if is_free || !grid.snaps {
        (first.max(0.0), last.min(duration))
    } else {
        (grid.nearest_beat(first), grid.nearest_beat(last))
    };
    if grid.snaps && !is_free && end - start < grid.beat_seconds * 0.5 {
        end = start + grid.beat_seconds;
    }
    end = end.min(duration);
    start = start.max(0.0);
    (end - start >= SHORTEST_LOOP_SECONDS).then_some((start, end))
}

/// The loop `region` with the edge `edge` moved to `seconds`, on the
/// nearest beat unless `is_free`, keeping the other edge. `None` when the
/// edges would cross.
pub(crate) fn with_edge_at(
    grid: &LoopGrid,
    (start, end): (f64, f64),
    edge: LoopEdge,
    seconds: f64,
    is_free: bool,
    duration: f64,
) -> Option<(f64, f64)> {
    let placed = if is_free {
        seconds
    } else {
        grid.nearest_beat(seconds)
    }
    .clamp(0.0, duration);
    let (start, end) = match edge {
        LoopEdge::In => (placed, end),
        LoopEdge::Out => (start, placed),
    };
    (end - start >= SHORTEST_LOOP_SECONDS).then_some((start, end))
}

/// The loop's own edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopEdge {
    In,
    Out,
}

/// The loop the player plays.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveLoop {
    pub(crate) path: PathBuf,
    pub(crate) start_seconds: f64,
    pub(crate) end_seconds: f64,
    /// The length in beats it was asked for with a loop button, so the
    /// button shows pressed. A drawn or moved loop has none.
    pub(crate) beats: Option<f64>,
}

impl ActiveLoop {
    pub(crate) fn region(&self) -> (f64, f64) {
        (self.start_seconds, self.end_seconds)
    }

    pub(crate) fn contains(&self, seconds: f64) -> bool {
        seconds >= self.start_seconds && seconds < self.end_seconds
    }
}

/// What a press on the waveform is doing to the loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LoopDrag {
    /// Drawing a new loop from `anchor_seconds`, pressed at `pressed_x`.
    Drawing {
        anchor_seconds: f64,
        pressed_x: f32,
        has_moved: bool,
    },
    /// Moving one edge of the loop.
    Edge(LoopEdge),
}

/// The name a new saved loop gets: "Loop 1", "Loop 2" and on, skipping the
/// names taken.
pub(crate) fn next_loop_name(saved: &[SavedLoop]) -> String {
    let mut number = saved.len() + 1;
    loop {
        let name = format!("Loop {number}");
        if saved.iter().all(|each| each.name != name) {
            return name;
        }
        number += 1;
    }
}

impl EuphoriaApp {
    /// The loop the player plays, when it is a loop of the selected track.
    pub(crate) fn selected_loop(&self) -> Option<&ActiveLoop> {
        let path = &self.selected_track()?.path;
        self.focus
            .active_loop
            .as_ref()
            .filter(|active| &active.path == path)
    }

    /// Whether the loop buttons can loop the selected track: it is the
    /// player's track, playing or held.
    pub(crate) fn can_loop_selection(&self) -> bool {
        self.selection_is_in_player() && self.voice.is_some()
    }

    fn selected_loop_grid(&self) -> Option<(LoopGrid, f64)> {
        let track = self.selected_track()?;
        let duration = f64::from(track.duration_seconds()?);
        Some((LoopGrid::of(track), duration))
    }

    /// Loops `beats` beats from the beat at or before the playhead, or, in
    /// a loop, makes the loop that long from its start. Pressed again for
    /// the same length, it leaves the loop.
    pub(crate) fn toggle_beat_loop(&mut self, beats: f64, cx: &mut Context<'_, Self>) {
        if self.selected_loop().and_then(|active| active.beats) == Some(beats) {
            self.exit_loop(cx);
            return;
        }
        if !self.can_loop_selection() {
            return;
        }
        let Some((grid, duration)) = self.selected_loop_grid() else {
            return;
        };
        let position = f64::from(self.player.position_seconds());
        if let Some(active) = self.selected_loop().cloned() {
            let start = active.start_seconds;
            let end = (start + beats * grid.beat_seconds).min(duration);
            if end - start >= SHORTEST_LOOP_SECONDS {
                self.set_active_loop(start, end, Some(beats), None, cx);
            }
            return;
        }
        let Some((start, end)) = beat_loop(&grid, position, beats, duration) else {
            return;
        };
        let entry = (!(start..end).contains(&position)).then_some(start);
        self.set_active_loop(start, end, Some(beats), entry, cx);
    }

    /// The L key: loops 4 bars, or leaves the loop playing.
    pub(crate) fn toggle_key_loop(&mut self, cx: &mut Context<'_, Self>) {
        if !self.focus.is_open {
            return;
        }
        if self.selected_loop().is_some() {
            self.exit_loop(cx);
        } else {
            self.toggle_beat_loop(KEY_LOOP_BEATS, cx);
        }
    }

    /// Halves the loop, or doubles it with a `factor` of 2, from its start.
    pub(crate) fn resize_loop(&mut self, factor: f64, cx: &mut Context<'_, Self>) {
        let Some(active) = self.selected_loop().cloned() else {
            return;
        };
        let Some((grid, duration)) = self.selected_loop_grid() else {
            return;
        };
        let Some((start, end)) = resized(&grid, active.region(), factor, duration) else {
            return;
        };
        let beats = active.beats.map(|beats| beats * factor);
        self.set_active_loop(start, end, beats, None, cx);
    }

    /// Leaves the loop: playing carries on past its end.
    pub(crate) fn exit_loop(&mut self, cx: &mut Context<'_, Self>) {
        if self.focus.active_loop.take().is_none() {
            return;
        }
        self.focus.loop_drag = None;
        if let Some(active) = &self.voice {
            active.state.set_loop(None, None);
        }
        cx.notify();
    }

    /// Esc: leaves the loop when the focus view shows one. Returns false
    /// when there was none.
    pub(crate) fn exit_loop_by_key(&mut self, cx: &mut Context<'_, Self>) -> bool {
        if !self.focus.is_open || self.selected_loop().is_none() {
            return false;
        }
        self.exit_loop(cx);
        true
    }

    /// Makes `start..end` of the selected track the loop, jumping to `entry`
    /// when given, and starting the track there when the player does not
    /// hold it.
    pub(crate) fn set_active_loop(
        &mut self,
        start: f64,
        end: f64,
        beats: Option<f64>,
        entry: Option<f64>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(path) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        self.focus.active_loop = Some(ActiveLoop {
            path,
            start_seconds: start,
            end_seconds: end,
            beats,
        });
        if self.can_loop_selection() {
            if let Some(active) = &self.voice {
                active.state.set_loop(Some((start, end)), entry);
            }
            if let Some(seconds) = entry {
                self.player.advance_to(seconds as f32);
            }
        } else if let Some(index) = self.selected {
            self.play_library_track(index, start as f32, cx);
        }
        cx.notify();
    }

    /// Gives a new voice the loop, or forgets a loop of a track the player
    /// no longer holds.
    pub(crate) fn loop_follows_voice(&mut self) {
        let player_track = self.player.track().map(Path::to_path_buf);
        let keeps = self
            .focus
            .active_loop
            .as_ref()
            .is_some_and(|active| Some(&active.path) == player_track.as_ref());
        if !keeps {
            self.focus.active_loop = None;
        }
        if let (Some(voice), Some(active)) = (&self.voice, &self.focus.active_loop) {
            voice.state.set_loop(Some(active.region()), None);
        }
    }

    /// A jump of the playhead to `seconds`: outside the loop, it leaves the
    /// loop.
    pub(crate) fn loop_follows_seek(&mut self, seconds: f64) {
        if self
            .focus
            .active_loop
            .as_ref()
            .is_some_and(|active| !active.contains(seconds))
        {
            self.focus.active_loop = None;
            self.focus.loop_drag = None;
            if let Some(active) = &self.voice {
                active.state.set_loop(None, None);
            }
        }
    }

    /// A press on the detail with Shift held starts drawing a loop there.
    pub(crate) fn start_drawing_loop(&mut self, pointer_x: Pixels) {
        let Some(seconds) = self.focus.seconds_under(pointer_x) else {
            return;
        };
        self.focus.loop_drag = Some(LoopDrag::Drawing {
            anchor_seconds: seconds,
            pressed_x: pointer_x / px(1.0),
            has_moved: false,
        });
    }

    /// A press on an edge of the loop starts moving it.
    pub(crate) fn press_loop_edge(&mut self, edge: LoopEdge) {
        if self.selected_loop().is_some() {
            self.focus.loop_drag = Some(LoopDrag::Edge(edge));
        }
    }

    /// The pointer moved with the button held: a loop being drawn or an edge
    /// being moved follows it, on the beats unless Option is held. Returns
    /// false when no loop is being dragged.
    pub(crate) fn drag_loop_to(
        &mut self,
        pointer_x: Pixels,
        modifiers: Modifiers,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(drag) = self.focus.loop_drag else {
            return false;
        };
        let Some(seconds) = self.focus.seconds_under(pointer_x) else {
            return true;
        };
        let Some((grid, duration)) = self.selected_loop_grid() else {
            return true;
        };
        let is_free = modifiers.alt;
        match drag {
            LoopDrag::Drawing {
                anchor_seconds,
                pressed_x,
                has_moved,
            } => {
                if !has_moved && (pointer_x / px(1.0) - pressed_x).abs() < DRAW_THRESHOLD {
                    return true;
                }
                self.focus.loop_drag = Some(LoopDrag::Drawing {
                    anchor_seconds,
                    pressed_x,
                    has_moved: true,
                });
                self.focus.drawn_loop = drawn(&grid, anchor_seconds, seconds, is_free, duration);
            }
            LoopDrag::Edge(edge) => {
                let Some(active) = self.selected_loop() else {
                    return true;
                };
                self.focus.drawn_loop =
                    with_edge_at(&grid, active.region(), edge, seconds, is_free, duration);
            }
        }
        cx.notify();
        true
    }

    /// The button came up: the loop drawn or moved plays.
    pub(crate) fn finish_loop_drag(&mut self, cx: &mut Context<'_, Self>) {
        let Some(_) = self.focus.loop_drag.take() else {
            return;
        };
        let Some((start, end)) = self.focus.drawn_loop.take() else {
            cx.notify();
            return;
        };
        let position = f64::from(self.player.position_seconds());
        let is_inside = self.can_loop_selection() && (start..end).contains(&position);
        let entry = (!is_inside).then_some(start);
        self.set_active_loop(start, end, None, entry, cx);
    }

    /// The loop being drawn or moved, or the one playing, as the detail
    /// shows it.
    pub(crate) fn shown_loop(&self) -> Option<(f64, f64)> {
        self.focus
            .drawn_loop
            .or_else(|| self.selected_loop().map(ActiveLoop::region))
    }

    /// Keeps the loop playing with the selected track, named "Loop 1" and
    /// on, unless it is kept already.
    pub(crate) fn save_loop(&mut self, cx: &mut Context<'_, Self>) {
        let Some(active) = self.selected_loop().cloned() else {
            return;
        };
        let Some(track) = self.selected_track() else {
            return;
        };
        let mut loops = track.saved_loops.clone();
        let is_kept = loops.iter().any(|saved| {
            (saved.start_seconds - active.start_seconds).abs() < 1e-6
                && (saved.end_seconds - active.end_seconds).abs() < 1e-6
        });
        if is_kept || loops.len() >= euphoria_core::MAXIMUM_SAVED_LOOPS {
            return;
        }
        loops.push(SavedLoop {
            start_seconds: active.start_seconds,
            end_seconds: active.end_seconds,
            name: next_loop_name(&loops),
        });
        self.set_saved_loops_of(&active.path, loops, cx);
    }

    /// Forgets the saved loop at `index` of the selected track.
    pub(crate) fn delete_saved_loop(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(track) = self.selected_track() else {
            return;
        };
        let path = track.path.clone();
        let mut loops = track.saved_loops.clone();
        if index >= loops.len() {
            return;
        }
        loops.remove(index);
        self.set_saved_loops_of(&path, loops, cx);
    }

    /// Jumps to the saved loop at `index` of the selected track and loops
    /// it.
    pub(crate) fn play_saved_loop(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(saved) = self
            .selected_track()
            .and_then(|track| track.saved_loops.get(index))
            .cloned()
        else {
            return;
        };
        self.set_active_loop(
            saved.start_seconds,
            saved.end_seconds,
            None,
            Some(saved.start_seconds),
            cx,
        );
    }

    fn set_saved_loops_of(
        &mut self,
        path: &Path,
        loops: Vec<SavedLoop>,
        cx: &mut Context<'_, Self>,
    ) {
        self.session.set_saved_loops(path, loops.clone());
        cx.notify();
        let Some(cache) = self.session.cache() else {
            return;
        };
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let kept = cx
                .background_executor()
                .spawn(async move { cache.set_saved_loops(&path, &loops) })
                .await;
            if let Err(error) = kept {
                this.update(cx, |this, cx| {
                    this.notice = Some(format!(
                        "The loop shows, but it could not be kept for next time: {}",
                        error.chain()
                    ));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> LoopGrid {
        LoopGrid {
            first_beat_seconds: 0.25,
            beat_seconds: 0.5,
            snaps: true,
        }
    }

    fn free() -> LoopGrid {
        LoopGrid {
            first_beat_seconds: 0.0,
            beat_seconds: 0.5,
            snaps: false,
        }
    }

    fn close(found: Option<(f64, f64)>, expected: (f64, f64)) {
        let (start, end) = found.expect("a loop");
        assert!(
            (start - expected.0).abs() < 1e-9 && (end - expected.1).abs() < 1e-9,
            "{start}..{end} against {expected:?}"
        );
    }

    #[test]
    fn a_beat_loop_starts_on_the_beat_at_or_before_the_playhead() {
        close(beat_loop(&grid(), 10.6, 16.0, 300.0), (10.25, 18.25));
        close(beat_loop(&grid(), 10.25, 4.0, 300.0), (10.25, 12.25));
        close(beat_loop(&grid(), 10.74, 0.5, 300.0), (10.25, 10.5));
        close(beat_loop(&grid(), 0.1, 1.0, 300.0), (0.1, 0.6));
        close(beat_loop(&grid(), 298.0, 16.0, 300.0), (297.75, 300.0));
        assert_eq!(beat_loop(&grid(), 300.0, 1.0, 299.76), None);
    }

    #[test]
    fn without_a_grid_a_loop_starts_at_the_playhead_and_lasts_by_the_tempo() {
        close(beat_loop(&free(), 10.6, 16.0, 300.0), (10.6, 18.6));
        close(drawn(&free(), 12.3, 10.1, false, 300.0), (10.1, 12.3));
        assert_eq!(free().nearest_beat(7.77), 7.77);
    }

    #[test]
    fn halving_and_doubling_keep_the_start_within_limits() {
        close(resized(&grid(), (10.25, 18.25), 0.5, 300.0), (10.25, 14.25));
        close(resized(&grid(), (10.25, 18.25), 2.0, 300.0), (10.25, 26.25));
        assert_eq!(resized(&grid(), (290.25, 298.25), 2.0, 300.0), None);
        let mut region = (10.25, 10.5);
        let mut halvings = 0;
        while let Some(halved) = resized(&grid(), region, 0.5, 300.0) {
            region = halved;
            halvings += 1;
        }
        assert!(halvings <= 4, "{halvings}");
        assert!(region.1 - region.0 >= SHORTEST_LOOP_SECONDS);
    }

    #[test]
    fn a_drawn_loop_snaps_each_edge_to_the_nearest_beat_unless_free() {
        close(drawn(&grid(), 12.4, 10.3, false, 300.0), (10.25, 12.25));
        close(drawn(&grid(), 10.3, 10.4, false, 300.0), (10.25, 10.75));
        close(drawn(&grid(), 10.3, 12.4, true, 300.0), (10.3, 12.4));
        close(
            with_edge_at(&grid(), (10.25, 12.25), LoopEdge::Out, 14.1, false, 300.0),
            (10.25, 14.25),
        );
        assert_eq!(
            with_edge_at(&grid(), (10.25, 12.25), LoopEdge::In, 13.0, false, 300.0),
            None
        );
    }

    #[test]
    fn lengths_have_short_labels_and_words() {
        let labels: Vec<String> = LOOP_BEATS
            .iter()
            .map(|beats| length_label(*beats))
            .collect();
        assert_eq!(labels, ["\u{bd}", "1", "1", "2", "4", "8", "16"]);
        assert_eq!(length_words(0.5), "half a beat");
        assert_eq!(length_words(1.0), "1 beat");
        assert_eq!(length_words(16.0), "4 bars");
        assert_eq!(length_words(6.0), "6 beats");
    }

    #[test]
    fn a_new_saved_loop_takes_the_next_free_name() {
        let saved = |name: &str| SavedLoop {
            start_seconds: 0.0,
            end_seconds: 1.0,
            name: name.to_string(),
        };
        assert_eq!(next_loop_name(&[]), "Loop 1");
        assert_eq!(next_loop_name(&[saved("Loop 2")]), "Loop 3");
        assert_eq!(next_loop_name(&[saved("Loop 2"), saved("Intro")]), "Loop 3");
        assert_eq!(next_loop_name(&[saved("Loop 3"), saved("x")]), "Loop 4");
    }
}
