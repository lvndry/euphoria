// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The motion of the surfaces that come and go: dialogs and sheets that fade
//! and rise, strips that grow, the focus view that crossfades with the
//! library, and the palette that blends when the theme changes. Each is a
//! [`Transition`] from [`crate::motion`], so each is instant under Reduce
//! motion and asks for frames only while it moves.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::Window;

use crate::motion::{self, Transition};
use crate::theme::Palette;

/// How far a dialog's card rises while it fades in, in pixels.
pub(crate) const CARD_RISE: f32 = 8.0;

/// How a surface that is shown or hidden by state moves between the two.
pub(crate) struct Presence {
    transition: Transition,
    is_closing: bool,
}

/// What asking a surface to close leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closing {
    /// Nothing to play: close now.
    Now,
    /// The fade out has begun: close when it ends.
    Started,
    /// A fade out is already under way.
    AlreadyClosing,
}

impl Presence {
    pub(crate) fn at_rest(is_shown: bool) -> Self {
        Self {
            transition: Transition::at_rest(is_shown),
            is_closing: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sources(mut self, sources: motion::Sources) -> Self {
        self.transition = self.transition.with_sources(sources);
        self
    }

    pub(crate) fn now(&self) -> Instant {
        self.transition.now()
    }

    /// How far in the surface is at `now`, from 0 (gone) to 1 (in place).
    pub(crate) fn value(&self, now: Instant) -> f32 {
        self.transition.value(now)
    }

    pub(crate) fn is_moving(&self, now: Instant) -> bool {
        self.transition.is_moving(now)
    }

    /// Starts the fade in when the surface has just appeared, and drops the
    /// fade when it went without one.
    pub(crate) fn sync_appearing(&mut self, is_present: bool, enter: Duration) {
        if is_present {
            if !self.is_closing && !self.transition.is_target_full() {
                self.transition.go_to(true, enter);
            }
        } else {
            self.is_closing = false;
            if self.transition.is_target_full() {
                self.transition.snap_to(false);
            }
        }
    }

    /// Follows `is_shown` both ways, fading in and out over `duration`.
    pub(crate) fn follow(&mut self, is_shown: bool, duration: Duration) {
        if is_shown != self.transition.is_target_full() {
            self.transition.go_to(is_shown, duration);
        }
    }

    /// Starts the fade out of a surface asked to close.
    pub(crate) fn start_closing(&mut self, exit: Duration) -> Closing {
        if self.is_closing {
            return Closing::AlreadyClosing;
        }
        let now = self.now();
        if self.transition.value(now) <= 0.0 || !self.transition.is_target_full() {
            return Closing::Now;
        }
        self.transition.go_to(false, exit);
        if self.transition.is_moving(now) {
            self.is_closing = true;
            Closing::Started
        } else {
            Closing::Now
        }
    }

    pub(crate) fn finish_closing(&mut self) {
        self.is_closing = false;
        self.transition.snap_to(false);
    }

    /// Settles and asks for the next frame while moving.
    pub(crate) fn drive(&mut self, window: &mut Window) -> bool {
        self.transition.drive(window)
    }

    pub(crate) fn is_shown_or_moving(&self) -> bool {
        let now = self.now();
        self.transition.is_target_full() || self.value(now) > 0.0
    }
}

/// The surfaces that cover the window and want an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modal {
    Settings,
    Export,
    TagPanel,
}

impl Modal {
    const ALL: [Modal; 3] = [Modal::Settings, Modal::Export, Modal::TagPanel];

    fn index(self) -> usize {
        match self {
            Modal::Settings => 0,
            Modal::Export => 1,
            Modal::TagPanel => 2,
        }
    }
}

/// The fade of every dialog.
pub(crate) struct ModalFades {
    presences: [Presence; 3],
}

impl Default for ModalFades {
    fn default() -> Self {
        Self {
            presences: Modal::ALL.map(|_| Presence::at_rest(false)),
        }
    }
}

impl ModalFades {
    #[cfg(test)]
    pub(crate) fn with_clock(clock: &motion::testing::FakeClock) -> Self {
        Self {
            presences: Modal::ALL.map(|_| Presence::at_rest(false).with_sources(clock.sources())),
        }
    }

    pub(crate) fn sync(&mut self, modal: Modal, is_present: bool) {
        self.presences[modal.index()].sync_appearing(is_present, motion::PANEL);
    }

    /// How far in the dialog is now.
    pub(crate) fn value(&self, modal: Modal) -> f32 {
        let presence = &self.presences[modal.index()];
        presence.value(presence.now())
    }

    pub(crate) fn start_closing(&mut self, modal: Modal) -> Closing {
        self.presences[modal.index()].start_closing(motion::FAST)
    }

    pub(crate) fn finish_closing(&mut self, modal: Modal) {
        self.presences[modal.index()].finish_closing();
    }

    pub(crate) fn drive(&mut self, window: &mut Window) {
        for presence in &mut self.presences {
            presence.drive(window);
        }
    }

    pub(crate) fn is_moving(&self) -> bool {
        self.presences
            .iter()
            .any(|presence| presence.is_moving(presence.now()))
    }
}

/// The strips along the bottom of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strip {
    Notice,
    HeldTags,
    LinkConfirmation,
    StemExport,
}

impl Strip {
    const ALL: [Strip; 4] = [
        Strip::Notice,
        Strip::HeldTags,
        Strip::LinkConfirmation,
        Strip::StemExport,
    ];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|strip| *strip == self)
            .unwrap_or(0)
    }
}

/// The height a strip had when last drawn, so that it can grow to it.
pub(crate) type SeenHeight = Rc<Cell<Option<f32>>>;

/// How tall to take a strip that was never drawn, until it is measured.
pub(crate) const UNMEASURED_STRIP_HEIGHT: f32 = 34.0;

/// The growth of every strip.
pub(crate) struct StripGrowth {
    presences: [Presence; 4],
    heights: [SeenHeight; 4],
}

impl Default for StripGrowth {
    fn default() -> Self {
        Self {
            presences: Strip::ALL.map(|_| Presence::at_rest(false)),
            heights: Strip::ALL.map(|_| Rc::new(Cell::new(None))),
        }
    }
}

impl StripGrowth {
    #[cfg(test)]
    pub(crate) fn with_clock(clock: &motion::testing::FakeClock) -> Self {
        Self {
            presences: Strip::ALL.map(|_| Presence::at_rest(false).with_sources(clock.sources())),
            ..Self::default()
        }
    }

    pub(crate) fn sync(&mut self, strip: Strip, is_present: bool) {
        self.presences[strip.index()].sync_appearing(is_present, motion::LAYOUT);
    }

    /// How much of its height the strip shows now, from 0 to 1.
    pub(crate) fn value(&self, strip: Strip) -> f32 {
        let presence = &self.presences[strip.index()];
        presence.value(presence.now())
    }

    pub(crate) fn seen_height(&self, strip: Strip) -> SeenHeight {
        self.heights[strip.index()].clone()
    }

    pub(crate) fn drive(&mut self, window: &mut Window) {
        for presence in &mut self.presences {
            presence.drive(window);
        }
    }
}

/// The blend from the palette that was on show to the one chosen.
pub(crate) struct ThemeFade {
    from: Palette,
    to: Palette,
    transition: Transition,
}

impl ThemeFade {
    /// A fade from `from` to `to`, or `None` when motion is reduced.
    pub(crate) fn between(from: Palette, to: Palette) -> Option<Self> {
        Self::between_with(from, to, Transition::at_rest(false))
    }

    pub(crate) fn between_with(
        from: Palette,
        to: Palette,
        mut transition: Transition,
    ) -> Option<Self> {
        transition.go_to(true, motion::THEME);
        transition.is_moving(transition.now()).then_some(Self {
            from,
            to,
            transition,
        })
    }

    /// The palette to draw with at `now`.
    pub(crate) fn palette(&self, now: Instant) -> Palette {
        Palette::mix(self.from, self.to, self.transition.value(now))
    }

    pub(crate) fn target(&self) -> Palette {
        self.to
    }

    pub(crate) fn now(&self) -> Instant {
        self.transition.now()
    }

    pub(crate) fn is_moving(&self, now: Instant) -> bool {
        self.transition.is_moving(now)
    }
}

#[cfg(test)]
mod tests {
    use crate::motion::testing::FakeClock;
    use crate::theme::{AppTheme, ThemeName};

    use super::*;

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 0.001
    }

    #[test]
    fn a_dialog_fades_in_over_180_ms_and_out_over_120() {
        let clock = FakeClock::new();
        let mut fades = ModalFades::with_clock(&clock);
        clock.at(0.0);
        fades.sync(Modal::Settings, true);
        assert!(close(fades.value(Modal::Settings), 0.0));
        clock.at(0.18);
        assert!(close(fades.value(Modal::Settings), 1.0));
        assert!(!fades.is_moving());

        assert_eq!(fades.start_closing(Modal::Settings), Closing::Started);
        assert_eq!(
            fades.start_closing(Modal::Settings),
            Closing::AlreadyClosing
        );
        clock.at(0.18 + 0.06);
        let middle = fades.value(Modal::Settings);
        assert!(middle > 0.0 && middle < 1.0, "{middle}");
        fades.sync(Modal::Settings, true);
        assert!(fades.value(Modal::Settings) < 1.0, "closing is not undone");
        clock.at(0.18 + 0.12);
        assert!(close(fades.value(Modal::Settings), 0.0));
        fades.finish_closing(Modal::Settings);
        fades.sync(Modal::Settings, false);
        assert!(!fades.is_moving());
    }

    #[test]
    fn under_reduce_motion_a_dialog_is_in_at_once_and_closes_at_once() {
        let clock = FakeClock::new();
        clock.reduce.set(true);
        let mut fades = ModalFades::with_clock(&clock);
        fades.sync(Modal::TagPanel, true);
        assert_eq!(fades.value(Modal::TagPanel), 1.0);
        assert!(!fades.is_moving());
        assert_eq!(fades.start_closing(Modal::TagPanel), Closing::Now);
    }

    #[test]
    fn a_dialog_that_went_without_a_fade_starts_again_from_nothing() {
        let clock = FakeClock::new();
        let mut fades = ModalFades::with_clock(&clock);
        fades.sync(Modal::Export, true);
        clock.at(1.0);
        fades.sync(Modal::Export, false);
        assert_eq!(fades.value(Modal::Export), 0.0);
        fades.sync(Modal::Export, true);
        assert!(close(fades.value(Modal::Export), 0.0));
    }

    #[test]
    fn a_strip_grows_over_220_ms() {
        let clock = FakeClock::new();
        let mut growth = StripGrowth::with_clock(&clock);
        growth.sync(Strip::Notice, true);
        assert!(close(growth.value(Strip::Notice), 0.0));
        clock.at(0.11);
        let middle = growth.value(Strip::Notice);
        assert!(middle > 0.8 && middle < 1.0, "{middle}");
        clock.at(0.22);
        assert!(close(growth.value(Strip::Notice), 1.0));
        growth.sync(Strip::Notice, false);
        assert_eq!(growth.value(Strip::Notice), 0.0);
        assert_eq!(growth.value(Strip::HeldTags), 0.0);
    }

    #[test]
    fn the_focus_view_crossfades_both_ways_over_180_ms() {
        let clock = FakeClock::new();
        let mut focus = Presence::at_rest(false).with_sources(clock.sources());
        focus.follow(true, motion::PANEL);
        assert!(focus.is_moving(clock.at(0.1)));
        assert!(close(focus.value(clock.at(0.18)), 1.0));
        focus.follow(false, motion::PANEL);
        assert!(focus.is_shown_or_moving());
        assert!(close(focus.value(clock.at(0.18 + 0.18)), 0.0));
        assert!(!focus.is_shown_or_moving() || focus.value(clock.at(1.0)) == 0.0);
    }

    #[test]
    fn the_theme_blend_starts_on_the_old_palette_and_ends_on_the_new() {
        let clock = FakeClock::new();
        let from = AppTheme::named(ThemeName::Slate).palette;
        let to = AppTheme::named(ThemeName::Paper).palette;
        let fade = ThemeFade::between_with(
            from,
            to,
            Transition::at_rest(false).with_sources(clock.sources()),
        )
        .unwrap();
        assert_eq!(fade.palette(clock.at(0.0)), from);
        let middle = fade.palette(clock.at(0.08));
        assert_ne!(middle.background, from.background);
        assert_ne!(middle.background, to.background);
        assert!(fade.is_moving(clock.at(0.15)));
        assert_eq!(fade.palette(clock.at(0.16)), to);
        assert!(!fade.is_moving(clock.at(0.161)));
    }

    #[test]
    fn the_theme_does_not_blend_under_reduce_motion() {
        let clock = FakeClock::new();
        clock.reduce.set(true);
        let from = AppTheme::named(ThemeName::Slate).palette;
        let to = AppTheme::named(ThemeName::Paper).palette;
        let fade = ThemeFade::between_with(
            from,
            to,
            Transition::at_rest(false).with_sources(clock.sources()),
        );
        assert!(fade.is_none());
    }
}
