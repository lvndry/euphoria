// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The checks every export of a library runs before it writes: the output
//! file is one that may be written, and is neither the user's own collection
//! nor one of the audio files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::rekordbox::{ExistingFile, Library, PlaylistNode};
use crate::is_supported_audio_file;

/// Why a check refused, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CheckError(String);

/// The outcome of a check.
pub type Result<T> = std::result::Result<T, CheckError>;

impl CheckError {
    /// An error that says `message`.
    pub fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// An error that says `message`, followed by its `cause`.
    pub fn new(message: impl Into<String>, cause: impl std::fmt::Display) -> Self {
        Self(format!("{}: {cause}", message.into()))
    }

    /// This error under a more general `message`.
    pub fn context(self, message: impl Into<String>) -> Self {
        Self::new(message, self.0)
    }
}

/// Refuses an output that is a folder, a file that exists while it is to be
/// kept, or a file in a folder that does not exist.
pub fn check_output_file(output_file: &Path, existing: ExistingFile) -> Result<()> {
    if output_file.is_dir() {
        return Err(CheckError::message(format!(
            "{} is a folder. Give the name of a file.",
            output_file.display()
        )));
    }
    if existing == ExistingFile::Keep && output_file.symlink_metadata().is_ok() {
        return Err(CheckError::message(format!(
            "{} already exists",
            output_file.display()
        )));
    }
    if let Some(folder) = output_file.parent()
        && !folder.is_dir()
    {
        return Err(CheckError::message(format!(
            "cannot write {}: the folder {} does not exist",
            output_file.display(),
            folder.display()
        )));
    }
    Ok(())
}

/// Fails when `output_file` is one of `collections`, whatever the spelling
/// of either path, so that not even replacing the file overwrites the
/// user's collection with the export. A collection that cannot be read is
/// skipped: the guard is about the file the user is about to overwrite, and a
/// missing file is not that.
pub fn check_output_is_not_a_collection(
    output_file: &Path,
    collections: &[PathBuf],
    program_name: &str,
) -> Result<()> {
    let output = match std::fs::canonicalize(output_file) {
        Ok(output) => output,
        Err(_) => {
            let folder = output_file.parent().unwrap_or(Path::new("."));
            let name = output_file.file_name().unwrap_or_default();
            std::fs::canonicalize(folder)
                .map(|folder| folder.join(name))
                .unwrap_or_else(|_| output_file.to_path_buf())
        }
    };
    for collection in collections {
        let Ok(collection) = std::fs::canonicalize(collection) else {
            continue;
        };
        if output == collection {
            return Err(CheckError::message(format!(
                "the output {} is the {program_name} collection in {}; choose another output \
                 file. Replacing it is not offered",
                output_file.display(),
                collection.display()
            )));
        }
    }
    Ok(())
}

/// Refuses an output file that is one of the audio files, or that would be
/// taken for one the next time its folder is exported. Paths are compared
/// once resolved, so a symbolic link to an audio file and another spelling of
/// its path are refused too.
pub fn check_output_is_not_an_input(
    output_file: &Path,
    audio_files: &[PathBuf],
    folders: &[PathBuf],
) -> Result<()> {
    let resolved_output = resolve_output(output_file)?;
    for audio_file in audio_files {
        if audio_file
            .canonicalize()
            .is_ok_and(|resolved_input| resolved_input == resolved_output)
        {
            return Err(CheckError::message(format!(
                "{} is one of the files to export, and the export would replace it. Choose another output file.",
                output_file.display()
            )));
        }
    }
    if is_supported_audio_file(&resolved_output)
        && let Some(folder) = folders
            .iter()
            .find(|folder| resolved_output.starts_with(folder))
    {
        return Err(CheckError::message(format!(
            "{} has the extension of an audio file and is inside {}, a folder to export. Give the output file another extension, such as .xml.",
            output_file.display(),
            folder.display()
        )));
    }
    Ok(())
}

/// Where the output file really is: the file a symbolic link at that path
/// leads to, or for a file that does not exist yet, its name in its resolved
/// folder.
pub fn resolve_output(output_file: &Path) -> Result<PathBuf> {
    if let Ok(resolved) = output_file.canonicalize() {
        return Ok(resolved);
    }
    let cannot_use = || format!("cannot use {} as the output", output_file.display());
    let folder = output_file
        .parent()
        .ok_or_else(|| CheckError::message(cannot_use()))?;
    let file_name = output_file
        .file_name()
        .ok_or_else(|| CheckError::message(cannot_use()))?;
    let resolved_folder = folder
        .canonicalize()
        .map_err(|error| CheckError::new(cannot_use(), error))?;
    Ok(resolved_folder.join(file_name))
}

/// Removes the tracks at `paths` from the collection and from every
/// playlist of `library`, and returns how many tracks it removed.
pub fn leave_out(library: &mut Library, paths: &HashSet<PathBuf>) -> usize {
    fn from_nodes(nodes: &mut [PlaylistNode], is_cued: &impl Fn(&Path) -> bool) {
        for node in nodes {
            match node {
                PlaylistNode::Folder(folder) => from_nodes(&mut folder.children, is_cued),
                PlaylistNode::Playlist(playlist) => {
                    playlist.tracks.retain(|path| !is_cued(path));
                }
            }
        }
    }
    if paths.is_empty() {
        return 0;
    }
    let is_cued = |path: &Path| {
        paths.contains(path)
            || path
                .canonicalize()
                .is_ok_and(|resolved| paths.contains(&resolved))
    };
    let before = library.tracks.len();
    library.tracks.retain(|track| !is_cued(&track.path));
    from_nodes(&mut library.playlists, &is_cued);
    before - library.tracks.len()
}
