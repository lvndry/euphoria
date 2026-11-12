// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How keys and tempos are shown: the key notation of the table and the
//! track detail, and the range tempos are placed in. Both change only what
//! is shown; nothing is analyzed again.

use euphoria_core::{Key, TempoRange};
use serde::{Deserialize, Serialize};

use crate::tag_settings::NotationChoice;

impl NotationChoice {
    /// `key` in this notation, such as "8A", "Am" or "1m".
    pub(crate) fn spell(self, key: &Key) -> String {
        match self {
            NotationChoice::Camelot => key.camelot(),
            NotationChoice::Musical => key.short_name(),
            NotationChoice::OpenKey => key.open_key(),
        }
    }
}

/// The range tempos are shown in, in whole beats per minute: from `slowest`
/// up to, not including, `fastest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TempoRangeSetting {
    /// The slowest tempo shown.
    pub(crate) slowest: u16,
    /// The tempo the range stops below.
    pub(crate) fastest: u16,
}

impl Default for TempoRangeSetting {
    fn default() -> Self {
        TempoPreset::HouseAndTechno.setting()
    }
}

impl TempoRangeSetting {
    /// The core's range, or why these ends do not make one.
    pub(crate) fn range(self) -> Result<TempoRange, String> {
        TempoRange::new(f32::from(self.slowest), f32::from(self.fastest))
            .map_err(|error| error.chain())
    }

    /// The core's range, or the default one when a settings file holds ends
    /// that do not make a range.
    pub(crate) fn range_or_default(self) -> TempoRange {
        self.range().unwrap_or_default()
    }

    /// The preset with these ends, if any.
    pub(crate) fn preset(self) -> Option<TempoPreset> {
        TempoPreset::ALL
            .into_iter()
            .find(|preset| preset.setting() == self)
    }
}

/// A tempo range suited to a style of music.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TempoPreset {
    /// 90 to 180, the default: house and techno, and most dance music.
    HouseAndTechno,
    /// 70 to 140.
    HipHop,
    /// 120 to 200.
    DrumAndBass,
}

impl TempoPreset {
    /// Every preset, in the order the settings list them.
    pub(crate) const ALL: [TempoPreset; 3] = [
        TempoPreset::HouseAndTechno,
        TempoPreset::HipHop,
        TempoPreset::DrumAndBass,
    ];

    /// The ends of the preset.
    pub(crate) fn setting(self) -> TempoRangeSetting {
        let (slowest, fastest) = match self {
            TempoPreset::HouseAndTechno => (90, 180),
            TempoPreset::HipHop => (70, 140),
            TempoPreset::DrumAndBass => (120, 200),
        };
        TempoRangeSetting { slowest, fastest }
    }

    /// The preset's name and range.
    pub(crate) fn label(self) -> &'static str {
        match self {
            TempoPreset::HouseAndTechno => "House and techno, 90 to 180",
            TempoPreset::HipHop => "Hip hop, 70 to 140",
            TempoPreset::DrumAndBass => "Drum and bass, 120 to 200",
        }
    }
}

/// How keys and tempos are shown, kept in the settings file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DisplaySettings {
    /// The notation of keys in the table and the track detail.
    pub(crate) key_notation: NotationChoice,
    /// The range tempos are shown in.
    pub(crate) tempo_range: TempoRangeSetting,
}

#[cfg(test)]
mod tests {
    use euphoria_core::Mode;

    use super::*;

    #[test]
    fn each_notation_spells_a_key_its_own_way() {
        let a_minor = Key::new(9, Mode::Minor);
        assert_eq!(NotationChoice::Camelot.spell(&a_minor), "8A");
        assert_eq!(NotationChoice::Musical.spell(&a_minor), "Am");
        assert_eq!(NotationChoice::OpenKey.spell(&a_minor), "1m");
    }

    #[test]
    fn the_default_range_is_the_house_and_techno_preset_and_the_core_default() {
        let setting = TempoRangeSetting::default();
        assert_eq!(setting.preset(), Some(TempoPreset::HouseAndTechno));
        assert_eq!(setting.range().unwrap(), TempoRange::DEFAULT);
    }

    #[test]
    fn every_preset_is_a_range_the_core_accepts() {
        for preset in TempoPreset::ALL {
            assert!(preset.setting().range().is_ok(), "{preset:?}");
            assert_eq!(preset.setting().preset(), Some(preset));
        }
    }

    #[test]
    fn a_custom_range_is_checked_by_the_core_and_a_bad_one_falls_back_to_the_default() {
        let custom = TempoRangeSetting {
            slowest: 80,
            fastest: 160,
        };
        assert_eq!(custom.preset(), None);
        assert_eq!(custom.range().unwrap().slowest(), 80.0);

        let too_wide = TempoRangeSetting {
            slowest: 60,
            fastest: 180,
        };
        let reason = too_wide.range().unwrap_err();
        assert!(reason.contains("octave"), "{reason}");
        assert_eq!(too_wide.range_or_default(), TempoRange::DEFAULT);

        let reversed = TempoRangeSetting {
            slowest: 150,
            fastest: 100,
        };
        assert!(reversed.range().is_err());
    }

    #[test]
    fn display_settings_are_stored_by_name_and_missing_ones_take_their_defaults() {
        let settings = DisplaySettings {
            key_notation: NotationChoice::OpenKey,
            tempo_range: TempoPreset::HipHop.setting(),
        };
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            r#"{"key_notation":"open-key","tempo_range":{"slowest":70,"fastest":140}}"#
        );
        let partial: DisplaySettings =
            serde_json::from_str(r#"{"key_notation":"musical"}"#).unwrap();
        assert_eq!(partial.key_notation, NotationChoice::Musical);
        assert_eq!(partial.tempo_range, TempoRangeSetting::default());
    }
}
