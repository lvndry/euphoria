// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What Live remembers, in a file of its own beside the app's settings, so
//! the menu bar item can change it while the main window is closed without
//! rewriting the main settings.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::next_rail::EnergyChoice;

const FILE_NAME: &str = "live.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LiveSettings {
    /// Keep Live running, in the menu bar, when the window is closed.
    pub(crate) keep_running_when_closed: bool,
    /// Whether Live was ever started. The setting above only applies once it
    /// was, so closing the window of someone who never used Live quits.
    pub(crate) started_once: bool,
    /// Where the energy goes with the next track.
    pub(crate) energy: EnergyChoice,
}

impl Default for LiveSettings {
    fn default() -> Self {
        Self {
            keep_running_when_closed: true,
            started_once: false,
            energy: EnergyChoice::default(),
        }
    }
}

impl LiveSettings {
    /// True when closing the last window leaves Live running.
    pub(crate) fn keeps_running(&self) -> bool {
        self.started_once && self.keep_running_when_closed
    }

    /// Reads the file. A missing or unreadable file gives the defaults.
    pub(crate) fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the file, beside the old one and then moved over it.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let temporary_path = path.with_extension("json.new");
        std::fs::write(&temporary_path, text)?;
        std::fs::rename(&temporary_path, path)
    }
}

/// The file beside the main settings, or `None` without a data folder.
pub(crate) fn live_settings_path() -> Option<PathBuf> {
    Some(crate::settings::settings_path()?.with_file_name(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keeps_running_only_after_live_was_started() {
        let mut settings = LiveSettings::default();
        assert!(settings.keep_running_when_closed);
        assert!(!settings.keeps_running());
        settings.started_once = true;
        assert!(settings.keeps_running());
        settings.keep_running_when_closed = false;
        assert!(!settings.keeps_running());
    }

    #[test]
    fn it_round_trips_through_a_file_and_defaults_when_missing() {
        let folder = std::env::temp_dir().join(format!("euphoria-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join(FILE_NAME);
        assert_eq!(LiveSettings::load(&path), LiveSettings::default());
        let settings = LiveSettings {
            keep_running_when_closed: false,
            started_once: true,
            energy: EnergyChoice::Lift,
        };
        settings.save(&path).unwrap();
        assert_eq!(LiveSettings::load(&path), settings);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(LiveSettings::load(&path), LiveSettings::default());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
