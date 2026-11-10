// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::EuphoriaError;

/// File extensions Euphoria can decode, in lower case.
pub const SUPPORTED_EXTENSIONS: [&str; 6] = ["mp3", "m4a", "flac", "wav", "aif", "aiff"];

/// True when the file name ends in one of [`SUPPORTED_EXTENSIONS`], in any
/// letter case.
pub fn is_supported_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

/// Expands files and folders into the audio files to analyze, sorted and
/// without duplicates.
///
/// A file named directly is always kept, so an unsupported one is reported
/// when it fails to decode rather than skipped silently. Folders are searched
/// recursively for supported files; hidden files and folders, such as the
/// `._` copies macOS leaves on external drives, are skipped.
///
/// A symbolic link to an audio file is included. A symbolic link to a folder
/// is not entered, so a scan stays inside the folders that were named and a
/// link that points back up the tree cannot make it loop. To include a linked
/// folder, name it as one of `paths`.
pub fn find_audio_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, EuphoriaError> {
    let mut audio_files = Vec::new();
    for path in paths {
        let metadata = std::fs::metadata(path).map_err(|source| EuphoriaError::Scan {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_dir() {
            audio_files.push(path.clone());
            continue;
        }
        let entries = WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry.file_name()));
        for entry in entries {
            let entry = entry.map_err(|error| EuphoriaError::Scan {
                path: error.path().unwrap_or(path).to_path_buf(),
                source: error.into(),
            })?;
            if is_supported_audio_file(entry.path()) && is_file_or_link_to_file(&entry) {
                audio_files.push(entry.into_path());
            }
        }
    }
    audio_files.sort();
    audio_files.dedup();
    Ok(audio_files)
}

fn is_file_or_link_to_file(entry: &walkdir::DirEntry) -> bool {
    if entry.file_type().is_file() {
        return true;
    }
    entry.path_is_symlink() && std::fs::metadata(entry.path()).is_ok_and(|target| target.is_file())
}

fn is_hidden(file_name: &std::ffi::OsStr) -> bool {
    file_name.to_str().is_some_and(|name| name.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_supported_extensions_in_any_case() {
        for name in ["a.mp3", "b.FLAC", "c.Aiff", "d.m4a", "e.wav", "f.aif"] {
            assert!(is_supported_audio_file(Path::new(name)), "{name}");
        }
        for name in ["notes.txt", "cover.jpg", "no_extension", "track.mp3.part"] {
            assert!(!is_supported_audio_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn finds_audio_in_nested_folders_and_skips_hidden_and_unsupported_files() {
        let root =
            std::env::temp_dir().join(format!("euphoria-library-test-{}", std::process::id()));
        let nested = root.join("house").join("deep");
        let hidden_folder = root.join(".cache");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&hidden_folder).unwrap();
        for file in [
            root.join("first.mp3"),
            nested.join("second.FLAC"),
            root.join("._first.mp3"),
            root.join("cover.jpg"),
            hidden_folder.join("cached.mp3"),
        ] {
            std::fs::write(file, b"").unwrap();
        }

        let found = find_audio_files(std::slice::from_ref(&root));
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            found.unwrap(),
            vec![root.join("first.mp3"), nested.join("second.FLAC")]
        );
    }

    #[test]
    fn keeps_a_named_file_even_when_unsupported_and_removes_duplicates() {
        let root =
            std::env::temp_dir().join(format!("euphoria-library-named-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let named_file = root.join("notes.txt");
        std::fs::write(&named_file, b"").unwrap();

        let found = find_audio_files(&[named_file.clone(), named_file.clone()]);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(found.unwrap(), vec![named_file]);
    }

    #[cfg(unix)]
    #[test]
    fn includes_linked_files_but_does_not_enter_linked_folders() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("euphoria-library-links-{}", std::process::id()));
        let library = root.join("library");
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("outside.mp3"), b"").unwrap();
        std::fs::write(library.join("inside.mp3"), b"").unwrap();
        symlink(elsewhere.join("outside.mp3"), library.join("linked.mp3")).unwrap();
        symlink(&elsewhere, library.join("linked-folder")).unwrap();
        symlink(&library, library.join("loop")).unwrap();
        symlink(library.join("missing.mp3"), library.join("broken.mp3")).unwrap();

        let found = find_audio_files(std::slice::from_ref(&library));
        let found_through_link = find_audio_files(&[library.join("linked-folder")]);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            found.unwrap(),
            vec![library.join("inside.mp3"), library.join("linked.mp3")]
        );
        assert_eq!(
            found_through_link.unwrap(),
            vec![library.join("linked-folder").join("outside.mp3")]
        );
    }

    #[test]
    fn a_missing_path_is_a_scan_error() {
        let missing = PathBuf::from("/definitely/not/here");
        assert!(matches!(
            find_audio_files(&[missing]),
            Err(EuphoriaError::Scan { .. })
        ));
    }
}
