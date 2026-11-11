// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing the cues of the selected track on its waveform: moving a cue by
//! dragging it onto the beats of the grid, adding one at the playhead or
//! with a double click, renaming it, giving it a hot cue pad and deleting
//! it, with an undo for each change.
//!
//! The first edit turns the cues analysis placed into the user's own set.
//! The set wins over analysis everywhere, is kept in the cache apart from
//! the analyses, like a correction, and can be reset to the automatic cues.

use std::path::{Path, PathBuf};

use euphoria_core::{BeatGrid, Cue, HotCue};
use gpui::{Context, Modifiers, Pixels, Window, px};

use crate::app::EuphoriaApp;
use crate::key_by_ear::LatestWriter;
use crate::player::fraction_along;

/// How far the pointer must move after pressing a cue before the press is
/// a drag rather than a click.
const DRAG_THRESHOLD: f32 = 3.0;

/// How many edits can be undone.
const UNDO_LIMIT: usize = 200;

/// What a moved or added cue lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapTo {
    /// The nearest beat of the grid.
    Beat,
    /// The nearest first beat of a bar, while Shift is held.
    Bar,
}

impl SnapTo {
    /// Bars while Shift is held, beats otherwise.
    pub(crate) fn of(modifiers: Modifiers) -> Self {
        if modifiers.shift {
            Self::Bar
        } else {
            Self::Beat
        }
    }
}

/// `seconds` moved onto the nearest beat or bar of `grid`, within the
/// track's `duration_seconds`. Without a grid the time is kept as it is.
pub(crate) fn snapped(
    seconds: f64,
    grid: Option<&BeatGrid>,
    snap_to: SnapTo,
    duration_seconds: f64,
) -> f64 {
    let end = duration_seconds.max(0.0);
    let Some(grid) = grid else {
        return seconds.clamp(0.0, end);
    };
    let step = match snap_to {
        SnapTo::Beat => grid.beat_seconds(),
        SnapTo::Bar => grid.bar_seconds(),
    };
    if !(step.is_finite() && step > 0.0) {
        return seconds.clamp(0.0, end);
    }
    let steps = ((seconds - grid.first_downbeat_seconds) / step).round();
    let mut snapped = grid.first_downbeat_seconds + steps * step;
    if snapped < 0.0 {
        snapped += step;
    }
    if snapped > end {
        snapped -= step;
    }
    snapped.clamp(0.0, end)
}

/// One change to the cues of a track.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CueChange {
    /// A new cue at this time, on the first free hot cue pad.
    Add {
        /// Seconds from the start of the track.
        seconds: f64,
    },
    /// The cue at `index` moved to `seconds`.
    Move {
        /// Which cue, in order of time.
        index: usize,
        /// Where it goes.
        seconds: f64,
    },
    /// The cue at `index` named `name`.
    Rename {
        /// Which cue, in order of time.
        index: usize,
        /// Its new name.
        name: String,
    },
    /// The cue at `index` on pad `hot_cue`, or on none. A cue already on
    /// that pad takes the pad the moved one leaves.
    SetHotCue {
        /// Which cue, in order of time.
        index: usize,
        /// The pad, or `None` for a memory cue only.
        hot_cue: Option<HotCue>,
    },
    /// The cue at `index` removed.
    Delete {
        /// Which cue, in order of time.
        index: usize,
    },
}

/// `cues` with `change` made, in order of time, or `None` when it changes
/// nothing or names a cue that is not there.
pub(crate) fn changed(cues: &[Cue], change: CueChange) -> Option<Vec<Cue>> {
    let mut changed = cues.to_vec();
    match change {
        CueChange::Add { seconds } => {
            let hot_cue = HotCue::ALL
                .into_iter()
                .find(|pad| cues.iter().all(|cue| cue.hot_cue != Some(*pad)));
            changed.push(Cue {
                seconds,
                name: format!("Cue {}", cues.len() + 1),
                hot_cue,
            });
        }
        CueChange::Move { index, seconds } => {
            let cue = changed.get_mut(index)?;
            if cue.seconds == seconds {
                return None;
            }
            cue.seconds = seconds;
        }
        CueChange::Rename { index, name } => {
            let cue = changed.get_mut(index)?;
            let name = name.trim().to_string();
            if cue.name == name {
                return None;
            }
            cue.name = name;
        }
        CueChange::SetHotCue { index, hot_cue } => {
            let previous = changed.get(index)?.hot_cue;
            if previous == hot_cue {
                return None;
            }
            if let Some(holder) = changed
                .iter_mut()
                .find(|cue| hot_cue.is_some() && cue.hot_cue == hot_cue)
            {
                holder.hot_cue = previous;
            }
            changed[index].hot_cue = hot_cue;
        }
        CueChange::Delete { index } => {
            if index >= changed.len() {
                return None;
            }
            changed.remove(index);
        }
    }
    changed.sort_by(|first, second| first.seconds.total_cmp(&second.seconds));
    Some(changed)
}

/// What a track's cues were before one edit, to go back to.
#[derive(Debug, Clone, PartialEq)]
struct UndoStep {
    track: PathBuf,
    /// The user's set before the edit, or `None` for the automatic cues.
    before: Option<Vec<Cue>>,
}

/// The edits that can be undone, latest last, across tracks.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CueHistory {
    steps: Vec<UndoStep>,
}

impl CueHistory {
    /// Remembers that the cues of `track` were `before` until now.
    pub(crate) fn record(&mut self, track: &Path, before: Option<Vec<Cue>>) {
        if self.steps.len() == UNDO_LIMIT {
            self.steps.remove(0);
        }
        self.steps.push(UndoStep {
            track: track.to_path_buf(),
            before,
        });
    }

    /// True when an edit of the cues of `track` can be undone.
    pub(crate) fn can_undo(&self, track: &Path) -> bool {
        self.steps.iter().any(|step| step.track == track)
    }

    /// Forgets the latest edit of the cues of `track` and returns what they
    /// were before it: `Some(None)` for the automatic cues.
    pub(crate) fn take_latest(&mut self, track: &Path) -> Option<Option<Vec<Cue>>> {
        let position = self.steps.iter().rposition(|step| step.track == track)?;
        Some(self.steps.remove(position).before)
    }
}

/// A cue held down on the waveform, from the press until the release.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CueDrag {
    /// The track whose cue it is.
    pub(crate) track: PathBuf,
    /// Which cue, in order of time.
    pub(crate) index: usize,
    /// Where the press was, across the window.
    pressed_x: f32,
    /// Where the cue would land, once the pointer has moved far enough to
    /// make the press a drag.
    pub(crate) seconds: Option<f64>,
}

/// The state of cue editing in the track detail.
#[derive(Debug, Default)]
pub(crate) struct CueEditor {
    /// The cue being pressed or dragged.
    pub(crate) drag: Option<CueDrag>,
    /// The cue whose name is being typed.
    pub(crate) renaming: Option<usize>,
    /// The cue whose hot cue pad is being chosen.
    pub(crate) choosing_hot_cue: Option<usize>,
    /// The edits that can be undone.
    pub(crate) history: CueHistory,
    /// Keeps the user's sets in the cache; `None` forgets a set.
    pub(crate) writer: LatestWriter<Option<Vec<Cue>>>,
}

impl CueEditor {
    /// Lets go of what was half done on the track that was selected.
    pub(crate) fn forget_selection(&mut self) {
        self.drag = None;
        self.renaming = None;
        self.choosing_hot_cue = None;
    }
}

impl EuphoriaApp {
    /// The time in the selected track under `pointer_x`, along the waveform.
    fn seconds_under(&self, pointer_x: Pixels) -> Option<f64> {
        let bounds = self.waveform_bounds.get()?;
        let duration = self.selected_track()?.duration_seconds()?;
        let fraction = fraction_along(
            pointer_x / px(1.0),
            bounds.left() / px(1.0),
            bounds.size.width / px(1.0),
        );
        Some(f64::from(fraction) * f64::from(duration))
    }

    /// `seconds` snapped onto the selected track's grid.
    fn snapped_in_selected(&self, seconds: f64, snap_to: SnapTo) -> Option<f64> {
        let track = self.selected_track()?;
        let duration = f64::from(track.duration_seconds()?);
        Some(snapped(
            seconds,
            track.beat_grid().as_ref(),
            snap_to,
            duration,
        ))
    }

    /// Makes `change` to the cues of the selected track, which become the
    /// user's own set.
    pub(crate) fn change_cues(&mut self, change: CueChange, cx: &mut Context<'_, Self>) {
        let Some(track) = self.selected_track() else {
            return;
        };
        let Some(cues) = changed(&track.cues().cues, change) else {
            cx.notify();
            return;
        };
        let path = track.path.clone();
        self.set_cues_of(&path, Some(cues), true, cx);
    }

    /// Gives the track at `path` the set `cues`, or the automatic cues with
    /// `None`, at once on screen and then in the cache, off the main thread
    /// since keeping them may read the file. With `undoable`, the edit can
    /// be undone.
    fn set_cues_of(
        &mut self,
        path: &Path,
        cues: Option<Vec<Cue>>,
        undoable: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(before) = self
            .session
            .library()
            .track_by_path(path)
            .map(|track| track.edited_cues.clone())
        else {
            return;
        };
        if undoable {
            self.cue_editor.history.record(path, before);
        }
        self.cue_editor.renaming = None;
        self.cue_editor.choosing_hot_cue = None;
        self.session.set_edited_cues(path, cues.clone());
        cx.notify();
        let Some(cache) = self.session.cache() else {
            return;
        };
        let writer = self.cue_editor.writer.clone();
        writer.queue(path, cues);
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let kept = cx
                .background_executor()
                .spawn(async move {
                    writer.write_latest(&path, |cues| cache.set_edited_cues(&path, cues.as_deref()))
                })
                .await;
            if let Err(error) = kept {
                this.update(cx, |this, cx| {
                    this.notice = Some(format!(
                        "The cues show, but they could not be kept for next time: {}",
                        error.chain()
                    ));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Adds a cue at the playhead, on the nearest beat.
    pub(crate) fn add_cue_at_playhead(&mut self, cx: &mut Context<'_, Self>) {
        let is_selected_in_player =
            self.selected_track().map(|track| track.path.as_path()) == self.player.track();
        let position = if is_selected_in_player {
            f64::from(self.player.position_seconds())
        } else {
            0.0
        };
        if let Some(seconds) = self.snapped_in_selected(position, SnapTo::Beat) {
            self.change_cues(CueChange::Add { seconds }, cx);
        }
    }

    /// Adds a cue where the waveform was double clicked, on the nearest
    /// beat, or bar with Shift.
    pub(crate) fn add_cue_at_pointer(
        &mut self,
        pointer_x: Pixels,
        modifiers: Modifiers,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(seconds) = self
            .seconds_under(pointer_x)
            .and_then(|seconds| self.snapped_in_selected(seconds, SnapTo::of(modifiers)))
        else {
            return;
        };
        self.change_cues(CueChange::Add { seconds }, cx);
    }

    /// A press on the marker of cue `index`: a click plays from it, and a
    /// drag moves it.
    pub(crate) fn press_cue(&mut self, index: usize, pointer_x: Pixels) {
        let Some(track) = self.selected_track() else {
            return;
        };
        self.cue_editor.drag = Some(CueDrag {
            track: track.path.clone(),
            index,
            pressed_x: pointer_x / px(1.0),
            seconds: None,
        });
    }

    /// The pointer moved with the button held: a pressed cue follows it,
    /// onto the nearest beat, or bar with Shift. Returns false when no cue
    /// is held.
    pub(crate) fn drag_cue_to(
        &mut self,
        pointer_x: Pixels,
        modifiers: Modifiers,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(drag) = &self.cue_editor.drag else {
            return false;
        };
        if drag.seconds.is_none() && (pointer_x / px(1.0) - drag.pressed_x).abs() < DRAG_THRESHOLD {
            return true;
        }
        let seconds = self
            .seconds_under(pointer_x)
            .and_then(|seconds| self.snapped_in_selected(seconds, SnapTo::of(modifiers)));
        if let (Some(drag), Some(seconds)) = (&mut self.cue_editor.drag, seconds) {
            drag.seconds = Some(seconds);
            cx.notify();
        }
        true
    }

    /// The button was let go: a dragged cue moves to where it was dropped,
    /// and a cue only clicked plays from where it is.
    pub(crate) fn release_cue(&mut self, cx: &mut Context<'_, Self>) {
        let Some(drag) = self.cue_editor.drag.take() else {
            return;
        };
        let selected = self.selected_track().map(|track| track.path.clone());
        if selected.as_ref() != Some(&drag.track) {
            return;
        }
        match drag.seconds {
            Some(seconds) => self.change_cues(
                CueChange::Move {
                    index: drag.index,
                    seconds,
                },
                cx,
            ),
            None => self.play_from_cue(drag.index, cx),
        }
    }

    /// Plays the selected track from cue `index`.
    pub(crate) fn play_from_cue(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(cue) = self
            .selected_track()
            .and_then(|track| track.cues().cues.get(index).cloned())
        else {
            return;
        };
        for request in self.player.play_from(cue.seconds as f32) {
            self.carry_out(Some(request), cx);
        }
        cx.notify();
    }

    /// Removes cue `index` of the selected track.
    pub(crate) fn delete_cue(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        self.change_cues(CueChange::Delete { index }, cx);
    }

    /// Opens or closes the choice of hot cue pad for cue `index`.
    pub(crate) fn toggle_hot_cue_choice(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        self.cue_editor.choosing_hot_cue =
            (self.cue_editor.choosing_hot_cue != Some(index)).then_some(index);
        cx.notify();
    }

    /// Puts cue `index` on pad `hot_cue`, or on none.
    pub(crate) fn set_cue_hot_cue(
        &mut self,
        index: usize,
        hot_cue: Option<HotCue>,
        cx: &mut Context<'_, Self>,
    ) {
        self.cue_editor.choosing_hot_cue = None;
        self.change_cues(CueChange::SetHotCue { index, hot_cue }, cx);
    }

    /// Puts the name of cue `index` in the field over it.
    pub(crate) fn start_renaming_cue(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(name) = self
            .selected_track()
            .and_then(|track| track.cues().cues.get(index).map(|cue| cue.name.clone()))
        else {
            return;
        };
        self.cue_editor.renaming = Some(index);
        self.cue_name_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// Takes the name typed for the cue being renamed.
    pub(crate) fn finish_renaming_cue(&mut self, cx: &mut Context<'_, Self>) {
        let Some(index) = self.cue_editor.renaming.take() else {
            return;
        };
        let name = self.cue_name_input.read(cx).value().to_string();
        self.change_cues(CueChange::Rename { index, name }, cx);
    }

    /// Goes back to the cues analysis placed for the selected track.
    pub(crate) fn reset_cues(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self
            .selected_track()
            .filter(|track| track.edited_cues.is_some())
            .map(|track| track.path.clone())
        else {
            return;
        };
        self.set_cues_of(&path, None, true, cx);
    }

    /// Undoes the latest edit of the selected track's cues.
    pub(crate) fn undo_cue_edit(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        if let Some(before) = self.cue_editor.history.take_latest(&path) {
            self.set_cues_of(&path, before, false, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> BeatGrid {
        BeatGrid {
            first_downbeat_seconds: 0.25,
            bpm: 120.0,
            salience: 10.0,
            drift_seconds: Some(0.001),
        }
    }

    fn cue(seconds: f64, name: &str, letter: Option<char>) -> Cue {
        Cue {
            seconds,
            name: name.to_string(),
            hot_cue: letter.and_then(HotCue::from_letter),
        }
    }

    #[test]
    fn a_time_snaps_to_the_nearest_beat_or_bar_and_stays_in_the_track() {
        let grid = grid();
        assert_eq!(snapped(1.4, Some(&grid), SnapTo::Beat, 30.0), 1.25);
        assert_eq!(snapped(1.6, Some(&grid), SnapTo::Beat, 30.0), 1.75);
        assert_eq!(snapped(1.4, Some(&grid), SnapTo::Bar, 30.0), 2.25);
        assert_eq!(snapped(0.9, Some(&grid), SnapTo::Bar, 30.0), 0.25);
        assert_eq!(snapped(0.0, Some(&grid), SnapTo::Beat, 30.0), 0.25);
        assert_eq!(snapped(29.99, Some(&grid), SnapTo::Bar, 30.0), 28.25);
        assert_eq!(snapped(1.4, None, SnapTo::Bar, 30.0), 1.4);
        assert_eq!(snapped(-3.0, None, SnapTo::Beat, 30.0), 0.0);
    }

    #[test]
    fn shift_snaps_to_bars() {
        assert_eq!(SnapTo::of(Modifiers::shift()), SnapTo::Bar);
        assert_eq!(SnapTo::of(Modifiers::default()), SnapTo::Beat);
    }

    #[test]
    fn a_new_cue_takes_the_first_free_pad_and_the_set_stays_in_order() {
        let cues = vec![cue(10.0, "Start", Some('A')), cue(20.0, "Drop", Some('C'))];
        let added = changed(&cues, CueChange::Add { seconds: 15.0 }).unwrap();
        assert_eq!(added[1], cue(15.0, "Cue 3", Some('B')));
        let every_pad: Vec<Cue> = HotCue::ALL
            .iter()
            .map(|pad| cue(f64::from(pad.pad()), "Full", Some(pad.letter())))
            .collect();
        let added = changed(&every_pad, CueChange::Add { seconds: 50.0 }).unwrap();
        assert_eq!(added[8].hot_cue, None, "a ninth cue is a memory cue");
    }

    #[test]
    fn moving_renaming_and_deleting_change_one_cue() {
        let cues = vec![cue(10.0, "Start", Some('A')), cue(20.0, "Drop", Some('B'))];
        let moved = changed(
            &cues,
            CueChange::Move {
                index: 0,
                seconds: 30.0,
            },
        )
        .unwrap();
        assert_eq!(
            moved,
            vec![cue(20.0, "Drop", Some('B')), cue(30.0, "Start", Some('A'))]
        );
        let renamed = changed(
            &cues,
            CueChange::Rename {
                index: 1,
                name: "  Big drop ".to_string(),
            },
        )
        .unwrap();
        assert_eq!(renamed[1].name, "Big drop");
        assert_eq!(
            changed(&cues, CueChange::Delete { index: 0 }).unwrap(),
            vec![cue(20.0, "Drop", Some('B'))]
        );
        assert_eq!(changed(&cues, CueChange::Delete { index: 5 }), None);
        assert_eq!(
            changed(
                &cues,
                CueChange::Move {
                    index: 0,
                    seconds: 10.0
                }
            ),
            None,
            "a cue dropped where it was changes nothing"
        );
    }

    #[test]
    fn taking_a_pad_another_cue_holds_swaps_their_pads() {
        let cues = vec![cue(10.0, "Start", Some('A')), cue(20.0, "Drop", Some('B'))];
        let swapped = changed(
            &cues,
            CueChange::SetHotCue {
                index: 0,
                hot_cue: HotCue::from_letter('B'),
            },
        )
        .unwrap();
        assert_eq!(
            swapped,
            vec![cue(10.0, "Start", Some('B')), cue(20.0, "Drop", Some('A'))]
        );
        let memory_only = changed(
            &cues,
            CueChange::SetHotCue {
                index: 1,
                hot_cue: None,
            },
        )
        .unwrap();
        assert_eq!(memory_only[1].hot_cue, None);
        assert_eq!(memory_only[0].hot_cue, HotCue::from_letter('A'));
        euphoria_core::check_cues(&swapped).unwrap();
    }

    #[test]
    fn undo_goes_back_one_edit_of_the_track_at_a_time() {
        let mut history = CueHistory::default();
        let first = Path::new("/music/first.flac");
        let second = Path::new("/music/second.flac");
        history.record(first, None);
        history.record(second, Some(Vec::new()));
        history.record(first, Some(vec![cue(1.0, "One", None)]));
        assert!(history.can_undo(first));
        assert_eq!(
            history.take_latest(first),
            Some(Some(vec![cue(1.0, "One", None)]))
        );
        assert_eq!(history.take_latest(first), Some(None));
        assert_eq!(history.take_latest(first), None);
        assert!(history.can_undo(second));
    }
}
