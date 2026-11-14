// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Planning a transition inside the app: the plan of the selected track into
//! each track it mixes into, worked out off the main thread and kept in
//! memory per pair, and the words the focus view and the planner show for
//! it. Separation is never started from here; a track without stems is
//! planned without them and its clashes are not checked.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use euphoria_core::harmony::Relation;
use euphoria_core::harmony::transition::{
    CandidateWindow, Clash, PlanError, PlanOptions, PlanSection, PlanTrack, Presence, SectionLabel,
    Side, StepKind, SuggestedCues, TransitionPlan, cues_for_window, plan_transition,
};
use euphoria_core::plan_signals::{
    SignalsCache, TrackSignals, measure_energy, signals_with_energy,
};
use euphoria_core::stems::{StemCache, StemFiles, measure_stem_levels};
use euphoria_core::{BeatGrid, ContentHash, Cue, CueSection, HotCue, Key, TrackAnalysis};
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::camelot::CamelotPosition;
use crate::cue_editing::UndoGroup;
use crate::focus::clock;
use crate::library::Track;
use crate::views::cues::shown_cues;

/// How many ranked windows the planner lists.
pub(crate) const WINDOWS_LISTED: usize = 5;

/// How many bars of A before the window the planner shows.
pub(crate) const BARS_BEFORE: usize = 32;

/// How many bars after the window it shows, of B going on alone.
pub(crate) const BARS_AFTER: usize = 8;

/// Plans worked out at once. Each decodes a track, so two keep the machine
/// calm.
const PLANS_AT_ONCE: usize = 2;

/// Everything a plan is made of that the views need.
#[derive(Debug)]
pub(crate) struct PlannedTransition {
    pub(crate) plan: TransitionPlan,
    pub(crate) a_key: Option<Key>,
    pub(crate) b_key: Option<Key>,
    pub(crate) cues_by_window: Vec<Option<SuggestedCues>>,
}

impl PlannedTransition {
    pub(crate) fn window(&self, index: usize) -> Option<&CandidateWindow> {
        self.plan.windows.get(index)
    }

    pub(crate) fn stems_known(&self) -> (bool, bool) {
        (self.plan.lanes_a.stems_known, self.plan.lanes_b.stems_known)
    }
}

/// Where the plan of a pair stands.
#[derive(Debug, Clone)]
pub(crate) enum PlanState {
    Computing,
    Ready(Arc<PlannedTransition>),
    Failed(PlanError),
}

type PairKey = (PathBuf, PathBuf);

struct PlanEntry {
    fingerprint: u64,
    state: PlanState,
}

struct PlanSide {
    path: PathBuf,
    analysis: TrackAnalysis,
    sections: Vec<PlanSection>,
    stem_files: Option<StemFiles>,
}

struct PlanJob {
    key: PairKey,
    fingerprint: u64,
    a: PlanSide,
    b: PlanSide,
    signals_directory: Option<PathBuf>,
}

/// The plans of the pairs asked for, and the planner sub-view.
#[derive(Default)]
pub(crate) struct Planner {
    entries: HashMap<PairKey, PlanEntry>,
    queue: VecDeque<PlanJob>,
    running: usize,
    /// Where plan signals are kept, instead of Euphoria's data folder.
    pub(crate) signals_directory: Option<PathBuf>,
    /// True while the planner sub-view covers the focus view.
    pub(crate) is_open: bool,
    /// The window the planner shows, as an index into the plan's windows.
    pub(crate) window: usize,
}

impl Planner {
    fn state(&self, key: &PairKey) -> Option<&PlanState> {
        self.entries.get(key).map(|entry| &entry.state)
    }

    pub(crate) fn forget_track(&mut self, path: &Path) {
        self.entries
            .retain(|key, _| key.0.as_path() != path && key.1.as_path() != path);
        self.queue
            .retain(|job| job.key.0.as_path() != path && job.key.1.as_path() != path);
    }
}

#[cfg(test)]
impl Planner {
    pub(crate) fn force_state(&mut self, from: &Path, to: &Path, state: PlanState) {
        self.entries.insert(
            (from.to_path_buf(), to.to_path_buf()),
            PlanEntry {
                fingerprint: 0,
                state,
            },
        );
    }
}

/// The sections of `cues` on `grid`, as bars, for the planner.
pub(crate) fn plan_sections(cues: &[Cue], grid: &BeatGrid) -> Vec<PlanSection> {
    let bar_seconds = grid.bar_seconds();
    if bar_seconds <= 0.0 {
        return Vec::new();
    }
    cues.iter()
        .filter_map(|cue| {
            let section = CueSection::from_name(&cue.name)?;
            let bars = (cue.seconds - grid.first_downbeat_seconds) / bar_seconds;
            (bars.is_finite() && bars > -0.5).then(|| PlanSection {
                start_bar: bars.round().max(0.0) as usize,
                label: section.into(),
            })
        })
        .collect()
}

fn plan_side(track: &Track, app: &EuphoriaApp) -> Option<PlanSide> {
    let mut analysis = track.analysis()?.clone();
    analysis.beat_grid = track.beat_grid();
    analysis.key = track.key();
    analysis.bpm = track.bpm();
    let sections = analysis
        .beat_grid
        .map(|grid| plan_sections(&shown_cues(track, app), &grid))
        .unwrap_or_default();
    let stem_files = app
        .stems
        .ready_files(&track.path)
        .or_else(|| app.decks.stem_library.files(&track.path));
    Some(PlanSide {
        path: track.path.clone(),
        analysis,
        sections,
        stem_files,
    })
}

fn fingerprint(a: &PlanSide, b: &PlanSide) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for side in [a, b] {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{}",
            side.analysis.beat_grid,
            side.analysis.key,
            side.analysis.duration_seconds,
            side.sections,
            side.stem_files.is_some()
        )
        .hash(&mut hasher);
    }
    hasher.finish()
}

fn signals_of(side: &PlanSide, directory: Option<&Path>) -> TrackSignals {
    let hash = ContentHash::of_file(&side.path).ok();
    let cache = match directory {
        Some(directory) => Some(SignalsCache::new(directory.to_path_buf())),
        None => SignalsCache::open_default().ok(),
    };
    let loaded = hash
        .as_ref()
        .zip(cache.as_ref())
        .and_then(|(hash, cache)| cache.load(hash));
    let mut signals = loaded.unwrap_or_else(|| {
        let energy = measure_energy(&side.path, side.analysis.bpm).unwrap_or_default();
        signals_with_energy(energy)
    });
    if signals.stems.is_none() {
        let files = side.stem_files.clone().or_else(|| {
            let hash = hash.as_ref()?;
            StemCache::open_default().ok()?.cached(hash)
        });
        signals.stems = files.and_then(|files| measure_stem_levels(&files).ok());
    }
    if let (Some(hash), Some(cache)) = (&hash, &cache) {
        cache.store(hash, &signals).ok();
    }
    signals
}

fn compute(job: &PlanJob) -> Result<PlannedTransition, PlanError> {
    let directory = job.signals_directory.as_deref();
    let signals_a = signals_of(&job.a, directory);
    let signals_b = signals_of(&job.b, directory);
    let track_a = PlanTrack {
        analysis: &job.a.analysis,
        sections: &job.a.sections,
        energy: &signals_a.energy,
        stems: signals_a.stems.as_ref(),
    };
    let track_b = PlanTrack {
        analysis: &job.b.analysis,
        sections: &job.b.sections,
        energy: &signals_b.energy,
        stems: signals_b.stems.as_ref(),
    };
    let plan = plan_transition(
        &track_a,
        &track_b,
        &PlanOptions {
            window_count: WINDOWS_LISTED,
        },
    )?;
    let cues_by_window = plan
        .windows
        .iter()
        .map(|window| cues_for_window(window, &track_a, &track_b))
        .collect();
    Ok(PlannedTransition {
        plan,
        a_key: job.a.analysis.key,
        b_key: job.b.analysis.key,
        cues_by_window,
    })
}

impl EuphoriaApp {
    /// The state of the plan of the selected track into the track at
    /// `b_index`.
    pub(crate) fn plan_state(&self, b_index: usize) -> Option<&PlanState> {
        let a_path = &self.selected_track()?.path;
        let b_path = &self.session.library().track(b_index)?.path;
        self.planner.state(&(a_path.clone(), b_path.clone()))
    }

    /// The plan of the selected track into the picked match, once worked
    /// out.
    pub(crate) fn picked_plan(&self) -> Option<&Arc<PlannedTransition>> {
        match self.plan_state(self.focus.selected_match?)? {
            PlanState::Ready(planned) => Some(planned),
            _ => None,
        }
    }

    /// The window the planner has chosen of the picked plan.
    pub(crate) fn planner_window(&self) -> Option<(&Arc<PlannedTransition>, &CandidateWindow)> {
        let planned = self.picked_plan()?;
        Some((planned, planned.window(self.planner.window)?))
    }

    /// The sentence the transition tab and the planner give for window
    /// `index` of the picked plan.
    pub(crate) fn window_summary_text(&self, index: usize) -> Option<String> {
        let planned = self.picked_plan()?;
        let window = planned.window(index)?;
        let matched = self.session.library().track(self.focus.selected_match?)?;
        Some(window_summary(
            planned,
            window,
            matched.title(),
            matched
                .key()
                .map(|key| self.display.key_notation.spell(&key)),
        ))
    }

    /// What the clash column says for the track at `b_index`.
    pub(crate) fn clash_label(&self, b_index: usize) -> &'static str {
        match self.plan_state(b_index) {
            Some(PlanState::Ready(planned)) => match planned.window(0) {
                Some(window) => {
                    if planned.stems_known() == (true, true) {
                        clash_verdict(window).label()
                    } else {
                        ClashVerdict::NotChecked.label()
                    }
                }
                None => "No window",
            },
            Some(PlanState::Failed(_)) => "No plan",
            Some(PlanState::Computing) => "Checking",
            None => "\u{2013}",
        }
    }

    /// Asks for the plans of the picked match and of the matches in rows
    /// `first..last` of the list, the nearest to the top first, and keeps
    /// at most [`PLANS_AT_ONCE`] running.
    pub(crate) fn request_plans(
        &mut self,
        rows: std::ops::Range<usize>,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.focus.is_open {
            return;
        }
        let Some(a_index) = self.selected else {
            return;
        };
        let mut targets: Vec<usize> = Vec::new();
        targets.extend(self.focus.selected_match);
        let end = rows.end.min(self.mixes.len());
        let start = rows.start.min(end);
        for mix in &self.mixes[start..end] {
            if !targets.contains(&mix.track_index) {
                targets.push(mix.track_index);
            }
        }
        let mut fresh: Vec<PlanJob> = Vec::new();
        for b_index in targets {
            if let Some(job) = self.plan_job(a_index, b_index) {
                let known = self
                    .planner
                    .entries
                    .get(&job.key)
                    .is_some_and(|entry| entry.fingerprint == job.fingerprint);
                let queued =
                    self.planner.queue.iter().any(|queued| {
                        queued.key == job.key && queued.fingerprint == job.fingerprint
                    });
                if !known && !queued {
                    fresh.push(job);
                }
            }
        }
        for job in &fresh {
            self.planner.entries.insert(
                job.key.clone(),
                PlanEntry {
                    fingerprint: job.fingerprint,
                    state: PlanState::Computing,
                },
            );
        }
        for job in fresh.into_iter().rev() {
            self.planner.queue.push_front(job);
        }
        self.pump_plans(cx);
    }

    fn plan_job(&self, a_index: usize, b_index: usize) -> Option<PlanJob> {
        let library = self.session.library();
        let a = plan_side(library.track(a_index)?, self)?;
        let b = plan_side(library.track(b_index)?, self)?;
        Some(PlanJob {
            key: (a.path.clone(), b.path.clone()),
            fingerprint: fingerprint(&a, &b),
            a,
            b,
            signals_directory: self.planner.signals_directory.clone(),
        })
    }

    fn pump_plans(&mut self, cx: &mut Context<'_, Self>) {
        while self.planner.running < PLANS_AT_ONCE {
            let Some(job) = self.planner.queue.pop_front() else {
                break;
            };
            self.planner.running += 1;
            cx.spawn(async move |this, cx| {
                let (job, outcome) = cx
                    .background_executor()
                    .spawn(async move {
                        let outcome = compute(&job);
                        (job, outcome)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    this.planner.running = this.planner.running.saturating_sub(1);
                    let current = this
                        .planner
                        .entries
                        .get(&job.key)
                        .is_some_and(|entry| entry.fingerprint == job.fingerprint);
                    if current {
                        let state = match outcome {
                            Ok(planned) => PlanState::Ready(Arc::new(planned)),
                            Err(error) => PlanState::Failed(error),
                        };
                        this.planner.entries.insert(
                            job.key,
                            PlanEntry {
                                fingerprint: job.fingerprint,
                                state,
                            },
                        );
                    }
                    this.pump_plans(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// The rows of the mixes list in view, with a little over, as a range
    /// of rows.
    pub(crate) fn visible_mix_rows(&self) -> std::ops::Range<usize> {
        let bottom = self.focus.mixes_scroll.bottom_item();
        let top = self.focus.mixes_scroll.top_item();
        let last = bottom.max(top + 4);
        top..(last + 2)
    }

    pub(crate) fn request_visible_plans(&mut self, cx: &mut Context<'_, Self>) {
        let rows = self.visible_mix_rows();
        self.request_plans(rows, cx);
    }

    /// The plans of the selected track are worked out again, after its
    /// cues or its stems changed.
    pub(crate) fn plans_of_track_changed(&mut self, path: &Path, cx: &mut Context<'_, Self>) {
        self.planner.forget_track(path);
        self.request_visible_plans(cx);
        cx.notify();
    }

    pub(crate) fn open_planner(&mut self, cx: &mut Context<'_, Self>) {
        let has_answer = self
            .focus
            .selected_match
            .and_then(|index| self.plan_state(index))
            .is_some_and(|state| !matches!(state, PlanState::Computing));
        if !self.focus.is_open || !has_answer {
            return;
        }
        self.planner.is_open = true;
        self.planner.window = 0;
        cx.notify();
    }

    pub(crate) fn close_planner(&mut self, cx: &mut Context<'_, Self>) {
        if self.planner.is_open {
            self.planner.is_open = false;
            cx.notify();
        }
    }

    pub(crate) fn select_plan_window(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if self
            .picked_plan()
            .is_some_and(|planned| index < planned.plan.windows.len())
        {
            self.planner.window = index;
            cx.notify();
        }
    }

    /// Gives A and B the cues that carry out window `index`, as one edit
    /// that one undo takes back.
    pub(crate) fn set_plan_cues(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(cues) = self
            .picked_plan()
            .and_then(|planned| planned.cues_by_window.get(index).cloned().flatten())
        else {
            return;
        };
        let Some(a_path) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        let Some(b_path) = self
            .focus
            .selected_match
            .and_then(|index| self.session.library().track(index))
            .map(|track| track.path.clone())
        else {
            return;
        };
        let group = UndoGroup::next();
        for (path, suggested) in [(a_path, cues.a_cues()), (b_path, cues.b_cues())] {
            let Some(track) = self.session.library().track_by_path(&path) else {
                continue;
            };
            let merged = merged_cues(&shown_cues(track, self), &suggested);
            self.set_cues_of_in_group(&path, Some(merged), group, cx);
        }
    }

    /// Loads A and B on the decks at the start of window `index`, B at A's
    /// tempo, each on its downbeat, paused.
    pub(crate) fn preview_on_decks(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(window) = self
            .picked_plan()
            .and_then(|planned| planned.window(index).cloned())
        else {
            return;
        };
        let Some(planned) = self.picked_plan().cloned() else {
            return;
        };
        let Some(a_index) = self.selected else {
            return;
        };
        let Some(b_index) = self.focus.selected_match else {
            return;
        };
        let a_seconds = planned
            .plan
            .lanes_a
            .bars
            .get(window.a_start_bar)
            .map_or(0.0, |bar| bar.start_seconds);
        let b_seconds = planned
            .plan
            .lanes_b
            .bars
            .get(window.b_start_bar)
            .map_or(0.0, |bar| bar.start_seconds);
        if !self.decks.is_open {
            self.toggle_decks(cx);
        }
        if !self.decks.is_open {
            return;
        }
        if !self.decks.sync {
            self.toggle_sync(cx);
        }
        self.load_on_deck(crate::deck_audio::DeckId::A, a_index, cx);
        self.load_on_deck(crate::deck_audio::DeckId::B, b_index, cx);
        self.open_deck_source(crate::deck_audio::DeckId::A, a_seconds, false, cx);
        self.open_deck_source(crate::deck_audio::DeckId::B, b_seconds, false, cx);
        self.close_planner(cx);
        self.close_focus_view(cx);
        cx.notify();
    }

    /// Marks the transition into the picked match as a favorite, with the
    /// summary of window `index` as its note.
    pub(crate) fn save_plan_as_favorite(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let Some(note) = self.window_summary_text(index) else {
            return;
        };
        let Some(from) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        let Some(to) = self
            .focus
            .selected_match
            .and_then(|index| self.session.library().track(index))
            .map(|track| track.path.clone())
        else {
            return;
        };
        if self.session.favorites().transition(&from, &to).is_none() {
            self.session.favorites_mut().toggle_transition(&from, &to);
        }
        self.session.favorites_mut().set_note(&from, &to, &note);
        self.refresh_mixes();
        self.save_favorites(cx);
        cx.notify();
    }

    /// Starts separating the stems of A (or of B) from the planner.
    pub(crate) fn separate_plan_stems(&mut self, side: Side, cx: &mut Context<'_, Self>) {
        let index = match side {
            Side::A => self.selected,
            Side::B => self.focus.selected_match,
        };
        let Some(path) = index
            .and_then(|index| self.session.library().track(index))
            .map(|track| track.path.clone())
        else {
            return;
        };
        self.request_stems_of(path, cx);
    }
}

/// `existing` with `suggested` laid on it: a cue of the same name is
/// replaced, so that setting the cues again does not pile up copies, and
/// each new cue takes the first pad no cue holds.
pub(crate) fn merged_cues(existing: &[Cue], suggested: &[Cue]) -> Vec<Cue> {
    let mut merged: Vec<Cue> = existing
        .iter()
        .filter(|cue| suggested.iter().all(|new| new.name != cue.name))
        .cloned()
        .collect();
    for cue in suggested {
        let hot_cue = HotCue::ALL
            .into_iter()
            .find(|pad| merged.iter().all(|held| held.hot_cue != Some(*pad)));
        merged.push(Cue {
            seconds: cue.seconds,
            name: cue.name.clone(),
            hot_cue,
        });
    }
    merged.sort_by(|first, second| first.seconds.total_cmp(&second.seconds));
    merged
}

/// The verdict on the clashes of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClashVerdict {
    Clean,
    VocalClash,
    BassClash,
    NotChecked,
}

impl ClashVerdict {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ClashVerdict::Clean => "Clean",
            ClashVerdict::VocalClash => "Vocal clash",
            ClashVerdict::BassClash => "Bass clash",
            ClashVerdict::NotChecked => "Not checked",
        }
    }
}

/// How the stems of `window` clash: both singing outranks both playing
/// bass, and a window with bars that could not be told, and no clash in the
/// rest, is not checked.
pub(crate) fn clash_verdict(window: &CandidateWindow) -> ClashVerdict {
    let counts = window.clash_counts;
    if counts.vocals > 0 {
        ClashVerdict::VocalClash
    } else if counts.bass > 0 {
        ClashVerdict::BassClash
    } else if counts.unknown > 0 {
        ClashVerdict::NotChecked
    } else {
        ClashVerdict::Clean
    }
}

/// `strip` shrunk to at most `cells` cells, each the worst clash of the bars
/// it stands for.
pub(crate) fn compressed_strip(strip: &[Clash], cells: usize) -> Vec<Clash> {
    if cells == 0 || strip.is_empty() {
        return Vec::new();
    }
    if strip.len() <= cells {
        return strip.to_vec();
    }
    let worst = |clash: &Clash| match clash {
        Clash::Vocals => 3,
        Clash::Bass => 2,
        Clash::Unknown => 1,
        Clash::Clear => 0,
    };
    (0..cells)
        .map(|cell| {
            let from = cell * strip.len() / cells;
            let to = ((cell + 1) * strip.len() / cells).max(from + 1);
            *strip[from..to]
                .iter()
                .max_by_key(|clash| worst(clash))
                .unwrap_or(&Clash::Clear)
        })
        .collect()
}

/// The name of a section as the focus view calls it.
pub(crate) fn section_name(label: SectionLabel) -> &'static str {
    match label {
        SectionLabel::Start => "Intro",
        SectionLabel::Main => "Main",
        SectionLabel::Breakdown => "Breakdown",
        SectionLabel::Drop => "Drop",
        SectionLabel::Outro => "Outro",
    }
}

/// Where B's key lies from A's on the wheel, in words.
pub(crate) fn wheel_phrase(
    from: Option<Key>,
    to: Option<Key>,
    relation: Option<Relation>,
) -> Option<String> {
    let (from, to) = (from?, to?);
    let Some(relation) = relation else {
        return Some("a key change the rules do not call smooth".to_string());
    };
    let steps = (i32::from(CamelotPosition::of(&to).number)
        - i32::from(CamelotPosition::of(&from).number))
    .rem_euclid(12);
    Some(match relation {
        Relation::Same => "the same key".to_string(),
        Relation::Relative => "the relative key".to_string(),
        Relation::Adjacent if steps == 1 => "one step up the wheel".to_string(),
        Relation::Adjacent => "one step down the wheel".to_string(),
        Relation::Diagonal if steps == 1 => "one step up and across the wheel".to_string(),
        Relation::Diagonal => "one step down and across the wheel".to_string(),
        Relation::EnergyBoost => "two steps up the wheel".to_string(),
        Relation::MoodChange => "the same note in the other mode".to_string(),
        Relation::SemitoneBoost => "a semitone up".to_string(),
    })
}

fn presence_in_window(
    planned: &PlannedTransition,
    window: &CandidateWindow,
    pick: fn(&euphoria_core::harmony::transition::BarLane) -> Presence,
) -> (bool, bool) {
    let sounds = |lanes: &euphoria_core::harmony::transition::TrackLanes, start: usize| {
        (0..window.bars).any(|offset| {
            lanes
                .bars
                .get(start + offset)
                .is_some_and(|bar| pick(bar) == Presence::Present)
        })
    };
    (
        sounds(&planned.plan.lanes_a, window.a_start_bar),
        sounds(&planned.plan.lanes_b, window.b_start_bar),
    )
}

fn stems_clause(planned: &PlannedTransition, window: &CandidateWindow) -> String {
    match planned.stems_known() {
        (false, false) => {
            "no stems on either track, so vocals and bass are not checked".to_string()
        }
        (false, true) => "A has no stems, so vocals and bass are not checked".to_string(),
        (true, false) => "B has no stems, so vocals and bass are not checked".to_string(),
        (true, true) => {
            let counts = window.clash_counts;
            let vocals = if counts.vocals > 0 {
                format!("{} bars of both vocals", counts.vocals)
            } else {
                match presence_in_window(planned, window, |bar| bar.vocals) {
                    (false, false) => "no vocals on either side".to_string(),
                    (true, false) => "vocals on A only".to_string(),
                    (false, true) => "vocals on B only".to_string(),
                    (true, true) => "vocals on both sides, never at once".to_string(),
                }
            };
            let bass = match (counts.bass, window.bass_swap_bar) {
                (0, _) => "no bass clash".to_string(),
                (_, Some(bar)) => format!("bass swap at bar {bar}"),
                (bars, None) => format!("{bars} bars of both bass, no clean swap"),
            };
            format!("{vocals}, {bass}")
        }
    }
}

/// The window in a sentence: where A mixes out into which part of B, how
/// the stems fit, and what B plays at.
pub(crate) fn window_summary(
    planned: &PlannedTransition,
    window: &CandidateWindow,
    b_title: &str,
    b_key_text: Option<String>,
) -> String {
    let lanes_a = &planned.plan.lanes_a;
    let lanes_b = &planned.plan.lanes_b;
    let a_clock = clock(
        lanes_a
            .bars
            .get(window.a_start_bar)
            .map_or(0.0, |bar| bar.start_seconds),
    );
    let b_clock = clock(
        lanes_b
            .bars
            .get(window.b_start_bar)
            .map_or(0.0, |bar| bar.start_seconds),
    );
    let from = match window.a_section {
        Some(label) => format!("this track's {} ({a_clock})", section_name(label)),
        None => format!("this track at {a_clock}"),
    };
    let into = match window.b_section {
        Some(label) => format!("{b_title}'s {}", section_name(label)),
        None => format!("{b_title} at {b_clock}"),
    };
    let alignment = &planned.plan.alignment;
    let mut tempo = format!(
        "{b_title} plays at {:.1} BPM ({})",
        alignment.bpm_a,
        crate::format::signed_percent(alignment.tempo_change_percent as f32)
    );
    if let Some(key) = b_key_text {
        tempo.push_str(&format!(", key {key}"));
        if let Some(phrase) = wheel_phrase(planned.a_key, planned.b_key, window.key_relation) {
            tempo.push_str(&format!(", {phrase}"));
        }
    }
    format!(
        "Mix out of {from} into {into}. {} bars, {}. {tempo}.",
        window.bars,
        stems_clause(planned, window)
    )
}

/// The title of a ranked window, such as "Outro (5:24) into Intro".
pub(crate) fn window_title(planned: &PlannedTransition, window: &CandidateWindow) -> String {
    let a_clock = clock(
        planned
            .plan
            .lanes_a
            .bars
            .get(window.a_start_bar)
            .map_or(0.0, |bar| bar.start_seconds),
    );
    let a_name = window.a_section.map_or("A", section_name);
    let b_name = window.b_section.map_or_else(
        || format!("bar {}", window.b_start_bar + 1),
        |label| section_name(label).to_string(),
    );
    format!("{a_name} ({a_clock}) into {b_name}")
}

/// The line under a ranked window's title.
pub(crate) fn window_detail(window: &CandidateWindow) -> String {
    let mut reason = window.reason.clone();
    if let Some(first) = reason.get(0..1) {
        reason = first.to_uppercase() + &reason[1..];
    }
    format!("{} bars. {reason}", window.bars)
}

/// What a step of a window says to do.
pub(crate) fn step_line(
    step: &euphoria_core::harmony::transition::Step,
    planned: &PlannedTransition,
    window: &CandidateWindow,
) -> String {
    let lanes_a = &planned.plan.lanes_a;
    let lanes_b = &planned.plan.lanes_b;
    let a_clock = clock(
        lanes_a
            .bars
            .get(step.a_bar)
            .map_or(0.0, |bar| bar.start_seconds),
    );
    let b_clock = clock(
        lanes_b
            .bars
            .get(step.b_bar)
            .map_or(0.0, |bar| bar.start_seconds),
    );
    let bar = step.window_bar;
    match step.kind {
        StepKind::StartB => {
            let on = window.a_section.map_or(String::new(), |label| {
                format!(" in A's {}", section_name(label))
            });
            format!("Bar {bar}: start B from {b_clock} on A's downbeat at {a_clock}{on}.")
        }
        StepKind::SwapBass => {
            format!("Bar {bar}: swap the basslines, B's bass in and A's out.")
        }
        StepKind::BringInVocals => format!("Bar {bar}: bring B's vocals in."),
        StepKind::AEnds => format!("Bar {bar}: A ends at {a_clock}."),
    }
}

/// The marks of the bar ruler of a window of `bars` bars, each with how
/// far along the shown bars it stands, from 0 to 1: bars from the 32nd
/// before the window to its end.
pub(crate) fn ruler_marks(bars: usize) -> Vec<(String, f32)> {
    let total = (BARS_BEFORE + bars + BARS_AFTER) as f32;
    let at = |offset: i32| (offset + BARS_BEFORE as i32) as f32 / total;
    let mut marks = vec![
        (
            format!("bar \u{2212}{BARS_BEFORE}"),
            at(-(BARS_BEFORE as i32)),
        ),
        (
            format!("bar \u{2212}{}", BARS_BEFORE / 2),
            at(-(BARS_BEFORE as i32 / 2)),
        ),
        ("bar 1".to_string(), at(0)),
    ];
    let mut bar = 9;
    while bar <= bars {
        marks.push((format!("bar {bar}"), at(bar as i32 - 1)));
        bar += 8;
    }
    marks.push((format!("bar {}", bars + 1), at(bars as i32)));
    marks
}

/// Why no plan could be made, in words.
pub(crate) fn failure_text(error: PlanError, a_title: &str, b_title: &str) -> String {
    match error {
        PlanError::NoBeatGrid(side) => {
            let title = match side {
                Side::A => a_title,
                Side::B => b_title,
            };
            format!(
                "No beat grid for {title}: the plan lays both tracks on bars, so analyze it \
                 again, or set its tempo."
            )
        }
        PlanError::TempoOutOfReach { rate } => format!(
            "Tempo out of reach: {b_title} would have to play at {rate:.2} times its speed to \
             match {a_title}, and a deck plays between {} and {} times.",
            euphoria_core::harmony::transition::MINIMUM_DECK_RATE,
            euphoria_core::harmony::transition::MAXIMUM_DECK_RATE,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(seconds: f64, name: &str, letter: Option<char>) -> Cue {
        Cue {
            seconds,
            name: name.to_string(),
            hot_cue: letter.and_then(HotCue::from_letter),
        }
    }

    #[test]
    fn a_verdict_names_the_worst_clash_and_unchecked_bars() {
        use euphoria_core::harmony::transition::ClashCounts;
        let window = |vocals, bass, unknown| {
            let mut window = sample_window();
            window.clash_counts = ClashCounts {
                vocals,
                bass,
                clear: 4,
                unknown,
            };
            window
        };
        assert_eq!(clash_verdict(&window(0, 0, 0)), ClashVerdict::Clean);
        assert_eq!(clash_verdict(&window(0, 3, 0)), ClashVerdict::BassClash);
        assert_eq!(clash_verdict(&window(2, 3, 0)), ClashVerdict::VocalClash);
        assert_eq!(clash_verdict(&window(0, 0, 4)), ClashVerdict::NotChecked);
        assert_eq!(ClashVerdict::VocalClash.label(), "Vocal clash");
        assert_eq!(ClashVerdict::NotChecked.label(), "Not checked");
    }

    fn sample_window() -> CandidateWindow {
        use euphoria_core::harmony::transition::{ClashCounts, ScoreBreakdown};
        CandidateWindow {
            rank: 1,
            bars: 16,
            a_start_bar: 0,
            b_start_bar: 0,
            a_section: None,
            b_section: None,
            score: 50.0,
            breakdown: ScoreBreakdown {
                clash: None,
                energy: None,
                key: None,
                room: 1.0,
            },
            clash_strip: Vec::new(),
            clash_counts: ClashCounts::default(),
            clean: true,
            key_relation: None,
            a_has_room: true,
            bass_swap_bar: None,
            vocals_in_bar: None,
            reason: "no vocal overlap".to_string(),
            steps: Vec::new(),
        }
    }

    #[test]
    fn a_long_strip_shrinks_to_the_worst_clash_of_each_cell() {
        let mut strip = vec![Clash::Clear; 32];
        strip[5] = Clash::Bass;
        strip[6] = Clash::Vocals;
        strip[30] = Clash::Unknown;
        let cells = compressed_strip(&strip, 16);
        assert_eq!(cells.len(), 16);
        assert_eq!(cells[3], Clash::Vocals);
        assert_eq!(cells[15], Clash::Unknown);
        assert_eq!(cells[0], Clash::Clear);
        assert_eq!(compressed_strip(&strip[..4], 16).len(), 4);
        assert!(compressed_strip(&[], 16).is_empty());
    }

    #[test]
    fn the_wheel_phrase_says_how_far_and_which_way() {
        let key = |text: &str| text.parse::<Key>().unwrap();
        let phrase = |from: &str, to: &str| {
            let (from, to) = (key(from), key(to));
            wheel_phrase(Some(from), Some(to), from.relation_to(&to)).unwrap()
        };
        assert_eq!(phrase("A minor", "E minor"), "one step up the wheel");
        assert_eq!(phrase("A minor", "D minor"), "one step down the wheel");
        assert_eq!(phrase("A minor", "A minor"), "the same key");
        assert_eq!(phrase("A minor", "C major"), "the relative key");
        assert_eq!(
            phrase("A minor", "F major"),
            "one step down and across the wheel"
        );
        assert_eq!(wheel_phrase(None, Some(key("A minor")), None), None);
    }

    #[test]
    fn the_ruler_runs_from_thirty_two_bars_before_to_the_end_of_the_window() {
        let marks = ruler_marks(32);
        let labels: Vec<&str> = marks.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "bar \u{2212}32",
                "bar \u{2212}16",
                "bar 1",
                "bar 9",
                "bar 17",
                "bar 25",
                "bar 33"
            ]
        );
        assert_eq!(marks[0].1, 0.0);
        assert!((marks[2].1 - 32.0 / 72.0).abs() < 1e-6);
        assert!((marks[6].1 - 64.0 / 72.0).abs() < 1e-6);
    }

    #[test]
    fn setting_cues_again_replaces_the_cues_of_the_same_name_and_fills_free_pads() {
        let existing = vec![
            cue(0.0, "Start", Some('A')),
            cue(40.0, "Mix out", Some('B')),
            cue(60.0, "Drop", Some('C')),
        ];
        let suggested = vec![cue(48.0, "Mix out", None), cue(80.0, "Mix out end", None)];
        let merged = merged_cues(&existing, &suggested);
        let listed: Vec<(f64, &str, Option<char>)> = merged
            .iter()
            .map(|cue| {
                (
                    cue.seconds,
                    cue.name.as_str(),
                    cue.hot_cue.map(|pad| pad.letter()),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                (0.0, "Start", Some('A')),
                (48.0, "Mix out", Some('B')),
                (60.0, "Drop", Some('C')),
                (80.0, "Mix out end", Some('D')),
            ]
        );
    }

    #[test]
    fn sections_are_the_named_cues_on_the_bars_of_the_grid() {
        let grid = BeatGrid {
            first_downbeat_seconds: 1.0,
            bpm: 120.0,
            salience: 10.0,
            drift_seconds: None,
        };
        let cues = [
            cue(1.0, "Start", None),
            cue(9.0, "My cue", None),
            cue(33.0, "Drop", None),
        ];
        let sections = plan_sections(&cues, &grid);
        assert_eq!(
            sections,
            [
                PlanSection {
                    start_bar: 0,
                    label: SectionLabel::Start
                },
                PlanSection {
                    start_bar: 16,
                    label: SectionLabel::Drop
                }
            ]
        );
    }

    #[test]
    fn a_window_detail_starts_with_a_capital_and_names_the_length() {
        assert_eq!(window_detail(&sample_window()), "16 bars. No vocal overlap");
    }

    #[test]
    fn a_failure_is_told_in_words() {
        let text = failure_text(PlanError::TempoOutOfReach { rate: 2.4 }, "Alpha", "Beta");
        assert!(text.starts_with("Tempo out of reach: Beta would have to play at 2.40 times"));
        assert!(text.contains("between 0.5 and 2 times"));
        let text = failure_text(PlanError::NoBeatGrid(Side::B), "Alpha", "Beta");
        assert!(text.starts_with("No beat grid for Beta"));
    }
}
