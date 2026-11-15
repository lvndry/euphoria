// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `next` command: the best tracks to play after one, ranked by the
//! same core code as the desktop app's "Next after" list.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Args;
use euphoria_core::harmony::words::{TempoMove, Tone, movement_label};
use euphoria_core::harmony::{EnergyDirection, MixOptions, TempoScaling, next_tracks_favoring};
use euphoria_core::{CachedBatch, read_track_name};
use serde::Serialize;

use crate::batch::{self, AnalysisOptions};
use crate::filters::{FilterArguments, Filtered};
use crate::set::{PoolTrack, pool_files, same_file};
use crate::style::Theme;
use crate::view::{self, KeyNotationChoice, shorten_middle, width_of};
use crate::{output, with_causes};

/// The number of tracks listed when `--limit` is not given.
const DEFAULT_LIMIT: usize = 10;

#[derive(Debug, Args)]
pub struct NextArguments {
    /// The track playing now.
    #[arg(value_name = "TRACK")]
    track: PathBuf,
    /// A folder or audio files to choose from, or the name of a playlist
    /// built in the desktop app. Defaults to the folder of the track.
    #[arg(long, value_name = "FOLDER|PLAYLIST")]
    from: Vec<PathBuf>,
    /// Rank tracks with more energy first.
    #[arg(long, conflicts_with = "drop")]
    lift: bool,
    /// Rank tracks with less energy first. Without --lift or --drop, tracks
    /// at the same energy come first.
    #[arg(long)]
    drop: bool,
    /// How many tracks to list.
    #[arg(long, default_value_t = DEFAULT_LIMIT, value_name = "COUNT")]
    limit: usize,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
    /// How keys are shown in the table.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    #[command(flatten)]
    filters: FilterArguments,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

impl NextArguments {
    fn direction(&self) -> EnergyDirection {
        if self.lift {
            EnergyDirection::Up
        } else if self.drop {
            EnergyDirection::Down
        } else {
            EnergyDirection::Same
        }
    }
}

/// The name of an energy direction in JSON and in the header.
fn direction_name(direction: EnergyDirection) -> &'static str {
    match direction {
        EnergyDirection::Up => "lift",
        EnergyDirection::Same => "keep",
        EnergyDirection::Down => "drop",
    }
}

/// One track to play next, with how it follows the track playing.
pub struct NextPick<'pool> {
    pub track: &'pool PoolTrack,
    pub relation: euphoria_core::Relation,
    pub tempo_percent: Option<f32>,
    pub scaling: Option<TempoScaling>,
    pub energy_change: Option<i32>,
}

/// The best tracks to play after `playing` from `candidates`, best first,
/// narrowed by `filters`.
pub fn rank<'pool>(
    playing: &PoolTrack,
    candidates: &'pool [PoolTrack],
    filters: &euphoria_core::TrackFilters,
    direction: EnergyDirection,
    limit: usize,
) -> Vec<NextPick<'pool>> {
    let kept: Vec<&PoolTrack> = candidates
        .iter()
        .filter(|candidate| {
            !same_file(&candidate.path, &playing.path)
                && filters.matches(&Filtered(&candidate.analysis, None))
        })
        .collect();
    let options = MixOptions {
        allow_half_and_double_tempo: filters.half_or_double,
        ..MixOptions::default()
    };
    next_tracks_favoring(playing, kept.iter().copied(), &options, direction, |_| {
        false
    })
    .into_iter()
    .take(limit)
    .map(|mix| NextPick {
        track: mix.track,
        relation: mix.relation,
        tempo_percent: mix.tempo.map(|fit| fit.difference_percent),
        scaling: mix.tempo.map(|fit| fit.scaling),
        energy_change: mix.energy_difference.map(|change| change.round() as i32),
    })
    .collect()
}

pub fn run(arguments: &NextArguments) -> Result<ExitCode> {
    if arguments.limit == 0 {
        bail!("--limit must be at least 1");
    }
    let filters = arguments.filters.track_filters()?;
    if !arguments.track.is_file() {
        bail!("{} is not a file", arguments.track.display());
    }
    let playing_file = arguments.track.canonicalize()?;
    let sources = if arguments.from.is_empty() {
        vec![
            playing_file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        ]
    } else {
        arguments.from.clone()
    };
    let (_, mut audio_files) = pool_files(&sources, &arguments.analysis)?;
    for file in &mut audio_files {
        if let Ok(canonical) = file.canonicalize() {
            *file = canonical;
        }
    }
    audio_files.retain(|file| !same_file(file, &playing_file));
    audio_files.insert(0, playing_file.clone());

    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(
        &audio_files,
        &arguments.analysis,
        if arguments.json {
            batch::Display::CountOnly
        } else {
            batch::Display::Stages
        },
    )?;
    let user_edits =
        batch::user_edits(&results, &arguments.analysis, batch::CorrectionCheck::Quick)?;
    let results = batch::corrected(results, &user_edits);
    let (analyzed, failures): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|result| result.outcome.is_ok());
    if let Some(failed) = failures
        .iter()
        .find(|result| same_file(&result.path, &playing_file))
    {
        let reason = failed.outcome.as_ref().err().map(output::describe);
        bail!(
            "{} could not be analyzed{}",
            arguments.track.display(),
            reason
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        );
    }
    let mut pool: Vec<PoolTrack> = analyzed
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
    let Some(playing_index) = pool
        .iter()
        .position(|track| same_file(&track.path, &playing_file))
    else {
        bail!("{} could not be analyzed", arguments.track.display());
    };
    let playing = pool.remove(playing_index);
    let direction = arguments.direction();
    let picks = rank(&playing, &pool, &filters, direction, arguments.limit);

    let mut standard_output = io::stdout().lock();
    if arguments.json {
        write_json(&mut standard_output, &playing, &picks, direction)?;
    } else {
        let width = if standard_output.is_terminal() {
            view::terminal_width()
        } else {
            view::DEFAULT_WIDTH
        };
        write_table(
            &mut standard_output,
            &Theme::stdout(),
            &playing,
            &picks,
            direction,
            arguments.key_notation,
            width,
        )?;
    }
    standard_output.flush()?;
    drop(standard_output);

    let mut report = Vec::new();
    let theme = Theme::stderr();
    if !arguments.json {
        output::write_failures(&mut report, &theme, &failures)?;
        write_notes(
            &mut report,
            &playing,
            &picks,
            pool.len(),
            &filters,
            arguments,
        )?;
        if cached_count > 0 {
            output::write_cache_summary(
                &mut report,
                cached_count,
                pool.len() + failures.len() + 1,
            )?;
        }
    }
    let report = String::from_utf8_lossy(&report);
    let report = match view::standard_error_width() {
        Some(width) if io::stderr().is_terminal() => view::wrap_lines(&report, width),
        _ => report.into_owned(),
    };
    io::stderr().lock().write_all(report.as_bytes())?;

    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(output::exit_status(false, false))
}

/// Says why the list is empty or short, and what to change.
fn write_notes(
    writer: &mut impl Write,
    playing: &PoolTrack,
    picks: &[NextPick<'_>],
    pool_size: usize,
    filters: &euphoria_core::TrackFilters,
    arguments: &NextArguments,
) -> io::Result<()> {
    if playing.analysis.key.is_none() {
        writeln!(
            writer,
            "note: {} has no key, so nothing can be ranked after it.",
            playing.display_name()
        )?;
    } else if picks.is_empty() && pool_size == 0 {
        writeln!(
            writer,
            "note: there is no other track to choose from. Pass --from with a folder or a playlist."
        )?;
    } else if picks.is_empty() {
        let narrowed = if filters.is_active() {
            " Loosen --bpm, --energy or --quality."
        } else {
            ""
        };
        writeln!(
            writer,
            "note: none of the {pool_size} other tracks mixes into {} by key and tempo.{narrowed}",
            playing.display_name()
        )?;
    } else if picks.len() == arguments.limit {
        writeln!(
            writer,
            "note: showing the first {}; --limit shows more.",
            arguments.limit
        )?;
    }
    Ok(())
}

fn signed(value: i32) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn percent_text(percent: f32) -> String {
    format!("{percent:+.1}%")
}

/// A track's header: what plays now.
fn write_header(
    writer: &mut impl Write,
    theme: &Theme,
    playing: &PoolTrack,
    direction: EnergyDirection,
    notation: KeyNotationChoice,
) -> io::Result<()> {
    let key = playing.analysis.key.map_or_else(
        || theme.dim("no key"),
        |key| theme.key(key, notation.short(key)),
    );
    let bpm = playing
        .analysis
        .bpm
        .map_or_else(|| "no tempo".to_string(), |bpm| format!("{bpm:.1} BPM"));
    let energy = playing.analysis.energy.map_or_else(
        || "no energy".to_string(),
        |energy| format!("energy {energy}"),
    );
    writeln!(
        writer,
        "{} {}  {key}  {bpm}  {energy}  {}",
        theme.bold("After"),
        crate::terminal::Printable(playing.display_name()),
        theme.dim(match direction {
            EnergyDirection::Up => "lifting the energy",
            EnergyDirection::Same => "keeping the energy",
            EnergyDirection::Down => "dropping the energy",
        }),
    )
}

const HEADINGS: [&str; 6] = ["#", "Key", "BPM", "Tempo", "Move", "Energy"];

pub fn write_table(
    writer: &mut impl Write,
    theme: &Theme,
    playing: &PoolTrack,
    picks: &[NextPick<'_>],
    direction: EnergyDirection,
    notation: KeyNotationChoice,
    width: usize,
) -> io::Result<()> {
    write_header(writer, theme, playing, direction, notation)?;
    if picks.is_empty() {
        return Ok(());
    }
    struct Cells {
        values: [(String, usize); 6],
        name: String,
    }
    let styled_key = |pick: &NextPick<'_>| match pick.track.analysis.key {
        Some(key) => {
            let text = notation.short(key);
            (theme.key(key, &text), width_of(&text))
        }
        None => (theme.dim("-"), 1),
    };
    let rows: Vec<Cells> = picks
        .iter()
        .enumerate()
        .map(|(index, pick)| {
            let plain = |text: String| {
                let width = width_of(&text);
                (text, width)
            };
            let tempo = match pick.tempo_percent {
                Some(percent) => {
                    let tempo_move = TempoMove::of(percent);
                    let label = format!("{} {}", percent_text(percent), tempo_move.label());
                    let styled = match tempo_move.tone() {
                        Tone::Success => theme.good(&label),
                        Tone::Neutral => label.clone(),
                        Tone::Warning => theme.caution(&label),
                    };
                    (styled, width_of(&label))
                }
                None => (theme.dim("-"), 1),
            };
            let energy = match (pick.energy_change, pick.track.analysis.energy) {
                (Some(change), Some(energy)) => plain(format!("{energy} ({})", signed(change))),
                (None, Some(energy)) => plain(energy.to_string()),
                _ => (theme.dim("-"), 1),
            };
            Cells {
                values: [
                    plain((index + 1).to_string()),
                    styled_key(pick),
                    pick.track
                        .analysis
                        .bpm
                        .map_or_else(|| (theme.dim("-"), 1), |bpm| plain(format!("{bpm:.1}"))),
                    tempo,
                    plain(movement_label(Some(pick.relation)).to_string()),
                    energy,
                ],
                name: crate::terminal::Printable(pick.track.display_name()).to_string(),
            }
        })
        .collect();
    let right = [true, true, true, false, false, false];
    let widths: Vec<usize> = (0..HEADINGS.len())
        .map(|column| {
            rows.iter()
                .map(|row| row.values[column].1)
                .chain(std::iter::once(HEADINGS[column].len()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let gap = "  ";
    let fixed: usize = widths.iter().map(|width| width + gap.len()).sum();
    let name_width = width.saturating_sub(fixed).max(12);
    let pad = |styled: &str, text_width: usize, column: usize| {
        let padding = " ".repeat(widths[column].saturating_sub(text_width));
        if right[column] {
            format!("{padding}{styled}")
        } else {
            format!("{styled}{padding}")
        }
    };
    let mut heading = String::new();
    for (column, title) in HEADINGS.iter().enumerate() {
        heading.push_str(&pad(title, title.len(), column));
        heading.push_str(gap);
    }
    heading.push_str("Track");
    writeln!(writer, "{}", theme.dim(heading))?;
    for row in &rows {
        let mut line = String::new();
        for (column, (styled, text_width)) in row.values.iter().enumerate() {
            line.push_str(&pad(styled, *text_width, column));
            line.push_str(gap);
        }
        line.push_str(&shorten_middle(&row.name, name_width));
        writeln!(writer, "{}", line.trim_end())?;
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonNext<'pool> {
    schema_version: u32,
    track: JsonTrack<'pool>,
    energy: &'static str,
    next: Vec<JsonPick<'pool>>,
}

#[derive(Serialize)]
struct JsonTrack<'pool> {
    path: &'pool Path,
    artist: Option<&'pool str>,
    title: Option<&'pool str>,
    key: Option<String>,
    key_name: Option<String>,
    bpm: Option<f32>,
    energy: Option<u8>,
}

#[derive(Serialize)]
struct JsonPick<'pool> {
    rank: usize,
    #[serde(flatten)]
    track: JsonTrack<'pool>,
    key_move: &'static str,
    key_move_label: &'static str,
    tempo_percent: Option<f32>,
    tempo_move: Option<&'static str>,
    tempo_scaling: Option<&'static str>,
    energy_change: Option<i32>,
}

fn json_track(track: &PoolTrack) -> JsonTrack<'_> {
    JsonTrack {
        path: &track.path,
        artist: track.artist.as_deref(),
        title: track.title.as_deref(),
        key: track.analysis.key.map(|key| key.camelot()),
        key_name: track.analysis.key.map(|key| key.to_string()),
        bpm: track.analysis.bpm.map(round_to_tenths),
        energy: track.analysis.energy,
    }
}

fn round_to_tenths(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn scaling_name(scaling: TempoScaling) -> &'static str {
    match scaling {
        TempoScaling::Direct => "direct",
        TempoScaling::HalfTempo => "half",
        TempoScaling::DoubleTempo => "double",
    }
}

pub fn write_json(
    writer: &mut impl Write,
    playing: &PoolTrack,
    picks: &[NextPick<'_>],
    direction: EnergyDirection,
) -> io::Result<()> {
    let document = JsonNext {
        schema_version: output::JSON_SCHEMA_VERSION,
        track: json_track(playing),
        energy: direction_name(direction),
        next: picks
            .iter()
            .enumerate()
            .map(|(index, pick)| JsonPick {
                rank: index + 1,
                track: json_track(pick.track),
                key_move: crate::set_view::move_name(pick.relation),
                key_move_label: movement_label(Some(pick.relation)),
                tempo_percent: pick.tempo_percent.map(round_to_tenths),
                tempo_move: pick
                    .tempo_percent
                    .map(|percent| TempoMove::of(percent).code()),
                tempo_scaling: pick.scaling.map(scaling_name),
                energy_change: pick.energy_change,
            })
            .collect(),
    };
    serde_json::to_writer_pretty(&mut *writer, &document)?;
    writeln!(writer)
}
