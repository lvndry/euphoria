// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the app remembers between launches: the folders the user added and
//! the theme, in a JSON file in the platform data directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::theme::ThemeName;

/// Environment variable that replaces the platform data directory.
pub(crate) const DATA_DIRECTORY_VARIABLE: &str = "EUPHORIA_DATA_DIR";

const SETTINGS_FILE_NAME: &str = "settings.json";

/// Why the settings could not be read or written.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SettingsError {
    /// The file could not be read or written.
    #[error("cannot access {path}: {source}")]
    Access {
        /// The settings file or its folder.
        path: PathBuf,
        /// The underlying input or output error.
        source: std::io::Error,
    },
    /// The file is not the JSON this version writes.
    #[error("cannot understand {path}: {source}")]
    Format {
        /// The settings file.
        path: PathBuf,
        /// The parser's error.
        source: serde_json::Error,
    },
}

/// The settings kept between launches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// The folders the user added, in the order they were added.
    pub(crate) folders: Vec<PathBuf>,
    /// The chosen theme.
    pub(crate) theme: ThemeName,
}

impl Settings {
    /// Reads the settings file. A file that does not exist yet gives the
    /// defaults.
    pub(crate) fn load(path: &Path) -> Result<Self, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(SettingsError::Access {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        serde_json::from_str(&text).map_err(|source| SettingsError::Format {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Writes the settings file, creating its folder when needed. The file is
    /// written beside the old one and then moved over it, so a crash never
    /// leaves half a file.
    pub(crate) fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let access = |failed_path: &Path| {
            let failed_path = failed_path.to_path_buf();
            move |source| SettingsError::Access {
                path: failed_path,
                source,
            }
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(access(folder))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|source| SettingsError::Format {
            path: path.to_path_buf(),
            source,
        })?;
        let temporary_path = path.with_extension("json.new");
        std::fs::write(&temporary_path, text).map_err(access(&temporary_path))?;
        std::fs::rename(&temporary_path, path).map_err(access(path))
    }
}

/// The settings file for this user, or `None` when the platform gives no
/// place to keep it.
pub(crate) fn settings_path() -> Option<PathBuf> {
    data_directory(|name| std::env::var_os(name)).map(|folder| folder.join(SETTINGS_FILE_NAME))
}

/// The folder Euphoria keeps its data in: `EUPHORIA_DATA_DIR` when set,
/// otherwise the platform's data directory with `euphoria` appended.
fn data_directory(variable: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let non_empty = |name: &str| variable(name).filter(|value| !value.is_empty());
    if let Some(overridden) = non_empty(DATA_DIRECTORY_VARIABLE) {
        return Some(PathBuf::from(overridden));
    }
    let platform_directory = if cfg!(target_os = "macos") {
        non_empty("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        non_empty("APPDATA").map(PathBuf::from)
    } else {
        non_empty("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| non_empty("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    };
    platform_directory.map(|folder| folder.join("euphoria"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("euphoria-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let path = temporary_folder("missing").join(SETTINGS_FILE_NAME);
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.theme, ThemeName::Slate);
        assert!(settings.folders.is_empty());
    }

    #[test]
    fn saved_settings_load_back_the_same() {
        let folder = temporary_folder("round-trip");
        let path = folder.join("nested").join(SETTINGS_FILE_NAME);
        let settings = Settings {
            folders: vec![PathBuf::from("/music/DJ tracks"), PathBuf::from("/promos")],
            theme: ThemeName::Graphite,
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(loaded.unwrap(), settings);
        assert_eq!(leftovers, [OsString::from(SETTINGS_FILE_NAME)]);
    }

    #[test]
    fn themes_are_stored_by_name() {
        let text = serde_json::to_string(&Settings {
            folders: Vec::new(),
            theme: ThemeName::KeyFirst,
        })
        .unwrap();
        assert_eq!(text, r#"{"folders":[],"theme":"key-first"}"#);
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let folder = temporary_folder("partial");
        let path = folder.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&path, r#"{"theme":"paper"}"#).unwrap();
        let loaded = Settings::load(&path);
        std::fs::remove_dir_all(&folder).unwrap();

        let settings = loaded.unwrap();
        assert_eq!(settings.theme, ThemeName::Paper);
        assert!(settings.folders.is_empty());
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        let folder = temporary_folder("damaged");
        let path = folder.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        let loaded = Settings::load(&path);
        std::fs::remove_dir_all(&folder).unwrap();

        assert!(matches!(loaded, Err(SettingsError::Format { .. })));
    }

    #[test]
    fn the_override_variable_replaces_the_platform_directory() {
        let folder = data_directory(|name| match name {
            DATA_DIRECTORY_VARIABLE => Some(OsString::from("/custom/place")),
            _ => Some(OsString::from("/home/someone")),
        });
        assert_eq!(folder, Some(PathBuf::from("/custom/place")));
    }

    #[test]
    fn the_platform_directory_ends_in_euphoria() {
        let folder = data_directory(|name| match name {
            DATA_DIRECTORY_VARIABLE => Some(OsString::new()),
            _ => Some(OsString::from("/home/someone")),
        })
        .unwrap();
        assert!(folder.starts_with("/home/someone"));
        assert!(folder.ends_with("euphoria"));
        #[cfg(target_os = "macos")]
        assert_eq!(
            folder,
            PathBuf::from("/home/someone/Library/Application Support/euphoria")
        );
    }

    #[test]
    fn no_home_means_no_data_directory() {
        assert_eq!(data_directory(|_| None), None);
    }
}
