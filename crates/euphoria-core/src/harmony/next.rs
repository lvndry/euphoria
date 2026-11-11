// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ranking the tracks to play next: [`mixes_into`] keeps the candidates
//! whose key and tempo mix with the track playing, and this orders them for
//! where the DJ wants the energy of the set to go.

use std::cmp::Ordering;

use super::mix::{MixMatch, MixOptions, MixTrack, TempoScaling, mixes_into};

/// Where the DJ wants the energy to go with the next track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EnergyDirection {
    /// Lift it: a track one or two points higher fits best.
    Up,
    /// Hold it: a track at the same energy fits best.
    #[default]
    Same,
    /// Bring it down: a track one or two points lower fits best.
    Down,
}

impl EnergyDirection {
    /// Every direction, in the order a control offers them.
    pub const ALL: [EnergyDirection; 3] = [
        EnergyDirection::Down,
        EnergyDirection::Same,
        EnergyDirection::Up,
    ];

    /// The direction's name, as a control shows it.
    pub fn label(self) -> &'static str {
        match self {
            EnergyDirection::Up => "Energy up",
            EnergyDirection::Same => "Same energy",
            EnergyDirection::Down => "Energy down",
        }
    }

    /// How far an energy change of `difference` points is from what this
    /// direction asks for: 0 for no change when holding, and for one or two
    /// points the right way when lifting or bringing down. A change the
    /// wrong way costs more than one too large the right way.
    pub fn distance(self, difference: f32) -> f32 {
        let toward = |change: f32| {
            if change <= 0.0 {
                2.0 + change.abs()
            } else if change < 1.0 {
                1.0 - change
            } else if change <= 2.0 {
                0.0
            } else {
                change - 2.0
            }
        };
        match self {
            EnergyDirection::Same => difference.abs(),
            EnergyDirection::Up => toward(difference),
            EnergyDirection::Down => toward(-difference),
        }
    }
}

/// How much each part weighs in the order of [`next_tracks`]: one step of
/// key relation, from same key to adjacent and so on, weighs as much as two
/// points of tempo difference in percent and one point of energy away from
/// the direction asked for.
const RELATION_STEP: f32 = 2.0;
const TEMPO_PERCENT: f32 = 1.0;
const ENERGY_POINT: f32 = 2.0;
/// The cost of a half or double tempo match over a direct one.
const SCALED_TEMPO: f32 = 1.5;
/// The cost of a candidate whose energy is not known.
const UNKNOWN_ENERGY: f32 = 1.5;

/// The candidates that mix into `reference`, as [`mixes_into`] keeps them,
/// ordered best first for playing next with the energy going `direction`.
///
/// The order weighs together how smooth the key move is, how close the tempo
/// is and how well the change of energy fits the direction. Candidates
/// without a tempo come after those with one, as in [`mixes_into`], and
/// candidates that tie keep the order they were given in.
pub fn next_tracks<'candidates, Reference, Candidate>(
    reference: &Reference,
    candidates: impl IntoIterator<Item = &'candidates Candidate>,
    options: &MixOptions,
    direction: EnergyDirection,
) -> Vec<MixMatch<'candidates, Candidate>>
where
    Reference: MixTrack + ?Sized,
    Candidate: MixTrack + 'candidates,
{
    let mut ranked: Vec<(bool, f32, MixMatch<'candidates, Candidate>)> =
        mixes_into(reference, candidates, options)
            .into_iter()
            .map(|mix_match| {
                (
                    mix_match.tempo.is_none(),
                    score(&mix_match, direction),
                    mix_match,
                )
            })
            .collect();
    ranked.sort_by(|first, second| {
        first
            .0
            .cmp(&second.0)
            .then_with(|| first.1.partial_cmp(&second.1).unwrap_or(Ordering::Equal))
    });
    ranked
        .into_iter()
        .map(|(_, _, mix_match)| mix_match)
        .collect()
}

/// The cost of playing `mix_match` next, lowest best.
fn score<Track>(mix_match: &MixMatch<'_, Track>, direction: EnergyDirection) -> f32 {
    let relation = f32::from(mix_match.relation as u8) * RELATION_STEP;
    let tempo = mix_match.tempo.map_or(0.0, |fit| {
        fit.difference_percent.abs() * TEMPO_PERCENT
            + if fit.scaling == TempoScaling::Direct {
                0.0
            } else {
                SCALED_TEMPO
            }
    });
    let energy = mix_match
        .energy_difference
        .map_or(UNKNOWN_ENERGY, |difference| direction.distance(difference))
        * ENERGY_POINT;
    relation + tempo + energy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::TrackFeatures;

    fn track(code: &str, bpm: f32, energy: f32) -> TrackFeatures {
        TrackFeatures {
            key: Some(code.parse().unwrap()),
            bpm: Some(bpm),
            energy: Some(energy),
        }
    }

    #[test]
    fn the_direction_decides_between_otherwise_equal_candidates() {
        let playing = track("8A", 124.0, 6.0);
        let calmer = track("8A", 124.0, 5.0);
        let same = track("8A", 124.0, 6.0);
        let lifted = track("8A", 124.0, 7.0);
        let candidates = [calmer, same, lifted];
        let order = |direction| -> Vec<f32> {
            next_tracks(&playing, &candidates, &MixOptions::default(), direction)
                .iter()
                .map(|mix| mix.track.energy.unwrap())
                .collect()
        };
        assert_eq!(order(EnergyDirection::Up), [7.0, 6.0, 5.0]);
        assert_eq!(order(EnergyDirection::Same)[0], 6.0);
        assert_eq!(order(EnergyDirection::Down), [5.0, 6.0, 7.0]);
    }

    #[test]
    fn key_tempo_and_energy_are_weighed_together() {
        let playing = track("8A", 124.0, 6.0);
        let same_key_far_tempo = track("8A", 130.0, 7.0);
        let adjacent_close = track("9A", 124.5, 7.0);
        let clashing = track("3B", 124.0, 7.0);
        let candidates = [same_key_far_tempo, adjacent_close, clashing];
        let ranked = next_tracks(
            &playing,
            &candidates,
            &MixOptions::default(),
            EnergyDirection::Up,
        );
        assert_eq!(ranked.len(), 2, "a key that does not mix is left out");
        assert_eq!(ranked[0].track.key, adjacent_close.key);
    }

    #[test]
    fn a_change_the_wrong_way_costs_more_than_a_large_one_the_right_way() {
        assert!(EnergyDirection::Up.distance(3.0) < EnergyDirection::Up.distance(-1.0));
        assert!(EnergyDirection::Down.distance(-3.0) < EnergyDirection::Down.distance(1.0));
        assert_eq!(EnergyDirection::Up.distance(1.0), 0.0);
        assert_eq!(EnergyDirection::Down.distance(-1.0), 0.0);
        assert_eq!(EnergyDirection::Same.distance(0.0), 0.0);
    }
}
