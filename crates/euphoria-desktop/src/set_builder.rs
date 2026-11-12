// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The set builder: an energy curve drawn by hand, a pool of tracks, and
//! the set ordered from them, which can be reordered, locked, built again
//! around the locks, saved as a playlist and exported.

use std::path::{Path, PathBuf};

use euphoria_core::harmony::set::{HIGHEST_ENERGY, LOWEST_ENERGY};
use euphoria_core::{
    CurvePreset, EnergyCurve, Key, MixTrack, Pin, PinPosition, SetOptions, SetPlan, SetTrack,
    build_set, evaluate_set,
};
use gpui::{Context, Task};

use crate::app::EuphoriaApp;
use crate::playlist_flow::PlaylistFormat;
use crate::playlists::PlaylistId;
use crate::views::waveform::SeenBounds;

/// How many points the drawn curve is kept as, spread evenly over the set.
pub(crate) const CURVE_SAMPLES: usize = 48;

pub(crate) const DEFAULT_TRACK_COUNT: usize = 20;
pub(crate) const FEWEST_TRACKS: usize = 2;
pub(crate) const MOST_TRACKS: usize = 60;

/// Where the tracks of the set come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetPool {
    AllTracks,
    Folder(PathBuf),
    Playlist(PlaylistId),
}

/// A set that was built: its tracks in order, which are locked in place,
/// and how each meets the curve and mixes into the next.
#[derive(Debug, Clone)]
pub(crate) struct PlannedSet {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) locked: Vec<bool>,
    /// Explained over `paths` as the pool, so `plan.tracks[position]` is
    /// the track at `paths[position]`.
    pub(crate) plan: SetPlan,
    /// The curve the set was built for.
    pub(crate) curve: Vec<f32>,
}

/// Everything the set builder shows and remembers while the app runs.
pub(crate) struct SetBuilder {
    pub(crate) is_open: bool,
    /// The energy asked for, from the start of the set to its end.
    pub(crate) curve: Vec<f32>,
    /// The preset the curve is, until it is drawn over.
    pub(crate) preset: Option<CurvePreset>,
    pub(crate) track_count: usize,
    pub(crate) pool: SetPool,
    pub(crate) planned: Option<PlannedSet>,
    /// Why the last attempt to build a set gave nothing.
    pub(crate) problem: Option<String>,
    pub(crate) is_building: bool,
    /// Where the curve was last drawn, for turning the pointer into a level.
    pub(crate) curve_bounds: SeenBounds,
    /// The point the pointer last drew, as (fraction along, level), while
    /// the button is held.
    pub(crate) stroke: Option<(f32, f32)>,
    /// The playlist the set was last saved as.
    pub(crate) saved_as: Option<PlaylistId>,
    build_generation: u64,
    build_task: Option<Task<()>>,
}

impl Default for SetBuilder {
    fn default() -> Self {
        Self {
            is_open: false,
            curve: preset_samples(CurvePreset::Journey),
            preset: Some(CurvePreset::Journey),
            track_count: DEFAULT_TRACK_COUNT,
            pool: SetPool::AllTracks,
            planned: None,
            problem: None,
            is_building: false,
            curve_bounds: SeenBounds::default(),
            stroke: None,
            saved_as: None,
            build_generation: 0,
            build_task: None,
        }
    }
}

/// The curve of `preset`, as [`CURVE_SAMPLES`] points.
pub(crate) fn preset_samples(preset: CurvePreset) -> Vec<f32> {
    resample(&EnergyCurve::preset(preset))
}

fn resample(curve: &EnergyCurve) -> Vec<f32> {
    (0..CURVE_SAMPLES)
        .map(|index| curve.target_at(index as f32 / (CURVE_SAMPLES - 1) as f32))
        .collect()
}

/// `samples` with each inner point averaged with its neighbours, so a hand
/// drawn stroke loses its jitter. The ends keep their level.
pub(crate) fn smoothed(samples: &[f32]) -> Vec<f32> {
    let mut result = samples.to_vec();
    for index in 1..samples.len().saturating_sub(1) {
        result[index] = (samples[index - 1] + 2.0 * samples[index] + samples[index + 1]) / 4.0;
    }
    result
}

/// The energy level at `fraction` of the height of the curve, from the
/// top: 10 at the top, 1 at the bottom.
pub(crate) fn level_at_height(fraction_down: f32) -> f32 {
    let fraction_up = 1.0 - fraction_down.clamp(0.0, 1.0);
    LOWEST_ENERGY + fraction_up * (HIGHEST_ENERGY - LOWEST_ENERGY)
}

/// A track of the pool, copied out of the library so the search can run
/// on another thread.
#[derive(Debug, Clone)]
pub(crate) struct PoolEntry {
    key: Option<Key>,
    bpm: Option<f32>,
    energy: Option<f32>,
    artist: Option<String>,
}

impl MixTrack for PoolEntry {
    fn key(&self) -> Option<Key> {
        self.key
    }

    fn bpm(&self) -> Option<f32> {
        self.bpm
    }

    fn energy(&self) -> Option<f32> {
        self.energy
    }
}

impl SetTrack for PoolEntry {
    fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }
}

impl EuphoriaApp {
    pub(crate) fn open_set_builder(&mut self, cx: &mut Context<'_, Self>) {
        let builder = &mut self.set_builder;
        builder.is_open = true;
        if builder.planned.is_none() {
            builder.pool = match (&self.playlist, &self.folder) {
                (Some(playlist), _) => SetPool::Playlist(*playlist),
                (None, Some(folder)) => SetPool::Folder(folder.clone()),
                (None, None) => SetPool::AllTracks,
            };
        }
        cx.notify();
    }

    pub(crate) fn close_set_builder(&mut self) {
        self.set_builder.is_open = false;
        self.set_builder.stroke = None;
    }

    /// The paths of the tracks the pool holds, in library order.
    pub(crate) fn set_pool_paths(&self, pool: &SetPool) -> Vec<PathBuf> {
        let library = self.session.library();
        match pool {
            SetPool::AllTracks => library
                .tracks()
                .iter()
                .map(|track| track.path.clone())
                .collect(),
            SetPool::Folder(folder) => library
                .tracks()
                .iter()
                .filter(|track| track.path.starts_with(folder))
                .map(|track| track.path.clone())
                .collect(),
            SetPool::Playlist(id) => self
                .session
                .playlists()
                .get(*id)
                .map(|playlist| {
                    playlist
                        .tracks
                        .iter()
                        .filter(|path| library.index_of(path).is_some())
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// What the set builder knows of the track at `path`.
    fn pool_entry(&self, path: &Path) -> PoolEntry {
        let library = self.session.library();
        let track = library
            .index_of(path)
            .and_then(|index| library.track(index));
        PoolEntry {
            key: track.and_then(|track| track.key()),
            bpm: track.and_then(|track| track.bpm()),
            energy: track.and_then(|track| track.energy()).map(f32::from),
            artist: track.and_then(|track| track.artist.clone()),
        }
    }

    pub(crate) fn choose_set_pool(&mut self, pool: SetPool, cx: &mut Context<'_, Self>) {
        self.set_builder.pool = pool;
        cx.notify();
    }

    pub(crate) fn choose_set_preset(&mut self, preset: CurvePreset, cx: &mut Context<'_, Self>) {
        self.set_builder.curve = preset_samples(preset);
        self.set_builder.preset = Some(preset);
        cx.notify();
    }

    pub(crate) fn change_set_length(&mut self, change: isize, cx: &mut Context<'_, Self>) {
        let count = self.set_builder.track_count as isize + change;
        self.set_builder.track_count =
            count.clamp(FEWEST_TRACKS as isize, MOST_TRACKS as isize) as usize;
        cx.notify();
    }

    /// Draws the curve to the pointer at `position`, in a straight line from
    /// where the stroke last was.
    pub(crate) fn draw_curve_to(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        is_new_stroke: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(bounds) = self.set_builder.curve_bounds.get() else {
            return;
        };
        if bounds.size.width <= gpui::px(0.0) || bounds.size.height <= gpui::px(0.0) {
            return;
        }
        let along = ((position.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
        let down = ((position.y - bounds.origin.y) / bounds.size.height).clamp(0.0, 1.0);
        let level = level_at_height(down);
        let builder = &mut self.set_builder;
        let (from_along, from_level) = match builder.stroke {
            Some(previous) if !is_new_stroke => previous,
            _ => (along, level),
        };
        draw_segment(&mut builder.curve, (from_along, from_level), (along, level));
        builder.stroke = Some((along, level));
        builder.preset = None;
        cx.notify();
    }

    /// Ends a stroke and smooths what it drew.
    pub(crate) fn finish_curve_stroke(&mut self, cx: &mut Context<'_, Self>) {
        if self.set_builder.stroke.take().is_some() {
            self.set_builder.curve = smoothed(&self.set_builder.curve);
            cx.notify();
        }
    }

    /// The options of a set of `track_count` tracks along the drawn curve.
    fn set_options(&self, track_count: usize) -> Option<SetOptions> {
        let curve = EnergyCurve::new(self.set_builder.curve.clone()).ok()?;
        Some(SetOptions::new(track_count, curve))
    }

    /// Orders tracks of the pool along the curve, keeping every locked
    /// track where it is, on a background thread.
    pub(crate) fn build_set(&mut self, cx: &mut Context<'_, Self>) {
        let builder = &self.set_builder;
        let mut pool_paths = self.set_pool_paths(&builder.pool);
        let track_count = builder.track_count;
        let Some(mut options) = self.set_options(track_count) else {
            return;
        };
        if let Some(planned) = &builder.planned {
            for (position, path) in planned.paths.iter().enumerate() {
                if !planned.locked[position] || position >= track_count {
                    continue;
                }
                let track = match pool_paths.iter().position(|pooled| pooled == path) {
                    Some(track) => track,
                    None => {
                        pool_paths.push(path.clone());
                        pool_paths.len() - 1
                    }
                };
                options.pins.push(Pin {
                    track,
                    position: PinPosition::At(position),
                });
            }
        }
        if pool_paths.is_empty() {
            self.set_builder.problem = Some("The pool has no tracks. Choose another.".to_string());
            cx.notify();
            return;
        }
        let entries: Vec<PoolEntry> = pool_paths
            .iter()
            .map(|path| self.pool_entry(path))
            .collect();
        let curve = self.set_builder.curve.clone();
        self.set_builder.build_generation += 1;
        let generation = self.set_builder.build_generation;
        self.set_builder.is_building = true;
        self.set_builder.problem = None;
        cx.notify();
        self.set_builder.build_task = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let plan = build_set(&entries, &options)?;
                    let order: Vec<usize> =
                        plan.tracks.iter().map(|planned| planned.track).collect();
                    let locked: Vec<bool> =
                        plan.tracks.iter().map(|planned| planned.pinned).collect();
                    let set_entries: Vec<PoolEntry> =
                        order.iter().map(|&track| entries[track].clone()).collect();
                    let explained = evaluate_set(
                        &set_entries,
                        &(0..order.len()).collect::<Vec<_>>(),
                        &pins_for(&locked, &options),
                    )?;
                    Ok::<_, euphoria_core::SetError>((order, locked, explained))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.set_builder.build_generation != generation {
                    return;
                }
                this.set_builder.is_building = false;
                match outcome {
                    Ok((order, locked, plan)) => {
                        this.set_builder.planned = Some(PlannedSet {
                            paths: order
                                .iter()
                                .map(|&track| pool_paths[track].clone())
                                .collect(),
                            locked,
                            plan,
                            curve,
                        });
                        this.set_builder.saved_as = None;
                    }
                    Err(error) => this.set_builder.problem = Some(sentence(&error.to_string())),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Explains the planned set again after it was changed by hand.
    fn explain_planned_set(&mut self) {
        let Some(planned) = &self.set_builder.planned else {
            return;
        };
        let entries: Vec<PoolEntry> = planned
            .paths
            .iter()
            .map(|path| self.pool_entry(path))
            .collect();
        let Ok(curve) = EnergyCurve::new(planned.curve.clone()) else {
            return;
        };
        let options = pins_for(
            &planned.locked,
            &SetOptions::new(planned.paths.len(), curve),
        );
        let order: Vec<usize> = (0..planned.paths.len()).collect();
        match evaluate_set(&entries, &order, &options) {
            Ok(plan) => {
                if let Some(planned) = &mut self.set_builder.planned {
                    planned.plan = plan;
                }
                self.set_builder.problem = None;
            }
            Err(error) => self.set_builder.problem = Some(sentence(&error.to_string())),
        }
        self.set_builder.saved_as = None;
    }

    pub(crate) fn toggle_set_lock(&mut self, position: usize, cx: &mut Context<'_, Self>) {
        if let Some(planned) = &mut self.set_builder.planned
            && let Some(locked) = planned.locked.get_mut(position)
        {
            *locked = !*locked;
            self.explain_planned_set();
            cx.notify();
        }
    }

    /// Moves the track at `position` one place earlier or later, taking its
    /// lock with it.
    pub(crate) fn move_set_track(
        &mut self,
        position: usize,
        later: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(planned) = &mut self.set_builder.planned else {
            return;
        };
        let other = if later {
            position + 1
        } else {
            match position.checked_sub(1) {
                Some(other) => other,
                None => return,
            }
        };
        if other >= planned.paths.len() {
            return;
        }
        planned.paths.swap(position, other);
        planned.locked.swap(position, other);
        self.explain_planned_set();
        cx.notify();
    }

    pub(crate) fn remove_set_track(&mut self, position: usize, cx: &mut Context<'_, Self>) {
        let Some(planned) = &mut self.set_builder.planned else {
            return;
        };
        if planned.paths.len() <= 1 || position >= planned.paths.len() {
            return;
        }
        planned.paths.remove(position);
        planned.locked.remove(position);
        self.explain_planned_set();
        cx.notify();
    }

    /// Saves the set as a new playlist and returns it, or the playlist it
    /// was saved as already when nothing changed since.
    pub(crate) fn save_set_as_playlist(
        &mut self,
        cx: &mut Context<'_, Self>,
    ) -> Option<PlaylistId> {
        let planned = self.set_builder.planned.as_ref()?;
        if let Some(id) = self.set_builder.saved_as
            && self.session.playlists().get(id).is_some()
        {
            return Some(id);
        }
        let paths = planned.paths.clone();
        let name = self.set_name();
        let id = self.session.playlists_mut().create();
        self.session.playlists_mut().rename(id, &name);
        self.add_to_playlist(id, &paths, cx);
        self.set_builder.saved_as = Some(id);
        let name = self
            .session
            .playlists()
            .get(id)
            .map(|playlist| playlist.name.clone())
            .unwrap_or(name);
        self.notice = Some(format!("Saved the set as the playlist {name}."));
        cx.notify();
        Some(id)
    }

    /// Saves the set as a playlist if it is not one yet, then asks where to
    /// write it as an M3U8 file.
    pub(crate) fn export_set(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(id) = self.save_set_as_playlist(cx) {
            self.export_playlist(id, PlaylistFormat::M3u8, cx);
        }
    }

    /// The name a saved set gets: its curve preset, or that it was drawn.
    fn set_name(&self) -> String {
        let shape = match self.set_builder.preset {
            Some(preset) => preset.label(),
            None => "Drawn curve",
        };
        let count = self
            .set_builder
            .planned
            .as_ref()
            .map_or(0, |planned| planned.paths.len());
        format!("Set: {shape}, {count} tracks")
    }
}

/// `options` with each locked position pinned to the track there, for a
/// pool that is the set itself.
fn pins_for(locked: &[bool], options: &SetOptions) -> SetOptions {
    let mut options = options.clone();
    options.pins = locked
        .iter()
        .enumerate()
        .filter(|(_, locked)| **locked)
        .map(|(position, _)| Pin {
            track: position,
            position: PinPosition::At(position),
        })
        .collect();
    options
}

/// Sets the samples of `curve` between two points of a stroke on a straight
/// line between them.
pub(crate) fn draw_segment(curve: &mut [f32], from: (f32, f32), to: (f32, f32)) {
    let last = (curve.len() - 1) as f32;
    let (start, end) = if from.0 <= to.0 {
        (from, to)
    } else {
        (to, from)
    };
    let first_index = (start.0 * last).round() as usize;
    let last_index = (end.0 * last).round() as usize;
    for index in first_index..=last_index.min(curve.len() - 1) {
        let along = index as f32 / last;
        let span = end.0 - start.0;
        let share = if span > 0.0 {
            ((along - start.0) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
        curve[index] = start.1 + (end.1 - start.1) * share;
    }
}

/// An error message as a sentence: a capital letter and a full stop.
fn sentence(message: &str) -> String {
    let mut characters = message.chars();
    let mut sentence: String = match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    };
    if !sentence.ends_with('.') {
        sentence.push('.');
    }
    sentence
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_become_evenly_spread_samples() {
        let samples = preset_samples(CurvePreset::WarmUp);
        assert_eq!(samples.len(), CURVE_SAMPLES);
        assert_eq!(samples[0], 3.0);
        assert_eq!(samples[CURVE_SAMPLES - 1], 7.0);
    }

    #[test]
    fn a_stroke_sets_the_samples_it_crosses_on_a_straight_line() {
        let mut curve = vec![5.0; 11];
        draw_segment(&mut curve, (0.2, 2.0), (0.6, 10.0));
        assert_eq!(curve[1], 5.0);
        assert_eq!(curve[2], 2.0);
        assert_eq!(curve[4], 6.0);
        assert_eq!(curve[6], 10.0);
        assert_eq!(curve[7], 5.0);

        let mut backwards = vec![5.0; 11];
        draw_segment(&mut backwards, (0.6, 10.0), (0.2, 2.0));
        assert_eq!(backwards, curve);

        let mut one_point = vec![5.0; 11];
        draw_segment(&mut one_point, (0.5, 8.0), (0.5, 8.0));
        assert_eq!(one_point[5], 8.0);
    }

    #[test]
    fn smoothing_softens_corners_and_keeps_the_ends() {
        let smooth = smoothed(&[1.0, 1.0, 9.0, 1.0, 1.0]);
        assert_eq!(smooth, [1.0, 3.0, 5.0, 3.0, 1.0]);
    }

    #[test]
    fn the_top_of_the_canvas_is_the_highest_energy() {
        assert_eq!(level_at_height(0.0), 10.0);
        assert_eq!(level_at_height(1.0), 1.0);
        assert_eq!(level_at_height(2.0), 1.0);
    }

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(sentence("the pool is empty"), "The pool is empty.");
        assert_eq!(sentence("Done."), "Done.");
    }
}
