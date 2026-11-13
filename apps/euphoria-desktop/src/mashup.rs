// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mashup mode of the decks: the tracks that layer over deck A's, found by
//! `euphoria_core::harmony::mashups_for`, stepped through on deck B with
//! the key shift and stems each one asks for, and the pair rendered to a
//! WAV file by `euphoria_core::mashup::write_mashup`.
//!
//! The stems of tracks separated in an earlier session are found in the
//! stem cache in the background, and measured, so the finder can check
//! vocals against instruments.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use euphoria_core::harmony::{
    KeyFit, Layering, MashupOptions, MashupReason, MashupTrack, MixTrack, Side, StemProfile,
    TempoFit, mashups_for,
};
use euphoria_core::mashup::{LayerAudio, MashupLayer, write_mashup};
use euphoria_core::shift::ExistingOutput;
use euphoria_core::stems::{Stem, StemCache, StemFiles, profile_stems};
use euphoria_core::{CachedAnalyzer, Key};
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::deck_audio::{DeckCommand, DeckId, GridTiming};
use crate::decks::{DeckMode, DeckView};
use crate::library::Track;

/// How many ideas the list holds.
const IDEA_LIMIT: usize = 40;

/// How often the window looks for stems found in the background.
const STEM_SCAN_POLL: Duration = Duration::from_millis(250);

/// Stem levels that leave only the vocals, in the order of [`Stem::ALL`].
pub(crate) const VOCALS_ONLY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Stem levels that leave everything but the vocals.
pub(crate) const INSTRUMENTAL_ONLY: [f32; 4] = [1.0, 1.0, 1.0, 0.0];

/// How a deck is set up when a track goes on it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct DeckSetup {
    pub(crate) shift_semitones: i8,
    /// The stem levels to play, or `None` to play the whole track.
    pub(crate) stem_levels: Option<[f32; 4]>,
}

/// A track that layers over deck A's, and why.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MashupIdea {
    pub(crate) track_index: usize,
    pub(crate) key: KeyFit,
    pub(crate) tempo: TempoFit,
    pub(crate) layering: Option<Layering>,
    pub(crate) reasons: Vec<MashupReason>,
}

impl MashupIdea {
    /// The stem levels of deck A and deck B the idea asks for: the vocals
    /// of one over the instrumental of the other. `None` for a deck that
    /// plays its whole track.
    pub(crate) fn stem_levels(&self) -> [Option<[f32; 4]>; 2] {
        match self.layering {
            Some(Layering::VocalsOver {
                vocals_from: Side::Candidate,
                ..
            }) => [Some(INSTRUMENTAL_ONLY), Some(VOCALS_ONLY)],
            Some(Layering::VocalsOver {
                vocals_from: Side::Reference,
                ..
            }) => [Some(VOCALS_ONLY), Some(INSTRUMENTAL_ONLY)],
            Some(Layering::Instrumentals { .. }) | None => [None, None],
        }
    }
}

/// A track as the mashup finder sees it: with its key shifted as on its
/// deck, and its stem profile when it has one.
struct Layerable<'track> {
    track: &'track Track,
    shift_semitones: i8,
    profile: Option<&'track StemProfile>,
}

impl MixTrack for Layerable<'_> {
    fn key(&self) -> Option<Key> {
        self.track
            .key()
            .map(|key| key.transposed(self.shift_semitones))
    }

    fn bpm(&self) -> Option<f32> {
        self.track.bpm()
    }

    fn energy(&self) -> Option<f32> {
        self.track.energy().map(f32::from)
    }
}

impl MashupTrack for Layerable<'_> {
    fn stem_profile(&self) -> Option<&StemProfile> {
        self.profile
    }
}

/// What a background check found for one track: its stems and how they
/// sound, or nothing.
type Found = (PathBuf, Option<(StemFiles, StemProfile)>);

/// The stems found in the cache, and their profiles.
#[derive(Default)]
pub(crate) struct StemLibrary {
    found: HashMap<PathBuf, (StemFiles, StemProfile)>,
    checked: HashSet<PathBuf>,
    scan: Option<Receiver<Found>>,
}

impl StemLibrary {
    /// The stems of the track at `path`, when they were found.
    pub(crate) fn files(&self, path: &Path) -> Option<StemFiles> {
        self.found.get(path).map(|(files, _)| files.clone())
    }

    /// How the stems of the track at `path` sound, when they were found.
    pub(crate) fn profile(&self, path: &Path) -> Option<&StemProfile> {
        self.found.get(path).map(|(_, profile)| profile)
    }

    /// True while a check runs in the background.
    pub(crate) fn is_scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Takes in what the background check found since the last call.
    /// Returns true when it found new stems.
    fn collect(&mut self) -> bool {
        let Some(scan) = &self.scan else {
            return false;
        };
        let mut changed = false;
        loop {
            match scan.try_recv() {
                Ok((path, found)) => {
                    if let Some(found) = found {
                        self.found.insert(path, found);
                        changed = true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.scan = None;
                    break;
                }
            }
        }
        changed
    }

    /// Forgets that the track at `path` was checked, as when it has just
    /// been separated.
    pub(crate) fn recheck(&mut self, path: &Path) {
        self.checked.remove(path);
    }
}

/// Looks for the stems of each of `paths` in the stem cache and measures
/// them, sending what it finds. `separated` holds stems the app already
/// knows of. Runs on a thread of its own.
fn check_stems(
    paths: Vec<PathBuf>,
    separated: HashMap<PathBuf, StemFiles>,
    analyzer: Option<Arc<CachedAnalyzer>>,
    found: std::sync::mpsc::Sender<Found>,
) {
    let stem_cache = StemCache::open_default().ok();
    for path in paths {
        let files = separated.get(&path).cloned().or_else(|| {
            let content = analyzer.as_ref()?.known_content(&path)?;
            stem_cache.as_ref()?.cached(&content)
        });
        let measured =
            files.and_then(|files| profile_stems(&files).ok().map(|profile| (files, profile)));
        if found.send((path, measured)).is_err() {
            return;
        }
    }
}

/// Sets the levels of `view`'s stems in the mix its sound follows.
pub(crate) fn apply_stem_levels(view: &DeckView) {
    for (stem, level) in Stem::ALL.iter().zip(view.stem_levels) {
        view.stem_mix.set_level(*stem, level);
    }
}

/// The downbeat at or before `seconds` in a track with `grid`, or its first
/// downbeat when `seconds` comes before it.
pub(crate) fn downbeat_at_or_before(grid: &GridTiming, seconds: f64) -> f64 {
    let bar = grid.bar_seconds();
    let bars = ((seconds - grid.first_downbeat_seconds) / bar + 1e-6)
        .floor()
        .max(0.0);
    grid.first_downbeat_seconds + bars * bar
}

/// The name suggested for a mashup of the tracks titled `first` and
/// `second`.
pub(crate) fn suggested_name(first: &str, second: &str) -> String {
    let clean = |title: &str| {
        title
            .chars()
            .map(|character| {
                if matches!(character, '/' | '\\' | ':') {
                    '-'
                } else {
                    character
                }
            })
            .collect::<String>()
    };
    format!("{} x {} (mashup).wav", clean(first), clean(second))
}

impl EuphoriaApp {
    /// Shows tracks to play next, or tracks to layer over deck A.
    pub(crate) fn set_deck_mode(&mut self, mode: DeckMode, cx: &mut Context<'_, Self>) {
        if self.decks.mode == mode {
            return;
        }
        self.decks.mode = mode;
        self.decks.idea_index = None;
        if mode == DeckMode::Mashup {
            self.scan_for_stems(cx);
        }
        self.refresh_suggestions();
        cx.notify();
    }

    /// Ranks the tracks that layer over deck A's, as it is shifted.
    pub(crate) fn refresh_mashup_ideas(&mut self) {
        let library = self.session.library();
        let deck_a = self.decks.view(DeckId::A);
        let Some(chosen) = deck_a.track_index.and_then(|index| library.track(index)) else {
            self.decks.ideas.clear();
            self.decks.idea_index = None;
            return;
        };
        let stems = &self.decks.stem_library;
        let reference = Layerable {
            track: chosen,
            shift_semitones: deck_a.shift_semitones,
            profile: stems.profile(&chosen.path),
        };
        let filter = self.decks.filter.clone();
        let candidates: Vec<Layerable<'_>> = self
            .suggestion_indexes()
            .into_iter()
            .filter_map(|index| library.track(index))
            .filter(|candidate| candidate.path != chosen.path)
            .filter(|candidate| filter.energy_fits(candidate.energy().map(|energy| energy as f32)))
            .filter(|candidate| filter.genre_fits(candidate.genre()))
            .map(|track| Layerable {
                track,
                shift_semitones: 0,
                profile: stems.profile(&track.path),
            })
            .collect();
        let previous = self
            .decks
            .idea_index
            .and_then(|position| self.decks.ideas.get(position))
            .map(|idea| idea.track_index);
        let options = MashupOptions {
            relations: filter
                .relations
                .relations()
                .map(|relations| relations.to_vec())
                .unwrap_or_else(|| MashupOptions::default().relations),
            max_tempo_difference_percent: filter
                .tempo_window_percent
                .unwrap_or(MashupOptions::default().max_tempo_difference_percent),
            bpm_range: filter.bpm_range(),
            energy_range: filter.energy_range(),
            ..MashupOptions::default()
        };
        let ideas: Vec<MashupIdea> = mashups_for(&reference, &candidates, &options)
            .into_iter()
            .take(IDEA_LIMIT)
                .filter_map(|found| {
                    Some(MashupIdea {
                        track_index: library.index_of(&found.track.track.path)?,
                        key: found.key,
                        tempo: found.tempo,
                        layering: found.layering,
                        reasons: found.reasons(),
                    })
                })
                .collect();
        self.decks.idea_index =
            previous.and_then(|track| ideas.iter().position(|idea| idea.track_index == track));
        self.decks.ideas = ideas;
    }

    /// Looks for the stems of the library's tracks in the background, once
    /// per track, and ranks the ideas again as they are found.
    pub(crate) fn scan_for_stems(&mut self, cx: &mut Context<'_, Self>) {
        if self.decks.stem_library.is_scanning() {
            return;
        }
        let library = self.session.library();
        let paths: Vec<PathBuf> = library
            .tracks()
            .iter()
            .map(|track| track.path.clone())
            .filter(|path| !self.decks.stem_library.checked.contains(path))
            .collect();
        if paths.is_empty() {
            return;
        }
        let separated: HashMap<PathBuf, StemFiles> = paths
            .iter()
            .filter_map(|path| Some((path.clone(), self.stems.ready_files(path)?)))
            .collect();
        self.decks
            .stem_library
            .checked
            .extend(paths.iter().cloned());
        let (sender, receiver) = channel();
        let analyzer = self.session.cache();
        let spawned = std::thread::Builder::new()
            .name("euphoria-mashup-stems".to_string())
            .spawn(move || check_stems(paths, separated, analyzer, sender));
        if spawned.is_err() {
            return;
        }
        self.decks.stem_library.scan = Some(receiver);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(STEM_SCAN_POLL).await;
                let goes_on = this.update(cx, |this, cx| {
                    if this.decks.stem_library.collect() {
                        for deck in DeckId::BOTH {
                            let view = this.decks.view(deck);
                            if view.stem_files.is_none()
                                && let Some(path) = view.path.clone()
                            {
                                let files = this.decks.stem_library.files(&path);
                                this.decks.view_mut(deck).stem_files = files;
                            }
                        }
                        this.refresh_suggestions();
                        cx.notify();
                    }
                    this.decks.stem_library.is_scanning()
                });
                if !matches!(goes_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Puts idea `position` on deck B with the shift it needs and, when
    /// both tracks have stems, the vocals of one over the instrumental of
    /// the other.
    pub(crate) fn load_mashup_idea(&mut self, position: usize, cx: &mut Context<'_, Self>) {
        let Some(idea) = self.decks.ideas.get(position).cloned() else {
            return;
        };
        self.decks.idea_index = Some(position);
        let [reference_levels, candidate_levels] = idea.stem_levels();
        self.load_on_deck_with(
            DeckId::B,
            idea.track_index,
            DeckSetup {
                shift_semitones: idea.key.shift_semitones,
                stem_levels: candidate_levels,
            },
            cx,
        );
        if let Some(levels) = reference_levels {
            self.set_deck_stems(DeckId::A, Some(levels), cx);
        }
        cx.notify();
    }

    /// Steps `step` ideas along, wrapping at either end, and puts that one
    /// on deck B.
    pub(crate) fn step_mashup_idea(&mut self, step: isize, cx: &mut Context<'_, Self>) {
        let count = self.decks.ideas.len();
        if count == 0 {
            return;
        }
        let position = match self.decks.idea_index {
            Some(position) => (position as isize + step).rem_euclid(count as isize) as usize,
            None if step < 0 => count - 1,
            None => 0,
        };
        self.load_mashup_idea(position, cx);
    }

    /// Shifts the pitch of `deck` by `semitones`, within the shifts the
    /// finder suggests and a little more.
    pub(crate) fn set_deck_shift(
        &mut self,
        deck: DeckId,
        semitones: i8,
        cx: &mut Context<'_, Self>,
    ) {
        let limit = euphoria_core::shift::MAXIMUM_SHIFT_SEMITONES;
        let semitones = semitones.clamp(-limit, limit);
        if self.decks.view(deck).shift_semitones == semitones {
            return;
        }
        self.decks.view_mut(deck).shift_semitones = semitones;
        self.decks.send(DeckCommand::SetShift { deck, semitones });
        if deck == DeckId::A && self.decks.mode == DeckMode::Mashup {
            self.refresh_suggestions();
        }
        cx.notify();
    }

    /// Plays the stems of `deck` at `levels`, or the whole track for
    /// `None`. Changing between the two opens the deck's sound again where
    /// it is, so it carries on in time.
    pub(crate) fn set_deck_stems(
        &mut self,
        deck: DeckId,
        levels: Option<[f32; 4]>,
        cx: &mut Context<'_, Self>,
    ) {
        let view = self.decks.view(deck);
        if view.stem_files.is_none() || view.track_index.is_none() {
            return;
        }
        let use_stems = levels.is_some();
        let reopen = view.use_stems != use_stems;
        let playing = view.playing;
        let position = self.decks.position_seconds(deck);
        let view = self.decks.view_mut(deck);
        view.use_stems = use_stems;
        if let Some(levels) = levels {
            view.stem_levels = levels;
            apply_stem_levels(view);
        }
        if reopen {
            self.open_deck_source(deck, position, playing, cx);
        }
        cx.notify();
    }

    /// Sets the level of one stem of `deck`, from 0 to 1.
    pub(crate) fn set_deck_stem_level(
        &mut self,
        deck: DeckId,
        stem: Stem,
        level: f32,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(slot) = Stem::ALL.iter().position(|each| *each == stem) else {
            return;
        };
        let view = self.decks.view_mut(deck);
        view.stem_levels[slot] = level.clamp(0.0, 1.0);
        view.stem_mix.set_level(stem, view.stem_levels[slot]);
        cx.notify();
    }

    /// The layers of the mashup on the decks: each deck from the downbeat
    /// at or before where it is, at its speed, shift, fader and stems.
    pub(crate) fn mashup_layers(&self) -> Option<Vec<MashupLayer>> {
        let mut layers = Vec::with_capacity(2);
        for deck in DeckId::BOTH {
            let view = self.decks.view(deck);
            let path = view.path.clone()?;
            view.problem.is_none().then_some(())?;
            let position = self.decks.position_seconds(deck);
            let start_seconds = view
                .grid
                .map_or(position, |grid| downbeat_at_or_before(&grid, position));
            let audio = match (&view.stem_files, view.use_stems) {
                (Some(files), true) => LayerAudio::Stems(
                    Stem::ALL
                        .iter()
                        .zip(view.stem_levels)
                        .map(|(stem, level)| (files.path(*stem), level))
                        .collect(),
                ),
                _ => LayerAudio::File(path),
            };
            layers.push(MashupLayer {
                audio,
                start_seconds,
                rate: view.rate,
                shift_semitones: view.shift_semitones,
                gain: view.volume,
            });
        }
        Some(layers)
    }

    /// Asks where to save the mashup on the decks, then renders it there
    /// off the main thread.
    pub(crate) fn export_mashup(&mut self, cx: &mut Context<'_, Self>) {
        let Some(layers) = self.mashup_layers() else {
            self.notice = Some("Load a track on both decks to export a mashup.".to_string());
            cx.notify();
            return;
        };
        let library = self.session.library();
        let title = |deck: DeckId| {
            self.decks
                .view(deck)
                .track_index
                .and_then(|index| library.track(index))
                .map(|track| track.title().to_string())
                .unwrap_or_default()
        };
        let name = suggested_name(&title(DeckId::A), &title(DeckId::B));
        let directory = self
            .decks
            .view(DeckId::A)
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            let destination = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.notice = Some(format!("Could not open the save dialog: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            this.update(cx, |this, cx| {
                this.notice = Some("Rendering the mashup…".to_string());
                cx.notify();
            })
            .ok();
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    // The save dialog has already asked whether to replace a
                    // file of that name.
                    write_mashup(&layers, None, &destination, ExistingOutput::Replace)
                })
                .await;
            this.update(cx, |this, cx| {
                this.notice = Some(match outcome {
                    Ok(written) => {
                        let seconds = written.frames as f32
                            / euphoria_core::mashup::MASHUP_SAMPLE_RATE as f32;
                        let turned_down = if written.gain < 1.0 {
                            format!(
                                ", turned down {:.1} dB to keep it from clipping",
                                -20.0 * written.gain.log10()
                            )
                        } else {
                            String::new()
                        };
                        format!(
                            "Saved the mashup, {}, to {}{turned_down}.",
                            crate::format::duration(seconds),
                            written.path.display()
                        )
                    }
                    Err(error) => format!("Could not export the mashup: {}", error.chain()),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Plays both decks together, deck B's downbeats on deck A's.
    pub(crate) fn play_mashup(&mut self, cx: &mut Context<'_, Self>) {
        for deck in DeckId::BOTH {
            let view = self.decks.view(deck);
            if view.loading || view.track_index.is_none() || view.problem.is_some() {
                return;
            }
        }
        for deck in DeckId::BOTH {
            if self.decks.view(deck).playing {
                continue;
            }
            self.start_deck(deck);
        }
        self.follow_decks(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idea(layering: Option<Layering>) -> MashupIdea {
        let key: Key = "8A".parse().unwrap();
        MashupIdea {
            track_index: 0,
            key: KeyFit {
                key,
                shift_semitones: 0,
                shifted_key: key,
                relation: euphoria_core::Relation::Same,
            },
            tempo: TempoFit {
                difference_percent: 0.0,
                scaling: euphoria_core::TempoScaling::Direct,
            },
            layering,
            reasons: Vec::new(),
        }
    }

    #[test]
    fn an_idea_puts_one_tracks_vocals_over_the_others_instrumental() {
        let candidate_vocals = idea(Some(Layering::VocalsOver {
            vocals_from: Side::Candidate,
            collision: 0.1,
            instrumental_has_vocals: true,
        }));
        assert_eq!(
            candidate_vocals.stem_levels(),
            [Some(INSTRUMENTAL_ONLY), Some(VOCALS_ONLY)]
        );
        let reference_vocals = idea(Some(Layering::VocalsOver {
            vocals_from: Side::Reference,
            collision: 0.1,
            instrumental_has_vocals: false,
        }));
        assert_eq!(
            reference_vocals.stem_levels(),
            [Some(VOCALS_ONLY), Some(INSTRUMENTAL_ONLY)]
        );
        assert_eq!(idea(None).stem_levels(), [None, None]);
        assert_eq!(
            idea(Some(Layering::Instrumentals { collision: 0.2 })).stem_levels(),
            [None, None]
        );
    }

    #[test]
    fn an_export_starts_on_the_downbeat_at_or_before_the_deck() {
        let grid = GridTiming {
            first_downbeat_seconds: 0.5,
            bpm: 120.0,
        };
        assert_eq!(downbeat_at_or_before(&grid, 0.0), 0.5);
        assert_eq!(downbeat_at_or_before(&grid, 0.5), 0.5);
        assert_eq!(downbeat_at_or_before(&grid, 2.4), 0.5);
        assert_eq!(downbeat_at_or_before(&grid, 2.5), 2.5);
        assert_eq!(downbeat_at_or_before(&grid, 9.0), 8.5);
    }

    #[test]
    fn the_suggested_name_has_both_titles_and_no_folder_separators() {
        assert_eq!(
            suggested_name("Night/Day", "Dawn: Edit"),
            "Night-Day x Dawn- Edit (mashup).wav"
        );
    }
}
