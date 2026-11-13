// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::key::{Key, Mode};

/// How the key of a candidate track relates to the key of a reference track,
/// for the moves that harmonic mixing guides name.
///
/// Variants are declared and ordered from the smoothest mix to the boldest,
/// so sorting relations puts the safest move first.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Relation {
    /// The same key, such as 8A to 8A.
    Same,
    /// One step either way on the Camelot wheel with the same letter, such as
    /// 8A to 7A or 9A. The keys are a fifth apart.
    Adjacent,
    /// The same number with the other letter, such as 8A to 8B: the relative
    /// major or minor.
    Relative,
    /// One step and the other letter, towards the key that shares the most
    /// notes: one number down from a minor key (8A to 7B) and one number up
    /// from a major key (8B to 9A).
    Diagonal,
    /// Two steps up with the same letter, such as 8A to 10A. The key rises by
    /// a whole tone.
    EnergyBoost,
    /// The same home note in the other mode, such as A minor to A major:
    /// three numbers up from a minor key (8A to 11B) and three numbers down
    /// from a major key (8B to 5A).
    MoodChange,
    /// Seven steps up with the same letter, such as 8A to 3A. The key rises
    /// by a semitone.
    SemitoneBoost,
}

impl Relation {
    /// Every relation, from the smoothest mix to the boldest.
    pub const ALL: [Relation; 7] = [
        Relation::Same,
        Relation::Adjacent,
        Relation::Relative,
        Relation::Diagonal,
        Relation::EnergyBoost,
        Relation::MoodChange,
        Relation::SemitoneBoost,
    ];

    /// A short name to show next to a track.
    pub fn label(&self) -> &'static str {
        match self {
            Relation::Same => "Same key",
            Relation::Adjacent => "Adjacent",
            Relation::Relative => "Relative",
            Relation::Diagonal => "Diagonal",
            Relation::EnergyBoost => "Energy boost",
            Relation::MoodChange => "Mood change",
            Relation::SemitoneBoost => "Semitone boost",
        }
    }

    /// One sentence on how the mix sounds.
    pub fn description(&self) -> &'static str {
        match self {
            Relation::Same => {
                "Both tracks are in one key, so the blend is seamless and can run as long as you like."
            }
            Relation::Adjacent => {
                "The keys are a fifth apart and share six of their seven notes, so the blend stays smooth."
            }
            Relation::Relative => {
                "The keys use the same notes, so the blend is smooth while the mood turns between minor and major."
            }
            Relation::Diagonal => {
                "The keys share six of their seven notes, so the blend holds together while the mood shifts."
            }
            Relation::EnergyBoost => {
                "The key rises by a whole tone, a clear lift in energy that works best as a quick mix."
            }
            Relation::MoodChange => {
                "The home note stays while minor and major swap, a dramatic change of mood that suits a quick mix."
            }
            Relation::SemitoneBoost => {
                "The key rises by a semitone, a dramatic lift whose melodies clash, so mix it quickly."
            }
        }
    }
}

impl Key {
    /// How `candidate` relates to this key when mixing from a track in this
    /// key into a track in `candidate`, or `None` when the move has no name
    /// in harmonic mixing.
    ///
    /// The direction matters for some moves: 8A to 10A is an energy boost,
    /// while 10A to 8A is not a named move.
    pub fn relation_to(&self, candidate: &Key) -> Option<Relation> {
        let steps_up = (candidate.camelot_number() + 12 - self.camelot_number()) % 12;
        let same_mode = self.mode() == candidate.mode();
        match (same_mode, steps_up, self.mode()) {
            (true, 0, _) => Some(Relation::Same),
            (true, 1 | 11, _) => Some(Relation::Adjacent),
            (true, 2, _) => Some(Relation::EnergyBoost),
            (true, 7, _) => Some(Relation::SemitoneBoost),
            (false, 0, _) => Some(Relation::Relative),
            (false, 11, Mode::Minor) | (false, 1, Mode::Major) => Some(Relation::Diagonal),
            (false, 3, Mode::Minor) | (false, 9, Mode::Major) => Some(Relation::MoodChange),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seven pitch classes of a key's scale: major, or natural minor.
    fn scale_notes(key: &Key) -> Vec<u8> {
        let intervals: [u8; 7] = match key.mode() {
            Mode::Major => [0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
        };
        intervals
            .iter()
            .map(|interval| (key.tonic() + interval) % 12)
            .collect()
    }

    fn shared_note_count(first: &Key, second: &Key) -> usize {
        let second_notes = scale_notes(second);
        scale_notes(first)
            .iter()
            .filter(|note| second_notes.contains(note))
            .count()
    }

    #[test]
    fn descriptions_state_the_true_number_of_shared_notes() {
        let mut checked_pairs = 0;
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let reference = Key::new(tonic, mode);
                for candidate_tonic in 0..12 {
                    for candidate_mode in [Mode::Major, Mode::Minor] {
                        let candidate = Key::new(candidate_tonic, candidate_mode);
                        let expected_shared = match reference.relation_to(&candidate) {
                            Some(Relation::Same | Relation::Relative) => 7,
                            Some(Relation::Adjacent | Relation::Diagonal) => 6,
                            _ => continue,
                        };
                        assert_eq!(
                            shared_note_count(&reference, &candidate),
                            expected_shared,
                            "{reference} to {candidate}"
                        );
                        checked_pairs += 1;
                    }
                }
            }
        }
        assert_eq!(checked_pairs, 24 * 5);
    }

    #[test]
    fn diagonal_from_a_minor_is_f_major() {
        assert_eq!(key("7B").to_string(), "F major");
        assert_eq!(key("8A").relation_to(&key("7B")), Some(Relation::Diagonal));
        assert_eq!(shared_note_count(&key("8A"), &key("7B")), 6);
    }

    fn key(code: &str) -> Key {
        code.parse().unwrap()
    }

    fn all_keys() -> Vec<Key> {
        [Mode::Minor, Mode::Major]
            .into_iter()
            .flat_map(|mode| (0..12).map(move |tonic| Key::new(tonic, mode)))
            .collect()
    }

    fn code(number: u8, letter: char) -> Key {
        let wrapped = (number + 11) % 12 + 1;
        key(&format!("{wrapped}{letter}"))
    }

    #[test]
    fn names_every_move_from_a_minor_key() {
        let reference = key("8A");
        let expectations = [
            ("8A", Some(Relation::Same)),
            ("7A", Some(Relation::Adjacent)),
            ("9A", Some(Relation::Adjacent)),
            ("8B", Some(Relation::Relative)),
            ("7B", Some(Relation::Diagonal)),
            ("10A", Some(Relation::EnergyBoost)),
            ("11B", Some(Relation::MoodChange)),
            ("3A", Some(Relation::SemitoneBoost)),
            ("9B", None),
            ("6A", None),
            ("5B", None),
            ("2A", None),
        ];
        for (candidate, relation) in expectations {
            assert_eq!(
                reference.relation_to(&key(candidate)),
                relation,
                "8A to {candidate}"
            );
        }
    }

    #[test]
    fn names_every_move_from_a_major_key() {
        let reference = key("8B");
        let expectations = [
            ("8B", Some(Relation::Same)),
            ("7B", Some(Relation::Adjacent)),
            ("9B", Some(Relation::Adjacent)),
            ("8A", Some(Relation::Relative)),
            ("9A", Some(Relation::Diagonal)),
            ("10B", Some(Relation::EnergyBoost)),
            ("5A", Some(Relation::MoodChange)),
            ("3B", Some(Relation::SemitoneBoost)),
            ("7A", None),
            ("6B", None),
            ("11A", None),
            ("2B", None),
        ];
        for (candidate, relation) in expectations {
            assert_eq!(
                reference.relation_to(&key(candidate)),
                relation,
                "8B to {candidate}"
            );
        }
    }

    #[test]
    fn relations_hold_at_every_position_including_across_twelve_and_one() {
        for number in 1..=12 {
            for letter in ['A', 'B'] {
                let other_letter = if letter == 'A' { 'B' } else { 'A' };
                let reference = code(number, letter);
                let relation = |candidate: Key| reference.relation_to(&candidate);

                assert_eq!(relation(code(number, letter)), Some(Relation::Same));
                assert_eq!(relation(code(number + 1, letter)), Some(Relation::Adjacent));
                assert_eq!(
                    relation(code(number + 11, letter)),
                    Some(Relation::Adjacent)
                );
                assert_eq!(
                    relation(code(number, other_letter)),
                    Some(Relation::Relative)
                );
                assert_eq!(
                    relation(code(number + 2, letter)),
                    Some(Relation::EnergyBoost)
                );
                assert_eq!(
                    relation(code(number + 7, letter)),
                    Some(Relation::SemitoneBoost)
                );

                let (diagonal, mood_change) = if letter == 'A' {
                    (code(number + 11, 'B'), code(number + 3, 'B'))
                } else {
                    (code(number + 1, 'A'), code(number + 9, 'A'))
                };
                assert_eq!(relation(diagonal), Some(Relation::Diagonal));
                assert_eq!(relation(mood_change), Some(Relation::MoodChange));
            }
        }
    }

    #[test]
    fn wraps_between_twelve_and_one() {
        assert_eq!(key("12A").relation_to(&key("1A")), Some(Relation::Adjacent));
        assert_eq!(key("1A").relation_to(&key("12A")), Some(Relation::Adjacent));
        assert_eq!(
            key("11B").relation_to(&key("1B")),
            Some(Relation::EnergyBoost)
        );
        assert_eq!(
            key("12B").relation_to(&key("2B")),
            Some(Relation::EnergyBoost)
        );
        assert_eq!(key("1A").relation_to(&key("12B")), Some(Relation::Diagonal));
        assert_eq!(key("12B").relation_to(&key("1A")), Some(Relation::Diagonal));
        assert_eq!(
            key("10A").relation_to(&key("1B")),
            Some(Relation::MoodChange)
        );
        assert_eq!(
            key("2B").relation_to(&key("11A")),
            Some(Relation::MoodChange)
        );
        assert_eq!(
            key("6A").relation_to(&key("1A")),
            Some(Relation::SemitoneBoost)
        );
    }

    #[test]
    fn each_key_has_exactly_eight_named_moves() {
        for reference in all_keys() {
            let named = all_keys()
                .iter()
                .filter(|candidate| reference.relation_to(candidate).is_some())
                .count();
            assert_eq!(named, 8, "{reference}");
        }
    }

    #[test]
    fn relations_match_their_musical_intervals() {
        for reference in all_keys() {
            for candidate in all_keys() {
                let Some(relation) = reference.relation_to(&candidate) else {
                    continue;
                };
                let semitones_up = (candidate.tonic() + 12 - reference.tonic()) % 12;
                let same_mode = reference.mode() == candidate.mode();
                let expected_semitones: &[u8] = match (relation, reference.mode()) {
                    (Relation::Same | Relation::MoodChange, _) => &[0],
                    (Relation::Adjacent, _) => &[5, 7],
                    (Relation::Relative, Mode::Minor) => &[3],
                    (Relation::Relative, Mode::Major) => &[9],
                    (Relation::Diagonal, Mode::Minor) => &[8],
                    (Relation::Diagonal, Mode::Major) => &[4],
                    (Relation::EnergyBoost, _) => &[2],
                    (Relation::SemitoneBoost, _) => &[1],
                };
                assert!(
                    expected_semitones.contains(&semitones_up),
                    "{reference} to {candidate} is {relation:?}"
                );
                let keeps_mode = matches!(
                    relation,
                    Relation::Same
                        | Relation::Adjacent
                        | Relation::EnergyBoost
                        | Relation::SemitoneBoost
                );
                assert_eq!(same_mode, keeps_mode, "{reference} to {candidate}");
            }
        }
    }

    #[test]
    fn symmetric_moves_read_the_same_both_ways() {
        for reference in all_keys() {
            for candidate in all_keys() {
                let forward = reference.relation_to(&candidate);
                let backward = candidate.relation_to(&reference);
                match forward {
                    Some(
                        Relation::Same
                        | Relation::Adjacent
                        | Relation::Relative
                        | Relation::Diagonal
                        | Relation::MoodChange,
                    ) => assert_eq!(backward, forward, "{reference} and {candidate}"),
                    Some(Relation::EnergyBoost | Relation::SemitoneBoost) => {
                        assert_eq!(backward, None, "{candidate} to {reference}");
                    }
                    None => {}
                }
            }
        }
    }

    #[test]
    fn relations_sort_from_smoothest_to_boldest() {
        let mut shuffled = [
            Relation::SemitoneBoost,
            Relation::Relative,
            Relation::MoodChange,
            Relation::Same,
            Relation::EnergyBoost,
            Relation::Diagonal,
            Relation::Adjacent,
        ];
        shuffled.sort();
        assert_eq!(shuffled, Relation::ALL);
    }

    #[test]
    fn every_relation_has_a_distinct_label_and_a_one_sentence_description() {
        let mut labels: Vec<&str> = Relation::ALL.iter().map(Relation::label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Relation::ALL.len());

        for relation in Relation::ALL {
            let description = relation.description();
            assert!(description.ends_with('.'), "{relation:?}");
            assert_eq!(description.matches(". ").count(), 0, "{relation:?}");
        }
    }
}
