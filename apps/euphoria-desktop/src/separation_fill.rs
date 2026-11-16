// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the progress bars of a separation show: the caption, and a fill
//! that eases towards each new reading instead of jumping. The running
//! separation is read when the app polls it, so the bars change at most as
//! often as that.

use std::mem::{Discriminant, discriminant};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::motion::{self, Sources};
use crate::stems::{SeparationPhase, StemSession, StemStatus};

/// What a bar shows for the separation of one track.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SeparationShown {
    pub(crate) caption: String,
    /// How much of the bar is filled, or `None` for a track in line.
    pub(crate) fill: Option<f32>,
}

struct Reading {
    path: PathBuf,
    phase: SeparationPhase,
    stage: Discriminant<SeparationPhase>,
    from: f32,
    target: f32,
    started: Instant,
    is_settled: bool,
}

impl Reading {
    fn value(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        let progress = elapsed / motion::PANEL.as_secs_f32();
        self.from + (self.target - self.from) * motion::ease_out(progress)
    }

    fn is_moving(&self, now: Instant) -> bool {
        self.from != self.target && now.saturating_duration_since(self.started) < motion::PANEL
    }
}

/// The last reading of the running separation, and the fill easing
/// towards it.
#[derive(Default)]
pub(crate) struct SeparationFill {
    reading: Option<Reading>,
    sources: Sources,
}

impl SeparationFill {
    #[cfg(test)]
    pub(crate) fn with_sources(sources: Sources) -> Self {
        Self {
            reading: None,
            sources,
        }
    }

    /// Takes a new reading of the separation that runs. Returns true when
    /// the bars need drawing again: the reading changed, or the fill is
    /// still easing towards it.
    pub(crate) fn follow(&mut self, running: Option<(&Path, SeparationPhase)>) -> bool {
        let now = self.sources.now();
        let Some((path, phase)) = running else {
            return self.reading.take().is_some();
        };
        let target = phase.fill().unwrap_or(0.0);
        let stage = discriminant(&phase);
        let reduces_motion = self.sources.reduces_motion();
        match &mut self.reading {
            Some(reading) if reading.path == path && reading.stage == stage => {
                let has_changed = reading.phase != phase;
                if reading.target != target {
                    let current = reading.value(now);
                    reading.from = if reduces_motion || target < current {
                        target
                    } else {
                        current
                    };
                    reading.target = target;
                    reading.started = now;
                    reading.is_settled = false;
                }
                reading.phase = phase;
                let is_moving = reading.is_moving(now);
                let has_just_settled = !is_moving && !reading.is_settled;
                reading.is_settled = !is_moving;
                has_changed || is_moving || has_just_settled
            }
            _ => {
                self.reading = Some(Reading {
                    path: path.to_path_buf(),
                    phase,
                    stage,
                    from: target,
                    target,
                    started: now,
                    is_settled: true,
                });
                true
            }
        }
    }

    fn shown_for(&self, path: &Path) -> Option<SeparationShown> {
        let reading = self
            .reading
            .as_ref()
            .filter(|reading| reading.path == path)?;
        Some(SeparationShown {
            caption: reading.phase.caption(),
            fill: Some(reading.value(self.sources.now())),
        })
    }
}

/// What the bars show for the track at `path` while its stems are being
/// separated or wait their turn, or `None` when they are not.
pub(crate) fn shown(
    stems: &StemSession,
    fill: &SeparationFill,
    path: &Path,
) -> Option<SeparationShown> {
    if !matches!(stems.status(path), Some(StemStatus::Separating)) {
        return None;
    }
    let phase = stems.phase_of(path).unwrap_or(SeparationPhase::Starting);
    if !matches!(phase, SeparationPhase::Queued { .. })
        && let Some(shown) = fill.shown_for(path)
    {
        return Some(shown);
    }
    Some(SeparationShown {
        caption: phase.caption(),
        fill: phase.fill(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::motion::testing::FakeClock;

    fn separating(done: usize, total: usize) -> SeparationPhase {
        SeparationPhase::Separating {
            done,
            total,
            remaining: None,
        }
    }

    #[test]
    fn the_fill_eases_towards_each_reading_and_stops_asking_for_frames() {
        let clock = FakeClock::new();
        let mut fill = SeparationFill::with_sources(clock.sources());
        let track = Path::new("/music/track.mp3");
        clock.at(0.0);
        assert!(fill.follow(Some((track, separating(1, 4)))));
        assert_eq!(fill.shown_for(track).unwrap().fill, Some(0.25));

        assert!(fill.follow(Some((track, separating(2, 4)))));
        clock.at(0.09);
        let halfway = fill.shown_for(track).unwrap().fill.unwrap();
        assert!(halfway > 0.25 && halfway < 0.5, "{halfway}");
        assert!(fill.follow(Some((track, separating(2, 4)))), "still easing");
        clock.at(0.5);
        assert_eq!(fill.shown_for(track).unwrap().fill, Some(0.5));
        assert!(
            fill.follow(Some((track, separating(2, 4)))),
            "one last frame"
        );
        assert!(!fill.follow(Some((track, separating(2, 4)))), "at rest");
        assert!(fill.follow(None));
        assert_eq!(fill.shown_for(track), None);
    }

    #[test]
    fn with_reduce_motion_or_a_new_stage_the_fill_jumps() {
        let clock = FakeClock::new();
        let mut fill = SeparationFill::with_sources(clock.sources());
        let track = Path::new("/music/track.mp3");
        clock.at(0.0);
        fill.follow(Some((
            track,
            SeparationPhase::Downloading {
                received: 90,
                total: 100,
            },
        )));
        fill.follow(Some((track, separating(0, 8))));
        assert_eq!(fill.shown_for(track).unwrap().fill, Some(0.0));

        clock.reduce.set(true);
        fill.follow(Some((track, separating(4, 8))));
        assert_eq!(fill.shown_for(track).unwrap().fill, Some(0.5));
    }

    #[test]
    fn the_caption_says_the_stage_the_percent_and_the_place_in_line() {
        assert_eq!(SeparationPhase::Starting.caption(), "Separating the stems");
        assert_eq!(
            separating(42, 100).caption(),
            "Separating the stems \u{b7} 42%"
        );
        assert_eq!(
            SeparationPhase::Separating {
                done: 30,
                total: 100,
                remaining: Some(Duration::from_secs(130)),
            }
            .caption(),
            "Separating the stems \u{b7} 30% \u{b7} about 3 min left"
        );
        assert_eq!(
            SeparationPhase::Downloading {
                received: 120_400_000,
                total: 213_200_000,
            }
            .caption(),
            "Separating the stems \u{b7} Downloading the model 120 of 214 MB"
        );
        assert_eq!(
            SeparationPhase::LoadingModel.caption(),
            "Separating the stems \u{b7} Loading the model"
        );
        assert_eq!(
            SeparationPhase::Queued { ahead: 2 }.caption(),
            "Waiting: 2 tracks ahead"
        );
        assert_eq!(
            SeparationPhase::Queued { ahead: 1 }.caption(),
            "Waiting: 1 track ahead"
        );
        assert_eq!(SeparationPhase::Queued { ahead: 1 }.fill(), None);
    }
}
