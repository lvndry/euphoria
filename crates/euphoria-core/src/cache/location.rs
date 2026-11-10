// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::EuphoriaError;

/// Environment variable naming the folder that holds Euphoria's data. When
/// set and not empty it replaces the platform's data folder. A relative
/// value is taken from the current directory.
pub const DATA_DIRECTORY_VARIABLE: &str = "EUPHORIA_DATA_DIR";

const DATABASE_FILE_NAME: &str = "analysis-cache.sqlite3";

/// Where the analysis cache lives unless the caller chooses another file.
///
/// The result is always absolute. The folder is the value of
/// `EUPHORIA_DATA_DIR` when set, made absolute against the current directory
/// at the time of the call. Otherwise it is
/// `~/Library/Application Support/euphoria` on macOS, `%APPDATA%\euphoria` on
/// Windows, and elsewhere `$XDG_DATA_HOME/euphoria`, or
/// `~/.local/share/euphoria` when `XDG_DATA_HOME` is unset or not absolute.
pub fn default_cache_path() -> Result<PathBuf, EuphoriaError> {
    let directory =
        data_directory(|name| std::env::var_os(name)).ok_or(EuphoriaError::NoDataDirectory)?;
    match std::path::absolute(&directory) {
        Ok(absolute_directory) => Ok(absolute_directory.join(DATABASE_FILE_NAME)),
        Err(source) => Err(EuphoriaError::Scan {
            path: directory,
            source,
        }),
    }
}

fn data_directory(variable: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let folder_from = |name: &str| {
        variable(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(chosen_directory) = folder_from(DATA_DIRECTORY_VARIABLE) {
        return Some(chosen_directory);
    }
    platform_data_directory(folder_from).map(|directory| directory.join("euphoria"))
}

#[cfg(target_os = "macos")]
fn platform_data_directory(folder_from: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    folder_from("HOME").map(|home| home.join("Library").join("Application Support"))
}

#[cfg(windows)]
fn platform_data_directory(folder_from: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    folder_from("APPDATA")
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_data_directory(folder_from: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    folder_from("XDG_DATA_HOME")
        .filter(|directory| directory.is_absolute())
        .or_else(|| folder_from("HOME").map(|home| home.join(".local").join("share")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_directory_with(variables: &[(&str, &str)]) -> Option<PathBuf> {
        data_directory(|name| {
            variables
                .iter()
                .find(|(variable_name, _)| *variable_name == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn the_override_variable_wins_and_is_used_as_given() {
        let directory = data_directory_with(&[
            (DATA_DIRECTORY_VARIABLE, "/tmp/euphoria-data"),
            ("HOME", "/home/dj"),
            ("XDG_DATA_HOME", "/home/dj/data"),
            ("APPDATA", "/home/dj/appdata"),
        ]);
        assert_eq!(directory, Some(PathBuf::from("/tmp/euphoria-data")));
    }

    #[test]
    fn an_empty_override_is_ignored() {
        let directory = data_directory_with(&[
            (DATA_DIRECTORY_VARIABLE, ""),
            ("HOME", "/home/dj"),
            ("APPDATA", "/home/dj/appdata"),
        ]);
        assert_ne!(directory, Some(PathBuf::new()));
        assert!(directory.is_some_and(|directory| directory.ends_with("euphoria")));
    }

    #[test]
    fn nothing_in_the_environment_gives_no_directory() {
        assert_eq!(data_directory_with(&[]), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_support() {
        assert_eq!(
            data_directory_with(&[("HOME", "/Users/dj")]),
            Some(PathBuf::from(
                "/Users/dj/Library/Application Support/euphoria"
            ))
        );
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn linux_prefers_an_absolute_xdg_data_home() {
        assert_eq!(
            data_directory_with(&[("HOME", "/home/dj"), ("XDG_DATA_HOME", "/data")]),
            Some(PathBuf::from("/data/euphoria"))
        );
        assert_eq!(
            data_directory_with(&[("HOME", "/home/dj"), ("XDG_DATA_HOME", "relative")]),
            Some(PathBuf::from("/home/dj/.local/share/euphoria"))
        );
        assert_eq!(
            data_directory_with(&[("HOME", "/home/dj")]),
            Some(PathBuf::from("/home/dj/.local/share/euphoria"))
        );
    }
}
