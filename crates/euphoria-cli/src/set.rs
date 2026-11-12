// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `set` command: orders a folder or a playlist into a set that follows
//! an energy curve, with every mix in key and small tempo steps.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use euphoria_core::export::rekordbox::ExistingFile;
use euphoria_core::harmony::set::{DEFAULT_MAX_BPM_STEP, DEFAULT_SET_RELATIONS};
use euphoria_core::{
    CachedBatch, EnergyCurve, M3uTrack, MixTrack, Pin, PinPosition, Relation, SetOptions, SetPlan,
    SetTrack, build_set, find_audio_files, read_track_name, write_m3u8_file,
};

use crate::batch::{self, AnalysisOptions};
use crate::export::{check_output_file, check_output_is_not_an_input, write_missing_files};
use crate::playlists::find_playlist;
use crate::set_view::{self, SetView};
use crate::style::Theme;
use crate::terminal::Printable;
use crate::view::{self, KeyNotationChoice};
use crate::{output, with_causes};

/// The length of a set when neither `--tracks` nor `--minutes` is given.
const DEFAULT_TRACK_COUNT: usize = 20;

#[derive(Debug, Subcommand)]
pub enum SetCommand {
    /// Order tracks into a set that follows an energy curve, with every mix
    /// in a compatible key and small tempo steps.
    #[command(
        long_about = "Order tracks into a set that follows an energy curve, with every mix \
                      in a compatible key and small tempo steps.\n\n\
                      The pool is a folder, audio files, or the name of a playlist built in \
                      the desktop app. Tracks are analyzed first, or read from the cache. \
                      When no set can follow the curve or keep every rule, the closest one \
                      is shown and every place it falls short is listed.",
        after_help = crate::help::set_examples(),
    )]
    Build(BuildArguments),
}

#[derive(Debug, Args)]
pub struct BuildArguments {
    /// A folder or audio files to choose from, or the name of a playlist
    /// built in the desktop app. A path that exists wins over a playlist of
    /// the same name.
    #[arg(required = true, value_name = "FOLDER|PLAYLIST")]
    sources: Vec<PathBuf>,
    /// The energy the set follows: levels from 1 to 10 separated by commas,
    /// spread evenly from the first track to the last, such as 4,6,8,9,7,5,
    /// or a preset: warm-up, peak-time or journey.
    #[arg(long, default_value = "journey", value_name = "CURVE")]
    curve: EnergyCurve,
    /// How many tracks the set has. Defaults to 20.
    #[arg(long, value_name = "COUNT", conflicts_with = "minutes")]
    tracks: Option<usize>,
    /// How long the set lasts, in minutes. The track count is this length
    /// divided by the median length of the tracks in the pool, as if each
    /// played in full.
    #[arg(long, value_name = "MINUTES")]
    minutes: Option<f32>,
    /// Open the set with this track. It joins the pool if it is not in it.
    #[arg(long, value_name = "FILE")]
    first: Option<PathBuf>,
    /// Close the set with this track. It joins the pool if it is not in it.
    #[arg(long, value_name = "FILE")]
    last: Option<PathBuf>,
    /// The largest tempo change between two tracks, in beats per minute.
    #[arg(long, default_value_t = DEFAULT_MAX_BPM_STEP, value_name = "BPM")]
    max_bpm_step: f32,
    /// The key moves allowed between two tracks, separated by commas.
    /// Defaults to same,adjacent,relative,diagonal,energy-boost.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "MOVES")]
    moves: Vec<MoveChoice>,
    /// Let two tracks by the same artist follow each other.
    #[arg(long)]
    allow_artist_repeats: bool,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
    /// Also write the set as an M3U8 playlist file, which nearly every DJ
    /// and music program can open.
    #[arg(long, value_name = "FILE")]
    m3u: Option<PathBuf>,
    /// Replace the M3U8 file if it exists.
    #[arg(long, requires = "m3u")]
    force: bool,
    /// How keys are shown in the table.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

/// A key move to allow with `--moves`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum MoveChoice {
    /// The same key, such as 8A to 8A.
    Same,
    /// One step on the wheel, such as 8A to 9A.
    Adjacent,
    /// The other letter, such as 8A to 8B.
    Relative,
    /// One step and the other letter, such as 8A to 7B.
    Diagonal,
    /// Two steps up, such as 8A to 10A.
    EnergyBoost,
    /// The same home note in the other mode, such as 8A to 11B.
    MoodChange,
    /// Seven steps up, such as 8A to 3A.
    SemitoneBoost,
}

impl MoveChoice {
    fn relation(self) -> Relation {
        match self {
            MoveChoice::Same => Relation::Same,
            MoveChoice::Adjacent => Relation::Adjacent,
            MoveChoice::Relative => Relation::Relative,
            MoveChoice::Diagonal => Relation::Diagonal,
            MoveChoice::EnergyBoost => Relation::EnergyBoost,
            MoveChoice::MoodChange => Relation::MoodChange,
            MoveChoice::SemitoneBoost => Relation::SemitoneBoost,
        }
    }
}

/// A track of the pool as the set builder sees it, with what the output
/// shows of it.
pub struct PoolTrack {
    pub path: PathBuf,
    pub analysis: euphoria_core::TrackAnalysis,
    pub artist: Option<String>,
    pub title: Option<String>,
}

impl MixTrack for PoolTrack {
    fn key(&self) -> Option<euphoria_core::Key> {
        self.analysis.key
    }

    fn bpm(&self) -> Option<f32> {
        self.analysis.bpm
    }

    fn energy(&self) -> Option<f32> {
        self.analysis.energy.map(f32::from)
    }
}

impl SetTrack for PoolTrack {
    fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }
}

impl PoolTrack {
    /// "Artist - Title" from the tags, the title alone without an artist,
    /// and the file name when the tags say nothing.
    pub fn display_name(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ => self
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

pub fn run(command: SetCommand) -> Result<ExitCode> {
    match command {
        SetCommand::Build(arguments) => build(&arguments),
    }
}

fn build(arguments: &BuildArguments) -> Result<ExitCode> {
    if let Some(minutes) = arguments.minutes
        && !(minutes.is_finite() && minutes > 0.0)
    {
        bail!("--minutes must be a positive number of minutes");
    }
    if !(arguments.max_bpm_step.is_finite() && arguments.max_bpm_step >= 0.0) {
        bail!("--max-bpm-step must be zero or a positive number of beats per minute");
    }
    if arguments.tracks == Some(0) {
        bail!("--tracks must be at least 1");
    }
    let m3u_file = match &arguments.m3u {
        Some(path) => {
            let absolute = std::path::absolute(path)
                .with_context(|| format!("cannot use {} as the output", path.display()))?;
            check_output_file(&absolute, existing_file(arguments.force))?;
            Some(absolute)
        }
        None => None,
    };

    let (source_name, mut audio_files) = pool_files(&arguments.sources, &arguments.analysis)?;
    let first = arguments.first.as_deref().map(resolve_file).transpose()?;
    let last = arguments.last.as_deref().map(resolve_file).transpose()?;
    for pinned in first.iter().chain(&last) {
        if !audio_files.iter().any(|file| same_file(file, pinned)) {
            audio_files.push(pinned.clone());
        }
    }
    if let Some(m3u_file) = &m3u_file {
        check_output_is_not_an_input(m3u_file, &audio_files, &[])?;
    }
    if audio_files.is_empty() {
        output::write_no_audio_files(&mut std::io::stderr().lock(), &Theme::stderr())?;
        return Ok(ExitCode::FAILURE);
    }

    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(&audio_files, &arguments.analysis)?;
    let corrections =
        batch::corrections(&results, &arguments.analysis, batch::CorrectionCheck::Quick)?;
    let results = batch::corrected(results, &corrections);
    let (analyzed, failures): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|result| result.outcome.is_ok());
    let pool: Vec<PoolTrack> = analyzed
        .into_iter()
        .filter_map(|result| {
            let analysis = result.outcome.ok()?;
            let name = read_track_name(&result.path).ok();
            Some(PoolTrack {
                artist: name.as_ref().and_then(|name| name.artist.clone()),
                title: name.and_then(|name| name.title),
                path: result.path,
                analysis,
            })
        })
        .collect();

    let track_count = match (arguments.tracks, arguments.minutes) {
        (Some(count), _) => count,
        (None, Some(minutes)) => count_for_minutes(&pool, minutes),
        (None, None) => DEFAULT_TRACK_COUNT,
    };
    let mut options = SetOptions::new(track_count, arguments.curve.clone());
    options.max_bpm_step = arguments.max_bpm_step;
    options.allow_repeated_artists = arguments.allow_artist_repeats;
    options.relations = if arguments.moves.is_empty() {
        DEFAULT_SET_RELATIONS.to_vec()
    } else {
        arguments
            .moves
            .iter()
            .map(|choice| choice.relation())
            .collect()
    };
    let index_of = |wanted: &Path| pool.iter().position(|track| same_file(&track.path, wanted));
    for (file, position) in [(&first, PinPosition::At(0)), (&last, PinPosition::Last)] {
        if let Some(file) = file {
            let Some(track) = index_of(file) else {
                bail!(
                    "{} could not be analyzed, so the set cannot start or end with it",
                    file.display()
                );
            };
            options.pins.push(Pin { track, position });
        }
    }

    let plan = build_set(&pool, &options)
        .map_err(|error| anyhow::anyhow!(set_error_message(&error, &pool)))?;

    let mut standard_output = std::io::stdout().lock();
    let view = SetView {
        pool: &pool,
        plan: &plan,
        options: &options,
        notation: arguments.key_notation,
        width: if standard_output.is_terminal() {
            view::terminal_width()
        } else {
            view::DEFAULT_WIDTH
        },
    };
    if arguments.json {
        set_view::write_json(&mut standard_output, &view, &failures)?;
    } else {
        set_view::write_table(&mut standard_output, &Theme::stdout(), &view)?;
    }
    standard_output.flush()?;
    drop(standard_output);

    let mut report = Vec::new();
    let theme = Theme::stderr();
    if !arguments.json {
        output::write_failures(&mut report, &theme, &failures)?;
        set_view::write_summary(&mut report, &theme, &view)?;
    }
    if let Some(m3u_file) = &m3u_file {
        write_m3u(m3u_file, &source_name, &pool, &plan, arguments.force)?;
        writeln!(report, "Wrote the set to {}", Printable(m3u_file.display()))?;
    }
    if cached_count > 0 && !arguments.json {
        output::write_cache_summary(&mut report, cached_count, pool.len() + failures.len())?;
    }
    let report = String::from_utf8_lossy(&report);
    let report = match view::standard_error_width() {
        Some(width) if std::io::stderr().is_terminal() => view::wrap_lines(&report, width),
        _ => report.into_owned(),
    };
    std::io::stderr().lock().write_all(report.as_bytes())?;

    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(ExitCode::SUCCESS)
}

fn existing_file(force: bool) -> ExistingFile {
    if force {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    }
}

/// The name of the pool, for the playlist file, and its audio files: those
/// found in the paths, or those of the playlist named by the one source
/// that is not a path.
fn pool_files(sources: &[PathBuf], analysis: &AnalysisOptions) -> Result<(String, Vec<PathBuf>)> {
    if let [source] = sources
        && !source.exists()
    {
        let Some(name) = source.to_str() else {
            bail!("{} does not exist", source.display());
        };
        let playlist = find_playlist(name, analysis.uses_cache())
            .with_context(|| format!("{name:?} is neither a file, a folder nor a playlist"))?;
        write_missing_files(&playlist.missing)?;
        return Ok((playlist.name, playlist.files));
    }
    for source in sources {
        if !source.exists() {
            bail!("{} does not exist", source.display());
        }
    }
    let name = match sources {
        [source] => source
            .canonicalize()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "Set".to_string()),
        _ => "Set".to_string(),
    };
    Ok((name, find_audio_files(sources).map_err(with_causes)?))
}

fn resolve_file(path: &Path) -> Result<PathBuf> {
    if !path.is_file() {
        bail!("{} is not a file", path.display());
    }
    Ok(path.to_path_buf())
}

/// Whether two paths name one file, whatever their spelling.
fn same_file(first: &Path, second: &Path) -> bool {
    first == second
        || matches!(
            (first.canonicalize(), second.canonicalize()),
            (Ok(first), Ok(second)) if first == second
        )
}

/// The track count that fills `minutes` with tracks of the median length
/// of the pool, at least one.
fn count_for_minutes(pool: &[PoolTrack], minutes: f32) -> usize {
    let mut lengths: Vec<f32> = pool
        .iter()
        .map(|track| track.analysis.duration_seconds)
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .collect();
    if lengths.is_empty() {
        return DEFAULT_TRACK_COUNT;
    }
    lengths.sort_by(f32::total_cmp);
    let median = lengths[lengths.len() / 2];
    ((minutes * 60.0 / median).round() as usize).max(1)
}

/// The error of the set builder, with pool indices named as files.
fn set_error_message(error: &euphoria_core::SetError, pool: &[PoolTrack]) -> String {
    use euphoria_core::SetError;
    let file = |track: usize| {
        pool.get(track)
            .map(|track| Printable(track.path.display()).to_string())
            .unwrap_or_else(|| format!("track {track}"))
    };
    match error {
        SetError::PinnedTrackUnusable { track, missing } => format!(
            "{} has no {missing}, so the set cannot start or end with it",
            file(*track)
        ),
        SetError::ConflictingPins { track, .. } => format!(
            "{} cannot both open and close the set: give a longer set or another track",
            file(*track)
        ),
        other => other.to_string(),
    }
}

fn write_m3u(
    output_file: &Path,
    name: &str,
    pool: &[PoolTrack],
    plan: &SetPlan,
    force: bool,
) -> Result<()> {
    let tracks: Vec<M3uTrack> = plan
        .tracks
        .iter()
        .map(|planned| {
            let track = &pool[planned.track];
            M3uTrack {
                path: track.path.clone(),
                duration_seconds: Some(track.analysis.duration_seconds),
                display_name: track.display_name(),
            }
        })
        .collect();
    write_m3u8_file(output_file, name, &tracks, existing_file(force)).map_err(with_causes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_track(seconds: f32, artist: Option<&str>) -> PoolTrack {
        PoolTrack {
            path: PathBuf::from("/music/a.flac"),
            analysis: euphoria_core::TrackAnalysis {
                duration_seconds: seconds,
                key: None,
                bpm: None,
                energy: None,
                tuning_cents: None,
                key_timeline: Vec::new(),
                quality: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
            },
            artist: artist.map(str::to_string),
            title: None,
        }
    }

    #[test]
    fn minutes_become_a_count_by_the_median_track_length() {
        let pool = [
            pool_track(300.0, None),
            pool_track(360.0, None),
            pool_track(420.0, None),
            pool_track(f32::NAN, None),
        ];
        assert_eq!(count_for_minutes(&pool, 60.0), 10);
        assert_eq!(count_for_minutes(&pool, 1.0), 1);
        assert_eq!(count_for_minutes(&[], 60.0), DEFAULT_TRACK_COUNT);
    }

    #[test]
    fn every_move_names_its_own_relation() {
        let relations: Vec<Relation> = MoveChoice::value_variants()
            .iter()
            .map(|choice| choice.relation())
            .collect();
        assert_eq!(relations, Relation::ALL);
    }

    #[test]
    fn display_names_fall_back_to_the_file_name() {
        let mut track = pool_track(300.0, Some("Someone"));
        assert_eq!(track.display_name(), "a");
        track.title = Some("Tune".to_string());
        assert_eq!(track.display_name(), "Someone - Tune");
        track.artist = None;
        assert_eq!(track.display_name(), "Tune");
    }
}
