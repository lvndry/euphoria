// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The timing of the logo while work runs: the mark is drawn tip to tip,
//! held, then erased the way it was drawn. The span of the curve on show
//! comes from a monotonic clock, so the loop runs at the same speed at any
//! frame rate.

use std::cell::RefCell;
use std::f32::consts::PI;
use std::rc::Rc;
use std::time::Instant;

/// Where the curve starts, in turns of its parameter.
pub(crate) const CURVE_START: f32 = 0.25;

/// Where the curve ends, in turns of its parameter.
pub(crate) const CURVE_END: f32 = 0.75;

/// Seconds the stroke takes to grow from tip to tip.
pub(crate) const DRAW_SECONDS: f32 = 1.6;

/// Seconds the full mark stays on show.
pub(crate) const HOLD_SECONDS: f32 = 0.5;

/// Seconds the stroke takes to pull back.
pub(crate) const ERASE_SECONDS: f32 = 1.2;

/// Seconds one loop lasts.
pub(crate) const LOOP_SECONDS: f32 = DRAW_SECONDS + HOLD_SECONDS + ERASE_SECONDS;

/// Seconds one breath of the pulse lasts, for people who reduce motion.
const PULSE_SECONDS: f32 = 2.4;

/// How much of the mark's strength the pulse gives up at its faintest.
const PULSE_DEPTH: f32 = 0.55;

/// What the mark looks like at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LogoFrame {
    /// The full mark, with nothing moving.
    Still,
    /// The stretch of the curve on show, from `from` to `to` turns.
    Trace { from: f32, to: f32 },
    /// The full mark faded toward the background by `fade`, from 0.0 to 1.0.
    Pulse { fade: f32 },
}

impl LogoFrame {
    /// True when the next frame looks different, so one has to be asked for.
    pub(crate) fn is_moving(self) -> bool {
        !matches!(self, LogoFrame::Still)
    }
}

fn ease_in_out(progress: f32) -> f32 {
    0.5 - 0.5 * (PI * progress.clamp(0.0, 1.0)).cos()
}

/// The stretch of the curve on show `seconds` into the loop, as turns of
/// the parameter, always within `CURVE_START` and `CURVE_END`.
pub(crate) fn visible_span(seconds: f32) -> (f32, f32) {
    let seconds = seconds.rem_euclid(LOOP_SECONDS);
    let length = CURVE_END - CURVE_START;
    if seconds < DRAW_SECONDS {
        let grown = ease_in_out(seconds / DRAW_SECONDS);
        (CURVE_START, CURVE_START + length * grown)
    } else if seconds < DRAW_SECONDS + HOLD_SECONDS {
        (CURVE_START, CURVE_END)
    } else {
        let erased = ease_in_out((seconds - DRAW_SECONDS - HOLD_SECONDS) / ERASE_SECONDS);
        (CURVE_START + length * erased, CURVE_END)
    }
}

/// When to stop, in seconds since the loop began, given that work ended
/// at `elapsed`. A stroke being drawn finishes drawing, the full mark
/// stays, and a stroke being erased finishes erasing and draws once more,
/// so the mark never jumps.
pub(crate) fn settle_at(elapsed: f32) -> f32 {
    let loops = (elapsed / LOOP_SECONDS).floor();
    let seconds = elapsed - loops * LOOP_SECONDS;
    if seconds < DRAW_SECONDS {
        loops * LOOP_SECONDS + DRAW_SECONDS
    } else if seconds < DRAW_SECONDS + HOLD_SECONDS {
        elapsed
    } else {
        (loops + 1.0) * LOOP_SECONDS + DRAW_SECONDS
    }
}

fn pulse_fade(seconds: f32) -> f32 {
    let breath = 0.5 - 0.5 * (2.0 * PI * seconds / PULSE_SECONDS).cos();
    PULSE_DEPTH * breath
}

#[derive(Default)]
struct ClockState {
    started: Option<Instant>,
    stop_at: Option<f32>,
}

/// Remembers when the logo started moving, so it can play from the first
/// moment of work and settle calmly when work ends.
#[derive(Clone, Default)]
pub(crate) struct LogoClock(Rc<RefCell<ClockState>>);

impl LogoClock {
    /// What to draw at `now`, given whether work is running and whether
    /// the system asks for less motion.
    pub(crate) fn frame(&self, is_busy: bool, reduces_motion: bool, now: Instant) -> LogoFrame {
        let mut state = self.0.borrow_mut();
        if is_busy {
            state.started.get_or_insert(now);
            state.stop_at = None;
        }
        let Some(started) = state.started else {
            return LogoFrame::Still;
        };
        let elapsed = now.saturating_duration_since(started).as_secs_f32();
        if !is_busy && state.stop_at.is_none() {
            state.stop_at = Some(if reduces_motion {
                elapsed
            } else {
                settle_at(elapsed)
            });
        }
        if state.stop_at.is_some_and(|stop| elapsed >= stop) {
            *state = ClockState::default();
            return LogoFrame::Still;
        }
        if reduces_motion {
            LogoFrame::Pulse {
                fade: pulse_fade(elapsed),
            }
        } else {
            let (from, to) = visible_span(elapsed);
            LogoFrame::Trace { from, to }
        }
    }
}

/// True when the system is set to reduce motion.
#[cfg(target_os = "macos")]
pub(crate) fn system_reduces_motion() -> bool {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// True when the system is set to reduce motion. Only macOS is asked.
#[cfg(not(target_os = "macos"))]
pub(crate) fn system_reduces_motion() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 1.0e-5
    }

    #[test]
    fn the_loop_starts_with_nothing_drawn_at_the_first_tip() {
        let (from, to) = visible_span(0.0);
        assert!(close(from, CURVE_START) && close(to, CURVE_START));
    }

    #[test]
    fn the_stroke_is_half_drawn_half_way_through_the_draw() {
        let (from, to) = visible_span(DRAW_SECONDS / 2.0);
        assert!(close(from, CURVE_START));
        assert!(close(to, 0.5));
    }

    #[test]
    fn the_full_mark_is_held() {
        for seconds in [DRAW_SECONDS, DRAW_SECONDS + HOLD_SECONDS / 2.0] {
            let (from, to) = visible_span(seconds);
            assert!(close(from, CURVE_START) && close(to, CURVE_END));
        }
    }

    #[test]
    fn the_start_of_the_stroke_is_half_way_half_way_through_the_erase() {
        let (from, to) = visible_span(DRAW_SECONDS + HOLD_SECONDS + ERASE_SECONDS / 2.0);
        assert!(close(from, 0.5));
        assert!(close(to, CURVE_END));
    }

    #[test]
    fn the_loop_wraps_back_to_nothing_drawn() {
        assert_eq!(visible_span(LOOP_SECONDS), visible_span(0.0));
        assert_eq!(visible_span(3.0 * LOOP_SECONDS + 0.4), visible_span(0.4));
        let (from, to) = visible_span(LOOP_SECONDS - 1.0e-4);
        assert!(close(from, CURVE_END) && close(to, CURVE_END));
    }

    #[test]
    fn the_span_stays_within_the_curve_and_never_runs_backwards() {
        let mut seconds = -1.0;
        while seconds < 10.0 {
            let (from, to) = visible_span(seconds);
            assert!(from >= CURVE_START - 1.0e-6 && to <= CURVE_END + 1.0e-6);
            assert!(from <= to + 1.0e-6);
            seconds += 0.01;
        }
    }

    #[test]
    fn work_ending_while_drawing_finishes_the_draw() {
        assert!(close(settle_at(0.8), DRAW_SECONDS));
        assert!(close(
            settle_at(LOOP_SECONDS + 0.8),
            LOOP_SECONDS + DRAW_SECONDS
        ));
    }

    #[test]
    fn work_ending_during_the_hold_stops_at_once() {
        assert!(close(settle_at(1.8), 1.8));
    }

    #[test]
    fn work_ending_while_erasing_erases_then_draws_once_more() {
        assert!(close(settle_at(2.5), LOOP_SECONDS + DRAW_SECONDS));
    }

    fn after(start: Instant, seconds: f32) -> Instant {
        start + Duration::from_secs_f32(seconds)
    }

    #[test]
    fn an_idle_clock_is_still() {
        let clock = LogoClock::default();
        assert_eq!(clock.frame(false, false, Instant::now()), LogoFrame::Still);
    }

    #[test]
    fn a_busy_clock_traces_from_the_moment_work_starts() {
        let clock = LogoClock::default();
        let start = Instant::now();
        assert!(matches!(
            clock.frame(true, false, start),
            LogoFrame::Trace { .. }
        ));
        let LogoFrame::Trace { from, to } = clock.frame(true, false, after(start, 0.8)) else {
            panic!("expected a trace");
        };
        assert!(close(from, CURVE_START) && close(to, 0.5));
    }

    #[test]
    fn a_clock_settles_on_the_full_mark_after_work_ends() {
        let clock = LogoClock::default();
        let start = Instant::now();
        clock.frame(true, false, start);
        assert!(clock.frame(false, false, after(start, 0.8)).is_moving());
        assert!(clock.frame(false, false, after(start, 1.2)).is_moving());
        assert_eq!(
            clock.frame(false, false, after(start, 1.7)),
            LogoFrame::Still
        );
        assert_eq!(
            clock.frame(false, false, after(start, 2.0)),
            LogoFrame::Still
        );
    }

    #[test]
    fn work_starting_again_while_settling_keeps_the_loop_going() {
        let clock = LogoClock::default();
        let start = Instant::now();
        clock.frame(true, false, start);
        clock.frame(false, false, after(start, 0.8));
        assert!(clock.frame(true, false, after(start, 1.7)).is_moving());
    }

    #[test]
    fn reduced_motion_pulses_the_full_mark_and_stops_at_once() {
        let clock = LogoClock::default();
        let start = Instant::now();
        let LogoFrame::Pulse { fade } = clock.frame(true, true, start) else {
            panic!("expected a pulse");
        };
        assert!(close(fade, 0.0));
        let LogoFrame::Pulse { fade } = clock.frame(true, true, after(start, PULSE_SECONDS / 2.0))
        else {
            panic!("expected a pulse");
        };
        assert!(close(fade, PULSE_DEPTH));
        assert_eq!(
            clock.frame(false, true, after(start, 0.7)),
            LogoFrame::Still
        );
    }
}
