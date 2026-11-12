// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `export` command: analyzes files and writes the results in the format
//! of other DJ software.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use euphoria_core::export::rekordbox::{
    ExistingFile, ExportOptions, KeyNotation, Library, PlaylistNode, write_xml_file,
};
use euphoria_core::export::traktor::{ModifiedTime, TraktorOptions, write_nml_file};
use euphoria_core::tags::KeyNotation as TagKeyNotation;
use euphoria_core::{
    AssembledLibrary, CachedAnalyzer, CachedBatch, EntryLocation, EuphoriaError, ExportContents,
    M3uTrack, PlaylistLayout, assemble_library_with, find_audio_files, is_supported_audio_file,
    playlist_count, read_track_name, write_m3u8_file,
};

use crate::batch::{self, AnalysisOptions};
use crate::collection::{self, CuedTracks};
use crate::playlists::find_playlist;
use crate::style::Theme;
use crate::terminal::Printable;
use crate::{output, traktor_collection, with_causes};

/// Used for the playlist of files named directly when the output file has no
/// name to borrow.
const FALLBACK_PLAYLIST_NAME: &str = "Euphoria";

#[derive(Debug, Subcommand)]
pub enum ExportTarget {
    /// Write a rekordbox XML file with the key and tempo of every track, and
    /// a playlist per folder.
    #[command(after_help = crate::help::export_examples())]
    Rekordbox(RekordboxArguments),
    /// Write a Traktor collection file (NML) with the key, tempo and comment
    /// of every track, and a playlist per folder.
    #[command(after_help = crate::help::traktor_export_examples())]
    Traktor(TraktorArguments),
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
    /// Export even though some tracks with cues in the collection match no
    /// file on this computer, so they cannot be recognised and protected.
    #[arg(long, requires = "collection")]
    allow_unmatched_cued_tracks: bool,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

#[derive(Debug, Args)]
pub struct TraktorArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required_unless_present = "playlist", conflicts_with = "playlist")]
    paths: Vec<PathBuf>,
    /// Export a playlist built in the desktop app, by name, in place of
    /// files and folders: its tracks in the collection and one playlist in
    /// its order.
    #[arg(long, value_name = "NAME", conflicts_with = "no_playlists")]
    playlist: Option<String>,
    /// The NML file to write. Never your Traktor collection.nml itself:
    /// import the file into Traktor instead.
    #[arg(long, short = 'o', value_name = "FILE")]
    output: PathBuf,
    /// Replace the output file if it exists.
    #[arg(long)]
    force: bool,
    /// How keys are written in the Key Text column. Traktor's Key column
    /// shows the key in its own notation whatever this is.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: TraktorKeyNotationChoice,
    /// Export the tracks only, without a playlist per folder.
    #[arg(long)]
    no_playlists: bool,
    /// Write each track's beat grid, except grids rated low confidence,
    /// which Traktor then makes itself.
    #[arg(long)]
    beat_grid: bool,
    /// Write cues where each track's sections start, as hot cues 1 to 8,
    /// on tracks whose beat grid is written. Needs --beat-grid, and
    /// --collection so that no track loses its own cues.
    #[arg(long, requires_all = ["beat_grid", "collection"])]
    cues: bool,
    /// Your Traktor collection.nml, in Documents/Native Instruments/Traktor
    /// <version>. It is only read. Tracks that already have cues or loops
    /// there are left out of the file, so that importing it cannot change
    /// them.
    #[arg(long, value_name = "FILE", requires = "cues")]
    collection: Option<PathBuf>,
    /// Export even though some tracks with cues in the collection match no
    /// file on this computer, so they cannot be recognised and protected.
    #[arg(long, requires = "collection")]
    allow_unmatched_cued_tracks: bool,
    /// The name of the Mac volume your system is on, which Traktor writes
    /// in front of every path outside /Volumes. Found by itself; give it
    /// only when the export is for another computer.
    #[arg(long, value_name = "NAME")]
    startup_volume: Option<String>,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum TraktorKeyNotationChoice {
    /// Camelot codes such as 8A.
    #[default]
    Camelot,
    /// Musical names such as Am.
    Musical,
    /// Open Key codes such as 1m, the notation Traktor shows its own keys
    /// in.
    OpenKey,
}

impl From<TraktorKeyNotationChoice> for TagKeyNotation {
    fn from(choice: TraktorKeyNotationChoice) -> Self {
        match choice {
            TraktorKeyNotationChoice::Camelot => TagKeyNotation::Camelot,
            TraktorKeyNotationChoice::Musical => TagKeyNotation::Musical,
            TraktorKeyNotationChoice::OpenKey => TagKeyNotation::OpenKey,
        }
    }
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
        ExportTarget::Traktor(arguments) => export_to_traktor(&arguments),
        ExportTarget::M3u8(arguments) => export_to_m3u8(&arguments),
    }
}

/// Which DJ program an export is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Rekordbox,
    Traktor,
}

impl Program {
    /// The program's name as it spells it.
    pub fn name(self) -> &'static str {
        match self {
            Program::Rekordbox => "rekordbox",
            Program::Traktor => "Traktor",
        }
    }
}

/// What every library export is asked for, whatever the program.
struct ExportRequest<'arguments> {
    program: Program,
    paths: &'arguments [PathBuf],
    playlist: Option<&'arguments str>,
    output: &'arguments Path,
    force: bool,
    no_playlists: bool,
    beat_grid: bool,
    cues: bool,
    collection: Option<&'arguments Path>,
    /// The collection files an export must never overwrite: the installed
    /// collections of the program on this computer.
    protected_collections: Vec<PathBuf>,
    allow_unmatched_cued_tracks: bool,
    analysis: &'arguments AnalysisOptions,
}

fn export_to_rekordbox(arguments: &RekordboxArguments) -> Result<ExitCode> {
    let options = ExportOptions {
        key_notation: arguments.key_notation.into(),
    };
    export_library(
        &ExportRequest {
            program: Program::Rekordbox,
            paths: &arguments.paths,
            playlist: arguments.playlist.as_deref(),
            output: &arguments.output,
            force: arguments.force,
            no_playlists: arguments.no_playlists,
            beat_grid: arguments.beat_grid,
            cues: arguments.cues,
            collection: arguments.collection.as_deref(),
            protected_collections: Vec::new(),
            allow_unmatched_cued_tracks: arguments.allow_unmatched_cued_tracks,
            analysis: &arguments.analysis,
        },
        collection::tracks_with_cues,
        |output_file, library, existing| write_xml_file(output_file, library, &options, existing),
    )
}

fn export_to_traktor(arguments: &TraktorArguments) -> Result<ExitCode> {
    let startup_volume = arguments
        .startup_volume
        .clone()
        .unwrap_or_else(traktor_collection::startup_volume_name);
    let options = TraktorOptions {
        key_notation: arguments.key_notation.into(),
        startup_volume: startup_volume.clone(),
        modified: ModifiedTime::now(),
    };
    export_library(
        &ExportRequest {
            program: Program::Traktor,
            paths: &arguments.paths,
            playlist: arguments.playlist.as_deref(),
            output: &arguments.output,
            force: arguments.force,
            no_playlists: arguments.no_playlists,
            beat_grid: arguments.beat_grid,
            cues: arguments.cues,
            collection: arguments.collection.as_deref(),
            protected_collections: traktor_collection::installed_collections(),
            allow_unmatched_cued_tracks: arguments.allow_unmatched_cued_tracks,
            analysis: &arguments.analysis,
        },
        |collection| traktor_collection::tracks_with_cues(collection, &startup_volume),
        |output_file, library, existing| write_nml_file(output_file, library, &options, existing),
    )
}

fn export_library(
    request: &ExportRequest<'_>,
    read_cued_tracks: impl Fn(&Path) -> Result<CuedTracks>,
    write: impl FnOnce(&Path, &Library, ExistingFile) -> Result<(), EuphoriaError>,
) -> Result<ExitCode> {
    let program = request.program;
    let output_file = std::path::absolute(request.output)
        .with_context(|| format!("cannot use {} as the output", request.output.display()))?;
    let existing = if request.force {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    };
    // Checked before the analysis, which can take hours on a large library.
    check_output_file(&output_file, existing)?;
    let cued_tracks = match request.collection {
        Some(collection) => {
            check_output_is_not_a_collection(&output_file, &[collection.to_path_buf()], program)?;
            let cued_tracks = read_cued_tracks(collection)?;
            if !cued_tracks.unmatched.is_empty() && !request.allow_unmatched_cued_tracks {
                bail!(
                    "{}",
                    unmatched_cued_tracks_message(&cued_tracks.unmatched, collection, program)
                );
            }
            cued_tracks
        }
        None => CuedTracks::default(),
    };
    // The user's installed collection is protected even without --collection,
    // so --force never overwrites the real one.
    if !request.protected_collections.is_empty() {
        check_output_is_not_a_collection(&output_file, &request.protected_collections, program)?;
    }
    let already_cued = &cued_tracks.paths;

    let playlist = match request.playlist {
        Some(name) => Some(find_playlist(name, request.analysis.uses_cache())?),
        None => None,
    };
    let (folders, audio_files) = match &playlist {
        Some(playlist) => (Vec::new(), playlist.files.clone()),
        None => {
            let paths = resolve(request.paths)?;
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

    // Before the analysis, which can take hours, and before anything is
    // written.
    batch::require_readable_corrections(request.analysis)?;
    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(&audio_files, request.analysis)?;
    let corrections =
        batch::corrections(&results, request.analysis, batch::CorrectionCheck::ReadFile)?;
    let results = batch::corrected(results, &corrections);

    let loose_playlist_name = output_file
        .file_stem()
        .map(|file_stem| file_stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| FALLBACK_PLAYLIST_NAME.to_string());
    let layout = if let Some(playlist) = &playlist {
        PlaylistLayout::Ordered {
            name: &playlist.name,
            tracks: &playlist.files,
        }
    } else if request.no_playlists {
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
            beat_grid: request.beat_grid,
            cues: request.cues,
        },
    );
    // A track that has cues of its own is left out of the file, so that no
    // import of the file can change them.
    let left_out = leave_out(&mut library, already_cued);

    // The file is written before anything is reported, so that a reader of
    // standard error that goes away cannot stop the export.
    let written = if library.tracks.is_empty() && left_out > 0 {
        Err(anyhow!(
            "every track already has cues in your {} collection, so nothing was \
             written; export again without --cues to update their key and tempo",
            program.name()
        ))
    } else if library.tracks.is_empty() {
        Err(anyhow!(
            "none of the files could be exported, so nothing was written"
        ))
    } else {
        write(&output_file, &library, existing).map_err(with_causes)
    };

    let mut standard_error = std::io::stderr().lock();
    let theme = Theme::stderr();
    output::write_skipped_tracks(&mut standard_error, &theme, &skipped)?;
    written?;
    output::write_unread_tags(&mut standard_error, &theme, &unread_tags)?;
    output::write_export_summary(
        &mut standard_error,
        &theme,
        &output::ExportSummary {
            track_count: library.tracks.len(),
            playlist_count: (!request.no_playlists).then(|| playlist_count(&library.playlists)),
            cached_count,
            output_file: &output_file,
        },
    )?;
    if request.beat_grid {
        let with_grid = library
            .tracks
            .iter()
            .filter(|track| track.first_beat.is_some())
            .count();
        output::write_beat_grid_summary(
            &mut standard_error,
            with_grid,
            library.tracks.len(),
            program,
        )?;
    }
    if request.cues {
        let with_cues = library
            .tracks
            .iter()
            .filter(|track| !track.cue_points.is_empty())
            .count();
        output::write_cue_summary(&mut standard_error, with_cues, library.tracks.len())?;
        if left_out > 0 {
            output::write_left_out_for_cues(&mut standard_error, left_out, program)?;
        }
        if !cued_tracks.unmatched.is_empty() {
            output::write_unmatched_cued_tracks(
                &mut standard_error,
                &cued_tracks.unmatched,
                program,
            )?;
        }
    }
    if !skipped.is_empty() {
        output::write_skipped_summary(&mut standard_error, skipped.len())?;
    }
    output::write_import_steps(&mut standard_error, &theme, program)?;

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
fn write_missing_files(missing: &[(PathBuf, EntryLocation)]) -> Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    let mut standard_error = std::io::stderr().lock();
    for (path, location) in missing {
        let reason = match location {
            EntryLocation::Replaced => {
                "the file there now holds other audio than the track that was added"
            }
            _ => "the file is no longer there",
        };
        writeln!(
            standard_error,
            "warning: left out {}: {reason}",
            Printable(path.display())
        )?;
    }
    Ok(())
}

/// How many cued tracks of the collection at `collection` match no file
/// here, which ones, and what to do about them.
fn unmatched_cued_tracks_message(
    unmatched: &[String],
    collection: &Path,
    program: Program,
) -> String {
    const LISTED: usize = 10;
    let mut message = format!(
        "{} with cues in {} match no file on this computer, so Euphoria cannot tell whether \
         they are among the tracks to export and cannot protect their cues:",
        output::counted(unmatched.len(), "track", "tracks"),
        collection.display()
    );
    for description in unmatched.iter().take(LISTED) {
        message.push_str("\n  ");
        message.push_str(description);
    }
    if unmatched.len() > LISTED {
        message.push_str(&format!("\n  and {} more", unmatched.len() - LISTED));
    }
    let name = program.name();
    let source = match program {
        Program::Rekordbox => "Export the collection from rekordbox on this computer",
        Program::Traktor => "Use the collection of Traktor on this computer",
    };
    message.push_str(&format!(
        "\nNothing was written. {source}, or relocate or remove those tracks in {name}. If \
         none of them is a track you are exporting, run again with \
         --allow-unmatched-cued-tracks."
    ));
    message
}

/// Fails when `output_file` is one of `collections`, whatever the spelling
/// of either path, so that not even `--force` replaces the user's
/// collection with the export. A collection that cannot be read is skipped:
/// the guard is about the file the user is about to overwrite, and a missing
/// file is not that.
fn check_output_is_not_a_collection(
    output_file: &Path,
    collections: &[PathBuf],
    program: Program,
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
            bail!(
                "the output {} is the {} collection in {}; choose another output \
                 file. --force does not change this",
                output_file.display(),
                program.name(),
                collection.display()
            );
        }
    }
    Ok(())
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
