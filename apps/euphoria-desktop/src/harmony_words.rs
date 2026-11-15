// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The words a DJ reads for how two tracks fit: the key relation and the
//! tempo move. Every surface that names them takes the words from here.

use euphoria_core::harmony::Relation;

/// A tempo change under this many percent is easy to match by ear.
pub(crate) const EASY_BELOW_PERCENT: f32 = 3.0;
/// A tempo change over this many percent is a big jump.
pub(crate) const BIG_JUMP_OVER_PERCENT: f32 = 6.0;

/// The column and field name for the tempo change between two tracks.
pub(crate) const TEMPO_LABEL: &str = "Tempo";

/// What the tempo change between two tracks asks of the pitch fader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TempoMove {
    Easy,
    PitchMove,
    BigJump,
}

/// How a chip is coloured: never the error red.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Success,
    Neutral,
    Warning,
}

impl TempoMove {
    /// The move for a tempo change of `percent`, up or down.
    pub(crate) fn of(percent: f32) -> Self {
        let size = percent.abs();
        if size < EASY_BELOW_PERCENT {
            TempoMove::Easy
        } else if size <= BIG_JUMP_OVER_PERCENT {
            TempoMove::PitchMove
        } else {
            TempoMove::BigJump
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            TempoMove::Easy => "Easy",
            TempoMove::PitchMove => "Pitch move",
            TempoMove::BigJump => "Big jump",
        }
    }

    pub(crate) fn tone(self) -> Tone {
        match self {
            TempoMove::Easy => Tone::Success,
            TempoMove::PitchMove => Tone::Neutral,
            TempoMove::BigJump => Tone::Warning,
        }
    }
}

/// The name of a key relation for a person mixing.
pub(crate) fn relation_name(relation: Relation) -> &'static str {
    match relation {
        Relation::Adjacent => "One step on the wheel",
        Relation::Diagonal => "Wheel diagonal",
        other => other.label(),
    }
}

/// The hint that explains a relation name, where the name alone does not.
pub(crate) fn relation_hint(relation: Relation) -> Option<&'static str> {
    match relation {
        Relation::Diagonal => Some("One step and a major/minor switch"),
        _ => None,
    }
}

/// The name of the vocal and bass overlap check.
pub(crate) const OVERLAP_LABEL: &str = "Vocals and bass overlap";
/// The link that separates the stems the overlap check needs.
pub(crate) const SEPARATE_STEMS_LINK: &str = "Separate stems to check";
/// The name of the set builder's source of tracks.
pub(crate) const PICK_FROM_LABEL: &str = "Pick from";
/// The name of the focus view's menu of rows to show.
pub(crate) const LAYERS_LABEL: &str = "Layers";

#[cfg(test)]
mod tests {
    use super::*;

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
