// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `export` command: analyzes files and writes the results in the format
//! of other DJ software.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use euphoria_core::export::rekordbox::{ExistingFile, ExportOptions, KeyNotation, write_xml_file};
use euphoria_core::{
    AssembledLibrary, CachedBatch, PlaylistLayout, assemble_library, find_audio_files,
    playlist_count,
};

use crate::batch::{self, AnalysisOptions};
use crate::{output, with_causes};

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

    let paths = resolve(&arguments.paths)?;
    let folders: Vec<PathBuf> = paths.iter().filter(|path| path.is_dir()).cloned().collect();
    let audio_files = find_audio_files(&paths).map_err(with_causes)?;
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
    let AssembledLibrary { library, skipped } = assemble_library(results, layout);

    let mut standard_error = std::io::stderr().lock();
    if !skipped.is_empty() {
        output::write_skipped_tracks(&mut standard_error, &skipped)?;
    }
    if library.tracks.is_empty() {
        bail!("none of the files could be exported, so nothing was written");
    }

    let options = ExportOptions {
        key_notation: arguments.key_notation.into(),
    };
    write_xml_file(&output_file, &library, &options, existing).map_err(with_causes)?;
    output::write_export_summary(
        &mut standard_error,
        &output::ExportSummary {
            track_count: library.tracks.len(),
            playlist_count: (!arguments.no_playlists).then(|| playlist_count(&library.playlists)),
            cached_count,
            output_file: &output_file,
        },
    )?;
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
