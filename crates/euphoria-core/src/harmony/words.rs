// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The words a DJ reads for how two tracks fit: the key relation and the
//! tempo move. Every surface that names them takes the words from here.

use super::Relation;

/// A tempo change under this many percent is easy to match by ear.
pub const EASY_BELOW_PERCENT: f32 = 3.0;
/// A tempo change over this many percent is a big jump.
pub const BIG_JUMP_OVER_PERCENT: f32 = 6.0;

/// The column and field name for the tempo change between two tracks.
pub const TEMPO_LABEL: &str = "Tempo";

/// What the tempo change between two tracks asks of the pitch fader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempoMove {
    /// Under 3 percent: easy to match by ear.
    Easy,
    /// From 3 to 6 percent: a move of the pitch fader.
    PitchMove,
    /// Over 6 percent: a big jump.
    BigJump,
}

/// How a chip is coloured: never the error red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Good news.
    Success,
    /// Plain information.
    Neutral,
    /// Worth a look.
    Warning,
}

impl TempoMove {
    /// Every move, from the easiest to the biggest.
    pub const ALL: [TempoMove; 3] = [TempoMove::Easy, TempoMove::PitchMove, TempoMove::BigJump];

    /// The move for a tempo change of `percent`, up or down.
    pub fn of(percent: f32) -> Self {
        let size = percent.abs();
        if size < EASY_BELOW_PERCENT {
            TempoMove::Easy
        } else if size <= BIG_JUMP_OVER_PERCENT {
            TempoMove::PitchMove
        } else {
            TempoMove::BigJump
        }
    }

    /// The words for the move, as a column or a chip shows them.
    pub fn label(self) -> &'static str {
        match self {
            TempoMove::Easy => "Easy",
            TempoMove::PitchMove => "Pitch move",
            TempoMove::BigJump => "Big jump",
        }
    }

    /// How a chip for the move is coloured.
    pub fn tone(self) -> Tone {
        match self {
            TempoMove::Easy => Tone::Success,
            TempoMove::PitchMove => Tone::Neutral,
            TempoMove::BigJump => Tone::Warning,
        }
    }
}

/// What the key column says when two tracks have no relation on the wheel.
pub const KEY_CLASH_LABEL: &str = "Key clash";

/// The name of a key relation for a person mixing.
pub fn relation_name(relation: Relation) -> &'static str {
    relation.label()
}

/// What the key column of a set says for the move to a track: the name of
/// its relation on the wheel, or [`KEY_CLASH_LABEL`] when it has none.
pub fn movement_label(relation: Option<Relation>) -> &'static str {
    relation.map_or(KEY_CLASH_LABEL, relation_name)
}

/// The hint that explains a relation name, where the name alone does not.
pub fn relation_hint(relation: Relation) -> Option<&'static str> {
    match relation {
        Relation::Diagonal => Some("One step and a major/minor switch"),
        _ => None,
    }
}

/// The name of the vocal and bass overlap check.
pub const OVERLAP_LABEL: &str = "Vocals and bass overlap";
/// The link that separates the stems the overlap check needs.
pub const SEPARATE_STEMS_LINK: &str = "Separate stems to check";
/// The name of the set builder's source of tracks.
pub const PICK_FROM_LABEL: &str = "Pick from";
/// The name of the focus view's menu of rows to show.
pub const LAYERS_LABEL: &str = "Layers";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_builder_names_moves_with_the_shared_words() {
        assert_eq!(
            movement_label(Some(Relation::Adjacent)),
            "One step on the wheel"
        );
        assert_eq!(movement_label(Some(Relation::EnergyBoost)), "Energy boost");
        assert_eq!(movement_label(None), "Key clash");
        for relation in [
            Relation::Same,
            Relation::Adjacent,
            Relation::Relative,
            Relation::Diagonal,
            Relation::EnergyBoost,
        ] {
            assert_ne!(movement_label(Some(relation)), "Adjacent");
            assert_eq!(movement_label(Some(relation)), relation_name(relation));
        }
    }

    #[test]
    fn tempo_thresholds_pick_the_chip_and_its_tone() {
        let cases = [
            (0.0, "Easy", Tone::Success),
            (2.99, "Easy", Tone::Success),
            (-2.0, "Easy", Tone::Success),
            (3.0, "Pitch move", Tone::Neutral),
            (-4.5, "Pitch move", Tone::Neutral),
            (6.0, "Pitch move", Tone::Neutral),
            (6.01, "Big jump", Tone::Warning),
            (-9.0, "Big jump", Tone::Warning),
        ];
        for (percent, label, tone) in cases {
            let tempo_move = TempoMove::of(percent);
            assert_eq!(tempo_move.label(), label, "{percent}%");
            assert_eq!(tempo_move.tone(), tone, "{percent}%");
        }
    }

    #[test]
    fn the_words_and_the_relation_labels_agree() {
        for relation in Relation::ALL {
            assert_eq!(relation_name(relation), relation.label());
            assert_eq!(movement_label(Some(relation)), relation.label());
        }
        assert_eq!(Relation::Diagonal.label(), "Wheel diagonal");
        assert_eq!(Relation::Adjacent.label(), "One step on the wheel");
    }

    #[test]
    fn the_thresholds_are_the_edges_of_the_moves() {
        assert_eq!(TempoMove::of(EASY_BELOW_PERCENT), TempoMove::PitchMove);
        assert_eq!(TempoMove::of(BIG_JUMP_OVER_PERCENT), TempoMove::PitchMove);
        assert_eq!(TempoMove::of(f32::NAN), TempoMove::BigJump);
        assert_eq!(
            TempoMove::ALL.map(TempoMove::label),
            ["Easy", "Pitch move", "Big jump"]
        );
    }

    #[test]
    fn relations_read_in_wheel_words() {
        assert_eq!(relation_name(Relation::Adjacent), "One step on the wheel");
        assert_eq!(relation_name(Relation::Diagonal), "Wheel diagonal");
        assert_eq!(relation_name(Relation::Same), "Same key");
        assert_eq!(
            relation_hint(Relation::Diagonal),
            Some("One step and a major/minor switch")
        );
        assert_eq!(relation_hint(Relation::Adjacent), None);
    }
}
