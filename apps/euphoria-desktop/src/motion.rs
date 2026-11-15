// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The motion the app shares: how long each kind of change takes, the one
//! ease-out curve, the system's Reduce motion setting, and [`Transition`], a
//! value that travels between rest and full from a monotonic clock. A
//! transition can turn around mid-way, asks for frames only while it moves,
//! and jumps when motion is reduced.

use std::time::{Duration, Instant};

/// A hover, a small state change.
pub(crate) const FAST: Duration = Duration::from_millis(120);
/// A panel, dialog or card appearing.
pub(crate) const PANEL: Duration = Duration::from_millis(180);
/// A change of layout: a strip growing, the sidebar sliding.
pub(crate) const LAYOUT: Duration = Duration::from_millis(220);
/// A change of theme.
pub(crate) const THEME: Duration = Duration::from_millis(160);

/// Eases out: fast at first, settling at the end.
pub(crate) fn ease_out(progress: f32) -> f32 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3)
}

/// True when the system is set to reduce motion.
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn system_reduces_motion() -> bool {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// True when the system is set to reduce motion. Only macOS is asked.
#[cfg(all(not(target_os = "macos"), not(test)))]
pub(crate) fn system_reduces_motion() -> bool {
    false
}

/// Tests open surfaces without waiting on animations.
#[cfg(test)]
pub(crate) fn system_reduces_motion() -> bool {
    true
}

/// Where a transition reads the time and the Reduce motion setting from.
pub(crate) struct Sources {
    now: Box<dyn Fn() -> Instant>,
    reduces_motion: Box<dyn Fn() -> bool>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            now: Box::new(Instant::now),
            reduces_motion: Box::new(system_reduces_motion),
        }
    }
}

impl Sources {
    #[cfg(test)]
    pub(crate) fn fake(
        now: impl Fn() -> Instant + 'static,
        reduces_motion: impl Fn() -> bool + 'static,
    ) -> Self {
        Self {
            now: Box::new(now),
            reduces_motion: Box::new(reduces_motion),
        }
    }

    pub(crate) fn now(&self) -> Instant {
        (self.now)()
    }

    pub(crate) fn reduces_motion(&self) -> bool {
        (self.reduces_motion)()
    }
}

#[derive(Clone, Copy)]
struct Travel {
    from: f32,
    started: Instant,
    seconds: f32,
}

/// A value between 0.0 (rest) and 1.0 (full) that eases towards a target. A
/// full run takes the duration given to [`Transition::go_to`], and a partial
/// run, such as a turn around, takes the same share of it as the distance.
pub(crate) struct Transition {
    target: f32,
    travel: Option<Travel>,
    sources: Sources,
}

impl Transition {
    pub(crate) fn at_rest(is_full: bool) -> Self {
        Self {
            target: if is_full { 1.0 } else { 0.0 },
            travel: None,
            sources: Sources::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sources(mut self, sources: Sources) -> Self {
        self.sources = sources;
        self
    }

    /// The moment to measure this frame at.
    pub(crate) fn now(&self) -> Instant {
        self.sources.now()
    }

    /// The value at `now`.
    pub(crate) fn value(&self, now: Instant) -> f32 {
        match self.travel {
            Some(travel) => {
                let elapsed = now.saturating_duration_since(travel.started).as_secs_f32();
                let progress = if travel.seconds <= 0.0 {
                    1.0
                } else {
                    elapsed / travel.seconds
                };
                travel.from + (self.target - travel.from) * ease_out(progress)
            }
            None => self.target,
        }
    }

    /// True while the value still changes after `now`, so another frame is due.
    pub(crate) fn is_moving(&self, now: Instant) -> bool {
        self.travel.is_some_and(|travel| {
            now.saturating_duration_since(travel.started).as_secs_f32() < travel.seconds
        })
    }

    /// Settles a finished run so nothing asks for frames any more.
    pub(crate) fn settle(&mut self, now: Instant) {
        if self.travel.is_some() && !self.is_moving(now) {
            self.travel = None;
        }
    }

    /// Heads for full or rest from wherever the value is now, taking
    /// `full_run` for the whole distance. With motion reduced it jumps.
    pub(crate) fn go_to(&mut self, is_full: bool, full_run: Duration) {
        let now = self.now();
        let current = self.value(now);
        self.target = if is_full { 1.0 } else { 0.0 };
        if self.sources.reduces_motion() || (current - self.target).abs() < f32::EPSILON {
            self.travel = None;
            return;
        }
        self.travel = Some(Travel {
            from: current,
            started: now,
            seconds: full_run.as_secs_f32() * (self.target - current).abs(),
        });
    }

    /// Takes full or rest without moving.
    pub(crate) fn snap_to(&mut self, is_full: bool) {
        self.target = if is_full { 1.0 } else { 0.0 };
        self.travel = None;
    }

    /// Whether the value is heading for full, or is there.
    pub(crate) fn is_target_full(&self) -> bool {
        self.target > 0.5
    }

    /// Asks the window for another frame when the value moves after now, and
    /// settles it when it does not. Returns whether it moves.
    pub(crate) fn drive(&mut self, window: &mut gpui::Window) -> bool {
        let now = self.now();
        self.settle(now);
        let is_moving = self.is_moving(now);
        if is_moving {
            window.request_animation_frame();
        }
        is_moving
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    /// A clock and a Reduce motion switch the test moves by hand.
    pub(crate) struct FakeClock {
        origin: Instant,
        offset: Rc<Cell<f32>>,
        pub(crate) reduce: Rc<Cell<bool>>,
    }

    impl FakeClock {
        pub(crate) fn new() -> Self {
            Self {
                origin: Instant::now(),
                offset: Rc::new(Cell::new(0.0)),
                reduce: Rc::new(Cell::new(false)),
            }
        }

        pub(crate) fn sources(&self) -> Sources {
            let (origin, offset, reduce) = (self.origin, self.offset.clone(), self.reduce.clone());
            Sources::fake(
                move || origin + Duration::from_secs_f32(offset.get()),
                move || reduce.get(),
            )
        }

        pub(crate) fn at(&self, seconds: f32) -> Instant {
            self.offset.set(seconds);
            self.origin + Duration::from_secs_f32(seconds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::FakeClock;
    use super::*;

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 0.001
    }

    fn transition(clock: &FakeClock, is_full: bool) -> Transition {
        Transition::at_rest(is_full).with_sources(clock.sources())
    }

    #[test]
    fn the_durations_are_the_agreed_ones() {
        assert_eq!(FAST, Duration::from_millis(120));
        assert_eq!(PANEL, Duration::from_millis(180));
        assert_eq!(LAYOUT, Duration::from_millis(220));
        assert_eq!(THEME, Duration::from_millis(160));
    }

    #[test]
    fn progress_runs_from_rest_to_full_and_eases_out() {
        let clock = FakeClock::new();
        let mut shown = transition(&clock, false);
        shown.go_to(true, PANEL);
        assert!(close(shown.value(clock.at(0.0)), 0.0));
        assert!(close(shown.value(clock.at(0.09)), 0.875));
        assert!(close(shown.value(clock.at(0.18)), 1.0));
        assert!(close(shown.value(clock.at(5.0)), 1.0));
    }

    #[test]
    fn reversing_turns_round_from_the_value_reached_over_the_distance_left() {
        let clock = FakeClock::new();
        let mut shown = transition(&clock, false);
        shown.go_to(true, PANEL);
        let reached = shown.value(clock.at(0.045));
        shown.go_to(false, PANEL);
        assert!(close(shown.value(clock.at(0.045)), reached));
        assert!(shown.value(clock.at(0.06)) < reached);
        let seconds = PANEL.as_secs_f32() * reached;
        assert!(shown.is_moving(clock.at(0.045 + seconds * 0.9)));
        assert!(!shown.is_moving(clock.at(0.045 + seconds + 0.001)));
        assert!(close(shown.value(clock.at(1.0)), 0.0));
    }

    #[test]
    fn reduced_motion_jumps_both_ways() {
        let clock = FakeClock::new();
        clock.reduce.set(true);
        let mut shown = transition(&clock, false);
        shown.go_to(true, PANEL);
        assert_eq!(shown.value(clock.at(0.0)), 1.0);
        assert!(!shown.is_moving(clock.at(0.0)));
        shown.go_to(false, PANEL);
        assert_eq!(shown.value(clock.at(0.0)), 0.0);
        assert!(!shown.is_moving(clock.at(0.0)));
    }

    #[test]
    fn nothing_asks_for_frames_at_rest_or_after_settling() {
        let clock = FakeClock::new();
        let mut shown = transition(&clock, false);
        assert!(!shown.is_moving(clock.at(0.0)));
        shown.go_to(false, PANEL);
        assert!(!shown.is_moving(clock.at(0.0)));
        shown.go_to(true, PANEL);
        assert!(shown.is_moving(clock.at(0.1)));
        assert!(!shown.is_moving(clock.at(0.19)));
        shown.settle(clock.at(0.19));
        assert!(!shown.is_moving(clock.at(0.19)));
        assert_eq!(shown.value(clock.at(9.0)), 1.0);
    }

    #[test]
    fn a_shorter_run_takes_the_duration_given() {
        let clock = FakeClock::new();
        let mut shown = transition(&clock, true);
        shown.go_to(false, FAST);
        assert!(shown.is_moving(clock.at(0.1)));
        assert!(!shown.is_moving(clock.at(0.121)));
    }
}
