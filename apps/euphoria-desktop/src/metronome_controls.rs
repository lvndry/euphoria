// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome as the window runs it: the button that starts and stops
//! the clicks, the tempo field beside it, and the sound, volume and accent
//! chosen in the settings.
//!
//! The tempo is the one the user typed, if they typed one, and otherwise
//! the tempo shown for the selected track, and otherwise 120. Once a value
//! is typed it stays until the user changes it.

use gpui::{Context, Window};

use crate::app::EuphoriaApp;
use crate::format;
use crate::metronome::{
    ClickSound, Metronome, MetronomeControl, MetronomeSettings, STARTING_BPM, clamp_bpm, parse_bpm,
};
use crate::playback::Voice;

/// How many steps the volume has.
pub(crate) const VOLUME_STEPS: u8 = 5;

/// The tempo field's tooltip, which is also the rule in the docs.
pub(crate) const TEMPO_RULE: &str = "Tempo: type 30 to 300 (127,5 and 127.5 both work), scroll \
     over it, or use the arrows (Alt: 0.1). Until you set a value it follows the selected \
     track's tempo, starting at 120. Once you set one, it stays until you change it.";

/// The tempo filter's tooltip: the same rule as the metronome field.
pub(crate) const TEMPO_RANGE_RULE: &str = "Tempo range: type 30 to 300 (127,5 and 127.5 both work). \
     Leave a side empty for no limit. Uses the corrected tempo when there is one.";

/// The step of the tempo arrows, in beats per minute.
pub(crate) const BPM_STEP: f32 = 1.0;

/// The step of the tempo arrows with Alt held.
pub(crate) const BPM_FINE_STEP: f32 = 0.1;

/// What the metronome holds in the window.
pub(crate) struct MetronomeState {
    /// The sound, volume and accent the audio thread reads.
    pub(crate) control: MetronomeControl,
    /// What is remembered between launches.
    pub(crate) settings: MetronomeSettings,
    /// The tempo the user typed or scrolled to, if any.
    pub(crate) typed_bpm: Option<f32>,
    /// Whether the tempo field is open for typing.
    pub(crate) is_editing: bool,
    voice: Option<Box<dyn Voice>>,
    preview: Option<Box<dyn Voice>>,
}

impl MetronomeState {
    /// A stopped metronome with `settings`.
    pub(crate) fn new(settings: MetronomeSettings) -> Self {
        Self {
            control: MetronomeControl::new(settings),
            settings,
            typed_bpm: None,
            is_editing: false,
            voice: None,
            preview: None,
        }
    }

    /// Whether the clicks are sounding.
    pub(crate) fn is_running(&self) -> bool {
        self.voice.is_some()
    }

    /// The tempo to click at: the typed one, else `track_bpm` when it is in
    /// range, else 120.
    pub(crate) fn bpm(&self, track_bpm: Option<f32>) -> f32 {
        self.typed_bpm
            .or(track_bpm.filter(|bpm| bpm.is_finite()).map(clamp_bpm))
            .unwrap_or(STARTING_BPM)
    }

    /// The volume step, 1 to [`VOLUME_STEPS`], nearest the volume.
    pub(crate) fn volume_step(&self) -> u8 {
        let step = (self.settings.volume * f32::from(VOLUME_STEPS)).round();
        (step as u8).clamp(1, VOLUME_STEPS)
    }
}

impl EuphoriaApp {
    /// The tempo the metronome clicks at now.
    pub(crate) fn metronome_bpm(&self) -> f32 {
        self.metronome
            .bpm(self.selected_track().and_then(|track| track.bpm()))
    }

    /// Keeps the audio thread on the tempo the field shows, so a new track
    /// or a corrected tempo is heard from the next beat.
    pub(crate) fn sync_metronome_tempo(&self) {
        self.metronome.control.set_bpm(self.metronome_bpm());
    }

    /// Starts the clicks, or stops them.
    pub(crate) fn toggle_metronome(&mut self, cx: &mut Context<'_, Self>) {
        if self.metronome.voice.take().is_none() {
            self.sync_metronome_tempo();
            match self
                .output
                .play_metronome(Metronome::running(self.metronome.control.clone()))
            {
                Ok(voice) => self.metronome.voice = Some(voice),
                Err(problem) => self.notice = Some(problem.message().to_string()),
            }
        }
        cx.notify();
    }

    /// Opens the tempo field on the tempo in use.
    pub(crate) fn edit_metronome_bpm(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.metronome.is_editing = true;
        let shown = format::bpm(self.metronome_bpm());
        self.metronome_input.update(cx, |input, cx| {
            input.set_value(shown, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Takes the tempo typed in the field, or says why it cannot be used.
    pub(crate) fn commit_metronome_bpm(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.metronome.is_editing {
            return;
        }
        self.metronome.is_editing = false;
        let typed = self.metronome_input.read(cx).value().to_string();
        match parse_bpm(&typed) {
            Some(bpm) => {
                self.metronome.typed_bpm = Some(bpm);
                self.sync_metronome_tempo();
            }
            None => {
                self.notice = Some(
                    "Type a metronome tempo from 30 to 300, such as 127.5 or 127,5.".to_string(),
                );
            }
        }
        self.window_focus.focus(window, cx);
        cx.notify();
    }

    /// Moves the tempo by `beats` and keeps the result as typed.
    pub(crate) fn nudge_metronome_bpm(&mut self, beats: f32, cx: &mut Context<'_, Self>) {
        self.metronome.typed_bpm = Some(clamp_bpm(self.metronome_bpm() + beats));
        self.sync_metronome_tempo();
        cx.notify();
    }

    /// Whether the tempo can go up, or down when `is_down`, from where it is.
    pub(crate) fn can_step_metronome_bpm(&self, is_down: bool) -> bool {
        let bpm = self.metronome_bpm();
        if is_down {
            bpm > clamp_bpm(f32::MIN)
        } else {
            bpm < clamp_bpm(f32::MAX)
        }
    }

    /// Moves the tempo by one beat, or a tenth when `is_fine`, keeping the
    /// decimals and the limits. While the field is open it starts from what
    /// is typed there and shows the result.
    pub(crate) fn step_metronome_bpm(
        &mut self,
        is_down: bool,
        is_fine: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let start = if self.metronome.is_editing {
            let typed = self.metronome_input.read(cx).value().to_string();
            parse_bpm(&typed).unwrap_or_else(|| self.metronome_bpm())
        } else {
            self.metronome_bpm()
        };
        let step = if is_fine { BPM_FINE_STEP } else { BPM_STEP };
        let signed = if is_down { -step } else { step };
        let stepped = (clamp_bpm(start + signed) * 10.0).round() / 10.0;
        self.metronome.typed_bpm = Some(stepped);
        self.sync_metronome_tempo();
        if self.metronome.is_editing {
            let shown = format::bpm(stepped);
            self.metronome_input
                .update(cx, |input, cx| input.set_value(shown, window, cx));
        }
        cx.notify();
    }

    /// Chooses the click sound and plays one click of it.
    pub(crate) fn set_metronome_sound(&mut self, sound: ClickSound, cx: &mut Context<'_, Self>) {
        self.metronome.settings.sound = sound;
        self.metronome_settings_changed(true, cx);
    }

    /// Sets the volume to step `step` of [`VOLUME_STEPS`] and plays one click.
    pub(crate) fn set_metronome_volume(&mut self, step: u8, cx: &mut Context<'_, Self>) {
        let step = step.clamp(1, VOLUME_STEPS);
        self.metronome.settings.volume = f32::from(step) / f32::from(VOLUME_STEPS);
        self.metronome_settings_changed(true, cx);
    }

    /// Turns the accent on the first beat of the bar on or off.
    pub(crate) fn toggle_metronome_accent(&mut self, cx: &mut Context<'_, Self>) {
        self.metronome.settings.accent = !self.metronome.settings.accent;
        self.metronome_settings_changed(false, cx);
    }

    fn metronome_settings_changed(&mut self, with_preview: bool, cx: &mut Context<'_, Self>) {
        self.metronome.control.apply(self.metronome.settings);
        if with_preview && !self.metronome.is_running() {
            self.metronome.preview = None;
            match self
                .output
                .play_metronome(Metronome::preview(self.metronome.control.clone()))
            {
                Ok(voice) => self.metronome.preview = Some(voice),
                Err(problem) => self.notice = Some(problem.message().to_string()),
            }
        }
        if let Some(problem) = self.save_settings() {
            self.report(problem);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tempo_starts_at_120_then_follows_the_track_until_one_is_typed() {
        let mut state = MetronomeState::new(MetronomeSettings::default());
        assert_eq!(state.bpm(None), 120.0);
        assert_eq!(state.bpm(Some(127.4)), 127.4);
        assert_eq!(state.bpm(Some(20.0)), 30.0);
        state.typed_bpm = Some(98.0);
        assert_eq!(state.bpm(Some(127.4)), 98.0);
        assert_eq!(state.bpm(None), 98.0);
    }

    #[test]
    fn the_volume_step_is_the_nearest_of_five() {
        let step = |volume: f32| {
            MetronomeState::new(MetronomeSettings {
                volume,
                ..MetronomeSettings::default()
            })
            .volume_step()
        };
        assert_eq!(step(0.0), 1);
        assert_eq!(step(0.7), 4);
        assert_eq!(step(1.0), 5);
        assert_eq!(step(7.0), 5);
    }
}
