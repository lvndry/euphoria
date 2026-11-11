// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking a track's key by ear and correcting what analysis got wrong.
//!
//! A soft drone of a key's root and fifth, or the notes of a small piano,
//! play over the track. When the ear disagrees with the detector, the user
//! takes the runner up key or the other tempo with a click, or picks a key
//! and types a tempo. A correction wins over the analysis everywhere, is
//! kept in the cache apart from analyses so that analyzing again never
//! loses it, and can be reset to the detected value.

use std::collections::VecDeque;
use std::path::PathBuf;

use euphoria_core::{Correction, Key};
use gpui::{Context, Window};

use crate::app::EuphoriaApp;
use crate::format;
use crate::playback::Voice;
use crate::reference_sound::{PIANO_LOWEST_C, ReferenceLevel, ReferenceSound};

/// How many steps the volume of the reference sounds has.
pub(crate) const LEVEL_STEPS: u8 = 5;

/// The step the volume starts at: loud enough to hear over most tracks,
/// soft enough not to bury them.
const STARTING_LEVEL_STEP: u8 = 3;

/// How many piano notes may ring at once. Each ends by itself; a quick run
/// past this cuts the oldest short.
const RINGING_NOTE_LIMIT: usize = 12;

/// The drone that is sounding, for which track and in which key.
struct Drone {
    track: PathBuf,
    key: Key,
    /// Dropping this stops the drone.
    _voice: Box<dyn Voice>,
}

/// The reference sounds and the correction editor of the track detail.
pub(crate) struct ByEar {
    drone: Option<Drone>,
    ringing_notes: VecDeque<Box<dyn Voice>>,
    level: ReferenceLevel,
    level_step: u8,
    /// True while the key picker and the tempo field are open.
    pub(crate) is_editing: bool,
    /// Why the tempo typed was not taken, when it was not.
    pub(crate) bpm_problem: Option<String>,
}

impl Default for ByEar {
    fn default() -> Self {
        Self {
            drone: None,
            ringing_notes: VecDeque::new(),
            level: ReferenceLevel::new(level_of(STARTING_LEVEL_STEP)),
            level_step: STARTING_LEVEL_STEP,
            is_editing: false,
            bpm_problem: None,
        }
    }
}

impl ByEar {
    /// The key the drone plays, while one plays.
    pub(crate) fn drone_key(&self) -> Option<Key> {
        self.drone.as_ref().map(|drone| drone.key)
    }

    /// The step the volume is at, 1 to [`LEVEL_STEPS`].
    pub(crate) fn level_step(&self) -> u8 {
        self.level_step
    }
}

/// The level of volume step `step`.
fn level_of(step: u8) -> f32 {
    f32::from(step.clamp(1, LEVEL_STEPS)) / f32::from(LEVEL_STEPS)
}

/// The notes of `key`'s scale, as pitch classes 0 (C) to 11 (B): the major
/// scale, or the natural minor scale DJs mix in.
pub(crate) fn scale_of(key: Key) -> [u8; 7] {
    let steps = match key.mode() {
        euphoria_core::Mode::Major => [0, 2, 4, 5, 7, 9, 11],
        euphoria_core::Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
    };
    steps.map(|step| (key.tonic() + step) % 12)
}

impl EuphoriaApp {
    /// The tuning of the selected track, so the reference matches a track a
    /// little sharp or flat.
    fn selected_tuning_cents(&self) -> f32 {
        self.selected_track()
            .and_then(|track| track.analysis())
            .and_then(|analysis| analysis.tuning_cents)
            .unwrap_or(0.0)
    }

    /// Starts a drone of `key` over the track, or stops it when it is the
    /// drone already sounding.
    pub(crate) fn toggle_drone(&mut self, key: Key, cx: &mut Context<'_, Self>) {
        let is_same = self.by_ear.drone_key() == Some(key);
        self.by_ear.drone = None;
        if !is_same && let Some(track) = self.selected_track().map(|track| track.path.clone()) {
            let sound =
                ReferenceSound::drone(key, self.selected_tuning_cents(), self.by_ear.level.clone());
            match self.output.play_reference(sound) {
                Ok(voice) => {
                    self.by_ear.drone = Some(Drone {
                        track,
                        key,
                        _voice: voice,
                    });
                }
                Err(problem) => self.notice = Some(problem.message().to_string()),
            }
        }
        cx.notify();
    }

    /// Strikes the piano key `pitch_class`, 0 (C) to 11 (B), in the octave
    /// from middle C.
    pub(crate) fn play_piano_note(&mut self, pitch_class: u8, cx: &mut Context<'_, Self>) {
        let note = PIANO_LOWEST_C + pitch_class % 12;
        let sound = ReferenceSound::piano_note(
            note,
            self.selected_tuning_cents(),
            self.by_ear.level.clone(),
        );
        match self.output.play_reference(sound) {
            Ok(voice) => {
                if self.by_ear.ringing_notes.len() == RINGING_NOTE_LIMIT {
                    self.by_ear.ringing_notes.pop_front();
                }
                self.by_ear.ringing_notes.push_back(voice);
            }
            Err(problem) => {
                self.notice = Some(problem.message().to_string());
                cx.notify();
            }
        }
    }

    /// Sets the volume of the reference sounds, those playing included, to
    /// step `step` of [`LEVEL_STEPS`].
    pub(crate) fn set_reference_level(&mut self, step: u8, cx: &mut Context<'_, Self>) {
        let step = step.clamp(1, LEVEL_STEPS);
        self.by_ear.level_step = step;
        self.by_ear.level.set(level_of(step));
        cx.notify();
    }

    /// Stops the drone when the track it was started for is no longer the
    /// one selected, since it plays that track's key.
    pub(crate) fn stop_drone_of_other_tracks(&mut self) {
        let selected = self.selected_track().map(|track| track.path.as_path());
        if self
            .by_ear
            .drone
            .as_ref()
            .is_some_and(|drone| Some(drone.track.as_path()) != selected)
        {
            self.by_ear.drone = None;
        }
    }

    /// Gives the selected track `correction`, at once on screen and then in
    /// the cache, off the main thread since keeping it may read the file.
    fn correct_selected(
        &mut self,
        change: impl FnOnce(Correction) -> Correction,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(track) = self.selected_track() else {
            return;
        };
        let path = track.path.clone();
        let correction = change(track.correction);
        self.session.set_correction(&path, correction);
        self.refresh_visible();
        self.refresh_mixes();
        cx.notify();
        let Some(cache) = self.session.cache() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let kept = cx
                .background_executor()
                .spawn(async move { cache.set_correction(&path, &correction) })
                .await;
            if let Err(error) = kept {
                this.update(cx, |this, cx| {
                    this.notice = Some(format!(
                        "The correction shows, but it could not be kept for next time: {}",
                        error.chain()
                    ));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Takes `key` as the selected track's key.
    pub(crate) fn accept_key(&mut self, key: Key, cx: &mut Context<'_, Self>) {
        self.correct_selected(|correction| correction.with_key(Some(key)), cx);
    }

    /// Takes `bpm` as the selected track's tempo. Tempos come from the
    /// detector's alternative or the field, both already checked.
    pub(crate) fn accept_bpm(&mut self, bpm: f32, cx: &mut Context<'_, Self>) {
        self.correct_selected(
            |correction| correction.with_bpm(Some(bpm)).unwrap_or(correction),
            cx,
        );
    }

    /// Goes back to the detected key of the selected track.
    pub(crate) fn reset_key(&mut self, cx: &mut Context<'_, Self>) {
        self.correct_selected(|correction| correction.with_key(None), cx);
    }

    /// Goes back to the detected tempo of the selected track.
    pub(crate) fn reset_bpm(&mut self, cx: &mut Context<'_, Self>) {
        self.correct_selected(
            |correction| correction.with_bpm(None).unwrap_or(correction),
            cx,
        );
    }

    /// Opens or closes the key picker and the tempo field. Opening fills
    /// the field with the tempo on show.
    pub(crate) fn toggle_value_editor(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.by_ear.is_editing = !self.by_ear.is_editing;
        self.by_ear.bpm_problem = None;
        if self.by_ear.is_editing {
            let shown = self
                .selected_track()
                .and_then(|track| track.bpm())
                .map(format::bpm)
                .unwrap_or_default();
            self.bpm_input.update(cx, |input, cx| {
                input.set_value(shown, window, cx);
            });
        }
        cx.notify();
    }

    /// Takes the tempo typed in the field, or says why it cannot.
    pub(crate) fn accept_typed_bpm(&mut self, cx: &mut Context<'_, Self>) {
        let typed = self.bpm_input.read(cx).value().trim().replace(',', ".");
        let parsed = typed
            .parse::<f32>()
            .ok()
            .filter(|bpm| bpm.is_finite() && (20.0..=400.0).contains(bpm));
        match parsed {
            Some(bpm) => {
                self.by_ear.bpm_problem = None;
                self.accept_bpm(bpm, cx);
            }
            None => {
                self.by_ear.bpm_problem =
                    Some("Type a tempo between 20 and 400, such as 124 or 87.5.".to_string());
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use euphoria_core::Mode;

    use super::*;

    #[test]
    fn scales_are_the_major_and_the_natural_minor() {
        assert_eq!(scale_of(Key::new(0, Mode::Major)), [0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(scale_of(Key::new(9, Mode::Minor)), [9, 11, 0, 2, 4, 5, 7]);
        let mut relative = scale_of(Key::new(9, Mode::Minor));
        relative.sort_unstable();
        assert_eq!(relative, scale_of(Key::new(0, Mode::Major)));
        assert_eq!(scale_of(Key::new(11, Mode::Major))[1], 1);
    }

    #[test]
    fn the_volume_has_five_steps_up_to_full() {
        assert_eq!(level_of(LEVEL_STEPS), 1.0);
        assert_eq!(level_of(1), 0.2);
        assert_eq!(level_of(0), 0.2);
        assert_eq!(level_of(9), 1.0);
        let by_ear = ByEar::default();
        assert_eq!(by_ear.level_step(), STARTING_LEVEL_STEP);
        assert!((by_ear.level.get() - 0.6).abs() < 1e-6);
        assert_eq!(by_ear.drone_key(), None);
    }
}
