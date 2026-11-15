// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The collapsible sections of the track detail: which are open, what opens
//! one by itself for a track that needs it, and the motion of opening and
//! closing. What the user toggled is kept per section, not per track, and
//! beats the automatic opening.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use euphoria_core::quality::QualityVerdict;
use serde::{Deserialize, Serialize};

use crate::library::{ReviewReason, Track};
use crate::motion::{self, Transition};

#[cfg(test)]
const MOTION_SECONDS: f32 = 0.18;

/// A section below the waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SectionKind {
    Mixes,
    ByEar,
    KeyShift,
    Stems,
    Quality,
    Location,
}

impl SectionKind {
    pub(crate) const ALL: [SectionKind; 6] = [
        SectionKind::Mixes,
        SectionKind::ByEar,
        SectionKind::KeyShift,
        SectionKind::Stems,
        SectionKind::Quality,
        SectionKind::Location,
    ];

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0)
    }

    /// The heading of the section.
    pub(crate) fn title(self) -> &'static str {
        match self {
            SectionKind::Mixes => "Mixes well with",
            SectionKind::ByEar => "Check by ear",
            SectionKind::KeyShift => "Hear it in another key",
            SectionKind::Stems => "Stems",
            SectionKind::Quality => "Sound quality",
            SectionKind::Location => "Location",
        }
    }

    /// The short name on a tab of the Deck strip.
    pub(crate) fn tab_title(self) -> &'static str {
        match self {
            SectionKind::Mixes => "Mixes",
            SectionKind::ByEar => "By ear",
            SectionKind::KeyShift => "Other key",
            SectionKind::Stems => "Stems",
            SectionKind::Quality => "Quality",
            SectionKind::Location => "Location",
        }
    }

    /// Whether the section is open for a user who never toggled it.
    pub(crate) fn opens_by_default(self) -> bool {
        self == SectionKind::Mixes
    }

    /// True when `track` is one this section should open by itself for:
    /// Check by ear for a track under Needs review for its key, Sound
    /// quality for a weak, re-encoded or damaged file.
    pub(crate) fn opens_for(self, track: &Track) -> bool {
        match self {
            SectionKind::ByEar => track.review_reasons().iter().any(|reason| {
                matches!(
                    reason,
                    ReviewReason::KeyUnsure | ReviewReason::KeyConflict { .. }
                )
            }),
            SectionKind::Quality => track.quality().is_some_and(|report| {
                matches!(
                    report.verdict,
                    QualityVerdict::WeakOnABigSystem | QualityVerdict::LikelyReencoded
                ) || report.is_damaged()
            }),
            _ => false,
        }
    }
}

/// What the user chose for each section, kept in the settings. `None` is
/// no choice yet: the section is open as its default or the track says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SectionChoices {
    pub(crate) mixes: Option<bool>,
    pub(crate) by_ear: Option<bool>,
    pub(crate) key_shift: Option<bool>,
    pub(crate) stems: Option<bool>,
    pub(crate) quality: Option<bool>,
    pub(crate) location: Option<bool>,
}

impl SectionChoices {
    pub(crate) fn get(&self, kind: SectionKind) -> Option<bool> {
        match kind {
            SectionKind::Mixes => self.mixes,
            SectionKind::ByEar => self.by_ear,
            SectionKind::KeyShift => self.key_shift,
            SectionKind::Stems => self.stems,
            SectionKind::Quality => self.quality,
            SectionKind::Location => self.location,
        }
    }

    pub(crate) fn set(&mut self, kind: SectionKind, is_open: bool) {
        let slot = match kind {
            SectionKind::Mixes => &mut self.mixes,
            SectionKind::ByEar => &mut self.by_ear,
            SectionKind::KeyShift => &mut self.key_shift,
            SectionKind::Stems => &mut self.stems,
            SectionKind::Quality => &mut self.quality,
            SectionKind::Location => &mut self.location,
        };
        *slot = Some(is_open);
    }

    /// Whether `kind` is open for `track`: the user's choice when there is
    /// one, else open by default or because the track needs it.
    pub(crate) fn is_open(&self, kind: SectionKind, track: Option<&Track>) -> bool {
        self.get(kind).unwrap_or_else(|| {
            kind.opens_by_default() || track.is_some_and(|track| kind.opens_for(track))
        })
    }
}

/// How far open one section is on its way, from 0.0 closed to 1.0 open.
pub(crate) struct SectionMotion {
    transition: Transition,
}

impl SectionMotion {
    pub(crate) fn at_rest(is_open: bool) -> Self {
        Self {
            transition: Transition::at_rest(is_open),
        }
    }

    #[cfg(test)]
    fn with_sources(
        mut self,
        now: impl Fn() -> Instant + 'static,
        reduces_motion: impl Fn() -> bool + 'static,
    ) -> Self {
        self.transition = self
            .transition
            .with_sources(motion::Sources::fake(now, reduces_motion));
        self
    }

    pub(crate) fn now(&self) -> Instant {
        self.transition.now()
    }

    /// How open the section is at `now`.
    pub(crate) fn fraction(&self, now: Instant) -> f32 {
        self.transition.value(now)
    }

    /// True while the height still changes after `now`, so another frame is due.
    pub(crate) fn is_moving(&self, now: Instant) -> bool {
        self.transition.is_moving(now)
    }

    pub(crate) fn settle(&mut self, now: Instant) {
        self.transition.settle(now);
    }

    /// Opens or closes from wherever the section is now. With motion
    /// reduced it jumps.
    pub(crate) fn go_to(&mut self, is_open: bool) {
        self.transition.go_to(is_open, motion::PANEL);
    }

    /// Takes the open or closed state without moving, for a section that
    /// changes because another track was selected.
    pub(crate) fn snap_to(&mut self, is_open: bool) {
        self.transition.snap_to(is_open);
    }

    pub(crate) fn is_target_open(&self) -> bool {
        self.transition.is_target_full()
    }
}

/// The height each section's contents had when last drawn, so that
/// opening can animate to it.
pub(crate) type SeenHeight = Rc<Cell<Option<f32>>>;

/// The state of every section: the user's choices, the motion of each and
/// the height of each one's contents.
pub(crate) struct InspectorSections {
    pub(crate) choices: SectionChoices,
    motions: [SectionMotion; 6],
    pub(crate) heights: [SeenHeight; 6],
}

impl InspectorSections {
    pub(crate) fn new(choices: SectionChoices) -> Self {
        Self {
            choices,
            motions: SectionKind::ALL
                .map(|kind| SectionMotion::at_rest(choices.is_open(kind, None))),
            heights: SectionKind::ALL.map(|_| Rc::new(Cell::new(None))),
        }
    }

    pub(crate) fn motion(&self, kind: SectionKind) -> &SectionMotion {
        &self.motions[kind.index()]
    }

    /// Lines the motion up with what `track` says each section's state is,
    /// without animating: the track changed, not the user's mind.
    pub(crate) fn sync(&mut self, track: Option<&Track>) {
        for kind in SectionKind::ALL {
            let wanted = self.choices.is_open(kind, track);
            let motion = &mut self.motions[kind.index()];
            if motion.is_target_open() != wanted {
                motion.snap_to(wanted);
            }
        }
    }

    /// The user opens or closes `kind`; the choice is theirs from now on.
    pub(crate) fn toggle(&mut self, kind: SectionKind, track: Option<&Track>) {
        let now_open = !self.choices.is_open(kind, track);
        self.choices.set(kind, now_open);
        self.motions[kind.index()].go_to(now_open);
    }

    /// Settles finished motions. True while any is still moving.
    pub(crate) fn advance(&mut self) -> bool {
        let mut is_moving = false;
        for motion in &mut self.motions {
            let now = motion.now();
            motion.settle(now);
            is_moving |= motion.is_moving(now);
        }
        is_moving
    }

    /// How open `kind` is now.
    pub(crate) fn fraction(&self, kind: SectionKind) -> f32 {
        let motion = self.motion(kind);
        motion.fraction(motion.now())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use super::*;

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 0.01
    }

    #[test]
    fn only_mixes_is_open_by_default() {
        let choices = SectionChoices::default();
        for kind in SectionKind::ALL {
            assert_eq!(
                choices.is_open(kind, None),
                kind == SectionKind::Mixes,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn old_settings_files_load_with_the_defaults() {
        let choices: SectionChoices = serde_json::from_str("{}").unwrap();
        assert_eq!(choices, SectionChoices::default());
        let partial: SectionChoices = serde_json::from_str(r#"{"stems": true}"#).unwrap();
        assert!(partial.is_open(SectionKind::Stems, None));
        assert!(partial.is_open(SectionKind::Mixes, None));
        assert!(!partial.is_open(SectionKind::ByEar, None));
    }

    #[test]
    fn a_toggle_is_kept_in_the_choices_and_survives_a_round_trip() {
        let mut sections = InspectorSections::new(SectionChoices::default());
        sections.toggle(SectionKind::Location, None);
        sections.toggle(SectionKind::Mixes, None);
        let text = serde_json::to_string(&sections.choices).unwrap();
        let reloaded: SectionChoices = serde_json::from_str(&text).unwrap();
        assert!(reloaded.is_open(SectionKind::Location, None));
        assert!(!reloaded.is_open(SectionKind::Mixes, None));
        assert!(!reloaded.is_open(SectionKind::Stems, None));
    }

    #[test]
    fn a_choice_wins_over_the_automatic_opening() {
        let mut choices = SectionChoices::default();
        choices.set(SectionKind::ByEar, false);
        assert!(!choices.is_open(SectionKind::ByEar, None));
        choices.set(SectionKind::ByEar, true);
        assert!(choices.is_open(SectionKind::ByEar, None));
    }

    fn fake_motion(is_open: bool) -> (SectionMotion, Rc<Cell<f32>>, Rc<Cell<bool>>, Instant) {
        let origin = Instant::now();
        let offset = Rc::new(Cell::new(0.0));
        let reduce = Rc::new(Cell::new(false));
        let (clock_offset, clock_reduce) = (offset.clone(), reduce.clone());
        let motion = SectionMotion::at_rest(is_open).with_sources(
            move || origin + Duration::from_secs_f32(clock_offset.get()),
            move || clock_reduce.get(),
        );
        (motion, offset, reduce, origin)
    }

    #[test]
    fn opening_takes_180_milliseconds_and_eases_out() {
        let (mut motion, offset, _, origin) = fake_motion(false);
        motion.go_to(true);
        assert!(close(motion.fraction(origin), 0.0));
        assert!(motion.is_moving(origin + Duration::from_millis(10)));
        assert!(close(
            motion.fraction(origin + Duration::from_millis(90)),
            0.875
        ));
        offset.set(MOTION_SECONDS);
        let end = origin + Duration::from_secs_f32(MOTION_SECONDS);
        assert!(close(motion.fraction(end), 1.0));
        assert!(!motion.is_moving(end + Duration::from_millis(1)));
    }

    #[test]
    fn reduced_motion_is_instant() {
        let (mut motion, _, reduce, origin) = fake_motion(false);
        reduce.set(true);
        motion.go_to(true);
        assert_eq!(motion.fraction(origin), 1.0);
        assert!(!motion.is_moving(origin));
        motion.go_to(false);
        assert_eq!(motion.fraction(origin), 0.0);
        assert!(!motion.is_moving(origin));
    }

    #[test]
    fn a_turn_around_starts_from_the_height_reached() {
        let (mut motion, offset, _, origin) = fake_motion(false);
        motion.go_to(true);
        offset.set(0.05);
        let reached = motion.fraction(origin + Duration::from_millis(50));
        motion.go_to(false);
        assert!(close(
            motion.fraction(origin + Duration::from_millis(50)),
            reached
        ));
    }

    #[test]
    fn settling_ends_the_frame_requests() {
        let (mut motion, _, _, origin) = fake_motion(false);
        motion.go_to(true);
        motion.settle(origin + Duration::from_secs(1));
        assert!(!motion.is_moving(origin + Duration::from_secs(1)));
        assert_eq!(motion.fraction(origin + Duration::from_secs(9)), 1.0);
    }
}
