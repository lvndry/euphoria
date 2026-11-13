// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cmp::Ordering;

use super::relation::Relation;
use crate::key::Key;

/// The largest tempo difference, in percent, that [`MixOptions::default`]
/// accepts.
pub const DEFAULT_MAX_TEMPO_DIFFERENCE_PERCENT: f32 = 6.0;

/// A closed range of tempo in beats per minute, both ends included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BpmRange {
    /// The slowest tempo in the range, in beats per minute.
    pub slowest: f32,
    /// The fastest tempo in the range, in beats per minute.
    pub fastest: f32,
}

impl BpmRange {
    /// A range from `slowest` to `fastest`, inclusive at both ends. `None`
    /// when the ends do not make a range.
    pub fn new(slowest: f32, fastest: f32) -> Option<Self> {
        (slowest.is_finite() && fastest.is_finite() && slowest <= fastest).then_some(Self {
            slowest,
            fastest,
        })
    }

    /// True when `bpm` lies in the range, both ends included.
    pub fn contains(self, bpm: f32) -> bool {
        self.slowest <= bpm && bpm <= self.fastest
    }
}

/// A closed range of energy on the scale the tracks are rated on, both ends
/// included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyRange {
    /// The lowest energy in the range.
    pub lowest: f32,
    /// The highest energy in the range.
    pub highest: f32,
}

impl EnergyRange {
    /// A range from `lowest` to `highest`, inclusive at both ends. `None`
    /// when the ends do not make a range.
    pub fn new(lowest: f32, highest: f32) -> Option<Self> {
        (lowest.is_finite() && highest.is_finite() && lowest <= highest).then_some(Self {
            lowest,
            highest,
        })
    }

    /// True when `energy` lies in the range, both ends included.
    pub fn contains(self, energy: f32) -> bool {
        self.lowest <= energy && energy <= self.highest
    }
}

/// What the ranking needs to know about a track. Front ends implement this
/// for their own track type.
pub trait MixTrack {
    /// The key of the track, or `None` when it is unknown.
    fn key(&self) -> Option<Key>;

    /// The tempo in beats per minute, or `None` when it is unknown.
    fn bpm(&self) -> Option<f32>;

    /// The energy level of the track, or `None` when it is unknown. Any scale
    /// works as long as every track uses the same one.
    fn energy(&self) -> Option<f32>;
}

impl<Track: MixTrack + ?Sized> MixTrack for &Track {
    fn key(&self) -> Option<Key> {
        (**self).key()
    }

    fn bpm(&self) -> Option<f32> {
        (**self).bpm()
    }

    fn energy(&self) -> Option<f32> {
        (**self).energy()
    }
}

/// The key, tempo and energy of a track, for callers that have no track type
/// of their own.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrackFeatures {
    /// The key of the track.
    pub key: Option<Key>,
    /// The tempo in beats per minute.
    pub bpm: Option<f32>,
    /// The energy level of the track.
    pub energy: Option<f32>,
}

impl MixTrack for TrackFeatures {
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

/// Which candidates count as mixing into the reference track.
#[derive(Debug, Clone, PartialEq)]
pub struct MixOptions {
    /// The key relations that count as compatible.
    pub relations: Vec<Relation>,
    /// The largest tempo difference that counts, in percent of the reference
    /// tempo. A candidate exactly this far away still counts.
    pub max_tempo_difference_percent: f32,
    /// Whether a candidate at about half or double the reference tempo counts
    /// as a tempo match.
    pub allow_half_and_double_tempo: bool,
    /// The tempo of the candidates in beats per minute, or `None` for any
    /// tempo. A candidate's own tempo must lie in the range; one without a
    /// tempo is left out, as it cannot be checked. The pool is narrowed
    /// before the tempo window, so a range wider than the window keeps
    /// more than the window's edge does.
    pub bpm_range: Option<BpmRange>,
    /// The energy of the candidates, on the scale the tracks are rated on,
    /// or `None` for any energy. A candidate's energy must lie in the
    /// range; one without an energy is left out of a pool narrowed to one.
    pub energy_range: Option<EnergyRange>,
}

impl Default for MixOptions {
    /// Same, adjacent and relative keys within 6 percent of the reference
    /// tempo, without half or double tempo matches.
    fn default() -> Self {
        Self {
            relations: vec![Relation::Same, Relation::Adjacent, Relation::Relative],
            max_tempo_difference_percent: DEFAULT_MAX_TEMPO_DIFFERENCE_PERCENT,
            allow_half_and_double_tempo: false,
            bpm_range: None,
            energy_range: None,
        }
    }
}

/// How a candidate's tempo was read to compare it with the reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TempoScaling {
    /// The tempos were compared as they are.
    Direct,
    /// The candidate runs at about half the reference tempo, so it was
    /// compared at twice its tempo: 70 against a reference of 140.
    HalfTempo,
    /// The candidate runs at about double the reference tempo, so it was
    /// compared at half its tempo: 174 against a reference of 87.
    DoubleTempo,
}

impl TempoScaling {
    /// What the candidate's tempo is multiplied by before the comparison.
    pub fn factor(self) -> f32 {
        match self {
            TempoScaling::Direct => 1.0,
            TempoScaling::HalfTempo => 2.0,
            TempoScaling::DoubleTempo => 0.5,
        }
    }
}

/// How far a candidate's tempo is from the reference tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoFit {
    /// The difference in percent of the reference tempo, after `scaling`.
    /// Positive when the candidate is faster than the reference.
    pub difference_percent: f32,
    /// How the candidate's tempo was read for the comparison.
    pub scaling: TempoScaling,
}

/// A candidate that mixes into the reference track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixMatch<'candidates, Track> {
    /// The candidate track.
    pub track: &'candidates Track,
    /// How the candidate's key relates to the reference key.
    pub relation: Relation,
    /// The tempo comparison, or `None` when the reference or the candidate
    /// has no tempo.
    pub tempo: Option<TempoFit>,
    /// The candidate's energy minus the reference's, or `None` when either
    /// has no energy.
    pub energy_difference: Option<f32>,
}

/// A candidate is kept when it lies in the pool the options narrow to
/// ([`MixOptions::bpm_range`] and [`MixOptions::energy_range`], when named),
/// when its key has one of the relations in [`MixOptions::relations`] to the
/// reference key, and when its tempo is within
/// [`MixOptions::max_tempo_difference_percent`] of the reference tempo.
/// Candidates without a key are left out. Candidates without a tempo are
/// kept, since the tempo cannot rule them out, and so is every candidate with
/// a compatible key when the reference has no tempo, unless the pool is
/// narrowed to a tempo or an energy range. The result is empty when the
/// reference has no key.
/// kept, since the tempo cannot rule them out, and so is every candidate with
/// a compatible key when the reference has no tempo. The result is empty when
/// the reference has no key.
///
/// The order is: candidates with a tempo before those without, then by
/// relation from smoothest to boldest, then direct tempo matches before half
/// and double tempo ones, then by smallest tempo difference, then by smallest
/// energy difference. Candidates that tie keep the order they were given in.
///
/// Candidates are not compared with the reference for identity, so a caller
/// that ranks a library against one of its own tracks leaves that track out
/// of `candidates`.
pub fn mixes_into<'candidates, Reference, Candidate>(
    reference: &Reference,
    candidates: impl IntoIterator<Item = &'candidates Candidate>,
    options: &MixOptions,
) -> Vec<MixMatch<'candidates, Candidate>>
where
    Reference: MixTrack + ?Sized,
    Candidate: MixTrack + 'candidates,
{
    let Some(reference_key) = reference.key() else {
        return Vec::new();
    };
    let reference_bpm = usable_bpm(reference.bpm());
    let reference_energy = usable_energy(reference.energy());

    let mut ranked: Vec<(bool, MixMatch<'candidates, Candidate>)> = candidates
        .into_iter()
        .filter(|candidate| pool_fits(candidate, options.bpm_range, options.energy_range))
        .filter_map(|candidate| {
            let relation = reference_key.relation_to(&candidate.key()?)?;
            if !options.relations.contains(&relation) {
                return None;
            }

            let candidate_bpm = usable_bpm(candidate.bpm());
            let tempo = match (reference_bpm, candidate_bpm) {
                (Some(reference_bpm), Some(candidate_bpm)) => Some(tempo_fit(
                    reference_bpm,
                    candidate_bpm,
                    options.max_tempo_difference_percent,
                    options.allow_half_and_double_tempo,
                )?),
                _ => None,
            };
            let energy_difference = usable_energy(candidate.energy())
                .zip(reference_energy)
                .map(|(candidate_energy, reference_energy)| candidate_energy - reference_energy);

            Some((
                candidate_bpm.is_none(),
                MixMatch {
                    track: candidate,
                    relation,
                    tempo,
                    energy_difference,
                },
            ))
        })
        .collect();

    ranked.sort_by(|(first_lacks_bpm, first), (second_lacks_bpm, second)| {
        first_lacks_bpm
            .cmp(second_lacks_bpm)
            .then_with(|| smoothness_order(first, second))
    });
    ranked.into_iter().map(|(_, mix_match)| mix_match).collect()
}
/// True when `candidate` lies in the pool the caller narrows to: its tempo
/// in `bpm_range` and its energy in `energy_range`, when a range is named.
/// A candidate that lacks the value a range asks for is out of the pool.
pub(super) fn pool_fits<Track: MixTrack + ?Sized>(
    track: &Track,
    bpm_range: Option<BpmRange>,
    energy_range: Option<EnergyRange>,
) -> bool {
    if let Some(range) = bpm_range {
        let Some(bpm) = usable_bpm(track.bpm()) else {
            return false;
        };
        if !range.contains(bpm) {
            return false;
        }
    }
    if let Some(range) = energy_range {
        let Some(energy) = usable_energy(track.energy()) else {
            return false;
        };
        if !range.contains(energy) {
            return false;
        }
    }
    true
}


pub(super) fn usable_bpm(bpm: Option<f32>) -> Option<f32> {
    bpm.filter(|bpm| bpm.is_finite() && *bpm > 0.0)
}

pub(super) fn usable_energy(energy: Option<f32>) -> Option<f32> {
    energy.filter(|energy| energy.is_finite())
}

/// The closest reading of the candidate tempo, at half or double only when
/// `allow_half_and_double_tempo`, or `None` when even that one is more than
/// `max_difference_percent` away.
pub(super) fn tempo_fit(
    reference_bpm: f32,
    candidate_bpm: f32,
    max_difference_percent: f32,
    allow_half_and_double_tempo: bool,
) -> Option<TempoFit> {
    let scalings: &[TempoScaling] = if allow_half_and_double_tempo {
        &[
            TempoScaling::Direct,
            TempoScaling::HalfTempo,
            TempoScaling::DoubleTempo,
        ]
    } else {
        &[TempoScaling::Direct]
    };
    scalings
        .iter()
        .map(|&scaling| TempoFit {
            difference_percent: (candidate_bpm * scaling.factor() - reference_bpm) * 100.0
                / reference_bpm,
            scaling,
        })
        .min_by(|first, second| {
            first
                .difference_percent
                .abs()
                .total_cmp(&second.difference_percent.abs())
        })
        .filter(|fit| fit.difference_percent.abs() <= max_difference_percent)
}

fn smoothness_order<Track>(first: &MixMatch<'_, Track>, second: &MixMatch<'_, Track>) -> Ordering {
    let scaled = |mix_match: &MixMatch<'_, Track>| {
        mix_match
            .tempo
            .is_some_and(|fit| fit.scaling != TempoScaling::Direct)
    };
    let tempo_distance = |mix_match: &MixMatch<'_, Track>| {
        mix_match
            .tempo
            .map_or(0.0, |fit| fit.difference_percent.abs())
    };
    let energy_distance = |mix_match: &MixMatch<'_, Track>| {
        mix_match.energy_difference.map_or(f32::INFINITY, f32::abs)
    };

    first
        .relation
        .cmp(&second.relation)
        .then_with(|| scaled(first).cmp(&scaled(second)))
        .then_with(|| tempo_distance(first).total_cmp(&tempo_distance(second)))
        .then_with(|| energy_distance(first).total_cmp(&energy_distance(second)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct LibraryTrack {
        title: &'static str,
        key: Option<Key>,
        bpm: Option<f32>,
        energy: Option<f32>,
    }

    impl MixTrack for LibraryTrack {
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

    fn track(title: &'static str, code: &str, bpm: f32) -> LibraryTrack {
        LibraryTrack {
            title,
            key: Some(code.parse().unwrap()),
            bpm: Some(bpm),
            energy: None,
        }
    }

    fn reference(code: &str, bpm: f32) -> TrackFeatures {
        TrackFeatures {
            key: Some(code.parse().unwrap()),
            bpm: Some(bpm),
            energy: None,
        }
    }

    fn every_relation() -> MixOptions {
        MixOptions {
            relations: Relation::ALL.to_vec(),
            ..MixOptions::default()
        }
    }

    fn titles(matches: &[MixMatch<'_, LibraryTrack>]) -> Vec<&'static str> {
        matches
            .iter()
            .map(|mix_match| mix_match.track.title)
            .collect()
    }

    #[test]
    fn default_options_keep_same_adjacent_and_relative_keys() {
        let options = MixOptions::default();
        assert_eq!(
            options.relations,
            [Relation::Same, Relation::Adjacent, Relation::Relative]
        );
        assert_eq!(options.max_tempo_difference_percent, 6.0);
        assert!(!options.allow_half_and_double_tempo);

        let candidates = [
            track("same", "8A", 120.0),
            track("down", "7A", 120.0),
            track("up", "9A", 120.0),
            track("relative", "8B", 120.0),
            track("diagonal", "7B", 120.0),
            track("energy boost", "10A", 120.0),
            track("mood change", "11B", 120.0),
            track("semitone boost", "3A", 120.0),
            track("unrelated", "2A", 120.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(titles(&matches), ["same", "down", "up", "relative"]);
    }

    #[test]
    fn reports_the_relation_of_every_enabled_move() {
        let candidates = [
            track("semitone boost", "3A", 120.0),
            track("mood change", "11B", 120.0),
            track("energy boost", "10A", 120.0),
            track("diagonal", "7B", 120.0),
            track("relative", "8B", 120.0),
            track("adjacent", "9A", 120.0),
            track("same", "8A", 120.0),
            track("unrelated", "2A", 120.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &every_relation());
        let relations: Vec<Relation> = matches.iter().map(|mix_match| mix_match.relation).collect();
        assert_eq!(relations, Relation::ALL);
        assert_eq!(
            titles(&matches),
            [
                "same",
                "adjacent",
                "relative",
                "diagonal",
                "energy boost",
                "mood change",
                "semitone boost"
            ]
        );
    }

    #[test]
    fn only_the_chosen_relations_count() {
        let candidates = [
            track("same", "8A", 120.0),
            track("energy boost", "10A", 120.0),
        ];
        let options = MixOptions {
            relations: vec![Relation::EnergyBoost],
            ..MixOptions::default()
        };
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(titles(&matches), ["energy boost"]);

        let nothing_counts = MixOptions {
            relations: Vec::new(),
            ..MixOptions::default()
        };
        assert!(mixes_into(&reference("8A", 120.0), &candidates, &nothing_counts).is_empty());
    }

    #[test]
    fn adjacent_keys_wrap_between_twelve_and_one() {
        let candidates = [
            track("one", "1A", 120.0),
            track("eleven", "11A", 120.0),
            track("two", "2A", 120.0),
        ];
        let matches = mixes_into(
            &reference("12A", 120.0),
            &candidates,
            &MixOptions::default(),
        );
        assert_eq!(titles(&matches), ["one", "eleven"]);

        let candidates = [track("twelve", "12B", 120.0), track("two", "2B", 120.0)];
        let matches = mixes_into(&reference("1B", 120.0), &candidates, &MixOptions::default());
        assert_eq!(titles(&matches), ["twelve", "two"]);
    }

    #[test]
    fn tempo_window_includes_its_edges() {
        let candidates = [
            track("below the window", "8A", 93.5),
            track("lower edge", "8A", 94.0),
            track("upper edge", "8A", 106.0),
            track("above the window", "8A", 106.5),
        ];
        let matches = mixes_into(&reference("8A", 100.0), &candidates, &MixOptions::default());
        assert_eq!(titles(&matches), ["lower edge", "upper edge"]);
        assert_eq!(matches[0].tempo.unwrap().difference_percent, -6.0);
        assert_eq!(matches[1].tempo.unwrap().difference_percent, 6.0);
    }

    #[test]
    fn tempo_window_follows_the_option() {
        let candidates = [
            track("three percent", "8A", 103.0),
            track("ten percent", "8A", 110.0),
        ];
        let narrow = MixOptions {
            max_tempo_difference_percent: 2.0,
            ..MixOptions::default()
        };
        assert!(mixes_into(&reference("8A", 100.0), &candidates, &narrow).is_empty());

        let wide = MixOptions {
            max_tempo_difference_percent: 10.0,
            ..MixOptions::default()
        };
        let matches = mixes_into(&reference("8A", 100.0), &candidates, &wide);
        assert_eq!(titles(&matches), ["three percent", "ten percent"]);

        let exact_only = MixOptions {
            max_tempo_difference_percent: 0.0,
            ..MixOptions::default()
        };
        let candidates = [track("identical", "8A", 100.0), track("close", "8A", 100.5)];
        let matches = mixes_into(&reference("8A", 100.0), &candidates, &exact_only);
        assert_eq!(titles(&matches), ["identical"]);
    }

    #[test]
    fn tempo_difference_is_signed_relative_to_the_reference() {
        let candidates = [track("faster", "8A", 126.0), track("slower", "8A", 114.0)];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default());
        let fits: Vec<TempoFit> = matches
            .iter()
            .map(|mix_match| mix_match.tempo.unwrap())
            .collect();
        assert_eq!(
            fits,
            [
                TempoFit {
                    difference_percent: 5.0,
                    scaling: TempoScaling::Direct
                },
                TempoFit {
                    difference_percent: -5.0,
                    scaling: TempoScaling::Direct
                },
            ]
        );
    }

    #[test]
    fn half_and_double_tempo_are_rejected_unless_allowed() {
        let candidates = [
            track("half", "8A", 70.0),
            track("double", "8A", 280.0),
            track("direct", "8A", 140.0),
        ];
        let matches = mixes_into(&reference("8A", 140.0), &candidates, &MixOptions::default());
        assert_eq!(titles(&matches), ["direct"]);
    }

    #[test]
    fn half_and_double_tempo_count_when_allowed() {
        let options = MixOptions {
            allow_half_and_double_tempo: true,
            ..MixOptions::default()
        };
        let candidates = [
            track("half", "8A", 70.0),
            track("double", "8A", 280.0),
            track("direct", "8A", 140.0),
            track("neither", "8A", 100.0),
        ];
        let matches = mixes_into(&reference("8A", 140.0), &candidates, &options);
        assert_eq!(titles(&matches), ["direct", "half", "double"]);
        assert_eq!(
            matches[1].tempo.unwrap(),
            TempoFit {
                difference_percent: 0.0,
                scaling: TempoScaling::HalfTempo
            }
        );
        assert_eq!(
            matches[2].tempo.unwrap(),
            TempoFit {
                difference_percent: 0.0,
                scaling: TempoScaling::DoubleTempo
            }
        );
    }

    #[test]
    fn half_and_double_tempo_use_the_same_window() {
        let options = MixOptions {
            allow_half_and_double_tempo: true,
            ..MixOptions::default()
        };
        let candidates = [
            track("half, below the window", "8A", 46.5),
            track("half, lower edge", "8A", 47.0),
            track("half, upper edge", "8A", 53.0),
            track("half, above the window", "8A", 53.5),
            track("double, below the window", "8A", 187.0),
            track("double, lower edge", "8A", 188.0),
            track("double, upper edge", "8A", 212.0),
            track("double, above the window", "8A", 213.0),
        ];
        let matches = mixes_into(&reference("8A", 100.0), &candidates, &options);
        let found: Vec<(&str, f32, TempoScaling)> = matches
            .iter()
            .map(|mix_match| {
                let fit = mix_match.tempo.unwrap();
                (mix_match.track.title, fit.difference_percent, fit.scaling)
            })
            .collect();
        assert_eq!(
            found,
            [
                ("half, lower edge", -6.0, TempoScaling::HalfTempo),
                ("half, upper edge", 6.0, TempoScaling::HalfTempo),
                ("double, lower edge", -6.0, TempoScaling::DoubleTempo),
                ("double, upper edge", 6.0, TempoScaling::DoubleTempo),
            ]
        );
    }

    #[test]
    fn direct_tempo_matches_come_before_half_and_double_ones() {
        let options = MixOptions {
            allow_half_and_double_tempo: true,
            ..MixOptions::default()
        };
        let candidates = [
            track("half, exact", "8A", 60.0),
            track("direct, five percent off", "8A", 126.0),
            track("adjacent, half", "9A", 60.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(
            titles(&matches),
            ["direct, five percent off", "half, exact", "adjacent, half"]
        );
    }

    #[test]
    fn sorts_by_relation_then_by_tempo_distance() {
        let candidates = [
            track("relative, exact", "8B", 120.0),
            track("adjacent, far", "9A", 126.0),
            track("same, far", "8A", 115.0),
            track("adjacent, near", "7A", 121.0),
            track("same, near", "8A", 122.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default());
        assert_eq!(
            titles(&matches),
            [
                "same, near",
                "same, far",
                "adjacent, near",
                "adjacent, far",
                "relative, exact"
            ]
        );
    }

    #[test]
    fn candidates_without_a_key_are_left_out() {
        let candidates = [
            LibraryTrack {
                title: "no key",
                key: None,
                bpm: Some(120.0),
                energy: None,
            },
            LibraryTrack {
                title: "nothing known",
                key: None,
                bpm: None,
                energy: None,
            },
            track("same", "8A", 120.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &every_relation());
        assert_eq!(titles(&matches), ["same"]);
    }

    #[test]
    fn candidates_without_a_tempo_are_kept_and_come_last() {
        let without_tempo = |title, code: &str| LibraryTrack {
            title,
            key: Some(code.parse().unwrap()),
            bpm: None,
            energy: None,
        };
        let candidates = [
            without_tempo("same, no tempo", "8A"),
            track("relative", "8B", 125.0),
            without_tempo("adjacent, no tempo", "9A"),
            track("adjacent", "9A", 120.0),
            track("same, too fast", "8A", 150.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default());
        assert_eq!(
            titles(&matches),
            [
                "adjacent",
                "relative",
                "same, no tempo",
                "adjacent, no tempo"
            ]
        );
        assert_eq!(matches[2].tempo, None);
        assert_eq!(matches[3].tempo, None);
    }

    #[test]
    fn unusable_tempos_count_as_missing() {
        let candidates = [
            track("zero", "8A", 0.0),
            track("negative", "8A", -120.0),
            track("not a number", "8A", f32::NAN),
            track("infinite", "8A", f32::INFINITY),
            track("real", "9A", 120.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default());
        assert_eq!(
            titles(&matches),
            ["real", "zero", "negative", "not a number", "infinite"]
        );
        assert!(
            matches[1..]
                .iter()
                .all(|mix_match| mix_match.tempo.is_none())
        );
    }

    #[test]
    fn reference_without_a_tempo_keeps_every_compatible_key() {
        let reference = TrackFeatures {
            key: Some("8A".parse().unwrap()),
            bpm: None,
            energy: None,
        };
        let candidates = [
            track("adjacent, fast", "9A", 174.0),
            track("same, slow", "8A", 70.0),
            track("unrelated", "2A", 120.0),
            LibraryTrack {
                title: "same, no tempo",
                key: Some("8A".parse().unwrap()),
                bpm: None,
                energy: None,
            },
        ];
        let matches = mixes_into(&reference, &candidates, &MixOptions::default());
        assert_eq!(
            titles(&matches),
            ["same, slow", "adjacent, fast", "same, no tempo"]
        );
        assert!(matches.iter().all(|mix_match| mix_match.tempo.is_none()));

        let zero_tempo_reference = TrackFeatures {
            bpm: Some(0.0),
            ..reference
        };
        let matches = mixes_into(&zero_tempo_reference, &candidates, &MixOptions::default());
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn reference_without_a_key_matches_nothing() {
        let reference = TrackFeatures {
            key: None,
            bpm: Some(120.0),
            energy: Some(5.0),
        };
        let candidates = [track("same tempo", "8A", 120.0)];
        assert!(mixes_into(&reference, &candidates, &every_relation()).is_empty());
    }

    #[test]
    fn no_candidates_give_no_matches() {
        let candidates: [LibraryTrack; 0] = [];
        assert!(
            mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default()).is_empty()
        );
    }

    #[test]
    fn energy_difference_is_reported_and_breaks_ties() {
        let with_energy = |title, energy: Option<f32>| LibraryTrack {
            title,
            key: Some("8A".parse().unwrap()),
            bpm: Some(120.0),
            energy,
        };
        let reference = TrackFeatures {
            key: Some("8A".parse().unwrap()),
            bpm: Some(120.0),
            energy: Some(6.0),
        };
        let candidates = [
            with_energy("no energy", None),
            with_energy("three lower", Some(3.0)),
            with_energy("one higher", Some(7.0)),
            with_energy("same energy", Some(6.0)),
        ];
        let matches = mixes_into(&reference, &candidates, &MixOptions::default());
        assert_eq!(
            titles(&matches),
            ["same energy", "one higher", "three lower", "no energy"]
        );
        let differences: Vec<Option<f32>> = matches
            .iter()
            .map(|mix_match| mix_match.energy_difference)
            .collect();
        assert_eq!(differences, [Some(0.0), Some(1.0), Some(-3.0), None]);
    }

    #[test]
    fn energy_difference_needs_both_energies() {
        let candidates = [LibraryTrack {
            title: "has energy",
            key: Some("8A".parse().unwrap()),
            bpm: Some(120.0),
            energy: Some(7.0),
        }];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default());
        assert_eq!(matches[0].energy_difference, None);
    }

    #[test]
    fn tied_candidates_keep_their_given_order() {
        let candidates = [
            track("first", "8A", 120.0),
            track("second", "8A", 120.0),
            track("third", "8A", 120.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &MixOptions::default());
        assert_eq!(titles(&matches), ["first", "second", "third"]);
    }

    #[test]
    fn matches_borrow_the_caller_tracks() {
        let candidates = [track("down", "7A", 120.0), track("same", "8A", 120.0)];
        let borrowed: Vec<&LibraryTrack> = candidates.iter().collect();
        let matches = mixes_into(&candidates[1], borrowed, &MixOptions::default());
        assert!(std::ptr::eq(matches[0].track, &candidates[1]));
        assert!(std::ptr::eq(matches[1].track, &candidates[0]));
    }

    fn tracked(title: &'static str, code: &str, bpm: f32, energy: Option<f32>) -> LibraryTrack {
        LibraryTrack {
            title,
            key: Some(code.parse().unwrap()),
            bpm: Some(bpm),
            energy,
        }
    }

    #[test]
    fn defaults_keep_every_candidate_the_options_do_not_narrow() {
        let options = MixOptions::default();
        assert!(options.bpm_range.is_none());
        assert!(options.energy_range.is_none());
        let candidates = [tracked("no energy", "8A", 120.0, None)];
        assert_eq!(
            titles(&mixes_into(&reference("8A", 120.0), &candidates, &options)),
            ["no energy"]
        );
    }

    #[test]
    fn a_tempo_range_keeps_only_the_candidates_in_it() {
        let options = MixOptions {
            bpm_range: Some(BpmRange::new(118.0, 122.0).unwrap()),
            ..MixOptions::default()
        };
        let candidates = [
            track("slow", "8A", 112.0),
            track("in range", "8A", 120.0),
            track("fast", "8A", 128.0),
            track("at the edge", "8A", 122.0),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(titles(&matches), ["in range", "at the edge"]);
    }

    #[test]
    fn a_tempo_range_leaves_out_a_candidate_without_a_tempo() {
        let options = MixOptions {
            bpm_range: Some(BpmRange::new(100.0, 140.0).unwrap()),
            ..MixOptions::default()
        };
        let reference = TrackFeatures {
            key: Some("8A".parse().unwrap()),
            bpm: None,
            energy: None,
        };
        let candidates = [
            track("has tempo", "8A", 120.0),
            LibraryTrack {
                title: "no tempo",
                key: Some("8A".parse().unwrap()),
                bpm: None,
                energy: None,
            },
        ];
        let matches = mixes_into(&reference, &candidates, &options);
        assert_eq!(titles(&matches), ["has tempo"]);
    }

    #[test]
    fn a_tempo_range_wider_than_the_window_keeps_everything_the_window_does() {
        let options = MixOptions {
            max_tempo_difference_percent: 2.0,
            bpm_range: Some(BpmRange::new(90.0, 160.0).unwrap()),
            ..MixOptions::default()
        };
        let candidates = [track("in both", "8A", 119.0), track("out of the window", "8A", 110.0)];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(titles(&matches), ["in both"]);
    }

    #[test]
    fn an_energy_range_keeps_only_the_candidates_in_it() {
        let options = MixOptions {
            energy_range: Some(EnergyRange::new(5.0, 8.0).unwrap()),
            ..MixOptions::default()
        };
        let candidates = [
            tracked("low", "8A", 120.0, Some(4.0)),
            tracked("in range", "8A", 120.0, Some(7.0)),
            tracked("high", "8A", 120.0, Some(9.0)),
            tracked("no energy", "8A", 120.0, None),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(titles(&matches), ["in range"]);
    }

    #[test]
    fn ranges_apply_before_the_relations_and_the_window() {
        let options = MixOptions {
            relations: vec![Relation::Same],
            bpm_range: Some(BpmRange::new(115.0, 125.0).unwrap()),
            energy_range: Some(EnergyRange::new(1.0, 10.0).unwrap()),
            ..MixOptions::default()
        };
        let candidates = [
            tracked("fits", "8A", 120.0, Some(5.0)),
            tracked("wrong key", "7A", 120.0, Some(5.0)),
            tracked("wrong tempo", "8A", 130.0, Some(5.0)),
            tracked("no energy", "8A", 120.0, None),
        ];
        let matches = mixes_into(&reference("8A", 120.0), &candidates, &options);
        assert_eq!(titles(&matches), ["fits"]);
    }

    #[test]
    fn range_ends_are_inclusive_and_bad_ends_make_no_range() {
        assert!(BpmRange::new(120.0, 120.0).unwrap().contains(120.0));
        assert_eq!(BpmRange::new(125.0, 120.0), None);
        assert_eq!(BpmRange::new(f32::NAN, 120.0), None);
        assert!(EnergyRange::new(1.0, 10.0).unwrap().contains(1.0));
        assert_eq!(EnergyRange::new(10.0, 1.0), None);
    }
}
