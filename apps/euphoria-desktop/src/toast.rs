// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The toast that ends a write: one short line that says what changed, with
//! Undo or Show in Finder beside it. One toast shows at a time and a new one
//! replaces the old. It slides up and fades in, stays ten seconds, and waits
//! while the pointer is over it. Time comes from a monotonic clock, so the
//! toast behaves the same at any frame rate.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::motion::{self, Sources, ease_out};

/// How long a toast stays, not counting the time the pointer is over it.
pub(crate) const STAY: Duration = Duration::from_secs(10);
/// Seconds the toast takes to slide up and fade in.
pub(crate) const ENTER_SECONDS: f32 = motion::PANEL.as_secs_f32();
/// How far the toast slides, in pixels.
pub(crate) const SLIDE_DISTANCE: f32 = 10.0;

/// What the toast offers beside its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToastAction {
    /// Puts back the tags the last write replaced.
    UndoTagWrite,
    /// Shows the file or folder that was written in the file manager.
    Reveal(PathBuf),
}

impl ToastAction {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            ToastAction::UndoTagWrite => "Undo",
            ToastAction::Reveal(_) => crate::reveal::show_label(),
        }
    }

    pub(crate) fn is_undo(&self) -> bool {
        matches!(self, ToastAction::UndoTagWrite)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Toast {
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
    pub(crate) action: Option<ToastAction>,
    id: u64,
    shown_at: Instant,
    remaining: Duration,
    running_since: Option<Instant>,
    enter_seconds: f32,
}

#[derive(Default)]
pub(crate) struct Toasts {
    current: Option<Toast>,
    next_id: u64,
    sources: Sources,
}

impl Toasts {
    #[cfg(test)]
    pub(crate) fn with_sources(
        mut self,
        now: impl Fn() -> Instant + 'static,
        reduces_motion: impl Fn() -> bool + 'static,
    ) -> Self {
        self.sources = Sources::fake(now, reduces_motion);
        self
    }

    /// The moment to measure this frame at.
    pub(crate) fn now(&self) -> Instant {
        self.sources.now()
    }

    pub(crate) fn current(&self) -> Option<&Toast> {
        self.current.as_ref()
    }

    /// Shows a toast in place of any that is showing, and returns its id.
    pub(crate) fn show(&mut self, message: String, action: Option<ToastAction>) -> u64 {
        let now = self.now();
        self.next_id += 1;
        let enter_seconds = if self.sources.reduces_motion() {
            0.0
        } else {
            ENTER_SECONDS
        };
        self.current = Some(Toast {
            message,
            detail: None,
            action,
            id: self.next_id,
            shown_at: now,
            remaining: STAY,
            running_since: Some(now),
            enter_seconds,
        });
        self.next_id
    }

    /// Adds a short second line to the toast showing.
    pub(crate) fn set_detail(&mut self, detail: String) {
        if let Some(toast) = &mut self.current {
            toast.detail = Some(detail);
        }
    }

    pub(crate) fn dismiss(&mut self) {
        self.current = None;
    }

    /// True when the toast with `id` is the one showing.
    pub(crate) fn is_showing(&self, id: u64) -> bool {
        self.current.as_ref().is_some_and(|toast| toast.id == id)
    }

    /// True when a toast with Undo shows.
    pub(crate) fn offers_undo(&self) -> bool {
        self.current
            .as_ref()
            .and_then(|toast| toast.action.as_ref())
            .is_some_and(ToastAction::is_undo)
    }

    /// Stops the clock while the pointer is over the toast, and starts it
    /// again when the pointer leaves.
    pub(crate) fn set_hovered(&mut self, is_hovered: bool) {
        let now = self.now();
        let Some(toast) = &mut self.current else {
            return;
        };
        match (is_hovered, toast.running_since) {
            (true, Some(since)) => {
                toast.remaining = toast
                    .remaining
                    .saturating_sub(now.saturating_duration_since(since));
                toast.running_since = None;
            }
            (false, None) => toast.running_since = Some(now),
            _ => {}
        }
    }

    /// How far in the toast is, from 0 (hidden) to 1 (in place).
    pub(crate) fn progress(&self, now: Instant) -> f32 {
        let Some(toast) = &self.current else {
            return 0.0;
        };
        if toast.enter_seconds <= 0.0 {
            return 1.0;
        }
        let elapsed = now.saturating_duration_since(toast.shown_at).as_secs_f32();
        ease_out(elapsed / toast.enter_seconds)
    }

    /// True while the toast still slides in, so another frame is due.
    pub(crate) fn is_animating(&self, now: Instant) -> bool {
        self.current.as_ref().is_some_and(|toast| {
            now.saturating_duration_since(toast.shown_at).as_secs_f32() < toast.enter_seconds
        })
    }

    /// Takes the toast away once its time is up. Returns whether it did.
    pub(crate) fn expire_if_due(&mut self) -> bool {
        let now = self.now();
        let is_due = self.current.as_ref().is_some_and(|toast| {
            toast
                .running_since
                .is_some_and(|since| now.saturating_duration_since(since) >= toast.remaining)
        });
        if is_due {
            self.current = None;
        }
        is_due
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    struct Fake {
        origin: Instant,
        offset: Rc<Cell<f32>>,
        reduce: Rc<Cell<bool>>,
    }

    impl Fake {
        fn toasts() -> (Toasts, Self) {
            let origin = Instant::now();
            let offset = Rc::new(Cell::new(0.0));
            let reduce = Rc::new(Cell::new(false));
            let (clock_offset, clock_reduce) = (offset.clone(), reduce.clone());
            let toasts = Toasts::default().with_sources(
                move || origin + Duration::from_secs_f32(clock_offset.get()),
                move || clock_reduce.get(),
            );
            (
                toasts,
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

    #[test]
    fn a_toast_expires_after_ten_seconds() {
        let (mut toasts, fake) = Fake::toasts();
        toasts.show("Wrote tags to 3 files".to_string(), None);
        fake.at(9.9);
        assert!(!toasts.expire_if_due());
        assert!(toasts.current().is_some());
        fake.at(10.0);
        assert!(toasts.expire_if_due());
        assert!(toasts.current().is_none());
    }

    #[test]
    fn hovering_pauses_the_ten_seconds() {
        let (mut toasts, fake) = Fake::toasts();
        toasts.show("Wrote tags".to_string(), Some(ToastAction::UndoTagWrite));
        fake.at(6.0);
        toasts.set_hovered(true);
        fake.at(60.0);
        assert!(!toasts.expire_if_due(), "held while hovered");
        toasts.set_hovered(false);
        fake.at(63.9);
        assert!(!toasts.expire_if_due(), "four seconds were left");
        fake.at(64.0);
        assert!(toasts.expire_if_due());
    }

    #[test]
    fn a_new_toast_replaces_the_old_and_starts_its_own_clock() {
        let (mut toasts, fake) = Fake::toasts();
        let first = toasts.show("First".to_string(), None);
        fake.at(8.0);
        let second = toasts.show("Second".to_string(), Some(ToastAction::UndoTagWrite));
        assert!(!toasts.is_showing(first));
        assert!(toasts.is_showing(second));
        assert_eq!(toasts.current().unwrap().message, "Second");
        fake.at(12.0);
        assert!(
            !toasts.expire_if_due(),
            "the second toast has its own ten seconds"
        );
        fake.at(18.0);
        assert!(toasts.expire_if_due());
    }

    #[test]
    fn the_toast_slides_in_over_180_milliseconds() {
        let (mut toasts, fake) = Fake::toasts();
        toasts.show("Wrote tags".to_string(), None);
        assert!(toasts.progress(fake.at(0.0)) < 0.01);
        assert!(toasts.is_animating(fake.at(0.05)));
        let halfway = toasts.progress(fake.at(0.09));
        assert!(
            halfway > 0.5 && halfway < 1.0,
            "ease-out is past half at half time"
        );
        assert!(!toasts.is_animating(fake.at(0.18)));
        assert!((toasts.progress(fake.at(0.18)) - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn reduced_motion_shows_the_toast_at_once_and_asks_for_no_frames() {
        let (mut toasts, fake) = Fake::toasts();
        fake.reduce.set(true);
        toasts.show("Wrote tags".to_string(), None);
        let now = fake.at(0.0);
        assert!((toasts.progress(now) - 1.0).abs() < f32::EPSILON);
        assert!(!toasts.is_animating(now));
    }

    #[test]
    fn only_a_toast_with_undo_offers_undo() {
        let (mut toasts, _) = Fake::toasts();
        assert!(!toasts.offers_undo());
        toasts.show(
            "Exported".to_string(),
            Some(ToastAction::Reveal("/a".into())),
        );
        assert!(!toasts.offers_undo());
        toasts.show("Wrote".to_string(), Some(ToastAction::UndoTagWrite));
        assert!(toasts.offers_undo());
    }
}
