// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::EuphoriaError;

/// File extensions Euphoria can decode, in lower case.
pub const SUPPORTED_EXTENSIONS: [&str; 8] =
    ["mp3", "m4a", "mp4", "flac", "ogg", "wav", "aif", "aiff"];

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

/// Audio file extensions Euphoria recognises but cannot decode, in lower
/// case. A folder scan counts the files with these extensions so it can say
/// what it left out; other files, such as artwork and playlists, are not
/// audio and are not counted.
pub const UNSUPPORTED_AUDIO_EXTENSIONS: [&str; 17] = [
    "opus", "wma", "wv", "ape", "dsf", "dff", "mpc", "tta", "aac", "ac3", "dts", "amr", "mka",
    "mp2", "oga", "webm", "caf",
];

/// A file or folder a scan could not read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanWarning {
    /// The file or folder that could not be read.
    pub path: PathBuf,
    /// What the system said, such as "Permission denied (os error 13)".
    pub reason: String,
    /// True when `path` is a folder.
    pub is_folder: bool,
}

/// How many files a scan left out for one unsupported extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedExtension {
    /// The extension in lower case, without the dot.
    pub extension: String,
    /// How many files had it.
    pub count: usize,
}

/// What a scan of files and folders found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioScan {
    /// The audio files to analyze, sorted and without duplicates.
    pub files: Vec<PathBuf>,
    /// The folders and files that could not be read. The scan went on
    /// without them, so the files in them are not in `files`.
    pub unreadable: Vec<ScanWarning>,
    /// The audio files of formats Euphoria cannot decode yet, counted by
    /// extension, most numerous first.
    pub skipped: Vec<SkippedExtension>,
    /// Those files, sorted.
    pub skipped_files: Vec<PathBuf>,
}

impl AudioScan {
    /// True when some folder or file could not be read.
    pub fn has_unreadable(&self) -> bool {
        !self.unreadable.is_empty()
    }

    /// How many files were left out for their format.
    pub fn skipped_count(&self) -> usize {
        self.skipped.iter().map(|skipped| skipped.count).sum()
    }

    /// "2 folders could not be read: /a, /b", or `None` when all were read.
    pub fn unreadable_summary(&self) -> Option<String> {
        if self.unreadable.is_empty() {
            return None;
        }
        let count = self.unreadable.len();
        let all_folders = self.unreadable.iter().all(|warning| warning.is_folder);
        let paths: Vec<String> = self
            .unreadable
            .iter()
            .map(|warning| warning.path.display().to_string())
            .collect();
        Some(format!(
            "{count} {} could not be read: {}",
            match (count == 1, all_folders) {
                (true, true) => "folder",
                (false, true) => "folders",
                (true, false) => "folder or file",
                (false, false) => "folders or files",
            },
            paths.join(", ")
        ))
    }

    /// "14 files skipped: 9 .opus, 5 .wma, which Euphoria can't decode yet",
    /// or `None` when nothing was skipped.
    pub fn skipped_summary(&self) -> Option<String> {
        let total = self.skipped_count();
        if total == 0 {
            return None;
        }
        let parts: Vec<String> = self
            .skipped
            .iter()
            .map(|skipped| format!("{} .{}", skipped.count, skipped.extension))
            .collect();
        Some(format!(
            "{total} {} skipped: {}, which Euphoria can't decode yet",
            if total == 1 { "file" } else { "files" },
            parts.join(", ")
        ))
    }
}

/// Expands files and folders into the audio files to analyze, sorted and
/// without duplicates. See [`scan_audio_files`] for what is left out and
/// what is reported.
///
/// This returns only the files. A caller that should tell the user about
/// folders it could not read, or files it skipped, uses [`scan_audio_files`].
pub fn find_audio_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, EuphoriaError> {
    scan_audio_files(paths).map(|scan| scan.files)
}

/// Expands files and folders into the audio files to analyze, sorted and
/// without duplicates, and reports what it could not read or left out.
///
/// A file named directly is always kept, so an unsupported one is reported
/// when it fails to decode rather than skipped silently. Folders are searched
/// recursively for supported files; hidden files and folders, such as the
/// `._` copies macOS leaves on external drives, are skipped. Files in a
/// folder whose extension is in [`UNSUPPORTED_AUDIO_EXTENSIONS`] are counted
/// in [`AudioScan::skipped`].
///
/// A folder or entry inside a folder that cannot be read, because the
/// permission is denied or a network drive fails, does not stop the scan: it
/// is listed in [`AudioScan::unreadable`] and the rest is searched. A path
/// that is named directly and does not exist is still an error, as is one
/// that cannot be examined at all.
///
/// A symbolic link to an audio file is included. A symbolic link to a folder
/// is not entered, so a scan stays inside the folders that were named and a
/// link that points back up the tree cannot make it loop. To include a linked
/// folder, name it as one of `paths`.
pub fn scan_audio_files(paths: &[PathBuf]) -> Result<AudioScan, EuphoriaError> {
    scan_audio_files_counting(paths, &std::sync::atomic::AtomicUsize::new(0))
}

/// [`scan_audio_files`], adding one to `found` for each audio file as the
/// walk finds it, so another thread can show how many have turned up so far.
/// The count can run a little above the final list, which drops duplicates.
pub fn scan_audio_files_counting(
    paths: &[PathBuf],
    found: &std::sync::atomic::AtomicUsize,
) -> Result<AudioScan, EuphoriaError> {
    let mut scan = AudioScan::default();
    let mut skipped: std::collections::BTreeMap<String, usize> = Default::default();
    for path in paths {
        let metadata = std::fs::metadata(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                EuphoriaError::Missing { path: path.clone() }
            } else {
                EuphoriaError::Scan {
                    path: path.clone(),
                    source,
                }
            }
        })?;
        if !metadata.is_dir() {
            scan.files.push(path.clone());
            found.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            continue;
        }
        let entries = WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry.file_name()));
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    let unreadable_path = error.path().unwrap_or(path).to_path_buf();
                    scan.unreadable.push(ScanWarning {
                        is_folder: std::fs::symlink_metadata(&unreadable_path)
                            .is_ok_and(|metadata| metadata.is_dir()),
                        path: unreadable_path,
                        reason: error
                            .io_error()
                            .map_or_else(|| error.to_string(), |source| source.to_string()),
                    });
                    continue;
                }
            };
            if !is_file_or_link_to_file(&entry) {
                continue;
            }
            if is_supported_audio_file(entry.path()) {
                scan.files.push(entry.into_path());
                found.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else if let Some(extension) = unsupported_audio_extension(entry.path()) {
                *skipped.entry(extension).or_default() += 1;
                scan.skipped_files.push(entry.into_path());
            }
        }
    }
    scan.files.sort();
    scan.files.dedup();
    scan.skipped_files.sort();
    scan.skipped_files.dedup();
    scan.unreadable
        .sort_by(|first, second| first.path.cmp(&second.path));
    scan.unreadable.dedup();
    let mut skipped: Vec<SkippedExtension> = skipped
        .into_iter()
        .map(|(extension, count)| SkippedExtension { extension, count })
        .collect();
    skipped.sort_by_key(|skipped| std::cmp::Reverse(skipped.count));
    scan.skipped = skipped;
    Ok(scan)
}

fn unsupported_audio_extension(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    UNSUPPORTED_AUDIO_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
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
        for name in [
            "a.mp3", "b.FLAC", "c.Aiff", "d.m4a", "e.wav", "f.aif", "g.ogg", "h.MP4",
        ] {
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

    #[cfg(unix)]
    #[test]
    fn an_unreadable_folder_is_reported_and_the_rest_is_still_scanned() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("euphoria-library-denied-{}", std::process::id()));
        let locked = root.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("hidden-from-us.mp3"), b"").unwrap();
        std::fs::write(root.join("readable.mp3"), b"").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let can_still_read = std::fs::read_dir(&locked).is_ok();

        let scan = scan_audio_files(std::slice::from_ref(&root));
        let through_wrapper = find_audio_files(std::slice::from_ref(&root));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        if can_still_read {
            return;
        }
        let scan = scan.unwrap();
        assert_eq!(scan.files, vec![root.join("readable.mp3")]);
        assert_eq!(scan.unreadable.len(), 1);
        assert_eq!(scan.unreadable[0].path, locked);
        assert!(scan.unreadable[0].reason.contains("Permission denied"));
        assert_eq!(
            scan.unreadable_summary().unwrap(),
            format!("1 folder could not be read: {}", locked.display())
        );
        assert_eq!(through_wrapper.unwrap(), vec![root.join("readable.mp3")]);
    }

    #[test]
    fn unsupported_audio_files_are_counted_by_extension() {
        let root =
            std::env::temp_dir().join(format!("euphoria-library-skipped-{}", std::process::id()));
        std::fs::create_dir_all(root.join("deep")).unwrap();
        for file in [
            "a.opus",
            "b.OPUS",
            "deep/c.opus",
            "d.wma",
            "deep/e.wma",
            "f.ape",
            "kept.mp3",
            "cover.jpg",
            "notes.txt",
            ".hidden.opus",
        ] {
            std::fs::write(root.join(file), b"").unwrap();
        }

        let scan = scan_audio_files(std::slice::from_ref(&root)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(scan.files, vec![root.join("kept.mp3")]);
        assert_eq!(scan.skipped_count(), 6);
        assert_eq!(
            scan.skipped_summary().unwrap(),
            "6 files skipped: 3 .opus, 2 .wma, 1 .ape, which Euphoria can't decode yet"
        );
        assert!(scan.unreadable_summary().is_none());
    }

    #[test]
    fn a_scan_with_nothing_to_report_has_no_summaries() {
        let scan = AudioScan::default();
        assert!(scan.skipped_summary().is_none());
        assert!(scan.unreadable_summary().is_none());
        assert!(!scan.has_unreadable());
    }

    #[test]
    fn a_missing_path_is_reported_as_missing() {
        let missing = PathBuf::from("/definitely/not/here");
        assert!(matches!(
            find_audio_files(std::slice::from_ref(&missing)),
            Err(EuphoriaError::Missing { path }) if path == missing
        ));
    }

    #[test]
    fn the_counting_scan_counts_each_audio_file_it_finds() {
        let root =
            std::env::temp_dir().join(format!("euphoria-library-counted-{}", std::process::id()));
        std::fs::create_dir_all(root.join("deep")).unwrap();
        for file in ["a.mp3", "deep/b.flac", "c.opus", "cover.jpg"] {
            std::fs::write(root.join(file), b"").unwrap();
        }
        let found = std::sync::atomic::AtomicUsize::new(0);

        let scan = scan_audio_files_counting(std::slice::from_ref(&root), &found).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(scan.files.len(), 2);
        assert_eq!(found.load(std::sync::atomic::Ordering::Relaxed), 2);
    }
}
