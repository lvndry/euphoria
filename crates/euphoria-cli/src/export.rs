// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `export` command: analyzes files and writes the results in the format
//! of other DJ software.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use euphoria_core::export::rekordbox::{
    ExistingFile, ExportOptions, KeyNotation, Library, PlaylistNode, write_xml_file,
};
use euphoria_core::{
    AssembledLibrary, CachedBatch, ExportContents, PlaylistLayout, assemble_library_with,
    find_audio_files, is_supported_audio_file, playlist_count,
};

use crate::batch::{self, AnalysisOptions};
use crate::{collection, output, with_causes};

/// Used for the playlist of files named directly when the output file has no
/// name to borrow.
const FALLBACK_PLAYLIST_NAME: &str = "Euphoria";

#[derive(Debug, Subcommand)]
pub enum ExportTarget {
    /// Write a rekordbox XML file with the key and tempo of every track.
    Rekordbox(RekordboxArguments),
}

#[derive(Debug, Args)]
pub struct RekordboxArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// The rekordbox XML file to write.
    #[arg(long, short = 'o', value_name = "FILE")]
    output: PathBuf,
    /// Replace the output file if it exists.
    #[arg(long)]
    force: bool,
    /// How keys are written. rekordbox shows them as they are in the file.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    /// Export the tracks only, without a playlist per folder.
    #[arg(long)]
    no_playlists: bool,
    /// Write each track's beat grid, except grids rated low confidence,
    /// which rekordbox then makes itself.
    #[arg(long)]
    beat_grid: bool,
    /// Write cues where each track's sections start, as memory cues and hot
    /// cues A to H, on tracks whose beat grid is written. Needs
    /// --beat-grid, and --collection so that no track loses its own cues.
    #[arg(long, requires_all = ["beat_grid", "collection"])]
    cues: bool,
    /// Your rekordbox collection exported as XML (File > Export Collection
    /// in xml format). Tracks that already have cues or loops there are
    /// left out of the file, so that importing it cannot change them.
    #[arg(long, value_name = "FILE", requires = "cues")]
    collection: Option<PathBuf>,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum KeyNotationChoice {
    /// Camelot codes such as 8A.
    #[default]
    Camelot,
    /// Musical names such as Am.
    Musical,
}

impl From<KeyNotationChoice> for KeyNotation {
    fn from(choice: KeyNotationChoice) -> Self {
        match choice {
            KeyNotationChoice::Camelot => KeyNotation::Camelot,
            KeyNotationChoice::Musical => KeyNotation::Musical,
        }
    }
}

pub fn run(target: ExportTarget) -> Result<ExitCode> {
    match target {
        ExportTarget::Rekordbox(arguments) => export_to_rekordbox(&arguments),
    }
}

fn export_to_rekordbox(arguments: &RekordboxArguments) -> Result<ExitCode> {
    let output_file = std::path::absolute(&arguments.output)
        .with_context(|| format!("cannot use {} as the output", arguments.output.display()))?;
    let existing = if arguments.force {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    };
    // Checked before the analysis, which can take hours on a large library.
    check_output_file(&output_file, existing)?;
    let already_cued = match &arguments.collection {
        Some(collection) => collection::tracks_with_cues(collection)?,
        None => HashSet::new(),
    };

    let paths = resolve(&arguments.paths)?;
    let folders: Vec<PathBuf> = paths.iter().filter(|path| path.is_dir()).cloned().collect();
    let audio_files = find_audio_files(&paths).map_err(with_causes)?;
    check_output_is_not_an_input(&output_file, &audio_files, &folders)?;
    if audio_files.is_empty() {
        bail!("no audio files found, so nothing was exported");
    }

    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(&audio_files, &arguments.analysis)?;

    let loose_playlist_name = output_file
        .file_stem()
        .map(|file_stem| file_stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| FALLBACK_PLAYLIST_NAME.to_string());
    let layout = if arguments.no_playlists {
        PlaylistLayout::CollectionOnly
    } else {
        PlaylistLayout::MirrorFolders {
            folders: &folders,
            loose_playlist_name: &loose_playlist_name,
        }
    };
    let AssembledLibrary {
        mut library,
        skipped,
        unread_tags,
    } = assemble_library_with(
        results,
        layout,
        ExportContents {
            beat_grid: arguments.beat_grid,
            cues: arguments.cues,
        },
    );
    // A track that has cues of its own is left out of the file, so that no
    // import of the file can change them.
    let left_out = leave_out(&mut library, &already_cued);

    // The file is written before anything is reported, so that a reader of
    // standard error that goes away cannot stop the export.
    let written = if library.tracks.is_empty() && left_out > 0 {
        Err(anyhow!(
            "every track already has cues in your rekordbox collection, so nothing was \
             written; export again without --cues to update their key and tempo"
        ))
    } else if library.tracks.is_empty() {
        Err(anyhow!(
            "none of the files could be exported, so nothing was written"
        ))
    } else {
        let options = ExportOptions {
            key_notation: arguments.key_notation.into(),
        };
        write_xml_file(&output_file, &library, &options, existing).map_err(with_causes)
    };

    let mut standard_error = std::io::stderr().lock();
    if !skipped.is_empty() {
        output::write_skipped_tracks(&mut standard_error, &skipped)?;
    }
    written?;
    if !unread_tags.is_empty() {
        output::write_unread_tags(&mut standard_error, &unread_tags)?;
    }
    output::write_export_summary(
        &mut standard_error,
        &output::ExportSummary {
            track_count: library.tracks.len(),
            playlist_count: (!arguments.no_playlists).then(|| playlist_count(&library.playlists)),
            cached_count,
            output_file: &output_file,
        },
    )?;
    if arguments.beat_grid {
        let with_grid = library
            .tracks
            .iter()
            .filter(|track| track.first_beat.is_some())
            .count();
        output::write_beat_grid_summary(&mut standard_error, with_grid, library.tracks.len())?;
    }
    if arguments.cues {
        let with_cues = library
            .tracks
            .iter()
            .filter(|track| !track.cue_points.is_empty())
            .count();
        output::write_cue_summary(&mut standard_error, with_cues, library.tracks.len())?;
        if left_out > 0 {
            output::write_left_out_for_cues(&mut standard_error, left_out)?;
        }
    }
    output::write_rekordbox_import_steps(&mut standard_error)?;

    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(if skipped.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Removes the tracks at `paths` from the collection and from every
/// playlist of `library`, and returns how many tracks it removed.
fn leave_out(library: &mut Library, paths: &HashSet<PathBuf>) -> usize {
    fn from_nodes(nodes: &mut [PlaylistNode], paths: &HashSet<PathBuf>) {
        for node in nodes {
            match node {
                PlaylistNode::Folder(folder) => from_nodes(&mut folder.children, paths),
                PlaylistNode::Playlist(playlist) => {
                    playlist.tracks.retain(|path| !paths.contains(path));
                }
            }
        }
    }
    if paths.is_empty() {
        return 0;
    }
    let before = library.tracks.len();
    library.tracks.retain(|track| !paths.contains(&track.path));
    from_nodes(&mut library.playlists, paths);
    before - library.tracks.len()
}

fn check_output_file(output_file: &Path, existing: ExistingFile) -> Result<()> {
    if output_file.is_dir() {
        bail!(
            "{} is a folder. Give --output the name of a file.",
            output_file.display()
        );
    }
    if existing == ExistingFile::Keep && output_file.symlink_metadata().is_ok() {
        bail!(
            "{} already exists. Pass --force to replace it.",
            output_file.display()
        );
    }
    if let Some(folder) = output_file.parent()
        && !folder.is_dir()
    {
        bail!(
            "cannot write {}: the folder {} does not exist",
            output_file.display(),
            folder.display()
        );
    }
    Ok(())
}

/// Refuses an output file that is one of the audio files, or that would be
/// taken for one the next time its folder is exported. Paths are compared
/// once resolved, so a symbolic link to an audio file and another spelling of
/// its path are refused too.
fn check_output_is_not_an_input(
    output_file: &Path,
    audio_files: &[PathBuf],
    folders: &[PathBuf],
) -> Result<()> {
    let resolved_output = resolve_output(output_file)?;
    for audio_file in audio_files {
        // A file that cannot be resolved cannot be opened either, and is
        // reported when its analysis fails.
        if audio_file
            .canonicalize()
            .is_ok_and(|resolved_input| resolved_input == resolved_output)
        {
            bail!(
                "{} is one of the files to export, and the export would replace it. Choose another output file.",
                output_file.display()
            );
        }
    }
    if is_supported_audio_file(&resolved_output)
        && let Some(folder) = folders
            .iter()
            .find(|folder| resolved_output.starts_with(folder))
    {
        bail!(
            "{} has the extension of an audio file and is inside {}, a folder to export. Give the output file another extension, such as .xml.",
            output_file.display(),
            folder.display()
        );
    }
    Ok(())
}

/// Where the output file really is: the file a symbolic link at that path
/// leads to, or for a file that does not exist yet, its name in its resolved
/// folder.
fn resolve_output(output_file: &Path) -> Result<PathBuf> {
    if let Ok(resolved) = output_file.canonicalize() {
        return Ok(resolved);
    }
    let cannot_use = || format!("cannot use {} as the output", output_file.display());
    let folder = output_file.parent().with_context(cannot_use)?;
    let file_name = output_file.file_name().with_context(cannot_use)?;
    Ok(folder
        .canonicalize()
        .with_context(cannot_use)?
        .join(file_name))
}

/// The full path of each argument, with `..` and symbolic links in the
/// argument itself followed. rekordbox finds tracks by their full path, and
/// folders are compared by path to build the playlists.
fn resolve(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    paths
        .iter()
        .map(|path| {
            path.canonicalize()
                .with_context(|| format!("cannot read {}", path.display()))
        })
        .collect()
}
