// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playing the planner's transition on the decks: A alone, B alone, or A
//! into B on the planned bars, with B stretched to the plan's tempo and an
//! equal-power crossfade over the window.

use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::deck_audio::{DeckCommand, DeckId, reachable_rate};
use crate::transition_plan::{BARS_AFTER, BARS_BEFORE, PlannedTransition};
use euphoria_core::harmony::transition::CandidateWindow;

/// How many bars of A play before the mix starts.
pub(crate) const LEAD_IN_BARS: f64 = 8.0;

/// B is started this many bars before the mix, so that the decks' own
/// downbeat alignment waits for the mix start itself.
const BRING_IN_BARS_BEFORE: f64 = 0.45;

const TICK: Duration = Duration::from_millis(30);

/// What the transport plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanPlay {
    Transition,
    A,
    B,
}

/// Whose window the transport plays: the planner's or the decks' own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanSource {
    Planner,
    Decks,
}

/// Where the bars of the window lie in each track's own audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlanTiming {
    pub(crate) a_mix_seconds: f64,
    pub(crate) a_bar_seconds: f64,
    pub(crate) b_mix_seconds: f64,
    pub(crate) b_bar_seconds: f64,
    pub(crate) b_rate: f64,
    pub(crate) window_bars: usize,
}

impl PlanTiming {
    pub(crate) fn of(planned: &PlannedTransition, window: &CandidateWindow) -> Option<Self> {
        let lanes_a = &planned.plan.lanes_a;
        let lanes_b = &planned.plan.lanes_b;
        Some(Self {
            a_mix_seconds: lanes_a.bars.get(window.a_start_bar)?.start_seconds,
            a_bar_seconds: lanes_a.bar_seconds,
            b_mix_seconds: lanes_b.bars.get(window.b_start_bar)?.start_seconds,
            b_bar_seconds: lanes_b.bar_seconds,
            b_rate: reachable_rate(planned.plan.alignment.rate),
            window_bars: window.bars,
        })
    }

    /// Bars shown on the timeline.
    pub(crate) fn total_bars(&self) -> f64 {
        (BARS_BEFORE + self.window_bars + BARS_AFTER) as f64
    }

    /// Seconds into A at `bar`, counted from the mix start.
    pub(crate) fn a_seconds_at(&self, bar: f64) -> f64 {
        (self.a_mix_seconds + bar * self.a_bar_seconds).max(0.0)
    }

    pub(crate) fn b_seconds_at(&self, bar: f64) -> f64 {
        (self.b_mix_seconds + bar * self.b_bar_seconds).max(0.0)
    }

    pub(crate) fn bar_of_a(&self, seconds: f64) -> f64 {
        (seconds - self.a_mix_seconds) / self.a_bar_seconds
    }

    pub(crate) fn bar_of_b(&self, seconds: f64) -> f64 {
        (seconds - self.b_mix_seconds) / self.b_bar_seconds
    }

    /// The bar, from the mix start, a point `fraction` along the timeline
    /// stands for.
    pub(crate) fn bar_at_fraction(&self, fraction: f32) -> f64 {
        f64::from(fraction.clamp(0.0, 1.0)) * self.total_bars() - BARS_BEFORE as f64
    }

    /// How loud A and B are at `bar`: an equal-power crossfade across the
    /// window.
    pub(crate) fn gains(&self, bar: f64) -> [f32; 2] {
        let progress = (bar / self.window_bars as f64).clamp(0.0, 1.0) as f32;
        [(progress * FRAC_PI_2).cos(), (progress * FRAC_PI_2).sin()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanPhase {
    Loading,
    LeadIn,
    Mixing,
}

/// A transport that plays.
#[derive(Debug, Clone)]
pub(crate) struct PlanPlayback {
    source: PlanSource,
    kind: PlanPlay,
    timing: PlanTiming,
    phase: PlanPhase,
    from_bar: f64,
    generation: u64,
    restore_sync: bool,
}

impl PlanPlayback {
    pub(crate) fn kind(&self) -> PlanPlay {
        self.kind
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

impl EuphoriaApp {
    /// The bar, from the mix start, the playhead stands at: where the sound
    /// is while it plays, otherwise where playing would start.
    pub(crate) fn plan_playhead_bar(&self) -> f64 {
        let Some(playback) = &self.planner.playback else {
            return self.planner.start_bar.unwrap_or(-LEAD_IN_BARS);
        };
        if playback.phase == PlanPhase::Loading {
            return playback.from_bar;
        }
        let timing = &playback.timing;
        match playback.kind {
            PlanPlay::B => timing.bar_of_b(self.decks.position_seconds(DeckId::B)),
            _ => timing.bar_of_a(self.decks.position_seconds(DeckId::A)),
        }
    }

    pub(crate) fn plan_is_playing(&self) -> bool {
        self.planner.playback.is_some()
    }

    /// Whose window the transport plays, while it plays.
    pub(crate) fn plan_source(&self) -> Option<PlanSource> {
        self.planner
            .playback
            .as_ref()
            .map(|playback| playback.source)
    }

    /// The Test transition button on the decks: plays A into B on the
    /// window the decks show, or stops it.
    pub(crate) fn test_deck_transition(&mut self, cx: &mut Context<'_, Self>) {
        if self.plan_source() == Some(PlanSource::Decks) {
            self.stop_plan_playback(cx);
        } else {
            self.play_plan_from(PlanSource::Decks, PlanPlay::Transition, None, cx);
        }
    }

    fn source_window(
        &self,
        source: PlanSource,
    ) -> Option<(&std::sync::Arc<PlannedTransition>, &CandidateWindow)> {
        match source {
            PlanSource::Planner => self.planner_window(),
            PlanSource::Decks => self.deck_plan_window(),
        }
    }

    fn source_tracks(&self, source: PlanSource) -> Option<(usize, usize)> {
        match source {
            PlanSource::Planner => Some((self.selected?, self.focus.selected_match?)),
            PlanSource::Decks => self.deck_pair_indexes(),
        }
    }

    pub(crate) fn toggle_plan_playback(&mut self, cx: &mut Context<'_, Self>) {
        if self.plan_is_playing() {
            self.stop_plan_playback(cx);
        } else {
            self.play_plan(PlanPlay::Transition, None, cx);
        }
    }

    /// Plays from the bar `from_bar` of the timeline, or from where the
    /// playhead waits.
    pub(crate) fn play_plan(
        &mut self,
        kind: PlanPlay,
        from_bar: Option<f64>,
        cx: &mut Context<'_, Self>,
    ) {
        self.play_plan_from(PlanSource::Planner, kind, from_bar, cx);
    }

    /// Plays the window of `source` from the bar `from_bar` of its
    /// timeline, or from where playing waits.
    pub(crate) fn play_plan_from(
        &mut self,
        source: PlanSource,
        kind: PlanPlay,
        from_bar: Option<f64>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some((planned, window)) = self.source_window(source) else {
            return;
        };
        let Some(timing) = PlanTiming::of(planned, window) else {
            return;
        };
        let Some((a_index, b_index)) = self.source_tracks(source) else {
            return;
        };
        let waiting_bar = match source {
            PlanSource::Planner => self.planner.start_bar,
            PlanSource::Decks => None,
        };
        let from_bar = from_bar.or(waiting_bar).unwrap_or(-LEAD_IN_BARS).clamp(
            -(BARS_BEFORE as f64),
            timing.total_bars() - BARS_BEFORE as f64,
        );
        let restore_sync = self
            .planner
            .playback
            .as_ref()
            .map_or(self.decks.sync, |playback| playback.restore_sync);
        self.pause_decks_for_plan();
        if !self.decks.is_open {
            self.toggle_decks(cx);
        }
        if !self.decks.is_open {
            return;
        }
        if source == PlanSource::Planner {
            self.planner.start_bar = Some(from_bar);
        }
        self.decks.sync = false;
        let needs_a = kind != PlanPlay::B;
        let needs_b = kind != PlanPlay::A;
        for (deck, index, needed) in [(DeckId::A, a_index, needs_a), (DeckId::B, b_index, needs_b)]
        {
            if needed && self.decks.view(deck).track_index != Some(index) {
                self.load_on_deck(deck, index, cx);
            }
        }
        self.decks.sync = false;
        self.decks.view_mut(DeckId::A).rate = 1.0;
        self.decks.view_mut(DeckId::B).rate = timing.b_rate;
        for deck in DeckId::BOTH {
            let view = self.decks.view_mut(deck);
            view.keylock = true;
            view.loop_bars = None;
        }
        self.decks.send(DeckCommand::SetRate {
            deck: DeckId::B,
            rate: timing.b_rate,
        });
        let (a_seconds, b_seconds) = match kind {
            PlanPlay::Transition if from_bar < 0.0 => {
                (timing.a_seconds_at(from_bar), timing.b_seconds_at(0.0))
            }
            PlanPlay::B => (0.0, timing.b_seconds_at(from_bar.max(0.0))),
            _ => (
                timing.a_seconds_at(from_bar),
                timing.b_seconds_at(from_bar.max(0.0)),
            ),
        };
        if needs_a {
            self.open_deck_source(DeckId::A, a_seconds, false, cx);
        }
        if needs_b {
            self.open_deck_source(DeckId::B, b_seconds, false, cx);
        }
        let generation = self
            .planner
            .playback
            .as_ref()
            .map_or(0, |playback| playback.generation)
            + 1;
        self.planner.playback = Some(PlanPlayback {
            source,
            kind,
            timing,
            phase: PlanPhase::Loading,
            from_bar,
            generation,
            restore_sync,
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let goes_on = this.update(cx, |this, cx| {
                    let a_seconds = this.decks.position_seconds(DeckId::A);
                    let b_seconds = this.decks.position_seconds(DeckId::B);
                    this.step_plan_playback(generation, a_seconds, b_seconds, cx)
                });
                if !matches!(goes_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// One tick of the transport, given where the decks are. Returns false
    /// once the transport has stopped.
    pub(crate) fn step_plan_playback(
        &mut self,
        generation: u64,
        a_seconds: f64,
        b_seconds: f64,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(playback) = self.planner.playback.clone() else {
            return false;
        };
        if playback.generation != generation {
            return false;
        }
        let source_open = match playback.source {
            PlanSource::Planner => self.planner.is_open,
            PlanSource::Decks => true,
        };
        let still_planned = source_open
            && self.decks.is_open
            && self
                .source_window(playback.source)
                .and_then(|(planned, window)| PlanTiming::of(planned, window))
                == Some(playback.timing);
        if !still_planned {
            self.stop_plan_playback(cx);
            return false;
        }
        let timing = playback.timing;
        let kind = playback.kind;
        let needed: Vec<DeckId> = match kind {
            PlanPlay::Transition => DeckId::BOTH.to_vec(),
            PlanPlay::A => vec![DeckId::A],
            PlanPlay::B => vec![DeckId::B],
        };
        match playback.phase {
            PlanPhase::Loading => {
                if needed.iter().any(|deck| self.decks.view(*deck).loading) {
                    return true;
                }
                if let Some(problem) = needed
                    .iter()
                    .find_map(|deck| self.decks.view(*deck).problem.clone())
                {
                    self.notice = Some(problem);
                    self.stop_plan_playback(cx);
                    return false;
                }
                let from_bar = playback.from_bar;
                let next = match kind {
                    PlanPlay::Transition if from_bar < 0.0 => {
                        self.decks.set_gains([1.0, 0.0]);
                        self.play_deck_for_plan(DeckId::A, false);
                        PlanPhase::LeadIn
                    }
                    PlanPlay::Transition => {
                        self.decks.set_gains(timing.gains(from_bar));
                        self.play_deck_for_plan(DeckId::A, false);
                        self.play_deck_for_plan(DeckId::B, false);
                        PlanPhase::Mixing
                    }
                    PlanPlay::A => {
                        self.decks.set_gains([1.0, 0.0]);
                        self.play_deck_for_plan(DeckId::A, false);
                        PlanPhase::Mixing
                    }
                    PlanPlay::B => {
                        self.decks.set_gains([0.0, 1.0]);
                        self.play_deck_for_plan(DeckId::B, false);
                        PlanPhase::Mixing
                    }
                };
                self.set_plan_phase(next);
                self.follow_decks(cx);
            }
            PlanPhase::LeadIn => {
                if timing.bar_of_a(a_seconds) >= -BRING_IN_BARS_BEFORE {
                    self.play_deck_for_plan(DeckId::B, true);
                    self.set_plan_phase(PlanPhase::Mixing);
                }
            }
            PlanPhase::Mixing => {
                if kind == PlanPlay::Transition {
                    self.decks
                        .set_gains(timing.gains(timing.bar_of_a(a_seconds)));
                }
                let end_bar = (timing.window_bars + BARS_AFTER) as f64;
                let position = match kind {
                    PlanPlay::B => timing.bar_of_b(b_seconds),
                    _ => timing.bar_of_a(a_seconds),
                };
                let finished = needed.iter().all(|deck| self.decks.is_finished(*deck));
                let through = position >= end_bar && kind == PlanPlay::Transition;
                if through && playback.source == PlanSource::Decks && !finished {
                    self.hand_decks_over_after_transition(cx);
                    return false;
                }
                if through || finished {
                    self.stop_plan_playback(cx);
                    return false;
                }
            }
        }
        cx.notify();
        true
    }

    fn set_plan_phase(&mut self, phase: PlanPhase) {
        if let Some(playback) = &mut self.planner.playback {
            playback.phase = phase;
        }
    }

    fn play_deck_for_plan(&mut self, deck: DeckId, aligned_to_other: bool) {
        self.decks.view_mut(deck).playing = true;
        self.decks.send(DeckCommand::Play {
            deck,
            quantize_to: aligned_to_other.then_some(deck.other()),
        });
    }

    pub(crate) fn pause_decks_for_plan(&mut self) {
        for deck in DeckId::BOTH {
            if self.decks.view(deck).playing {
                self.decks.view_mut(deck).playing = false;
                self.decks.send(DeckCommand::Pause(deck));
            }
        }
    }

    /// Ends a transition played from the decks with both still playing and
    /// the crossfader on B, where the transition left the sound.
    pub(crate) fn hand_decks_over_after_transition(&mut self, cx: &mut Context<'_, Self>) {
        let Some(playback) = self.planner.playback.take() else {
            return;
        };
        self.decks.sync = playback.restore_sync;
        self.decks.crossfader = 1.0;
        self.decks.apply_gains();
        cx.notify();
    }

    /// Stops the sound, keeps the playhead where it was, and gives the
    /// decks back as the planner found them.
    pub(crate) fn stop_plan_playback(&mut self, cx: &mut Context<'_, Self>) {
        let Some(playback) = self.planner.playback.take() else {
            return;
        };
        self.pause_decks_for_plan();
        self.decks.sync = playback.restore_sync;
        self.decks.apply_gains();
        cx.notify();
    }

    /// A press on the timeline at `fraction` of its width: playing starts
    /// from there, or, when stopped, the playhead moves there.
    pub(crate) fn seek_plan_timeline(&mut self, fraction: f32, cx: &mut Context<'_, Self>) {
        let Some(timing) = self
            .planner_window()
            .and_then(|(planned, window)| PlanTiming::of(planned, window))
        else {
            return;
        };
        let bar = timing.bar_at_fraction(fraction).round();
        match self.planner.playback.as_ref().map(PlanPlayback::kind) {
            Some(kind) => self.play_plan(kind, Some(bar), cx),
            None => {
                self.planner.start_bar = Some(bar);
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> PlanTiming {
        PlanTiming {
            a_mix_seconds: 40.0,
            a_bar_seconds: 2.0,
            b_mix_seconds: 10.0,
            b_bar_seconds: 2.0,
            b_rate: 1.02,
            window_bars: 16,
        }
    }

    #[test]
    fn bars_are_counted_from_the_mix_start_in_each_tracks_own_audio() {
        let timing = timing();
        assert_eq!(timing.a_seconds_at(-8.0), 24.0);
        assert_eq!(timing.a_seconds_at(0.0), 40.0);
        assert_eq!(timing.b_seconds_at(2.0), 14.0);
        assert_eq!(timing.bar_of_a(24.0), -8.0);
        assert_eq!(timing.bar_of_b(14.0), 2.0);
        assert_eq!(timing.a_seconds_at(-100.0), 0.0);
    }

    #[test]
    fn the_crossfade_is_equal_power_across_the_window() {
        let timing = timing();
        assert_eq!(timing.gains(-4.0), [1.0, 0.0]);
        let [a, b] = timing.gains(8.0);
        assert!((a - b).abs() < 1e-6);
        assert!((a * a + b * b - 1.0).abs() < 1e-5);
        let [a, b] = timing.gains(16.0);
        assert!(a.abs() < 1e-6 && (b - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_click_along_the_timeline_is_a_bar_from_the_mix_start() {
        let timing = timing();
        assert_eq!(timing.bar_at_fraction(0.0), -32.0);
        assert!(timing.bar_at_fraction(32.0 / 56.0).abs() < 1e-3);
        assert_eq!(timing.bar_at_fraction(1.0), 24.0);
    }
}
