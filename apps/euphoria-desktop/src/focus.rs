// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The focus view: one track given the whole window, as stacked lanes on a
//! single time axis, with the tracks that mix out of it and a place to plan
//! the transition. This module holds what the view remembers and the
//! arithmetic behind it; `views::focus` draws it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use euphoria_core::stems::{Stem, StemActivity, StemCache, stem_activity};
use euphoria_core::{Cue, CueSection};
use gpui::Context;
use serde::{Deserialize, Serialize};

use crate::app::EuphoriaApp;
use crate::library::Track;
use crate::track_overview::{EnergyCurve, EnergyStep};
use crate::views::waveform::SeenBounds;
use crate::waveforms::Waveform;

/// How many times closer than the whole track the detail can zoom. The
/// waveform keeps 720 columns for a track, so deeper than this the bars
/// would only get wider.
const ZOOM_LIMIT: f64 = 6.0;

/// The detail shows at least this many seconds when the track is longer
/// than four times that.
const DEFAULT_SPAN_SECONDS: f64 = 30.0;

/// One of the lanes of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Overview,
    Sections,
    Detail,
    Energy,
    Vocals,
    Stems,
}

impl Lane {
    /// Every lane, top to bottom.
    pub(crate) const ALL: [Lane; 6] = [
        Lane::Overview,
        Lane::Sections,
        Lane::Detail,
        Lane::Energy,
        Lane::Vocals,
        Lane::Stems,
    ];

    /// The name in the Lanes menu and beside the lane.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Lane::Overview => "Overview",
            Lane::Sections => "Sections",
            Lane::Detail => "Detail",
            Lane::Energy => "Energy",
            Lane::Vocals => "Vocals",
            Lane::Stems => "Stems",
        }
    }

    /// Where the lane sits in [`Lane::ALL`].
    pub(crate) fn number(self) -> usize {
        self as usize
    }
}

/// Which lanes of the focus view are shown. Kept in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct FocusLanes {
    pub(crate) overview: bool,
    pub(crate) sections: bool,
    pub(crate) detail: bool,
    pub(crate) energy: bool,
    pub(crate) vocals: bool,
    pub(crate) stems: bool,
}

impl Default for FocusLanes {
    fn default() -> Self {
        Self {
            overview: true,
            sections: true,
            detail: true,
            energy: true,
            vocals: true,
            stems: true,
        }
    }
}

impl FocusLanes {
    pub(crate) fn is_shown(&self, lane: Lane) -> bool {
        *self.flag(lane)
    }

    pub(crate) fn toggle(&mut self, lane: Lane) {
        let flag = self.flag_mut(lane);
        *flag = !*flag;
    }

    fn flag(&self, lane: Lane) -> &bool {
        match lane {
            Lane::Overview => &self.overview,
            Lane::Sections => &self.sections,
            Lane::Detail => &self.detail,
            Lane::Energy => &self.energy,
            Lane::Vocals => &self.vocals,
            Lane::Stems => &self.stems,
        }
    }

    fn flag_mut(&mut self, lane: Lane) -> &mut bool {
        match lane {
            Lane::Overview => &mut self.overview,
            Lane::Sections => &mut self.sections,
            Lane::Detail => &mut self.detail,
            Lane::Energy => &mut self.energy,
            Lane::Vocals => &mut self.vocals,
            Lane::Stems => &mut self.stems,
        }
    }
}

/// The tabs of the plan panel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PlanTab {
    #[default]
    Transition,
    Mashup,
    Cues,
}

impl PlanTab {
    pub(crate) const ALL: [PlanTab; 3] = [PlanTab::Transition, PlanTab::Mashup, PlanTab::Cues];

    pub(crate) fn label(self) -> &'static str {
        match self {
            PlanTab::Transition => "Transition",
            PlanTab::Mashup => "Mashup",
            PlanTab::Cues => "Cues",
        }
    }
}

/// The kind of a part of a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SectionKind {
    Intro,
    Main,
    Breakdown,
    Drop,
    Outro,
}

impl SectionKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SectionKind::Intro => "Intro",
            SectionKind::Main => "Main",
            SectionKind::Breakdown => "Breakdown",
            SectionKind::Drop => "Drop",
            SectionKind::Outro => "Outro",
        }
    }
}

impl From<CueSection> for SectionKind {
    fn from(section: CueSection) -> Self {
        match section {
            CueSection::Start => SectionKind::Intro,
            CueSection::Main => SectionKind::Main,
            CueSection::Breakdown => SectionKind::Breakdown,
            CueSection::Drop => SectionKind::Drop,
            CueSection::Outro => SectionKind::Outro,
        }
    }
}

/// A named stretch of a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SectionBand {
    pub(crate) start_seconds: f64,
    pub(crate) end_seconds: f64,
    pub(crate) kind: SectionKind,
}

/// Where the sections of a track come from. The lane and the tint under
/// the detail read only this, so a better section detector replaces
/// [`CueSections`] and nothing else.
pub(crate) trait SectionSource {
    /// The sections of `track` in order, given the cues as drawn now and
    /// the track's length.
    fn sections(
        &self,
        track: &Track,
        shown_cues: &[Cue],
        duration_seconds: f64,
    ) -> Vec<SectionBand>;
}

/// Sections read off the automatic cues: each cue named Start, Main,
/// Breakdown, Drop or Outro begins a section that lasts until the next
/// such cue, or the end of the track.
pub(crate) struct CueSections;

impl SectionSource for CueSections {
    fn sections(
        &self,
        _track: &Track,
        shown_cues: &[Cue],
        duration_seconds: f64,
    ) -> Vec<SectionBand> {
        bands_from_cues(shown_cues, duration_seconds)
    }
}

fn bands_from_cues(cues: &[Cue], duration_seconds: f64) -> Vec<SectionBand> {
    let starts: Vec<(f64, SectionKind)> = cues
        .iter()
        .filter_map(|cue| {
            CueSection::from_name(&cue.name).map(|section| (cue.seconds, section.into()))
        })
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(position, (start, kind))| SectionBand {
            start_seconds: *start,
            end_seconds: starts
                .get(position + 1)
                .map_or(duration_seconds, |next| next.0),
            kind: *kind,
        })
        .filter(|band| band.end_seconds > band.start_seconds)
        .collect()
}

/// The part of the track the detail shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Viewport {
    pub(crate) start: f64,
    pub(crate) span: f64,
}

impl Viewport {
    /// What the detail shows when the view opens on a track of
    /// `duration` seconds.
    pub(crate) fn initial(duration: f64) -> Self {
        let span = (duration / 4.0).max(DEFAULT_SPAN_SECONDS).min(duration);
        Self { start: 0.0, span }.clamped(duration)
    }

    fn shortest_span(duration: f64) -> f64 {
        duration / ZOOM_LIMIT
    }

    pub(crate) fn end(&self) -> f64 {
        self.start + self.span
    }

    /// Kept within the track and within the zoom limit.
    pub(crate) fn clamped(self, duration: f64) -> Self {
        if duration <= 0.0 {
            return Self {
                start: 0.0,
                span: 0.0,
            };
        }
        let span = self.span.clamp(Self::shortest_span(duration), duration);
        let start = self.start.clamp(0.0, duration - span);
        Self { start, span }
    }

    /// How far along the detail `seconds` is, from 0.0 to 1.0 inside the
    /// view.
    pub(crate) fn fraction_of(&self, seconds: f64) -> f32 {
        if self.span <= 0.0 {
            return 0.0;
        }
        ((seconds - self.start) / self.span) as f32
    }

    pub(crate) fn seconds_at(&self, fraction: f32) -> f64 {
        self.start + f64::from(fraction) * self.span
    }

    pub(crate) fn contains(&self, seconds: f64) -> bool {
        seconds >= self.start && seconds <= self.end()
    }

    /// Zoomed by `factor` (below 1.0 closer in), keeping the time at
    /// `anchor_fraction` of the view where it is.
    pub(crate) fn zoomed(self, factor: f64, anchor_fraction: f32, duration: f64) -> Self {
        let anchor = self.seconds_at(anchor_fraction);
        let span = self.span * factor;
        Self {
            start: anchor - f64::from(anchor_fraction) * span,
            span,
        }
        .clamped(duration)
    }

    pub(crate) fn panned(self, seconds: f64, duration: f64) -> Self {
        Self {
            start: self.start + seconds,
            span: self.span,
        }
        .clamped(duration)
    }

    /// Moved so that `seconds` is its middle.
    pub(crate) fn centered_on(self, seconds: f64, duration: f64) -> Self {
        Self {
            start: seconds - self.span / 2.0,
            span: self.span,
        }
        .clamped(duration)
    }

    /// Paged on when `seconds` has left the view, so it sits a tenth in.
    pub(crate) fn following(self, seconds: f64, duration: f64) -> Self {
        if self.contains(seconds) {
            return self;
        }
        Self {
            start: seconds - self.span * 0.1,
            span: self.span,
        }
        .clamped(duration)
    }
}

/// The section `seconds` falls in.
pub(crate) fn section_at(sections: &[SectionBand], seconds: f64) -> Option<&SectionBand> {
    sections
        .iter()
        .find(|band| seconds >= band.start_seconds && seconds < band.end_seconds)
}

/// The energy rating of the step at `fraction` of the track.
pub(crate) fn energy_at(steps: &[EnergyStep], fraction: f32) -> Option<u8> {
    steps
        .iter()
        .find(|step| fraction >= step.start && fraction < step.end)
        .and_then(|step| step.rating)
}

/// The time as minutes and whole seconds, rounded down, as a clock reads.
pub(crate) fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// What the line under the lanes says about the moment under the cursor,
/// from whichever lanes have data, such as "2:51 · Breakdown · Energy 3 ·
/// no vocals · bass out".
pub(crate) fn readout(
    seconds: f64,
    sections: &[SectionBand],
    energy: Option<u8>,
    stems: Option<&StemActivity>,
) -> String {
    let mut parts = vec![clock(seconds)];
    if let Some(band) = section_at(sections, seconds) {
        parts.push(band.kind.label().to_string());
    }
    if let Some(rating) = energy {
        parts.push(format!("Energy {rating}"));
    }
    if let Some(activity) = stems {
        match activity.is_active_at(Stem::Vocals, seconds) {
            Some(true) => parts.push("vocals".to_string()),
            Some(false) => parts.push("no vocals".to_string()),
            None => {}
        }
        for stem in [Stem::Drums, Stem::Bass, Stem::Other] {
            if activity.is_active_at(stem, seconds) == Some(false) {
                parts.push(format!("{} out", stem.name()));
            }
        }
    }
    parts.join(" \u{b7} ")
}

/// How the verdict on a mix reads, from its key and its tempo alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MixVerdict {
    Smooth,
    Workable,
    Stretch,
    YourMix,
}

impl MixVerdict {
    pub(crate) fn label(self) -> &'static str {
        match self {
            MixVerdict::Smooth => "Smooth",
            MixVerdict::Workable => "Workable",
            MixVerdict::Stretch => "Stretch",
            MixVerdict::YourMix => "Your mix",
        }
    }
}

/// The verdict for a mix with this key relation and tempo change. A
/// transition the user marked, whose keys do not mix by the rules, is
/// theirs.
pub(crate) fn mix_verdict(
    relation: Option<euphoria_core::harmony::Relation>,
    tempo_difference_percent: Option<f32>,
) -> MixVerdict {
    use euphoria_core::harmony::Relation;
    let Some(relation) = relation else {
        return MixVerdict::YourMix;
    };
    let key_cost = match relation {
        Relation::Same | Relation::Adjacent | Relation::Relative => 0,
        Relation::Diagonal => 1,
        Relation::EnergyBoost | Relation::MoodChange | Relation::SemitoneBoost => 2,
    };
    let tempo_cost = match tempo_difference_percent.map(f32::abs) {
        Some(difference) if difference > 4.0 => 2,
        Some(difference) if difference > 2.0 => 1,
        _ => 0,
    };
    match key_cost.max(tempo_cost) {
        0 => MixVerdict::Smooth,
        1 => MixVerdict::Workable,
        _ => MixVerdict::Stretch,
    }
}

/// Where the stems of a track stand for the stems lane.
#[derive(Debug, Clone)]
pub(crate) enum StemsLane {
    NotSeparated,
    Ready(Arc<StemActivity>),
}

/// What the focus view remembers.
pub(crate) struct FocusView {
    pub(crate) is_open: bool,
    pub(crate) viewport: Viewport,
    /// The time under the pointer, over the lanes.
    pub(crate) hover_seconds: Option<f64>,
    pub(crate) plan_tab: PlanTab,
    pub(crate) lanes_menu_open: bool,
    pub(crate) lanes: FocusLanes,
    /// The track, as an index into the library, picked in the mixes list.
    pub(crate) selected_match: Option<usize>,
    /// Where the time axis shared by the lanes was last drawn.
    pub(crate) axis_bounds: SeenBounds,
    /// Where the overview was last drawn.
    pub(crate) overview_bounds: SeenBounds,
    pub(crate) is_moving_window: bool,
    stems: HashMap<PathBuf, StemsLane>,
    checking_stems: HashSet<PathBuf>,
}

impl FocusView {
    pub(crate) fn new(lanes: FocusLanes) -> Self {
        Self {
            is_open: false,
            viewport: Viewport {
                start: 0.0,
                span: 0.0,
            },
            hover_seconds: None,
            plan_tab: PlanTab::default(),
            lanes_menu_open: false,
            lanes,
            selected_match: None,
            axis_bounds: SeenBounds::default(),
            overview_bounds: SeenBounds::default(),
            is_moving_window: false,
            stems: HashMap::new(),
            checking_stems: HashSet::new(),
        }
    }

    pub(crate) fn stems_of(&self, path: &Path) -> Option<&StemsLane> {
        self.stems.get(path)
    }

    /// The time under `pointer_x` along the detail.
    pub(crate) fn seconds_under(&self, pointer_x: gpui::Pixels) -> Option<f64> {
        let bounds = self.axis_bounds.get()?;
        let fraction = crate::player::fraction_along(
            pointer_x / gpui::px(1.0),
            bounds.left() / gpui::px(1.0),
            bounds.size.width / gpui::px(1.0),
        );
        Some(self.viewport.seconds_at(fraction))
    }
}

impl EuphoriaApp {
    fn selected_duration(&self) -> f64 {
        self.selected_track()
            .and_then(Track::duration_seconds)
            .map_or(0.0, f64::from)
    }

    /// Gives the selected track the whole window.
    pub(crate) fn open_focus_view(&mut self, cx: &mut Context<'_, Self>) {
        if self.focus.is_open || self.selected_track().is_none() {
            return;
        }
        let duration = self.selected_duration();
        let is_playing_selected =
            self.selected_track().map(|track| track.path.as_path()) == self.player.track();
        let mut viewport = Viewport::initial(duration);
        if is_playing_selected {
            viewport = viewport.following(f64::from(self.player.position_seconds()), duration);
        }
        self.focus.viewport = viewport;
        self.focus.is_open = true;
        self.focus.hover_seconds = None;
        self.focus.lanes_menu_open = false;
        self.focus.selected_match = self.mixes.first().map(|mix| mix.track_index);
        self.load_focus_stems(cx);
        cx.notify();
    }

    /// Back to the library, with the same track selected.
    pub(crate) fn close_focus_view(&mut self, cx: &mut Context<'_, Self>) {
        if !self.focus.is_open {
            return;
        }
        self.focus.is_open = false;
        self.focus.is_moving_window = false;
        self.focus.hover_seconds = None;
        self.focus.lanes_menu_open = false;
        self.cue_editor.forget_selection();
        cx.notify();
    }

    pub(crate) fn toggle_focus_lanes_menu(&mut self, cx: &mut Context<'_, Self>) {
        self.focus.lanes_menu_open = !self.focus.lanes_menu_open;
        cx.notify();
    }

    pub(crate) fn toggle_focus_lane(&mut self, lane: Lane, cx: &mut Context<'_, Self>) {
        self.focus.lanes.toggle(lane);
        if let Some(problem) = self.save_settings() {
            self.report(problem);
        }
        cx.notify();
    }

    pub(crate) fn set_plan_tab(&mut self, tab: PlanTab, cx: &mut Context<'_, Self>) {
        self.focus.plan_tab = tab;
        cx.notify();
    }

    pub(crate) fn zoom_focus(
        &mut self,
        factor: f64,
        anchor_fraction: f32,
        cx: &mut Context<'_, Self>,
    ) {
        let duration = self.selected_duration();
        self.focus.viewport = self
            .focus
            .viewport
            .zoomed(factor, anchor_fraction, duration);
        cx.notify();
    }

    pub(crate) fn pan_focus(&mut self, seconds: f64, cx: &mut Context<'_, Self>) {
        let duration = self.selected_duration();
        self.focus.viewport = self.focus.viewport.panned(seconds, duration);
        cx.notify();
    }

    /// The wheel over the detail: with Command (Control elsewhere) it zooms
    /// about the pointer, otherwise it moves the view along the track.
    pub(crate) fn scroll_focus(&mut self, event: &gpui::ScrollEvent, cx: &mut Context<'_, Self>) {
        let Some(bounds) = self.focus.axis_bounds.get() else {
            return;
        };
        let width = bounds.size.width / gpui::px(1.0);
        if width <= 0.0 {
            return;
        }
        let delta = event.delta.pixel_delta(gpui::px(16.0));
        let (across, down) = (delta.x / gpui::px(1.0), delta.y / gpui::px(1.0));
        if event.modifiers.secondary(cx) {
            let anchor = crate::player::fraction_along(
                event.position.x / gpui::px(1.0),
                bounds.left() / gpui::px(1.0),
                width,
            );
            let factor = (1.0 - f64::from(down) / 200.0).clamp(0.5, 2.0);
            self.zoom_focus(factor, anchor, cx);
        } else {
            let along = if across != 0.0 { across } else { down };
            let seconds = -f64::from(along) / f64::from(width) * self.focus.viewport.span;
            self.pan_focus(seconds, cx);
        }
    }

    /// The pointer is over the lanes at `pointer_x`, or has left them.
    pub(crate) fn hover_focus(
        &mut self,
        pointer_x: Option<gpui::Pixels>,
        cx: &mut Context<'_, Self>,
    ) {
        let on_axis = pointer_x.filter(|x| {
            self.focus
                .axis_bounds
                .get()
                .is_some_and(|bounds| bounds.left() <= *x && *x <= bounds.right())
        });
        let seconds = on_axis.and_then(|x| self.focus.seconds_under(x));
        let inside = seconds.filter(|seconds| self.focus.viewport.contains(*seconds));
        if inside != self.focus.hover_seconds {
            self.focus.hover_seconds = inside;
            cx.notify();
        }
    }

    /// A press on the overview moves the visible window there.
    pub(crate) fn start_moving_window(
        &mut self,
        pointer_x: gpui::Pixels,
        cx: &mut Context<'_, Self>,
    ) {
        self.focus.is_moving_window = true;
        self.move_window_to(pointer_x, cx);
    }

    /// The pointer moved with the button held: a window being moved follows
    /// it. Returns false when none is.
    pub(crate) fn drag_window_to(
        &mut self,
        pointer_x: gpui::Pixels,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        if !self.focus.is_moving_window {
            return false;
        }
        self.move_window_to(pointer_x, cx);
        true
    }

    pub(crate) fn finish_moving_window(&mut self) {
        self.focus.is_moving_window = false;
    }

    fn move_window_to(&mut self, pointer_x: gpui::Pixels, cx: &mut Context<'_, Self>) {
        let Some(bounds) = self.focus.overview_bounds.get() else {
            return;
        };
        let duration = self.selected_duration();
        let fraction = crate::player::fraction_along(
            pointer_x / gpui::px(1.0),
            bounds.left() / gpui::px(1.0),
            bounds.size.width / gpui::px(1.0),
        );
        self.focus.viewport = self
            .focus
            .viewport
            .centered_on(f64::from(fraction) * duration, duration);
        cx.notify();
    }

    /// A click on the detail plays from that time.
    pub(crate) fn seek_in_detail(&mut self, pointer_x: gpui::Pixels, cx: &mut Context<'_, Self>) {
        let duration = self.selected_duration();
        let Some(seconds) = self.focus.seconds_under(pointer_x) else {
            return;
        };
        if duration <= 0.0 {
            return;
        }
        let request = self
            .player
            .seek_to_fraction((seconds / duration).clamp(0.0, 1.0) as f32);
        self.carry_out(request, cx);
        cx.notify();
    }

    /// Keeps the playhead in the detail while the view is open.
    pub(crate) fn follow_playhead_in_focus(&mut self) {
        if !self.focus.is_open || !self.player.is_playing() {
            return;
        }
        let duration = self.selected_duration();
        self.focus.viewport = self
            .focus
            .viewport
            .following(f64::from(self.player.position_seconds()), duration);
    }

    /// The energy ratings of the selected track, when they are worked out.
    pub(crate) fn focus_energy_steps(&self) -> Option<&[EnergyStep]> {
        let path = &self.selected_track()?.path;
        match self.waveform_of(path)? {
            Waveform::Ready(_, Some(EnergyCurve::Steps(steps))) => Some(steps),
            _ => None,
        }
    }

    /// The line the lanes show for the moment under the cursor.
    pub(crate) fn focus_readout(&self) -> Option<String> {
        let seconds = self.focus.hover_seconds?;
        let track = self.selected_track()?;
        let duration = f64::from(track.duration_seconds()?);
        let cues = crate::views::cues::shown_cues(track, self);
        let sections = CueSections.sections(track, &cues, duration);
        let energy = self
            .focus_energy_steps()
            .and_then(|steps| energy_at(steps, (seconds / duration) as f32));
        let stems = match self.focus.stems_of(&track.path) {
            Some(StemsLane::Ready(activity)) => Some(activity.as_ref()),
            _ => None,
        };
        Some(readout(seconds, &sections, energy, stems))
    }

    /// Looks for the stems of the selected track, in the session and in
    /// the stem cache, and measures where each sounds, off the main thread.
    /// A track already looked at is not looked at again until its stems are
    /// separated.
    pub(crate) fn load_focus_stems(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        if matches!(self.focus.stems.get(&path), Some(StemsLane::Ready(_)))
            || self.focus.checking_stems.contains(&path)
        {
            return;
        }
        let separated = self
            .stems
            .ready_files(&path)
            .or_else(|| self.decks.stem_library.files(&path));
        let analyzer = self.session.cache();
        self.focus.checking_stems.insert(path.clone());
        let task_path = path.clone();
        cx.spawn(async move |this, cx| {
            let measured = cx
                .background_executor()
                .spawn(async move {
                    let files = separated.or_else(|| {
                        let content = analyzer.as_ref()?.known_content(&task_path)?;
                        StemCache::open_default().ok()?.cached(&content)
                    })?;
                    stem_activity(&files).ok()
                })
                .await;
            this.update(cx, |this, cx| {
                this.focus.checking_stems.remove(&path);
                this.focus.stems.insert(
                    path,
                    match measured {
                        Some(activity) => StemsLane::Ready(Arc::new(activity)),
                        None => StemsLane::NotSeparated,
                    },
                );
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Another track was selected while the view is open: the lanes now show
    /// it, from the start, with the matches it has.
    pub(crate) fn focus_selection_changed(&mut self, cx: &mut Context<'_, Self>) {
        let duration = self.selected_duration();
        self.focus.viewport = Viewport::initial(duration);
        self.focus.hover_seconds = None;
        self.focus.selected_match = self.mixes.first().map(|mix| mix.track_index);
        self.load_focus_stems(cx);
    }

    /// The stems of the track at `path` were separated just now.
    pub(crate) fn focus_stems_separated(&mut self, path: &Path, cx: &mut Context<'_, Self>) {
        self.focus.stems.remove(path);
        if self.selected_track().map(|track| track.path.as_path()) == Some(path) {
            self.load_focus_stems(cx);
        }
    }

    /// Opens the decks in mashup mode, which finds tracks to layer over this
    /// one, and leaves the focus view.
    pub(crate) fn open_mashup_from_focus(&mut self, cx: &mut Context<'_, Self>) {
        self.close_focus_view(cx);
        if !self.decks.is_open {
            self.toggle_decks(cx);
        }
        self.set_deck_mode(crate::decks::DeckMode::Mashup, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(seconds: f64, name: &str) -> Cue {
        Cue {
            seconds,
            name: name.to_string(),
            hot_cue: None,
        }
    }

    #[test]
    fn sections_run_from_each_named_cue_to_the_next_and_ignore_other_cues() {
        let cues = [
            cue(0.0, "Start"),
            cue(30.0, "My cue"),
            cue(64.0, "Breakdown"),
            cue(96.0, "Drop"),
        ];
        let bands = bands_from_cues(&cues, 200.0);
        assert_eq!(
            bands
                .iter()
                .map(|band| (band.start_seconds, band.end_seconds, band.kind.label()))
                .collect::<Vec<_>>(),
            [
                (0.0, 64.0, "Intro"),
                (64.0, 96.0, "Breakdown"),
                (96.0, 200.0, "Drop")
            ]
        );
        assert_eq!(
            section_at(&bands, 70.0).unwrap().kind,
            SectionKind::Breakdown
        );
        assert!(section_at(&bands, 200.0).is_none());
    }

    #[test]
    fn the_viewport_stays_inside_the_track_and_the_zoom_limit() {
        let duration = 360.0;
        let initial = Viewport::initial(duration);
        assert_eq!(initial.span, 90.0);
        let closest = initial.zoomed(0.01, 0.5, duration);
        assert_eq!(closest.span, 60.0);
        let widest = initial.zoomed(100.0, 0.5, duration);
        assert_eq!((widest.start, widest.span), (0.0, 360.0));
        let panned = initial.panned(1000.0, duration);
        assert_eq!(panned.end(), 360.0);
        let centered = initial.centered_on(180.0, duration);
        assert_eq!(centered.start, 135.0);
        assert_eq!(Viewport::initial(20.0).span, 20.0);
        assert_eq!(Viewport::initial(0.0).span, 0.0);
    }

    #[test]
    fn zooming_keeps_the_time_under_the_pointer_in_place() {
        let view = Viewport {
            start: 100.0,
            span: 90.0,
        };
        let zoomed = view.zoomed(0.5, 0.25, 360.0);
        assert!((zoomed.seconds_at(0.25) - view.seconds_at(0.25)).abs() < 1e-9);
    }

    #[test]
    fn the_view_pages_on_when_the_playhead_leaves_it() {
        let view = Viewport {
            start: 0.0,
            span: 90.0,
        };
        assert_eq!(view.following(50.0, 360.0), view);
        assert_eq!(view.following(200.0, 360.0).start, 191.0);
    }

    #[test]
    fn the_readout_names_only_what_the_lanes_know() {
        let bands = [SectionBand {
            start_seconds: 100.0,
            end_seconds: 200.0,
            kind: SectionKind::Breakdown,
        }];
        assert_eq!(
            readout(171.9, &bands, Some(3), None),
            "2:51 \u{b7} Breakdown \u{b7} Energy 3"
        );
        assert_eq!(readout(5.0, &[], None, None), "0:05");
    }

    #[test]
    fn verdicts_follow_the_key_relation_and_the_tempo_change() {
        use euphoria_core::harmony::Relation;
        assert_eq!(
            mix_verdict(Some(Relation::Same), Some(0.5)),
            MixVerdict::Smooth
        );
        assert_eq!(
            mix_verdict(Some(Relation::Diagonal), Some(0.5)),
            MixVerdict::Workable
        );
        assert_eq!(
            mix_verdict(Some(Relation::Same), Some(-3.0)),
            MixVerdict::Workable
        );
        assert_eq!(
            mix_verdict(Some(Relation::Adjacent), Some(5.0)),
            MixVerdict::Stretch
        );
        assert_eq!(
            mix_verdict(Some(Relation::SemitoneBoost), None),
            MixVerdict::Stretch
        );
        assert_eq!(mix_verdict(None, Some(9.0)), MixVerdict::YourMix);
    }

    #[test]
    fn lanes_toggle_and_serialize_with_every_lane_shown_by_default() {
        let mut lanes = FocusLanes::default();
        assert!(Lane::ALL.iter().all(|lane| lanes.is_shown(*lane)));
        lanes.toggle(Lane::Stems);
        assert!(!lanes.is_shown(Lane::Stems));
        let json = serde_json::to_string(&lanes).unwrap();
        let back: FocusLanes = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lanes);
        let older: FocusLanes = serde_json::from_str("{}").unwrap();
        assert_eq!(older, FocusLanes::default());
    }
}
