// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `export` command: analyzes files and writes the results in the format
//! of other DJ software.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use euphoria_core::export::rekordbox::{ExistingFile, ExportOptions, KeyNotation, write_xml_file};
use euphoria_core::{
    AssembledLibrary, CachedAnalyzer, CachedBatch, M3uTrack, PlaylistLayout, assemble_library,
    find_audio_files, is_supported_audio_file, playlist_count, read_track_name, write_m3u8_file,
};

use crate::batch::{self, AnalysisOptions};
use crate::playlists::find_playlist;
use crate::terminal::Printable;
use crate::{output, with_causes};

/// Used for the playlist of files named directly when the output file has no
/// name to borrow.
const FALLBACK_PLAYLIST_NAME: &str = "Euphoria";

#[derive(Debug, Subcommand)]
pub enum ExportTarget {
    /// Write a rekordbox XML file with the key and tempo of every track.
    Rekordbox(RekordboxArguments),
    /// Write a playlist built in the desktop app as an M3U8 file, which
    /// nearly every DJ and music program can open.
    M3u8(M3u8Arguments),
}

#[derive(Debug, Args)]
pub struct M3u8Arguments {
    /// The name of the playlist, as `euphoria playlists` lists it.
    #[arg(long, value_name = "NAME")]
    playlist: String,
    /// The M3U8 file to write.
    #[arg(long, short = 'o', value_name = "FILE")]
    output: PathBuf,
    /// Replace the output file if it exists.
    #[arg(long)]
    force: bool,
    /// Look neither in the cache for files that moved nor for their
    /// lengths.
    #[arg(long)]
    no_cache: bool,
}

#[derive(Debug, Args)]
pub struct RekordboxArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required_unless_present = "playlist", conflicts_with = "playlist")]
    paths: Vec<PathBuf>,
    /// Export a playlist built in the desktop app, by name, in place of
    /// files and folders: its tracks in the collection and one playlist in
    /// its order.
    #[arg(long, value_name = "NAME", conflicts_with = "no_playlists")]
    playlist: Option<String>,
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
        ExportTarget::M3u8(arguments) => export_to_m3u8(&arguments),
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

    let playlist = match &arguments.playlist {
        Some(name) => Some(find_playlist(name, arguments.analysis.uses_cache())?),
        None => None,
    };
    let (folders, audio_files) = match &playlist {
        Some(playlist) => (Vec::new(), playlist.files.clone()),
        None => {
            let paths = resolve(&arguments.paths)?;
            let folders: Vec<PathBuf> =
                paths.iter().filter(|path| path.is_dir()).cloned().collect();
            let audio_files = find_audio_files(&paths).map_err(with_causes)?;
            (folders, audio_files)
        }
    };
    check_output_is_not_an_input(&output_file, &audio_files, &folders)?;
    if let Some(playlist) = &playlist {
        write_missing_files(&playlist.missing)?;
    }
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
    let layout = if let Some(playlist) = &playlist {
        PlaylistLayout::Ordered {
            name: &playlist.name,
            tracks: &playlist.files,
        }
    } else if arguments.no_playlists {
        PlaylistLayout::CollectionOnly
    } else {
        PlaylistLayout::MirrorFolders {
            folders: &folders,
            loose_playlist_name: &loose_playlist_name,
        }
    };
    let AssembledLibrary {
        library,
        skipped,
        unread_tags,
    } = assemble_library(results, layout);

    // The file is written before anything is reported, so that a reader of
    // standard error that goes away cannot stop the export.
    let written = if library.tracks.is_empty() {
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

fn export_to_m3u8(arguments: &M3u8Arguments) -> Result<ExitCode> {
    let output_file = std::path::absolute(&arguments.output)
        .with_context(|| format!("cannot use {} as the output", arguments.output.display()))?;
    let existing = if arguments.force {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    };
    check_output_file(&output_file, existing)?;
    let playlist = find_playlist(&arguments.playlist, !arguments.no_cache)?;
    check_output_is_not_an_input(&output_file, &playlist.files, &[])?;
    write_missing_files(&playlist.missing)?;

    let cache = if arguments.no_cache {
        None
    } else {
        batch::open_cache().and_then(CachedAnalyzer::new).ok()
    };
    let tracks: Vec<M3uTrack> = playlist
        .files
        .iter()
        .map(|path| M3uTrack {
            path: path.clone(),
            duration_seconds: cache
                .as_ref()
                .and_then(|cache| cache.stored_analysis(path))
                .map(|analysis| analysis.duration_seconds),
            display_name: display_name(path),
        })
        .collect();
    write_m3u8_file(&output_file, &playlist.name, &tracks, existing).map_err(with_causes)?;
    eprintln!(
        "Exported {} {} of {} to {}",
        tracks.len(),
        if tracks.len() == 1 { "track" } else { "tracks" },
        Printable(&playlist.name),
        Printable(output_file.display())
    );
    Ok(if playlist.missing.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// "Artist - Title" from the file's tags, the title alone without an
/// artist, and the file name when the tags say nothing.
fn display_name(path: &Path) -> String {
    let fallback = || {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match read_track_name(path) {
        Ok(name) => match (name.artist, name.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title,
            (_, None) => fallback(),
        },
        Err(_) => fallback(),
    }
}

/// Says which tracks of the playlist are no longer anywhere, so they are
/// left out.
fn write_missing_files(missing: &[PathBuf]) -> Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    let mut standard_error = std::io::stderr().lock();
    for path in missing {
        writeln!(
            standard_error,
            "warning: left out {}: the file is no longer there",
            Printable(path.display())
        )?;
    }
    Ok(())
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
