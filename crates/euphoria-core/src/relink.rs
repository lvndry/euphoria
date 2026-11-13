// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linking a track whose file went missing to the file it is now, checked
//! by content so a different song with the same name is never taken for it.

use std::path::{Path, PathBuf};

use crate::cache::ContentHash;
use crate::error::EuphoriaError;

/// How a file picked for a missing track compares with the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelinkMatch {
    /// The file holds the same audio the track held.
    SameAudio,
    /// The file holds other audio.
    OtherAudio,
    /// What the track held was never recorded, so there is nothing to
    /// compare the file with.
    NothingToCompare,
}

/// Compares the file at `candidate` with `expected`, the content a missing
/// track held, by reading the whole file.
///
/// # Errors
///
/// [`EuphoriaError::Missing`] when `candidate` is not there, and
/// [`EuphoriaError::Open`] when it cannot be read.
pub fn check_relink(
    expected: Option<&ContentHash>,
    candidate: &Path,
) -> Result<RelinkMatch, EuphoriaError> {
    let found = ContentHash::of_file(candidate).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            EuphoriaError::Missing {
                path: candidate.to_path_buf(),
            }
        } else {
            EuphoriaError::Open {
                path: candidate.to_path_buf(),
                source,
            }
        }
    })?;
    Ok(match expected {
        None => RelinkMatch::NothingToCompare,
        Some(expected) if *expected == found => RelinkMatch::SameAudio,
        Some(_) => RelinkMatch::OtherAudio,
    })
}

/// The missing tracks found in `folder`: each pair of `missing`, a path and
/// the content it held, whose file name is in `folder` with the same
/// content, as the old path and the new one. Used after one track of a
/// moved folder was located, to find the others moved with it.
pub fn find_in_folder(
    folder: &Path,
    missing: &[(PathBuf, ContentHash)],
) -> Vec<(PathBuf, PathBuf)> {
    missing
        .iter()
        .filter_map(|(old_path, content)| {
            let candidate = folder.join(old_path.file_name()?);
            (candidate != *old_path
                && candidate.is_file()
                && matches!(
                    check_relink(Some(content), &candidate),
                    Ok(RelinkMatch::SameAudio)
                ))
            .then(|| (old_path.clone(), candidate))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("euphoria-relink-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_picked_file_is_compared_by_content() {
        let folder = Folder::new("compare");
        let track = folder.file("track.mp3", b"the track");
        let other = folder.file("other.mp3", b"another track");
        let content = ContentHash::of_file(&track).unwrap();

        assert!(matches!(
            check_relink(Some(&content), &track),
            Ok(RelinkMatch::SameAudio)
        ));
        assert!(matches!(
            check_relink(Some(&content), &other),
            Ok(RelinkMatch::OtherAudio)
        ));
        assert!(matches!(
            check_relink(None, &other),
            Ok(RelinkMatch::NothingToCompare)
        ));
        assert!(matches!(
            check_relink(Some(&content), &folder.0.join("gone.mp3")),
            Err(EuphoriaError::Missing { .. })
        ));
    }

    #[test]
    fn tracks_moved_together_are_found_by_name_and_content() {
        let old_folder = Folder::new("old");
        let new_folder = Folder::new("new");
        let moved = new_folder.file("moved.mp3", b"moved track");
        new_folder.file("renamed-elsewhere.mp3", b"same name, other audio");
        let moved_content = ContentHash::of_file(&moved).unwrap();
        let impostor_content = {
            let original = old_folder.file("probe.mp3", b"the original");
            ContentHash::of_file(&original).unwrap()
        };
        let missing = vec![
            (old_folder.0.join("moved.mp3"), moved_content),
            (old_folder.0.join("renamed-elsewhere.mp3"), impostor_content),
            (old_folder.0.join("not-moved.mp3"), moved_content),
        ];

        assert_eq!(
            find_in_folder(&new_folder.0, &missing),
            vec![(old_folder.0.join("moved.mp3"), moved)]
        );
    }
}
