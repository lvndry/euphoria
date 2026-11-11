// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Two decks to prepare a set: a track on each, played, paused and moved
//! through, with deck B's tempo following deck A's and starting on A's
//! downbeat, a volume for each and a crossfader, and the tracks that would
//! mix best after the one on deck A, for the energy going up, staying or
//! going down.

use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;
use std::time::Duration;

use euphoria_core::harmony::{EnergyDirection, MixOptions, Relation, next_tracks};
use euphoria_core::{SectionCue, TrackAnalysis};
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::deck_audio::{DeckCommand, DeckId, DeckLink, GridTiming, deck_mix, matching_rate};
use crate::library::Track;
use crate::playback::{TrackSource, Voice};
use crate::views::waveform::SeenBounds;

/// How many tracks the suggestions list.
const SUGGESTION_LIMIT: usize = 40;

/// How often the decks are drawn again while one plays.
const DECK_REFRESH: Duration = Duration::from_millis(40);

/// What is on one deck.
#[derive(Debug, Clone, Default)]
pub(crate) struct DeckView {
    /// Index of the track in the library.
    pub(crate) track_index: Option<usize>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) grid: Option<GridTiming>,
    /// The track's tempo, from its grid when it has one.
    pub(crate) bpm: Option<f64>,
    pub(crate) cues: Vec<SectionCue>,
    pub(crate) duration_seconds: Option<f64>,
    /// Played at this many times its own speed.
    pub(crate) rate: f64,
    pub(crate) keylock: bool,
    /// The deck's fader, 0 to 1.
    pub(crate) volume: f32,
    /// True while the window asked the deck to play.
    pub(crate) playing: bool,
    /// Why the track could not be loaded, when it could not.
    pub(crate) problem: Option<String>,
}

impl DeckView {
    fn empty() -> Self {
        Self {
            rate: 1.0,
            keylock: true,
            volume: 1.0,
            ..Self::default()
        }
    }

    /// The tempo heard: the track's own times its speed.
    pub(crate) fn heard_bpm(&self) -> Option<f64> {
        self.bpm.map(|bpm| bpm * self.rate)
    }
}

/// Which tracks the suggestions come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SuggestionScope {
    /// Every track in the library.
    #[default]
    Library,
    /// The tracks the table shows: the folder on show, filtered by the
    /// search.
    Shown,
}

/// A track that would mix well after the one on deck A.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NextTrack {
    pub(crate) track_index: usize,
    pub(crate) relation: Relation,
    pub(crate) tempo_difference_percent: Option<f32>,
    pub(crate) energy_difference: Option<f32>,
}

/// The decks, their mixer and the suggestions.
pub(crate) struct Decks {
    /// True while the decks are on screen.
    pub(crate) is_open: bool,
    link: Option<DeckLink>,
    /// Dropping this stops the sound of the decks.
    voice: Option<Box<dyn Voice>>,
    pub(crate) views: [DeckView; 2],
    /// From 0, deck A alone, to 1, deck B alone.
    pub(crate) crossfader: f32,
    /// True while deck B's tempo follows deck A's.
    pub(crate) sync: bool,
    pub(crate) direction: EnergyDirection,
    pub(crate) scope: SuggestionScope,
    pub(crate) suggestions: Vec<NextTrack>,
    /// Counts loads, so a file opened after another was asked for is
    /// dropped.
    load_generation: [u64; 2],
    is_refreshing: bool,
    /// Where each deck's waveform was last drawn, for turning a click into
    /// a position in its track.
    pub(crate) waveform_bounds: [SeenBounds; 2],
    /// Where each deck's fader and the crossfader were last drawn.
    pub(crate) fader_bounds: [SeenBounds; 2],
    pub(crate) crossfader_bounds: SeenBounds,
}

impl Default for Decks {
    fn default() -> Self {
        Self {
            is_open: false,
            link: None,
            voice: None,
            views: [DeckView::empty(), DeckView::empty()],
            crossfader: 0.5,
            sync: true,
            direction: EnergyDirection::Same,
            scope: SuggestionScope::Library,
            suggestions: Vec::new(),
            load_generation: [0, 0],
            is_refreshing: false,
            waveform_bounds: Default::default(),
            fader_bounds: Default::default(),
            crossfader_bounds: Default::default(),
        }
    }
}

impl Decks {
    /// What is on `deck`.
    pub(crate) fn view(&self, deck: DeckId) -> &DeckView {
        &self.views[index(deck)]
    }

    fn view_mut(&mut self, deck: DeckId) -> &mut DeckView {
        &mut self.views[index(deck)]
    }

    /// Where `deck` is heard now, in seconds, from the sound itself.
    pub(crate) fn position_seconds(&self, deck: DeckId) -> f64 {
        self.link
            .as_ref()
            .map_or(0.0, |link| link.meter(deck).position_seconds())
    }

    /// True while `deck` waits for the other's downbeat to start.
    pub(crate) fn is_waiting(&self, deck: DeckId) -> bool {
        self.link
            .as_ref()
            .is_some_and(|link| link.meter(deck).is_waiting())
    }

    /// How hard the decks work, for the owner to see while listening: the
    /// share of one core each deck takes, and the frames the decoders
    /// could not fill in time.
    pub(crate) fn load_readout(&self) -> Option<String> {
        let link = self.link.as_ref()?;
        let a = link.meter(DeckId::A);
        let b = link.meter(DeckId::B);
        Some(format!(
            "A {:.1}%, B {:.1}% of a core; {} dropped frames",
            a.load() * 100.0,
            b.load() * 100.0,
            a.starved_frames() + b.starved_frames()
        ))
    }

    fn send(&self, command: DeckCommand) {
        if let Some(link) = &self.link {
            link.send(command);
        }
    }

    /// The volume of each deck after its fader and the crossfader, with
    /// equal power across the crossfader so the mix keeps its loudness.
    pub(crate) fn gains(&self) -> [f32; 2] {
        let position = self.crossfader.clamp(0.0, 1.0);
        let a = (position * FRAC_PI_2).cos() * std::f32::consts::SQRT_2;
        let b = (position * FRAC_PI_2).sin() * std::f32::consts::SQRT_2;
        [
            self.views[0].volume * a.min(1.0),
            self.views[1].volume * b.min(1.0),
        ]
    }

    fn apply_gains(&self) {
        if let Some(link) = &self.link {
            let gains = self.gains();
            for deck in DeckId::BOTH {
                link.meter(deck).set_gain(gains[index(deck)]);
            }
        }
    }

    /// The speed deck B plays at to follow deck A's tempo, when sync is on
    /// and both tempos are known.
    fn synced_rate(&self) -> Option<f64> {
        if !self.sync {
            return None;
        }
        let leading = self.views[0].heard_bpm()?;
        let own = self.views[1].bpm?;
        Some(matching_rate(own, leading))
    }

    /// Sets deck B's speed to follow deck A, or back to its own when sync is
    /// off.
    fn follow_tempo(&mut self) {
        let rate = self.synced_rate().unwrap_or(1.0);
        self.views[1].rate = rate;
        self.send(DeckCommand::SetRate {
            deck: DeckId::B,
            rate,
        });
    }
}

fn index(deck: DeckId) -> usize {
    match deck {
        DeckId::A => 0,
        DeckId::B => 1,
    }
}

/// The grid of a track, unless the user set its tempo by hand: a grid
/// fitted to the detected tempo would not match.
fn grid_of(track: &Track, analysis: &TrackAnalysis) -> Option<GridTiming> {
    if track.correction.bpm.is_some() {
        return None;
    }
    analysis.beat_grid.map(|grid| GridTiming {
        first_downbeat_seconds: grid.first_downbeat_seconds,
        bpm: grid.bpm,
    })
}

impl EuphoriaApp {
    /// Shows the decks, or hides them and stops their sound.
    pub(crate) fn toggle_decks(&mut self, cx: &mut Context<'_, Self>) {
        if self.decks.is_open {
            self.decks.is_open = false;
            self.decks.voice = None;
            self.decks.link = None;
            for view in &mut self.decks.views {
                view.playing = false;
            }
            cx.notify();
            return;
        }
        let (link, mix) = deck_mix();
        match self.output.play_decks(mix) {
            Ok(voice) => {
                self.stop_player(cx);
                self.decks.voice = Some(voice);
                self.decks.link = Some(link);
                self.decks.is_open = true;
                self.decks.apply_gains();
                // A deck whose track was loaded before goes back on.
                for deck in DeckId::BOTH {
                    if let Some(track_index) = self.decks.view(deck).track_index {
                        self.load_on_deck(deck, track_index, cx);
                    }
                }
            }
            Err(problem) => self.notice = Some(problem.message().to_string()),
        }
        cx.notify();
    }

    /// Loads the selected track on `deck`.
    pub(crate) fn load_selected_on_deck(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if let Some(track_index) = self.selected {
            self.load_on_deck(deck, track_index, cx);
        }
    }

    /// Opens the track at `track_index` off the main thread and puts it on
    /// `deck`, paused at its start.
    pub(crate) fn load_on_deck(
        &mut self,
        deck: DeckId,
        track_index: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(track) = self.session.library().track(track_index) else {
            return;
        };
        let path = track.path.clone();
        let analysis = track.analysis().cloned();
        let grid = analysis
            .as_ref()
            .and_then(|analysis| grid_of(track, analysis));
        let view = self.decks.view_mut(deck);
        view.playing = false;
        view.track_index = Some(track_index);
        view.path = Some(path.clone());
        view.grid = grid;
        view.bpm = grid
            .map(|grid| grid.bpm)
            .or_else(|| track.bpm().map(f64::from));
        view.cues = analysis
            .as_ref()
            .filter(|_| grid.is_some())
            .map(|analysis| analysis.cues.clone())
            .unwrap_or_default();
        view.duration_seconds = analysis
            .as_ref()
            .map(|analysis| f64::from(analysis.duration_seconds));
        view.problem = None;
        view.rate = 1.0;
        self.decks.send(DeckCommand::Pause(deck));
        self.load_deck_waveform(&path, track.bpm(), cx);
        if deck == DeckId::A {
            self.refresh_suggestions();
        }
        self.decks.follow_tempo();
        self.decks.load_generation[index(deck)] += 1;
        let generation = self.decks.load_generation[index(deck)];
        cx.notify();
        if self.decks.link.is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let opened_path = path.clone();
            let opened = cx
                .background_executor()
                .spawn(async move { TrackSource::open(&opened_path, 0.0) })
                .await;
            this.update(cx, |this, cx| {
                if this.decks.load_generation[index(deck)] != generation {
                    return;
                }
                match opened {
                    Ok(source) => {
                        if this.decks.view(deck).duration_seconds.is_none() {
                            this.decks.view_mut(deck).duration_seconds =
                                source.duration_seconds().map(f64::from);
                        }
                        this.decks.send(DeckCommand::Load {
                            deck,
                            source: Box::new(source),
                            grid,
                        });
                        let view = this.decks.view(deck);
                        this.decks.send(DeckCommand::SetKeylock {
                            deck,
                            keylock: view.keylock,
                        });
                        this.decks.send(DeckCommand::SetRate {
                            deck,
                            rate: view.rate,
                        });
                    }
                    Err(problem) => {
                        this.decks.view_mut(deck).problem = Some(problem.message().to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Plays `deck`, on the other deck's next downbeat when the other
    /// plays, or pauses it.
    pub(crate) fn toggle_deck_play(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let view = self.decks.view(deck);
        if view.track_index.is_none() || view.problem.is_some() {
            return;
        }
        if view.playing {
            self.decks.view_mut(deck).playing = false;
            self.decks.send(DeckCommand::Pause(deck));
        } else {
            self.start_deck(deck);
        }
        self.follow_decks(cx);
        cx.notify();
    }

    fn start_deck(&mut self, deck: DeckId) {
        let other_plays = self.decks.view(deck.other()).playing;
        self.decks.view_mut(deck).playing = true;
        self.decks.send(DeckCommand::Play {
            deck,
            quantize_to: other_plays.then_some(deck.other()),
        });
    }

    /// Moves `deck` to `fraction` of its track.
    pub(crate) fn seek_deck(&mut self, deck: DeckId, fraction: f32, cx: &mut Context<'_, Self>) {
        let Some(duration) = self.decks.view(deck).duration_seconds else {
            return;
        };
        self.decks.send(DeckCommand::Seek {
            deck,
            seconds: duration * f64::from(fraction.clamp(0.0, 1.0)),
        });
        cx.notify();
    }

    /// Plays `deck` from its cue `cue_index`, on the other deck's next
    /// downbeat when the other plays.
    pub(crate) fn play_deck_cue(
        &mut self,
        deck: DeckId,
        cue_index: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(cue) = self.decks.view(deck).cues.get(cue_index).copied() else {
            return;
        };
        self.decks.send(DeckCommand::Pause(deck));
        self.decks.send(DeckCommand::Seek {
            deck,
            seconds: cue.seconds,
        });
        self.start_deck(deck);
        self.follow_decks(cx);
        cx.notify();
    }

    /// Sets the fader of `deck`.
    pub(crate) fn set_deck_volume(
        &mut self,
        deck: DeckId,
        volume: f32,
        cx: &mut Context<'_, Self>,
    ) {
        self.decks.view_mut(deck).volume = volume.clamp(0.0, 1.0);
        self.decks.apply_gains();
        cx.notify();
    }

    /// Moves the crossfader, from 0 for deck A alone to 1 for deck B alone.
    pub(crate) fn set_crossfader(&mut self, position: f32, cx: &mut Context<'_, Self>) {
        self.decks.crossfader = position.clamp(0.0, 1.0);
        self.decks.apply_gains();
        cx.notify();
    }

    /// Keeps the pitch of `deck` when its speed changes, or lets it follow.
    pub(crate) fn toggle_keylock(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let keylock = !self.decks.view(deck).keylock;
        self.decks.view_mut(deck).keylock = keylock;
        self.decks.send(DeckCommand::SetKeylock { deck, keylock });
        cx.notify();
    }

    /// Makes deck B follow deck A's tempo, or play at its own.
    pub(crate) fn toggle_sync(&mut self, cx: &mut Context<'_, Self>) {
        self.decks.sync = !self.decks.sync;
        self.decks.follow_tempo();
        cx.notify();
    }

    /// Chooses where the energy should go with the next track.
    pub(crate) fn set_energy_direction(
        &mut self,
        direction: EnergyDirection,
        cx: &mut Context<'_, Self>,
    ) {
        self.decks.direction = direction;
        self.refresh_suggestions();
        cx.notify();
    }

    /// Chooses where the suggestions come from.
    pub(crate) fn set_suggestion_scope(
        &mut self,
        scope: SuggestionScope,
        cx: &mut Context<'_, Self>,
    ) {
        self.decks.scope = scope;
        self.refresh_suggestions();
        cx.notify();
    }

    /// Loads a suggestion on deck B.
    pub(crate) fn load_suggestion(&mut self, track_index: usize, cx: &mut Context<'_, Self>) {
        self.load_on_deck(DeckId::B, track_index, cx);
    }

    /// Works out the tracks that mix best after the one on deck A.
    pub(crate) fn refresh_suggestions(&mut self) {
        let library = self.session.library();
        let Some(playing) = self
            .decks
            .view(DeckId::A)
            .track_index
            .and_then(|index| library.track(index))
        else {
            self.decks.suggestions.clear();
            return;
        };
        let indexes: Vec<usize> = match self.decks.scope {
            SuggestionScope::Library => (0..library.tracks().len()).collect(),
            SuggestionScope::Shown => self.visible.clone(),
        };
        let candidates: Vec<&Track> = indexes
            .iter()
            .filter_map(|index| library.track(*index))
            .filter(|candidate| candidate.path != playing.path)
            .collect();
        let options = MixOptions {
            allow_half_and_double_tempo: true,
            ..MixOptions::default()
        };
        self.decks.suggestions = next_tracks(
            playing,
            candidates.iter().copied(),
            &options,
            self.decks.direction,
        )
        .into_iter()
        .take(SUGGESTION_LIMIT)
        .filter_map(|mix| {
            Some(NextTrack {
                track_index: library.index_of(&mix.track.path)?,
                relation: mix.relation,
                tempo_difference_percent: mix.tempo.map(|tempo| tempo.difference_percent),
                energy_difference: mix.energy_difference,
            })
        })
        .collect();
    }

    /// Draws the decks again while one plays, so their positions move.
    fn follow_decks(&mut self, cx: &mut Context<'_, Self>) {
        if self.decks.is_refreshing {
            return;
        }
        self.decks.is_refreshing = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DECK_REFRESH).await;
                let goes_on = this.update(cx, |this, cx| {
                    cx.notify();
                    let goes_on =
                        this.decks.is_open && this.decks.views.iter().any(|view| view.playing);
                    if !goes_on {
                        this.decks.is_refreshing = false;
                    }
                    goes_on
                });
                if !matches!(goes_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crossfader_keeps_the_loudness_and_the_faders_scale_it() {
        let mut decks = Decks::default();
        let gains = decks.gains();
        assert!((gains[0] - 1.0).abs() < 1e-6 && (gains[1] - 1.0).abs() < 1e-6);
        decks.crossfader = 0.0;
        assert_eq!(decks.gains(), [1.0, 0.0]);
        decks.crossfader = 1.0;
        let gains = decks.gains();
        assert!(gains[0].abs() < 1e-6 && (gains[1] - 1.0).abs() < 1e-6);
        decks.crossfader = 0.25;
        decks.views[0].volume = 0.5;
        let gains = decks.gains();
        assert!((gains[0] - 0.5).abs() < 1e-6, "{gains:?}");
        assert!(gains[1] > 0.5 && gains[1] < 1.0, "{gains:?}");
    }

    #[test]
    fn deck_b_follows_deck_a_only_while_synced() {
        let mut decks = Decks::default();
        decks.views[0].bpm = Some(120.0);
        decks.views[1].bpm = Some(125.0);
        decks.follow_tempo();
        assert!((decks.views[1].rate - 0.96).abs() < 1e-9);
        assert!((decks.views[1].heard_bpm().unwrap() - 120.0).abs() < 1e-9);
        decks.sync = false;
        decks.follow_tempo();
        assert_eq!(decks.views[1].rate, 1.0);
    }
}
