// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sidebar sliding out of view and back. The width on show comes from a
//! monotonic clock, so the slide takes the same time at any frame rate. A
//! change of mind mid-slide starts from the width reached, not from an end.

use std::time::Instant;

use crate::panels::SIDEBAR_WIDTH;

/// Seconds a slide across the full sidebar width takes.
pub(crate) const SLIDE_SECONDS: f32 = 0.22;

pub(crate) fn ease_out(progress: f32) -> f32 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3)
}

#[derive(Clone, Copy)]
struct Travel {
    from: f32,
    started: Instant,
    seconds: f32,
}

/// Where the sidebar is on its way in or out.
pub(crate) struct SidebarSlide {
    target: f32,
    travel: Option<Travel>,
    now: Box<dyn Fn() -> Instant>,
    reduces_motion: Box<dyn Fn() -> bool>,
}

impl SidebarSlide {
    /// A sidebar at rest, folded or shown, with no slide to play.
    pub(crate) fn at_rest(is_folded: bool) -> Self {
        Self {
            target: rest_width(is_folded),
            travel: None,
            now: Box::new(Instant::now),
            reduces_motion: Box::new(system_reduces_motion),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sources(
        mut self,
        now: impl Fn() -> Instant + 'static,
        reduces_motion: impl Fn() -> bool + 'static,
    ) -> Self {
        self.now = Box::new(now);
        self.reduces_motion = Box::new(reduces_motion);
        self
    }

    /// The moment to measure this frame at.
    pub(crate) fn now(&self) -> Instant {
        (self.now)()
    }

    /// The width on show at `now`.
    pub(crate) fn width(&self, now: Instant) -> f32 {
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

    /// True while the width still changes after `now`, so another frame is due.
    pub(crate) fn is_moving(&self, now: Instant) -> bool {
        self.travel.is_some_and(|travel| {
            now.saturating_duration_since(travel.started).as_secs_f32() < travel.seconds
        })
    }

    /// Settles a finished slide so nothing asks for frames any more.
    pub(crate) fn settle(&mut self, now: Instant) {
        if self.travel.is_some() && !self.is_moving(now) {
            self.travel = None;
        }
    }

    /// Heads for the folded or shown width from wherever the sidebar is now.
    /// With motion reduced the sidebar jumps.
    pub(crate) fn go_to(&mut self, is_folded: bool) {
        let now = self.now();
        let current = self.width(now);
        self.target = rest_width(is_folded);
        if (self.reduces_motion)() || (current - self.target).abs() < f32::EPSILON {
            self.travel = None;
            return;
        }
        self.travel = Some(Travel {
            from: current,
            started: now,
            seconds: SLIDE_SECONDS * (self.target - current).abs() / SIDEBAR_WIDTH,
        });
    }
}

#[cfg(not(test))]
pub(crate) use crate::logo_motion::system_reduces_motion;

#[cfg(test)]
pub(crate) fn system_reduces_motion() -> bool {
    true
}

fn rest_width(is_folded: bool) -> f32 {
    if is_folded { 0.0 } else { SIDEBAR_WIDTH }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;

    struct Fake {
        origin: Instant,
        offset: Rc<Cell<f32>>,
        reduce: Rc<Cell<bool>>,
    }

    impl Fake {
        fn slide(is_folded: bool) -> (SidebarSlide, Self) {
            let origin = Instant::now();
            let offset = Rc::new(Cell::new(0.0));
            let reduce = Rc::new(Cell::new(false));
            let (clock_offset, clock_reduce) = (offset.clone(), reduce.clone());
            let slide = SidebarSlide::at_rest(is_folded).with_sources(
                move || origin + Duration::from_secs_f32(clock_offset.get()),
                move || clock_reduce.get(),
            );
            (
                slide,
                Self {
                    origin,
                    offset,
                    reduce,
                },
            )
        }

        fn at(&self, seconds: f32) -> Instant {
            self.offset.set(seconds);
            self.origin + Duration::from_secs_f32(seconds)
        }
    }

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 0.01
    }

    #[test]
    fn a_slide_out_starts_at_full_width_and_ends_at_nothing() {
        let (mut slide, fake) = Fake::slide(false);
        slide.go_to(true);
        assert!(close(slide.width(fake.at(0.0)), SIDEBAR_WIDTH));
        assert!(slide.is_moving(fake.at(0.01)));
        assert!(close(slide.width(fake.at(SLIDE_SECONDS)), 0.0));
        assert!(!slide.is_moving(fake.at(SLIDE_SECONDS + 0.001)));
        assert!(close(slide.width(fake.at(5.0)), 0.0));
    }

    #[test]
    fn the_middle_of_the_slide_is_past_half_way_because_it_eases_out() {
        let (mut slide, fake) = Fake::slide(false);
        slide.go_to(true);
        let width = slide.width(fake.at(SLIDE_SECONDS / 2.0));
        assert!(close(width, SIDEBAR_WIDTH * (1.0 - 0.875)), "{width}");
    }

    #[test]
    fn a_slide_in_is_the_way_back() {
        let (mut slide, fake) = Fake::slide(true);
        slide.go_to(false);
        assert!(close(slide.width(fake.at(0.0)), 0.0));
        assert!(close(
            slide.width(fake.at(SLIDE_SECONDS / 2.0)),
            SIDEBAR_WIDTH * 0.875
        ));
        assert!(close(slide.width(fake.at(SLIDE_SECONDS)), SIDEBAR_WIDTH));
    }

    #[test]
    fn a_click_mid_slide_turns_around_from_the_width_reached() {
        let (mut slide, fake) = Fake::slide(false);
        slide.go_to(true);
        let reached = slide.width(fake.at(0.08));
        slide.go_to(false);
        assert!(close(slide.width(fake.at(0.08)), reached));
        let later = slide.width(fake.at(0.09));
        assert!(later > reached, "{later} > {reached}");
        assert!(close(slide.width(fake.at(1.0)), SIDEBAR_WIDTH));
    }

    #[test]
    fn a_turn_around_covers_only_the_distance_left() {
        let (mut slide, fake) = Fake::slide(false);
        slide.go_to(true);
        let reached = slide.width(fake.at(0.1));
        slide.go_to(false);
        let seconds = SLIDE_SECONDS * (SIDEBAR_WIDTH - reached) / SIDEBAR_WIDTH;
        assert!(slide.is_moving(fake.at(0.1 + seconds * 0.9)));
        assert!(!slide.is_moving(fake.at(0.1 + seconds + 0.001)));
    }

    #[test]
    fn reduced_motion_jumps_at_once() {
        let (mut slide, fake) = Fake::slide(false);
        fake.reduce.set(true);
        slide.go_to(true);
        assert_eq!(slide.width(fake.at(0.0)), 0.0);
        assert!(!slide.is_moving(fake.at(0.0)));
        slide.go_to(false);
        assert_eq!(slide.width(fake.at(0.0)), SIDEBAR_WIDTH);
        assert!(!slide.is_moving(fake.at(0.0)));
    }

    #[test]
    fn a_sidebar_opened_folded_is_at_rest_and_asks_for_no_frames() {
        let (slide, fake) = Fake::slide(true);
        assert_eq!(slide.width(fake.at(0.0)), 0.0);
        assert!(!slide.is_moving(fake.at(0.0)));
        let (shown, fake) = Fake::slide(false);
        assert_eq!(shown.width(fake.at(0.0)), SIDEBAR_WIDTH);
        assert!(!shown.is_moving(fake.at(0.0)));
    }

    #[test]
    fn going_where_it_already_is_does_not_move() {
        let (mut slide, fake) = Fake::slide(false);
        slide.go_to(false);
        assert!(!slide.is_moving(fake.at(0.0)));
    }

    #[test]
    fn settling_ends_the_frame_requests() {
        let (mut slide, fake) = Fake::slide(false);
        slide.go_to(true);
        slide.settle(fake.at(SLIDE_SECONDS + 0.01));
        assert!(slide.travel.is_none());
        assert_eq!(slide.width(fake.at(9.0)), 0.0);
    }
}
