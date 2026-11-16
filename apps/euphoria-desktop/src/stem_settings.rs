// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How tracks are separated into stems, kept in the settings file.

use euphoria_core::stems::SeparationMode;
use serde::{Deserialize, Serialize};

/// How tracks are separated into stems.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct StemSettings {
    /// Whether tracks are separated in Advanced mode. Off until turned on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) advanced: bool,
}

impl StemSettings {
    /// The settings that separate in `mode`.
    pub(crate) fn for_mode(mode: SeparationMode) -> Self {
        Self {
            advanced: mode == SeparationMode::Advanced,
        }
    }

    /// The mode the core separates in.
    pub(crate) fn mode(self) -> SeparationMode {
        if self.advanced {
            SeparationMode::Advanced
        } else {
            SeparationMode::Default
        }
    }

    /// Whether these are the defaults, which the settings file leaves out.
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The choices, in the order the settings list them.
pub(crate) const MODES: [SeparationMode; 2] = [SeparationMode::Default, SeparationMode::Advanced];

/// What the choice is called in the interface.
pub(crate) fn mode_label(mode: SeparationMode) -> &'static str {
    match mode {
        SeparationMode::Default => "Default mode (faster)",
        SeparationMode::Advanced => "Advanced mode (slower)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_default_mode_and_writes_nothing() {
        let settings = StemSettings::default();
        assert_eq!(settings.mode(), SeparationMode::Default);
        assert!(settings.is_default());
        assert_eq!(serde_json::to_string(&settings).unwrap(), "{}");
    }

    #[test]
    fn the_advanced_mode_is_stored_and_read_back() {
        let settings = StemSettings::for_mode(SeparationMode::Advanced);
        let text = serde_json::to_string(&settings).unwrap();
        assert_eq!(text, r#"{"advanced":true}"#);
        let loaded: StemSettings = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.mode(), SeparationMode::Advanced);
        assert!(!loaded.is_default());
    }

    #[test]
    fn each_mode_has_its_own_label() {
        assert_eq!(mode_label(SeparationMode::Default), "Default mode (faster)");
        assert_eq!(
            mode_label(SeparationMode::Advanced),
            "Advanced mode (slower)"
        );
    }
}
