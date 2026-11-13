// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the app remembers between launches: the folders the user added, the
//! theme and how the window is arranged, in a JSON file in the platform data
//! directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::columns::ColumnWidths;
use crate::display_settings::DisplaySettings;
use crate::panels::Panels;
use crate::tag_settings::TagSettings;
use crate::theme::ThemeName;

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
    /// Whether the sidebar is folded and how large the track detail is.
    pub(crate) panels: Panels,
    /// Which folders of the sidebar tree are unfolded and which is shown.
    pub(crate) folder_tree: FolderTreeMemory,
    /// The widths the user gave columns of the track table.
    pub(crate) column_widths: ColumnWidths,
    /// How tags are written.
    pub(crate) tags: TagSettings,
    /// How keys and tempos are shown.
    pub(crate) display: DisplaySettings,
}

/// What the sidebar's folder tree looked like.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct FolderTreeMemory {
    /// The unfolded folders. `None` until the tree was first saved, when
    /// the added folders open unfolded.
    pub(crate) expanded: Option<Vec<PathBuf>>,
    /// The folder the table showed, if any.
    pub(crate) selected: Option<PathBuf>,
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
        // `std::fs::rename` replaces an existing destination on every
        // platform, Windows included, so saving over earlier settings works.
        std::fs::rename(&temporary_path, path).map_err(access(path))
    }
}

/// The settings file for this user: beside the analysis cache, in the data
/// folder the core chooses (`EUPHORIA_DATA_DIR`, or the platform's). `None`
/// when there is no such folder.
pub(crate) fn settings_path() -> Option<PathBuf> {
    let cache_path = euphoria_core::default_cache_path().ok()?;
    settings_path_beside(&cache_path)
}

fn settings_path_beside(cache_path: &Path) -> Option<PathBuf> {
    Some(cache_path.parent()?.join(SETTINGS_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;
    use crate::columns::ColumnId;
    use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice};

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
            panels: Panels {
                sidebar_folded: true,
                inspector_width: 412,
                strip_height: 240,
            },
            folder_tree: FolderTreeMemory {
                expanded: Some(vec![PathBuf::from("/music/DJ tracks/afro_house")]),
                selected: Some(PathBuf::from("/music/DJ tracks/afro_house/warmup")),
            },
            column_widths: {
                let mut widths = ColumnWidths::default();
                widths.set(ColumnId::Title, 420.0);
                widths
            },
            tags: TagSettings {
                program: ProgramChoice::Serato,
                key_notation: NotationChoice::Musical,
                label: LabelChoice::Grouping,
                write_tempo: false,
                write_cue_markers: true,
            },
            display: DisplaySettings {
                key_notation: NotationChoice::OpenKey,
                tempo_range: crate::display_settings::TempoPreset::DrumAndBass.setting(),
            },
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
            panels: Panels::default(),
            folder_tree: FolderTreeMemory::default(),
            column_widths: ColumnWidths::default(),
            tags: TagSettings::default(),
            display: DisplaySettings::default(),
        })
        .unwrap();
        assert_eq!(
            text,
            concat!(
                r#"{"folders":[],"theme":"key-first","#,
                r#""panels":{"sidebar_folded":false,"inspector_width":320,"strip_height":180},"#,
                r#""folder_tree":{"expanded":null,"selected":null},"column_widths":{},"#,
                r#""tags":{"program":"generic","key_notation":"camelot","#,
                r#""label":"comment-start","write_tempo":true,"write_cue_markers":false},"#,
                r#""display":{"key_notation":"camelot","#,
                r#""tempo_range":{"slowest":90,"fastest":180}}}"#
            )
        );
    }

    #[test]
    fn a_settings_file_from_before_tags_and_column_widths_still_loads() {
        let folder = temporary_folder("before-tags");
        let path = folder.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            &path,
            concat!(
                r#"{"folders":["/music"],"theme":"deck","#,
                r#""panels":{"sidebar_folded":true,"inspector_width":400,"strip_height":200},"#,
                r#""folder_tree":{"expanded":["/music"],"selected":null}}"#
            ),
        )
        .unwrap();
        let loaded = Settings::load(&path);
        std::fs::remove_dir_all(&folder).unwrap();

        let settings = loaded.unwrap();
        assert_eq!(settings.theme, ThemeName::Deck);
        assert!(settings.panels.sidebar_folded);
        assert_eq!(settings.column_widths, ColumnWidths::default());
        assert_eq!(settings.tags, TagSettings::default());
        assert_eq!(settings.display, DisplaySettings::default());
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
        assert_eq!(settings.panels, Panels::default());
    }

    #[test]
    fn a_settings_file_from_before_the_panels_still_loads() {
        let folder = temporary_folder("before-panels");
        let path = folder.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            &path,
            r#"{"folders":["/music/DJ tracks"],"theme":"graphite"}"#,
        )
        .unwrap();
        let loaded = Settings::load(&path);
        std::fs::remove_dir_all(&folder).unwrap();

        let settings = loaded.unwrap();
        assert_eq!(settings.folders, [PathBuf::from("/music/DJ tracks")]);
        assert_eq!(settings.theme, ThemeName::Graphite);
        assert!(!settings.panels.sidebar_folded);
        assert_eq!(settings.panels.inspector_width, 320);
        assert_eq!(settings.panels.strip_height, 180);
        assert_eq!(settings.folder_tree, FolderTreeMemory::default());
    }

    #[test]
    fn a_remembered_tree_naming_folders_that_are_gone_still_loads() {
        let folder = temporary_folder("gone-folders");
        let path = folder.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            &path,
            r#"{"folder_tree":{"expanded":["/not/here"],"selected":"/not/here/either"}}"#,
        )
        .unwrap();
        let loaded = Settings::load(&path);
        std::fs::remove_dir_all(&folder).unwrap();

        let tree = loaded.unwrap().folder_tree;
        assert_eq!(tree.expanded, Some(vec![PathBuf::from("/not/here")]));
        assert_eq!(tree.selected, Some(PathBuf::from("/not/here/either")));
    }

    #[test]
    fn a_partly_written_arrangement_keeps_what_it_has() {
        let folder = temporary_folder("partial-panels");
        let path = folder.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&path, r#"{"panels":{"inspector_width":500}}"#).unwrap();
        let loaded = Settings::load(&path);
        std::fs::remove_dir_all(&folder).unwrap();

        let panels = loaded.unwrap().panels;
        assert_eq!(panels.inspector_width, 500);
        assert!(!panels.sidebar_folded);
        assert_eq!(panels.strip_height, 180);
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
    fn saving_again_replaces_the_earlier_settings() {
        let folder = temporary_folder("repeated");
        let path = folder.join(SETTINGS_FILE_NAME);
        let mut settings = Settings::default();
        for (save_number, theme) in ThemeName::ALL.into_iter().enumerate() {
            settings.theme = theme;
            settings
                .folders
                .push(PathBuf::from(format!("/music/folder-{save_number}")));
            settings.save(&path).unwrap();
            assert_eq!(Settings::load(&path).unwrap(), settings);
        }
        let leftovers: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(settings.folders.len(), ThemeName::ALL.len());
        assert_eq!(leftovers, [OsString::from(SETTINGS_FILE_NAME)]);
    }

    #[test]
    fn the_settings_file_sits_beside_the_analysis_cache() {
        let cache_path = Path::new("/data/euphoria/analysis-cache.sqlite3");
        assert_eq!(
            settings_path_beside(cache_path),
            Some(PathBuf::from("/data/euphoria/settings.json"))
        );
        assert_eq!(settings_path_beside(Path::new("/")), None);
    }
}
