// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How tags are written: for which DJ program, in which key notation, where
//! the energy label goes and whether the tempo is written. Kept in the
//! settings file under names that stay the same between versions, and
//! turned into the core's write options.

use euphoria_core::tags::{DjSoftware, KeyNotation, LabelPlacement, TagWriteOptions};
use serde::{Deserialize, Serialize};

/// The DJ program whose tag fields are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProgramChoice {
    /// The fields every common program reads.
    #[default]
    Generic,
    /// AlphaTheta rekordbox.
    Rekordbox,
    /// Serato DJ.
    Serato,
    /// Native Instruments Traktor.
    Traktor,
    /// Engine DJ.
    EngineDj,
    /// Atomix VirtualDJ.
    #[serde(rename = "virtualdj")]
    VirtualDj,
    /// Mixxx.
    Mixxx,
}

impl ProgramChoice {
    /// Every program, in the order the settings list them.
    pub(crate) const ALL: [ProgramChoice; 7] = [
        ProgramChoice::Rekordbox,
        ProgramChoice::Serato,
        ProgramChoice::Traktor,
        ProgramChoice::EngineDj,
        ProgramChoice::VirtualDj,
        ProgramChoice::Mixxx,
        ProgramChoice::Generic,
    ];

    /// The program's name as people write it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ProgramChoice::Generic => "Any program",
            ProgramChoice::Rekordbox => "rekordbox",
            ProgramChoice::Serato => "Serato",
            ProgramChoice::Traktor => "Traktor",
            ProgramChoice::EngineDj => "Engine DJ",
            ProgramChoice::VirtualDj => "VirtualDJ",
            ProgramChoice::Mixxx => "Mixxx",
        }
    }

    /// The core's preset for this program.
    pub(crate) fn dj_software(self) -> DjSoftware {
        match self {
            ProgramChoice::Generic => DjSoftware::Generic,
            ProgramChoice::Rekordbox => DjSoftware::Rekordbox,
            ProgramChoice::Serato => DjSoftware::Serato,
            ProgramChoice::Traktor => DjSoftware::Traktor,
            ProgramChoice::EngineDj => DjSoftware::EngineDj,
            ProgramChoice::VirtualDj => DjSoftware::VirtualDj,
            ProgramChoice::Mixxx => DjSoftware::Mixxx,
        }
    }
}

/// How the key is spelled, in the key field and in the label.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NotationChoice {
    /// Camelot codes such as 8A.
    #[default]
    Camelot,
    /// Musical names such as Am.
    Musical,
    /// Open Key codes such as 1m.
    OpenKey,
}

impl NotationChoice {
    /// Every notation, in the order the settings list them.
    pub(crate) const ALL: [NotationChoice; 3] = [
        NotationChoice::Camelot,
        NotationChoice::Musical,
        NotationChoice::OpenKey,
    ];

    /// The notation's name, with an example.
    pub(crate) fn label(self) -> &'static str {
        match self {
            NotationChoice::Camelot => "Camelot (8A)",
            NotationChoice::Musical => "Musical (Am)",
            NotationChoice::OpenKey => "Open Key (1m)",
        }
    }

    fn key_notation(self) -> KeyNotation {
        match self {
            NotationChoice::Camelot => KeyNotation::Camelot,
            NotationChoice::Musical => KeyNotation::Musical,
            NotationChoice::OpenKey => KeyNotation::OpenKey,
        }
    }
}

/// Where the key and energy label, such as "8A - Energy 7", goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LabelChoice {
    /// In front of the comment.
    #[default]
    CommentStart,
    /// After the comment.
    CommentEnd,
    /// In front of the grouping.
    Grouping,
    /// Nowhere: the energy is not written.
    None,
}

impl LabelChoice {
    /// Every place, in the order the settings list them.
    pub(crate) const ALL: [LabelChoice; 4] = [
        LabelChoice::CommentStart,
        LabelChoice::CommentEnd,
        LabelChoice::Grouping,
        LabelChoice::None,
    ];

    /// Where the label goes, in a few words.
    pub(crate) fn label(self) -> &'static str {
        match self {
            LabelChoice::CommentStart => "Start of the comment",
            LabelChoice::CommentEnd => "End of the comment",
            LabelChoice::Grouping => "Start of the grouping",
            LabelChoice::None => "Nowhere",
        }
    }

    fn placement(self) -> LabelPlacement {
        match self {
            LabelChoice::CommentStart => LabelPlacement::CommentStart,
            LabelChoice::CommentEnd => LabelPlacement::CommentEnd,
            LabelChoice::Grouping => LabelPlacement::Grouping,
            LabelChoice::None => LabelPlacement::None,
        }
    }
}

/// The choices that decide what a tag write writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TagSettings {
    /// The DJ program whose fields are written.
    pub(crate) program: ProgramChoice,
    /// How the key is spelled.
    pub(crate) key_notation: NotationChoice,
    /// Where the key and energy label goes.
    pub(crate) label: LabelChoice,
    /// Whether the tempo is written.
    pub(crate) write_tempo: bool,
}

impl Default for TagSettings {
    fn default() -> Self {
        Self {
            program: ProgramChoice::default(),
            key_notation: NotationChoice::default(),
            label: LabelChoice::default(),
            write_tempo: ProgramChoice::default()
                .dj_software()
                .writes_bpm_by_default(),
        }
    }
}

impl TagSettings {
    /// Chooses the program, and with it whether the tempo is written: Serato
    /// detects the tempo itself unless a file already has one, so for
    /// Serato the tempo starts off unticked. It can still be ticked after.
    pub(crate) fn choose_program(&mut self, program: ProgramChoice) {
        if self.program != program {
            self.program = program;
            self.write_tempo = program.dj_software().writes_bpm_by_default();
        }
    }

    /// The core's options for a write with these settings.
    pub(crate) fn write_options(&self) -> TagWriteOptions {
        TagWriteOptions {
            key_notation: self.key_notation.key_notation(),
            write_bpm: self.write_tempo,
            label: self.label.placement(),
            preset: self.program.dj_software(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_match_the_command_line_defaults() {
        assert_eq!(
            TagSettings::default().write_options(),
            TagWriteOptions::default()
        );
    }

    #[test]
    fn each_program_selects_its_own_preset() {
        let presets: Vec<DjSoftware> = ProgramChoice::ALL
            .iter()
            .map(|program| program.dj_software())
            .collect();
        for preset in DjSoftware::ALL {
            assert_eq!(
                presets.iter().filter(|chosen| **chosen == preset).count(),
                1,
                "{preset:?}"
            );
        }
    }

    #[test]
    fn choosing_serato_unticks_the_tempo_and_another_program_ticks_it_again() {
        let mut settings = TagSettings::default();
        settings.choose_program(ProgramChoice::Serato);
        assert!(!settings.write_tempo);
        assert!(!settings.write_options().write_bpm);
        settings.write_tempo = true;
        settings.choose_program(ProgramChoice::Serato);
        assert!(settings.write_tempo, "choosing the same program again");
        settings.choose_program(ProgramChoice::Rekordbox);
        assert!(settings.write_tempo);
        assert_eq!(settings.write_options().preset, DjSoftware::Rekordbox);
    }

    #[test]
    fn every_choice_reaches_the_write_options() {
        let settings = TagSettings {
            program: ProgramChoice::Traktor,
            key_notation: NotationChoice::OpenKey,
            label: LabelChoice::Grouping,
            write_tempo: false,
        };
        assert_eq!(
            settings.write_options(),
            TagWriteOptions {
                key_notation: KeyNotation::OpenKey,
                write_bpm: false,
                label: LabelPlacement::Grouping,
                preset: DjSoftware::Traktor,
            }
        );
    }

    #[test]
    fn choices_are_stored_by_name_and_missing_ones_take_their_defaults() {
        let settings = TagSettings {
            program: ProgramChoice::VirtualDj,
            key_notation: NotationChoice::OpenKey,
            label: LabelChoice::CommentEnd,
            write_tempo: true,
        };
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            concat!(
                r#"{"program":"virtualdj","key_notation":"open-key","#,
                r#""label":"comment-end","write_tempo":true}"#
            )
        );
        let engine: TagSettings = serde_json::from_str(r#"{"program":"engine-dj"}"#).unwrap();
        assert_eq!(engine.program, ProgramChoice::EngineDj);
        assert_eq!(engine.key_notation, NotationChoice::Camelot);
        assert!(engine.write_tempo);
    }
}
