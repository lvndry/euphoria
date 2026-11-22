// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playing the selected track faster or slower, in the focus view: a target
//! tempo within the common pitch-fader range, with the pitch kept or not.
//!
//! It changes playback only. Nothing is written to the file or the cache,
//! and the tempo goes back to the track's own when another track is played.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use crate::app::EuphoriaApp;
use crate::format;
use crate::metronome::parse_bpm;
use crate::playback::PlaybackTempo;

/// How far from the track's tempo the target may go, in percent either way.
pub(crate) const TEMPO_RANGE_PERCENT: f64 = 16.0;

/// The step of the arrows, in beats per minute.
pub(crate) const TEMPO_STEP: f32 = 1.0;

/// The step of the arrows with Alt held.
pub(crate) const TEMPO_FINE_STEP: f32 = 0.1;

/// The tooltip of the tempo field, which is also the rule in the docs.
pub(crate) const TEMPO_FIELD_RULE: &str = "Playback tempo: type a BPM (127,5 and 127.5 both \
     work), or use the arrows (Alt: 0.1). It is clamped to 16% either way of the track's own \
     tempo. Changes playback only; the file is not changed.";

/// The tooltip of the reset button.
pub(crate) const TEMPO_RESET_RULE: &str =
    "Back to the track's own tempo. Changes playback only; the file is not changed.";

/// The tooltip of the key lock switch.
pub(crate) const KEY_LOCK_RULE: &str = "Key lock on keeps the pitch while the tempo changes. \
     Off, the pitch follows the speed, as on a turntable. Changes playback only; the file is \
     not changed.";

/// The slowest and fastest rate the range allows.
pub(crate) fn rate_limits() -> (f64, f64) {
    (
        1.0 - TEMPO_RANGE_PERCENT / 100.0,
        1.0 + TEMPO_RANGE_PERCENT / 100.0,
    )
}

/// The rate that plays a track of tempo `original_bpm` at `target_bpm`,
/// kept within the range.
pub(crate) fn rate_for_target(original_bpm: f32, target_bpm: f32) -> f64 {
    let (slowest, fastest) = rate_limits();
    if !original_bpm.is_finite() || original_bpm <= 0.0 || !target_bpm.is_finite() {
        return 1.0;
    }
    (f64::from(target_bpm) / f64::from(original_bpm)).clamp(slowest, fastest)
}

/// The tempo heard when a track of `original_bpm` plays at `rate`, to a
/// tenth.
pub(crate) fn target_for_rate(original_bpm: f32, rate: f64) -> f32 {
    ((f64::from(original_bpm) * rate * 10.0).round() / 10.0) as f32
}

/// The rate for a typed tempo, or `None` when the text is not a tempo.
pub(crate) fn rate_for_typed(original_bpm: f32, typed: &str) -> Option<f64> {
    parse_bpm(typed).map(|target| rate_for_target(original_bpm, target))
}

/// The change of speed in percent, to a tenth, from the rate.
pub(crate) fn percent_of(rate: f64) -> f32 {
    (((rate - 1.0) * 1000.0).round() / 10.0) as f32
}

/// The readout of the change, such as "+3.3%", "-2.5%" or "0.0%".
pub(crate) fn percent_text(rate: f64) -> String {
    let percent = percent_of(rate);
    if percent == 0.0 {
        "0.0%".to_string()
    } else {
        format!("{percent:+.1}%")
    }
}

/// True when a rate plays the track as recorded.
pub(crate) fn is_original(rate: f64) -> bool {
    percent_of(rate) == 0.0 && (rate - 1.0).abs() < 1e-6
}

/// What the player bar says while the tempo differs from the original:
/// "120.0 \u{2192} 124.0 BPM".
pub(crate) fn bar_text(original_bpm: f32, rate: f64) -> Option<String> {
    if is_original(rate) {
        return None;
    }
    Some(format!(
        "{} \u{2192} {} BPM",
        format::bpm(original_bpm),
        format::bpm(target_for_rate(original_bpm, rate))
    ))
}

/// The tempo control of the focus view.
pub(crate) struct TempoState {
    /// The track the tempo was set for.
    pub(crate) track: Option<PathBuf>,
    /// Seconds of the track per second heard: 1.0 is the track's tempo.
    pub(crate) rate: f64,
    /// Keep the pitch while the tempo changes.
    pub(crate) key_lock: bool,
    /// Whether the tempo field is open for typing.
    pub(crate) is_editing: bool,
}

impl Default for TempoState {
    fn default() -> Self {
        Self {
            track: None,
            rate: 1.0,
            key_lock: true,
            is_editing: false,
        }
    }
}

impl TempoState {
    /// The rate for the track at `path`: the one set, when it was set for
    /// that track, and the track's own tempo otherwise.
    pub(crate) fn rate_for(&self, path: &Path) -> f64 {
        if self.track.as_deref() == Some(path) {
            self.rate
        } else {
            1.0
        }
    }

    /// What a voice of the track at `path` plays with.
    pub(crate) fn playback_for(&self, path: &Path) -> PlaybackTempo {
        PlaybackTempo {
            rate: self.rate_for(path),
            key_lock: self.key_lock,
        }
    }

    /// Back to the track's own tempo, keeping the key lock choice.
    pub(crate) fn reset(&mut self) {
        self.track = None;
        self.rate = 1.0;
        self.is_editing = false;
    }

    /// Forgets the tempo unless it belongs to the track at `path`.
    pub(crate) fn forget_unless_for(&mut self, path: Option<&Path>) {
        if self.track.as_deref() != path {
            self.reset();
        }
    }
}

impl EuphoriaApp {
    /// The tempo of the selected track as recorded, when it has one.
    pub(crate) fn tempo_original_bpm(&self) -> Option<f32> {
        self.selected_track()
            .and_then(|track| track.bpm())
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
    }

    /// The rate the selected track plays at.
    pub(crate) fn tempo_rate(&self) -> f64 {
        self.selected_track()
            .map_or(1.0, |track| self.tempo.rate_for(&track.path))
    }

    /// The tempo the selected track plays at, when it has one.
    pub(crate) fn tempo_target_bpm(&self) -> Option<f32> {
        self.tempo_original_bpm()
            .map(|original| target_for_rate(original, self.tempo_rate()))
    }

    /// What the player bar says of the tempo of the track that plays: the
    /// voice's own rate against that track's tempo.
    pub(crate) fn playing_tempo_text(&self) -> Option<String> {
        let active = self.voice.as_ref()?;
        let path = self.player.track()?;
        let original = self.session.library().track_by_path(path)?.bpm()?;
        bar_text(original, active.state.tempo().rate)
    }

    /// Gives the playing voice the tempo chosen, when the selected track is
    /// the one playing.
    pub(crate) fn apply_tempo_to_playback(&self) {
        let (Some(active), Some(playing)) = (&self.voice, self.player.track()) else {
            return;
        };
        if self.selected_track().map(|track| track.path.as_path()) != Some(playing) {
            return;
        }
        active.state.set_tempo(self.tempo.playback_for(playing));
    }

    fn set_tempo_rate(&mut self, rate: f64, cx: &mut Context<'_, Self>) {
        let Some(path) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        let (slowest, fastest) = rate_limits();
        self.tempo.track = Some(path);
        self.tempo.rate = rate.clamp(slowest, fastest);
        self.apply_tempo_to_playback();
        cx.notify();
    }

    /// Plays the selected track at `target_bpm`, within the range.
    pub(crate) fn set_tempo_target(&mut self, target_bpm: f32, cx: &mut Context<'_, Self>) {
        if let Some(original) = self.tempo_original_bpm() {
            self.set_tempo_rate(rate_for_target(original, target_bpm), cx);
        }
    }

    /// Back to the track's own tempo.
    pub(crate) fn reset_tempo(&mut self, cx: &mut Context<'_, Self>) {
        self.tempo.reset();
        self.apply_tempo_to_playback();
        cx.notify();
    }

    /// Switches key lock, keeping the pitch or letting it follow the speed.
    pub(crate) fn toggle_key_lock(&mut self, cx: &mut Context<'_, Self>) {
        self.tempo.key_lock = !self.tempo.key_lock;
        self.apply_tempo_to_playback();
        cx.notify();
    }

    /// Whether the tempo can go up, or down when `is_down`, from where it is.
    pub(crate) fn can_step_tempo(&self, is_down: bool) -> bool {
        let rate = self.tempo_rate();
        let (slowest, fastest) = rate_limits();
        self.tempo_original_bpm().is_some()
            && if is_down {
                rate > slowest + 1e-9
            } else {
                rate < fastest - 1e-9
            }
    }

    /// Moves the tempo by a beat, or a tenth when `is_fine`. While the field
    /// is open it starts from what is typed there and shows the result.
    pub(crate) fn step_tempo(
        &mut self,
        is_down: bool,
        is_fine: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(original) = self.tempo_original_bpm() else {
            return;
        };
        let current = if self.tempo.is_editing {
            let typed = self.tempo_input.read(cx).value().to_string();
            parse_bpm(&typed).unwrap_or_else(|| target_for_rate(original, self.tempo_rate()))
        } else {
            target_for_rate(original, self.tempo_rate())
        };
        let step = if is_fine { TEMPO_FINE_STEP } else { TEMPO_STEP };
        let signed = if is_down { -step } else { step };
        let stepped = ((current + signed) * 10.0).round() / 10.0;
        self.set_tempo_target(stepped, cx);
        if self.tempo.is_editing {
            let shown = format::bpm(self.tempo_target_bpm().unwrap_or(stepped));
            self.tempo_input
                .update(cx, |input, cx| input.set_value(shown, window, cx));
        }
    }

    /// Opens the tempo field on the tempo in use.
    pub(crate) fn edit_tempo(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(shown) = self.tempo_target_bpm().map(format::bpm) else {
            return;
        };
        self.tempo.is_editing = true;
        self.tempo_input.update(cx, |input, cx| {
            input.set_value(shown, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Takes the tempo typed in the field, or says why it cannot be used.
    pub(crate) fn commit_tempo(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.tempo.is_editing {
            return;
        }
        self.tempo.is_editing = false;
        let typed = self.tempo_input.read(cx).value().to_string();
        match self
            .tempo_original_bpm()
            .and_then(|original| rate_for_typed(original, &typed))
        {
            Some(rate) => self.set_tempo_rate(rate, cx),
            None => {
                self.notice =
                    vec!["Type a tempo from 30 to 300, such as 124 or 124,5.".to_string()];
            }
        }
        self.window_focus.focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_tempo_reads_comma_decimals_and_the_range_is_sixteen_percent() {
        assert_eq!(rate_for_typed(120.0, "124"), Some(124.0 / 120.0));
        assert_eq!(rate_for_typed(120.0, "124,5"), Some(124.5 / 120.0));
        assert_eq!(rate_for_typed(120.0, "124.5"), Some(124.5 / 120.0));
        assert_eq!(rate_for_typed(120.0, "abc"), None);
        assert_eq!(rate_for_typed(120.0, "20"), None);
        assert_eq!(rate_for_typed(120.0, "400"), None);
    }

    #[test]
    fn the_target_is_clamped_to_sixteen_percent_either_way() {
        assert!((rate_for_target(100.0, 150.0) - 1.16).abs() < 1e-9);
        assert!((rate_for_target(100.0, 50.0) - 0.84).abs() < 1e-9);
        assert!((rate_for_target(100.0, 116.0) - 1.16).abs() < 1e-9);
        assert!((rate_for_target(100.0, 84.0) - 0.84).abs() < 1e-9);
        assert_eq!(target_for_rate(100.0, rate_for_target(100.0, 150.0)), 116.0);
        assert_eq!(target_for_rate(120.0, rate_for_target(120.0, 90.0)), 100.8);
        assert_eq!(rate_for_target(0.0, 120.0), 1.0);
        assert_eq!(rate_for_target(f32::NAN, 120.0), 1.0);
    }

    #[test]
    fn the_readout_and_the_bar_text_show_the_change() {
        assert_eq!(percent_text(124.0 / 120.0), "+3.3%");
        assert_eq!(percent_text(0.975), "-2.5%");
        assert_eq!(percent_text(1.0), "0.0%");
        assert_eq!(
            bar_text(120.0, 124.0 / 120.0).unwrap(),
            "120.0 \u{2192} 124.0 BPM"
        );
        assert_eq!(bar_text(120.0, 1.0), None);
    }

    #[test]
    fn the_tempo_belongs_to_one_track_and_forgets_it_for_another() {
        let mut state = TempoState::default();
        let first = Path::new("/music/first.mp3");
        let second = Path::new("/music/second.mp3");
        state.track = Some(first.to_path_buf());
        state.rate = 1.1;
        assert_eq!(state.rate_for(first), 1.1);
        assert_eq!(state.rate_for(second), 1.0);
        state.forget_unless_for(Some(first));
        assert_eq!(state.rate, 1.1);
        state.forget_unless_for(Some(second));
        assert_eq!(state.rate, 1.0);
        assert!(state.key_lock);
    }
}
