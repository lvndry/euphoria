// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How tags are written: for which DJ program, in which key notation, where
//! the energy label goes, whether the tempo is written and whether the
//! track's cues go in as Serato cue points. Kept in the settings file under
//! names that stay the same between versions, and turned into the core's
//! write options.

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

    /// The program's name inside a sentence, such as "for any program".
    pub(crate) fn label_in_sentence(self) -> &'static str {
        match self {
            ProgramChoice::Generic => "any program",
            program => program.label(),
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

    pub(crate) fn export_notation(self) -> euphoria_core::export::rekordbox::KeyNotation {
        use euphoria_core::export::rekordbox::KeyNotation as ExportNotation;
        match self {
            NotationChoice::Camelot => ExportNotation::Camelot,
            NotationChoice::Musical => ExportNotation::Musical,
            NotationChoice::OpenKey => ExportNotation::OpenKey,
        }
    }

    pub(crate) fn key_notation(self) -> KeyNotation {
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

    pub(crate) fn placement(self) -> LabelPlacement {
        match self {
            LabelChoice::CommentStart => LabelPlacement::CommentStart,
            LabelChoice::CommentEnd => LabelPlacement::CommentEnd,
            LabelChoice::Grouping => LabelPlacement::Grouping,
            LabelChoice::None => LabelPlacement::None,
        }
    }
}

/// Which version of the defaults a stored [`TagSettings`] was written
/// under. Files from before the safe tag writing have none, which reads as
/// 0: they always stored the label, so a stored label cannot tell a choice
/// from the old default.
const CURRENT_SCHEMA: u32 = 1;

/// The choices that decide what a tag write writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredTagSettings")]
pub(crate) struct TagSettings {
    /// The DJ program whose fields are written.
    pub(crate) program: ProgramChoice,
    /// How the key is spelled.
    pub(crate) key_notation: NotationChoice,
    /// Where the key and energy label goes, when the user chose a place.
    /// `None` follows the program: no label for any program, and the start
    /// of the comment for a DJ program. Only a choice is stored, so a
    /// default is never mistaken for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) label: Option<LabelChoice>,
    /// Whether the tempo is written.
    pub(crate) write_tempo: bool,
    /// Whether the track's cues and beat grid are written as Serato cue
    /// points, which Serato DJ, Mixxx and VirtualDJ read too.
    pub(crate) write_cue_markers: bool,
    /// Whether the cues analysis placed are coloured by the energy of their
    /// sections, in cue markers and in exports to DJ programs. Off keeps
    /// the colours each program gives its pads.
    pub(crate) colour_cues_by_energy: bool,
    /// Whether a key or tempo the file already has is replaced.
    pub(crate) replace_existing: bool,
    /// Whether a key or tempo rated low confidence is written.
    pub(crate) include_low_confidence: bool,
    /// Whether each track's key and tempo are written into its file once
    /// its analysis finishes. On by default: the safe defaults above keep
    /// any key or tempo a file already has.
    pub(crate) write_after_analysis: bool,
    pub(crate) schema: u32,
}

#[derive(Deserialize)]
#[serde(default)]
struct StoredTagSettings {
    program: ProgramChoice,
    key_notation: NotationChoice,
    label: Option<LabelChoice>,
    write_tempo: bool,
    write_cue_markers: bool,
    colour_cues_by_energy: bool,
    replace_existing: bool,
    include_low_confidence: bool,
    write_after_analysis: bool,
    schema: u32,
}

impl Default for StoredTagSettings {
    fn default() -> Self {
        let current = TagSettings::default();
        Self {
            program: current.program,
            key_notation: current.key_notation,
            label: current.label,
            write_tempo: current.write_tempo,
            write_cue_markers: current.write_cue_markers,
            colour_cues_by_energy: current.colour_cues_by_energy,
            replace_existing: current.replace_existing,
            include_low_confidence: current.include_low_confidence,
            write_after_analysis: current.write_after_analysis,
            schema: 0,
        }
    }
}

impl From<StoredTagSettings> for TagSettings {
    fn from(stored: StoredTagSettings) -> Self {
        let label = if stored.schema >= CURRENT_SCHEMA {
            stored.label
        } else {
            // Before the schema was recorded the label was always stored, so
            // the only value that can be told from the default is another
            // one. The old default with the generic program is read as the
            // default, which is now no label.
            stored.label.filter(|label| {
                !(stored.program == ProgramChoice::Generic && *label == LabelChoice::CommentStart)
            })
        };
        Self {
            program: stored.program,
            key_notation: stored.key_notation,
            label,
            write_tempo: stored.write_tempo,
            write_cue_markers: stored.write_cue_markers,
            colour_cues_by_energy: stored.colour_cues_by_energy,
            replace_existing: stored.replace_existing,
            include_low_confidence: stored.include_low_confidence,
            write_after_analysis: stored.write_after_analysis,
            schema: CURRENT_SCHEMA,
        }
    }
}

impl Default for TagSettings {
    fn default() -> Self {
        Self {
            program: ProgramChoice::default(),
            key_notation: NotationChoice::default(),
            label: None,
            write_tempo: ProgramChoice::default()
                .dj_software()
                .writes_bpm_by_default(),
            write_cue_markers: false,
            colour_cues_by_energy: true,
            replace_existing: false,
            include_low_confidence: false,
            write_after_analysis: true,
            schema: CURRENT_SCHEMA,
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

    /// How cues are coloured, as the colour setting chooses.
    pub(crate) fn cue_colours(&self) -> euphoria_core::CueColours {
        if self.colour_cues_by_energy {
            euphoria_core::CueColours::Energy
        } else {
            euphoria_core::CueColours::Fixed
        }
    }

    /// Where the label goes: the user's choice, or the program's default.
    pub(crate) fn effective_label(&self) -> LabelChoice {
        self.label.unwrap_or(match self.program {
            ProgramChoice::Generic => LabelChoice::None,
            _ => LabelChoice::CommentStart,
        })
    }

    /// The core's options for a write with these settings.
    pub(crate) fn write_options(&self) -> TagWriteOptions {
        TagWriteOptions {
            key_notation: self.key_notation.key_notation(),
            write_bpm: self.write_tempo,
            label: self.effective_label().placement(),
            preset: self.program.dj_software(),
            keep_existing: !self.replace_existing,
            skip_low_confidence: !self.include_low_confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_keep_existing_values_and_skip_low_confidence_without_a_label() {
        assert_eq!(
            TagSettings::default().write_options(),
            TagWriteOptions {
                label: LabelPlacement::None,
                keep_existing: true,
                skip_low_confidence: true,
                ..TagWriteOptions::default()
            }
        );
    }

    #[test]
    fn the_label_follows_the_program_until_one_is_chosen() {
        for (program, expected) in [
            (ProgramChoice::Generic, LabelChoice::None),
            (ProgramChoice::Rekordbox, LabelChoice::CommentStart),
            (ProgramChoice::Serato, LabelChoice::CommentStart),
            (ProgramChoice::Traktor, LabelChoice::CommentStart),
            (ProgramChoice::EngineDj, LabelChoice::CommentStart),
            (ProgramChoice::VirtualDj, LabelChoice::CommentStart),
            (ProgramChoice::Mixxx, LabelChoice::CommentStart),
        ] {
            let settings = TagSettings {
                program,
                ..TagSettings::default()
            };
            assert_eq!(settings.effective_label(), expected, "{program:?}");
        }
        let chosen = TagSettings {
            label: Some(LabelChoice::Grouping),
            ..TagSettings::default()
        };
        assert_eq!(chosen.effective_label(), LabelChoice::Grouping);
    }

    #[test]
    fn settings_saved_before_the_safe_defaults_keep_a_chosen_label_and_drop_the_default() {
        let load = |text: &str| -> TagSettings { serde_json::from_str(text).unwrap() };
        let old_default = load(
            r#"{"program":"generic","key_notation":"camelot","label":"comment-start","write_tempo":true,"write_cue_markers":false}"#,
        );
        assert_eq!(old_default.label, None);
        assert_eq!(old_default.effective_label(), LabelChoice::None);
        for chosen in ["comment-end", "grouping", "none"] {
            let kept = load(&format!(r#"{{"program":"generic","label":"{chosen}"}}"#));
            assert!(kept.label.is_some(), "{chosen}");
        }
        let with_program = load(r#"{"program":"traktor","label":"comment-start"}"#);
        assert_eq!(with_program.effective_label(), LabelChoice::CommentStart);
        assert!(!with_program.replace_existing);
        assert!(!with_program.include_low_confidence);
    }

    #[test]
    fn a_label_chosen_after_the_change_is_kept_even_when_it_is_the_old_default() {
        let chosen = TagSettings {
            label: Some(LabelChoice::CommentStart),
            ..TagSettings::default()
        };
        let stored = serde_json::to_string(&chosen).unwrap();
        let loaded: TagSettings = serde_json::from_str(&stored).unwrap();
        assert_eq!(loaded.label, Some(LabelChoice::CommentStart));
        let untouched = serde_json::to_string(&TagSettings::default()).unwrap();
        assert!(!untouched.contains("label"), "{untouched}");
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
            label: Some(LabelChoice::Grouping),
            write_tempo: false,
            replace_existing: true,
            include_low_confidence: true,
            ..TagSettings::default()
        };
        assert_eq!(
            settings.write_options(),
            TagWriteOptions {
                key_notation: KeyNotation::OpenKey,
                write_bpm: false,
                label: LabelPlacement::Grouping,
                preset: DjSoftware::Traktor,
                keep_existing: false,
                skip_low_confidence: false,
            }
        );
    }

    #[test]
    fn choices_are_stored_by_name_and_missing_ones_take_their_defaults() {
        let settings = TagSettings {
            program: ProgramChoice::VirtualDj,
            key_notation: NotationChoice::OpenKey,
            label: Some(LabelChoice::CommentEnd),
            write_tempo: true,
            write_cue_markers: true,
            colour_cues_by_energy: false,
            ..TagSettings::default()
        };
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            concat!(
                r#"{"program":"virtualdj","key_notation":"open-key","#,
                r#""label":"comment-end","write_tempo":true,"write_cue_markers":true,"#,
                r#""colour_cues_by_energy":false,"replace_existing":false,"include_low_confidence":false,"#,
                r#""write_after_analysis":true,"schema":1}"#
            )
        );
        let engine: TagSettings = serde_json::from_str(r#"{"program":"engine-dj"}"#).unwrap();
        assert_eq!(engine.program, ProgramChoice::EngineDj);
        assert!(engine.colour_cues_by_energy);
        assert_eq!(engine.key_notation, NotationChoice::Camelot);
        assert!(engine.write_tempo);
        assert!(
            !engine.write_cue_markers,
            "a setting that was not there takes its default"
        );
    }

    #[test]
    fn writing_after_analysis_is_on_for_old_files_and_remembered_once_turned_off() {
        let old: TagSettings = serde_json::from_str(
            r#"{"program":"rekordbox","replace_existing":true,"include_low_confidence":false,"schema":1}"#,
        )
        .unwrap();
        assert!(old.write_after_analysis);
        assert!(old.replace_existing, "the other choices survive");
        assert!(TagSettings::default().write_after_analysis);

        let chosen = TagSettings {
            write_after_analysis: false,
            ..TagSettings::default()
        };
        let stored = serde_json::to_string(&chosen).unwrap();
        assert_eq!(
            serde_json::from_str::<TagSettings>(&stored).unwrap(),
            chosen
        );
    }
}
