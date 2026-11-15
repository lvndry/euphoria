// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `mashup` command: the tracks that layer over a track, ranked by the
//! same core code as the mashup mode of the desktop app's decks, and a pair
//! rendered to a WAV file the way the decks export it.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use euphoria_core::harmony::transition::matching_rate;
use euphoria_core::harmony::{
    BpmRange, EnergyRange, Layering, MashupMatch, MashupOptions, MashupReason, MashupTrack,
    MixTrack, Side, StemPart, StemProfile, TempoScaling, genre_fits, mashups_for, pair_parts,
};
use euphoria_core::mashup::{MASHUP_SAMPLE_RATE, MashupDeck, WrittenMashup, write_mashup};
use euphoria_core::shift::ExistingOutput;
use euphoria_core::stems::{CachedSeparator, StemCache, StemFiles, layerable_profile};
use euphoria_core::{CachedBatch, ContentHash, Key, TrackFilters, read_track_name};
use serde::Serialize;

use crate::batch::{self, AnalysisOptions};
use crate::set::{MoveChoice, PoolTrack, pool_files, same_file};
use crate::style::Theme;
use crate::terminal::Printable;
use crate::view::{self, KeyNotationChoice, shorten_middle};
use crate::{output, with_causes};

/// The number of ideas listed when `--limit` is not given.
const DEFAULT_LIMIT: usize = 10;

#[derive(Debug, Args)]
pub struct MashupArguments {
    /// The track to layer over, A. It is only read, never changed.
    #[arg(value_name = "TRACK_A")]
    track_a: PathBuf,
    /// The track to layer on top, B. Give it with --render to write the
    /// pair to a WAV file.
    #[arg(value_name = "TRACK_B", requires = "render")]
    track_b: Option<PathBuf>,
    /// Render TRACK_A and TRACK_B together into this WAV file: both from
    /// their first downbeat, B stretched to A's tempo and shifted.
    #[arg(long, value_name = "OUT.wav", requires = "track_b")]
    render: Option<PathBuf>,
    /// Shift B by this many semitones, from -6 to 6, in place of the shift
    /// the ideas suggest for the pair.
    #[arg(
        long,
        allow_negative_numbers = true,
        value_parser = clap::value_parser!(i8).range(-6..=6),
        value_name = "N",
        requires = "render"
    )]
    shift: Option<i8>,
    /// Replace the --render file if it exists.
    #[arg(long, requires = "render")]
    force: bool,
    /// What each track plays, separated by commas: a=vocals, b=instrumental
    /// or full. A side left out takes the other part: a=vocals plays B's
    /// instrumental. Listing ideas, a part for A ranks the tracks that go
    /// with it first.
    #[arg(
        long,
        value_delimiter = ',',
        value_parser = parse_stem_choice,
        value_name = "TRACK=PART"
    )]
    stems: Vec<StemChoice>,
    /// Folders to choose from. Defaults to the folder of TRACK_A.
    #[arg(long, value_name = "DIR", conflicts_with_all = ["playlist", "render"])]
    library: Vec<PathBuf>,
    /// A playlist built in the desktop app to choose from.
    #[arg(long, value_name = "NAME", conflicts_with = "render")]
    playlist: Option<String>,
    /// The key moves that count, separated by commas. Defaults to same,
    /// relative and adjacent, the keys that share most of their notes.
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "MOVE",
        conflicts_with = "render"
    )]
    key_moves: Vec<MoveChoice>,
    /// The largest tempo difference B is stretched across, in percent of
    /// A's tempo. Defaults to 8.
    #[arg(long, value_name = "PERCENT", conflicts_with = "render")]
    tempo_window: Option<f32>,
    /// Choose only tracks in this tempo range, in beats per minute: 120-126,
    /// 120- or -126.
    #[arg(
        long,
        value_name = "RANGE",
        allow_hyphen_values = true,
        conflicts_with = "render"
    )]
    bpm: Option<String>,
    /// Choose only tracks in this energy range, from 1 to 10: 6-8, 6- or -8.
    #[arg(
        long,
        value_name = "RANGE",
        allow_hyphen_values = true,
        conflicts_with = "render"
    )]
    energy: Option<String>,
    /// Choose only tracks whose genre tag contains this text, ignoring case.
    #[arg(long, value_name = "TEXT", conflicts_with = "render")]
    genre: Option<String>,
    /// How many ideas to list.
    #[arg(long, default_value_t = DEFAULT_LIMIT, value_name = "COUNT", conflicts_with = "render")]
    limit: usize,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    /// How keys are written.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

/// A part asked for one track with `--stems`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StemChoice {
    side: Side,
    part: StemPart,
}

fn parse_stem_choice(text: &str) -> Result<StemChoice, String> {
    let problem = || format!("{text:?} is not TRACK=PART, such as a=vocals or b=instrumental");
    let (track, part) = text.split_once('=').ok_or_else(problem)?;
    let side = match track.trim().to_lowercase().as_str() {
        "a" => Side::Reference,
        "b" => Side::Candidate,
        _ => return Err(problem()),
    };
    let part = match part.trim().to_lowercase().as_str() {
        "full" => StemPart::Full,
        "vocals" => StemPart::Vocals,
        "instrumental" => StemPart::Instrumental,
        _ => {
            return Err(format!(
                "{part:?} is not a part: use full, vocals or instrumental"
            ));
        }
    };
    Ok(StemChoice { side, part })
}

/// The parts given with `--stems`, A's first.
fn chosen_parts(choices: &[StemChoice]) -> Result<[Option<StemPart>; 2]> {
    let mut parts = [None, None];
    for choice in choices {
        let slot = match choice.side {
            Side::Reference => 0,
            Side::Candidate => 1,
        };
        if parts[slot].is_some_and(|part| part != choice.part) {
            bail!(
                "--stems gives two parts for {}; give one",
                choice.side.letter()
            );
        }
        parts[slot] = Some(choice.part);
    }
    Ok(parts)
}

/// A track as the mashup finder sees it: its analysis, genre and stems.
struct Layerable {
    track: PoolTrack,
    genre: Option<String>,
    stems: Option<(StemFiles, StemProfile)>,
}

impl MixTrack for Layerable {
    fn key(&self) -> Option<Key> {
        self.track.analysis.key
    }

    fn bpm(&self) -> Option<f32> {
        self.track.analysis.bpm
    }

    fn energy(&self) -> Option<f32> {
        self.track.analysis.energy.map(f32::from)
    }
}

impl MashupTrack for Layerable {
    fn stem_profile(&self) -> Option<&StemProfile> {
        self.stems.as_ref().map(|(_, profile)| profile)
    }
}

impl Layerable {
    fn of(track: PoolTrack) -> Self {
        let name = read_track_name(&track.path).ok();
        let genre = name.and_then(|name| name.genre);
        Self {
            track,
            genre,
            stems: None,
        }
    }

    /// Takes in the track's stems when they are in the stem cache.
    fn find_stems(&mut self, stem_cache: Option<&StemCache>) {
        self.stems = stem_cache.and_then(|stem_cache| {
            let hash = ContentHash::of_file(&self.track.path).ok()?;
            let files = stem_cache.cached(&hash)?;
            let profile = layerable_profile(&files)?;
            Some((files, profile))
        });
    }
}

pub fn run(arguments: &MashupArguments) -> Result<ExitCode> {
    let parts = chosen_parts(&arguments.stems)?;
    match (&arguments.track_b, &arguments.render) {
        (Some(track_b), Some(render)) => render_pair(arguments, track_b, render, parts),
        _ => list_ideas(arguments, pair_parts(parts[0], parts[1])[0]),
    }
}

/// Analyzes `files`, with the user's corrections, and returns the tracks
/// that were analyzed, in order, with the cache summary.
fn analyzed_tracks(
    files: &[PathBuf],
    analysis: &AnalysisOptions,
    json: bool,
) -> Result<(Vec<PoolTrack>, Vec<euphoria_core::FileResult>, CachedBatch)> {
    let mut batch = batch::analyze(
        files,
        analysis,
        if json {
            batch::Display::CountOnly
        } else {
            batch::Display::Stages
        },
    )?;
    let results = std::mem::take(&mut batch.results);
    let user_edits = batch::user_edits(&results, analysis, batch::CorrectionCheck::Quick)?;
    let results = batch::corrected(results, &user_edits);
    let (analyzed, failures): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|result| result.outcome.is_ok());
    let tracks = analyzed
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
    Ok((tracks, failures, batch))
}

fn canonical_file(path: &Path) -> Result<PathBuf> {
    if !path.is_file() {
        bail!("{} is not a file", Printable(path.display()));
    }
    path.canonicalize()
        .with_context(|| format!("cannot use {}", Printable(path.display())))
}

/// Takes the track at `path` out of `tracks`, or says why it is not there.
fn take_track(
    tracks: &mut Vec<PoolTrack>,
    failures: &[euphoria_core::FileResult],
    path: &Path,
    shown: &Path,
) -> Result<PoolTrack> {
    if let Some(failed) = failures.iter().find(|result| same_file(&result.path, path)) {
        let reason = failed.outcome.as_ref().err().map(output::describe);
        bail!(
            "{} could not be analyzed{}",
            Printable(shown.display()),
            reason
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        );
    }
    let position = tracks
        .iter()
        .position(|track| same_file(&track.path, path))
        .ok_or_else(|| anyhow!("{} could not be analyzed", Printable(shown.display())))?;
    Ok(tracks.remove(position))
}

/// The options the flags ask for, as the desktop's filter above the ideas
/// sets them.
fn mashup_options(
    arguments: &MashupArguments,
    reference_part: Option<StemPart>,
) -> Result<MashupOptions> {
    let defaults = MashupOptions::default();
    let mut filters = TrackFilters::default();
    if let Some(text) = &arguments.bpm {
        filters = filters
            .with_bpm_text(text)
            .map_err(|problem| anyhow!("--bpm {text}: {problem}"))?;
    }
    if let Some(text) = &arguments.energy {
        filters = filters
            .with_energy_text(text)
            .map_err(|problem| anyhow!("--energy {text}: {problem}"))?;
    }
    let bpm_range = (arguments.bpm.is_some())
        .then(|| {
            BpmRange::new(
                filters.bpm_min.unwrap_or(0.0),
                filters.bpm_max.unwrap_or(f32::MAX),
            )
        })
        .flatten();
    let energy_range = (arguments.energy.is_some())
        .then(|| {
            EnergyRange::new(
                f32::from(filters.energy_min.unwrap_or(1)),
                f32::from(filters.energy_max.unwrap_or(10)),
            )
        })
        .flatten();
    let max_tempo_difference_percent = match arguments.tempo_window {
        None => defaults.max_tempo_difference_percent,
        Some(percent) if percent.is_finite() && percent > 0.0 => percent,
        Some(percent) => bail!("--tempo-window {percent}: the window is a percent above zero"),
    };
    let mut relations: Vec<_> = arguments
        .key_moves
        .iter()
        .map(|choice| choice.relation())
        .collect();
    relations.sort();
    relations.dedup();
    Ok(MashupOptions {
        relations: if relations.is_empty() {
            defaults.relations.clone()
        } else {
            relations
        },
        max_tempo_difference_percent,
        bpm_range,
        energy_range,
        reference_part: reference_part.filter(|part| *part != StemPart::Full),
        ..defaults
    })
}

fn list_ideas(arguments: &MashupArguments, reference_part: Option<StemPart>) -> Result<ExitCode> {
    if arguments.limit == 0 {
        bail!("--limit must be at least 1");
    }
    let options = mashup_options(arguments, reference_part)?;
    let track_a = canonical_file(&arguments.track_a)?;
    let mut audio_files = match &arguments.playlist {
        Some(name) => {
            let playlist = crate::playlists::find_playlist(name, arguments.analysis.uses_cache())?;
            crate::export::write_missing_files(&playlist.missing)?;
            playlist.files
        }
        None if arguments.library.is_empty() => {
            pool_files(
                &[track_a.parent().map(Path::to_path_buf).unwrap_or_default()],
                &arguments.analysis,
            )?
            .1
        }
        None => pool_files(&arguments.library, &arguments.analysis)?.1,
    };
    for file in &mut audio_files {
        if let Ok(canonical) = file.canonicalize() {
            *file = canonical;
        }
    }
    audio_files.retain(|file| !same_file(file, &track_a));
    audio_files.insert(0, track_a.clone());

    let (mut tracks, failures, batch) =
        analyzed_tracks(&audio_files, &arguments.analysis, arguments.json)?;
    let reference = take_track(&mut tracks, &failures, &track_a, &arguments.track_a)?;
    let pool_size = tracks.len();
    let mut reference = Layerable::of(reference);
    let mut candidates: Vec<Layerable> = tracks
        .into_iter()
        .map(Layerable::of)
        .filter(|candidate| genre_fits(arguments.genre.as_deref(), candidate.genre.as_deref()))
        .collect();

    // Stems only change the order, so only the tracks that layer by key and
    // tempo are looked up and measured.
    let layering: Vec<PathBuf> = mashups_for(&reference, &candidates, &options)
        .into_iter()
        .map(|found| found.track.track.path.clone())
        .collect();
    let stem_cache = StemCache::open_default().ok();
    reference.find_stems(stem_cache.as_ref());
    for candidate in &mut candidates {
        if layering.contains(&candidate.track.path) {
            candidate.find_stems(stem_cache.as_ref());
        }
    }
    let ideas: Vec<MashupMatch<'_, Layerable>> = mashups_for(&reference, &candidates, &options)
        .into_iter()
        .take(arguments.limit)
        .collect();

    let mut standard_output = io::stdout().lock();
    if arguments.json {
        write_ideas_json(&mut standard_output, &reference, &ideas, reference_part)?;
    } else {
        let width = if standard_output.is_terminal() {
            view::terminal_width()
        } else {
            view::DEFAULT_WIDTH
        };
        write_ideas(
            &mut standard_output,
            &Theme::stdout(),
            &reference,
            &ideas,
            reference_part,
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
            &theme,
            &reference,
            &ideas,
            pool_size,
            arguments,
        )?;
        if batch.cached_count > 0 {
            output::write_cache_summary(
                &mut report,
                batch.cached_count,
                pool_size + failures.len() + 1,
            )?;
        }
    }
    let report = String::from_utf8_lossy(&report);
    let report = match view::standard_error_width() {
        Some(width) if io::stderr().is_terminal() => view::wrap_lines(&report, width),
        _ => report.into_owned(),
    };
    io::stderr().lock().write_all(report.as_bytes())?;

    if let Some(store_error) = batch.store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(output::exit_status(false, false))
}

/// Says why the list is empty or short, and what to change.
fn write_notes(
    writer: &mut impl Write,
    theme: &Theme,
    reference: &Layerable,
    ideas: &[MashupMatch<'_, Layerable>],
    pool_size: usize,
    arguments: &MashupArguments,
) -> io::Result<()> {
    let name = reference.track.display_name();
    if reference.track.analysis.key.is_none() || reference.track.analysis.bpm.is_none() {
        let missing = if reference.track.analysis.key.is_none() {
            "key"
        } else {
            "tempo"
        };
        return output::write_note(
            writer,
            theme,
            format_args!(
                "{} has no {missing}, so nothing can be layered over it.",
                Printable(&name)
            ),
        );
    }
    if pool_size == 0 {
        return output::write_note(
            writer,
            theme,
            "there is no other track to choose from. Pass --library with a folder or --playlist.",
        );
    }
    if ideas.is_empty() {
        return output::write_note(
            writer,
            theme,
            format_args!(
                "none of the {pool_size} other tracks layers over {} by key and tempo. \
                 Allow more --key-moves or a wider --tempo-window.",
                Printable(&name)
            ),
        );
    }
    if reference.stems.is_none() {
        output::write_note(
            writer,
            theme,
            format_args!(
                "{} has no stems yet, so vocals were not checked against instruments. \
                 Separate it with euphoria stems.",
                Printable(&name)
            ),
        )?;
    }
    if ideas.len() == arguments.limit {
        output::write_note(
            writer,
            theme,
            format_args!("showing the first {}; --limit shows more.", arguments.limit),
        )?;
    }
    Ok(())
}

fn part_words(part: StemPart) -> &'static str {
    match part {
        StemPart::Full => "the whole track",
        StemPart::Vocals => "its vocals",
        StemPart::Instrumental => "its instrumental",
    }
}

fn write_ideas(
    writer: &mut impl Write,
    theme: &Theme,
    reference: &Layerable,
    ideas: &[MashupMatch<'_, Layerable>],
    reference_part: Option<StemPart>,
    notation: KeyNotationChoice,
    width: usize,
) -> io::Result<()> {
    let analysis = &reference.track.analysis;
    let key = analysis.key.map_or_else(
        || theme.dim("no key"),
        |key| theme.key(key, notation.short(key)),
    );
    let bpm = analysis
        .bpm
        .map_or_else(|| "no tempo".to_string(), |bpm| format!("{bpm:.1} BPM"));
    let energy = analysis.energy.map_or_else(
        || "no energy".to_string(),
        |energy| format!("energy {energy}"),
    );
    let playing = reference_part
        .filter(|part| *part != StemPart::Full)
        .map(|part| format!("  {}", theme.dim(format!("A plays {}", part_words(part)))))
        .unwrap_or_default();
    writeln!(
        writer,
        "{} {}  {key}  {bpm}  {energy}{playing}",
        theme.bold("Layer over"),
        Printable(reference.track.display_name()),
    )?;
    let rank_width = ideas.len().to_string().len();
    let name_width = width.saturating_sub(rank_width + 8).max(12);
    for (index, idea) in ideas.iter().enumerate() {
        let candidate = &idea.track.track;
        let own_key = candidate
            .analysis
            .key
            .map_or_else(|| theme.dim("-"), |key| theme.key(key, notation.short(key)));
        let name = Printable(candidate.display_name()).to_string();
        writeln!(
            writer,
            "{:>rank_width$}  {own_key}  {}",
            index + 1,
            theme.bold(shorten_middle(&name, name_width)),
        )?;
        let indent = " ".repeat(rank_width + 2);
        for reason in idea.reasons() {
            let text = reason.describe(|key| notation.short(*key));
            let styled = match reason {
                MashupReason::StemsUnknown => theme.dim(&text),
                _ => text,
            };
            writeln!(writer, "{indent}{styled}")?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonTrack<'pool> {
    path: &'pool Path,
    artist: Option<&'pool str>,
    title: Option<&'pool str>,
    genre: Option<&'pool str>,
    key: Option<String>,
    key_name: Option<String>,
    bpm: Option<f32>,
    energy: Option<u8>,
    has_stems: bool,
}

fn json_track(track: &Layerable) -> JsonTrack<'_> {
    let analysis = &track.track.analysis;
    JsonTrack {
        path: &track.track.path,
        artist: track.track.artist.as_deref(),
        title: track.track.title.as_deref(),
        genre: track.genre.as_deref(),
        key: analysis.key.map(|key| key.camelot()),
        key_name: analysis.key.map(|key| key.to_string()),
        bpm: analysis.bpm.map(round_to_tenths),
        energy: analysis.energy,
        has_stems: track.stems.is_some(),
    }
}

#[derive(Serialize)]
struct JsonStems {
    a: &'static str,
    b: &'static str,
    overlap_percent: f32,
    muted_vocals: Option<&'static str>,
}

fn json_stems(layering: Layering) -> JsonStems {
    let [reference, candidate] = layering.parts();
    let muted_vocals = match layering {
        Layering::VocalsOver {
            vocals_from,
            instrumental_has_vocals: true,
            ..
        } => Some(if vocals_from == Side::Reference {
            "b"
        } else {
            "a"
        }),
        _ => None,
    };
    JsonStems {
        a: reference.name(),
        b: candidate.name(),
        overlap_percent: (layering.collision() * 100.0).round(),
        muted_vocals,
    }
}

#[derive(Serialize)]
struct JsonIdea<'pool> {
    rank: usize,
    #[serde(flatten)]
    track: JsonTrack<'pool>,
    shift_semitones: i8,
    shifted_key: String,
    key_move: &'static str,
    tempo_percent: f32,
    tempo_scaling: &'static str,
    energy_change: Option<f32>,
    stems: Option<JsonStems>,
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct JsonIdeas<'pool> {
    schema_version: u32,
    track: JsonTrack<'pool>,
    part: Option<&'static str>,
    ideas: Vec<JsonIdea<'pool>>,
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

fn write_ideas_json(
    writer: &mut impl Write,
    reference: &Layerable,
    ideas: &[MashupMatch<'_, Layerable>],
    reference_part: Option<StemPart>,
) -> io::Result<()> {
    let document = JsonIdeas {
        schema_version: output::JSON_SCHEMA_VERSION,
        track: json_track(reference),
        part: reference_part.map(StemPart::name),
        ideas: ideas
            .iter()
            .enumerate()
            .map(|(index, idea)| JsonIdea {
                rank: index + 1,
                track: json_track(idea.track),
                shift_semitones: idea.key.shift_semitones,
                shifted_key: idea.key.shifted_key.camelot(),
                key_move: crate::set_view::move_name(idea.key.relation),
                tempo_percent: round_to_tenths(idea.tempo.difference_percent),
                tempo_scaling: scaling_name(idea.tempo.scaling),
                energy_change: idea.energy_difference.map(f32::round),
                stems: idea.layering.map(json_stems),
                reasons: idea.reasons().iter().map(ToString::to_string).collect(),
            })
            .collect(),
    };
    serde_json::to_writer_pretty(&mut *writer, &document)?;
    writeln!(writer)
}

/// One deck of the render, as it was set up.
struct RenderedDeck {
    track: Layerable,
    part: StemPart,
    shift_semitones: i8,
    rate: f64,
    start_seconds: f64,
}

fn render_pair(
    arguments: &MashupArguments,
    track_b: &Path,
    render: &Path,
    parts: [Option<StemPart>; 2],
) -> Result<ExitCode> {
    let output = std::path::absolute(render)
        .with_context(|| format!("cannot use {} as the output", Printable(render.display())))?;
    if !output
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        bail!(
            "{} must end in .wav, the format a mashup is written in",
            Printable(output.display())
        );
    }
    if output.exists() && !arguments.force {
        bail!(
            "{} exists; pass --force to replace it",
            Printable(output.display())
        );
    }
    let path_a = canonical_file(&arguments.track_a)?;
    let path_b = canonical_file(track_b)?;
    if same_file(&path_a, &path_b) {
        bail!("TRACK_A and TRACK_B are the same file; give two tracks");
    }
    let files = vec![path_a.clone(), path_b.clone()];
    let (mut tracks, failures, batch) =
        analyzed_tracks(&files, &arguments.analysis, arguments.json)?;
    let mut reference = Layerable::of(take_track(
        &mut tracks,
        &failures,
        &path_a,
        &arguments.track_a,
    )?);
    let mut candidate = Layerable::of(take_track(&mut tracks, &failures, &path_b, track_b)?);
    let stem_cache = StemCache::open_default().ok();
    reference.find_stems(stem_cache.as_ref());
    candidate.find_stems(stem_cache.as_ref());

    let idea = mashups_for(
        &reference,
        std::slice::from_ref(&candidate),
        &MashupOptions::default(),
    )
    .into_iter()
    .next()
    .map(|found| (found.key.shift_semitones, found.layering));
    let shift_semitones = arguments
        .shift
        .or(idea.map(|(shift, _)| shift))
        .unwrap_or(0);
    let [part_a, part_b] = match pair_parts(parts[0], parts[1]) {
        [None, None] => idea
            .and_then(|(_, layering)| layering)
            .map_or([StemPart::Full; 2], |layering| layering.parts()),
        [part_a, part_b] => [
            part_a.unwrap_or(StemPart::Full),
            part_b.unwrap_or(StemPart::Full),
        ],
    };

    let bpm_a = deck_bpm(&reference);
    let bpm_b = deck_bpm(&candidate);
    let Some((bpm_a, bpm_b)) = bpm_a.zip(bpm_b) else {
        let missing = if bpm_a.is_none() {
            &arguments.track_a
        } else {
            track_b
        };
        bail!(
            "{} has no tempo, so the two cannot be beatmatched",
            Printable(missing.display())
        );
    };
    let rate_b = matching_rate(bpm_b, bpm_a);

    let mut separator = None;
    let deck_a = mashup_deck(&mut reference, part_a, 1.0, 0, &mut separator)?;
    let deck_b = mashup_deck(
        &mut candidate,
        part_b,
        rate_b,
        shift_semitones,
        &mut separator,
    )?;
    let layers = [deck_a.layer(), deck_b.layer()];
    let existing = if arguments.force {
        ExistingOutput::Replace
    } else {
        ExistingOutput::Keep
    };
    let written = write_mashup(&layers, None, &output, existing).map_err(with_causes)?;

    let decks = [
        RenderedDeck {
            track: reference,
            part: part_a,
            shift_semitones: 0,
            rate: 1.0,
            start_seconds: layers[0].start_seconds,
        },
        RenderedDeck {
            track: candidate,
            part: part_b,
            shift_semitones,
            rate: rate_b,
            start_seconds: layers[1].start_seconds,
        },
    ];
    let mut standard_output = io::stdout().lock();
    if arguments.json {
        write_render_json(&mut standard_output, &written, &decks)?;
    } else {
        write_render(
            &mut standard_output,
            &Theme::stdout(),
            &written,
            &decks,
            arguments.key_notation,
        )?;
    }
    standard_output.flush()?;
    if !arguments.json && written.gain < 1.0 {
        output::write_note(
            &mut io::stderr().lock(),
            &Theme::stderr(),
            format_args!(
                "the two layers together would have clipped, so the file was turned down {:.1} dB.",
                -20.0 * written.gain.log10()
            ),
        )?;
    }
    if let Some(store_error) = batch.store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(ExitCode::SUCCESS)
}

/// The tempo a deck plays the track at before any stretch: its beat
/// grid's, or the track's when it has no grid.
fn deck_bpm(track: &Layerable) -> Option<f64> {
    let analysis = &track.track.analysis;
    analysis
        .beat_grid
        .map(|grid| grid.bpm)
        .or(analysis.bpm.map(f64::from))
}

/// The deck that plays `part` of `track` from its first downbeat. A part
/// other than the whole track needs stems: when the cache has none, the
/// track is separated first.
fn mashup_deck(
    track: &mut Layerable,
    part: StemPart,
    rate: f64,
    shift_semitones: i8,
    separator: &mut Option<CachedSeparator>,
) -> Result<MashupDeck> {
    let path = track.track.path.clone();
    if part != StemPart::Full && track.stems.is_none() {
        if separator.is_none() {
            *separator = Some(CachedSeparator::open_default().map_err(with_causes)?);
        }
        if let Some(separator) = separator.as_mut() {
            let progress = crate::stems::SeparationProgress::new(&path);
            let separated = separator.stems(&path, &progress);
            progress.finish();
            let files = separated.map_err(with_causes)?;
            let profile = layerable_profile(&files).unwrap_or_default();
            track.stems = Some((files, profile));
        }
    }
    let analysis = &track.track.analysis;
    Ok(MashupDeck {
        path,
        stems: track.stems.as_ref().map(|(files, _)| files.clone()),
        stem_levels: part.levels(),
        position_seconds: 0.0,
        grid: analysis
            .beat_grid
            .map(|grid| (grid.first_downbeat_seconds, grid.bpm)),
        rate,
        shift_semitones,
        gain: 1.0,
    })
}

fn write_render(
    writer: &mut impl Write,
    theme: &Theme,
    written: &WrittenMashup,
    decks: &[RenderedDeck; 2],
    notation: KeyNotationChoice,
) -> io::Result<()> {
    let seconds = written.frames as f32 / MASHUP_SAMPLE_RATE as f32;
    writeln!(
        writer,
        "Wrote {}, {}",
        theme.bold(Printable(written.path.display())),
        view::format_duration(seconds)
    )?;
    for (letter, deck) in ["A", "B"].iter().zip(decks) {
        let mut details = vec![part_words(deck.part).to_string()];
        if deck.shift_semitones != 0 {
            let semitones = deck.shift_semitones.unsigned_abs();
            let shifted = deck
                .track
                .track
                .analysis
                .key
                .map(|key| {
                    format!(
                        " to {}",
                        notation.short(key.transposed(deck.shift_semitones))
                    )
                })
                .unwrap_or_default();
            details.push(format!(
                "shifted {} {semitones} semitone{}{shifted}",
                if deck.shift_semitones > 0 {
                    "up"
                } else {
                    "down"
                },
                if semitones == 1 { "" } else { "s" },
            ));
        }
        if (deck.rate - 1.0).abs() > 1e-6 {
            details.push(format!("played at {:.1}% speed", deck.rate * 100.0));
        }
        details.push(format!("from {:.2} s", deck.start_seconds));
        writeln!(
            writer,
            "{}  {}: {}",
            theme.bold(letter),
            Printable(deck.track.track.display_name()),
            details.join(", ")
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonDeck<'pool> {
    #[serde(flatten)]
    track: JsonTrack<'pool>,
    part: &'static str,
    shift_semitones: i8,
    rate: f64,
    start_seconds: f64,
}

#[derive(Serialize)]
struct JsonRender<'pool> {
    schema_version: u32,
    path: &'pool Path,
    seconds: f64,
    turned_down_db: f32,
    a: JsonDeck<'pool>,
    b: JsonDeck<'pool>,
}

fn json_deck(deck: &RenderedDeck) -> JsonDeck<'_> {
    JsonDeck {
        track: json_track(&deck.track),
        part: deck.part.name(),
        shift_semitones: deck.shift_semitones,
        rate: (deck.rate * 10_000.0).round() / 10_000.0,
        start_seconds: (deck.start_seconds * 1_000.0).round() / 1_000.0,
    }
}

fn write_render_json(
    writer: &mut impl Write,
    written: &WrittenMashup,
    decks: &[RenderedDeck; 2],
) -> io::Result<()> {
    let [deck_a, deck_b] = decks;
    let document = JsonRender {
        schema_version: output::JSON_SCHEMA_VERSION,
        path: &written.path,
        seconds: (written.frames as f64 / f64::from(MASHUP_SAMPLE_RATE) * 1_000.0).round()
            / 1_000.0,
        turned_down_db: ((-20.0 * written.gain.log10()) * 10.0).round() / 10.0,
        a: json_deck(deck_a),
        b: json_deck(deck_b),
    };
    serde_json::to_writer_pretty(&mut *writer, &document)?;
    writeln!(writer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stem_choice_names_a_track_and_a_part() {
        assert_eq!(
            parse_stem_choice("a=vocals"),
            Ok(StemChoice {
                side: Side::Reference,
                part: StemPart::Vocals
            })
        );
        assert_eq!(
            parse_stem_choice("B=Instrumental"),
            Ok(StemChoice {
                side: Side::Candidate,
                part: StemPart::Instrumental
            })
        );
        assert!(parse_stem_choice("c=vocals").is_err());
        assert!(parse_stem_choice("a=drums").is_err());
        assert!(parse_stem_choice("vocals").is_err());
    }

    #[test]
    fn two_different_parts_for_one_track_are_refused() {
        let vocals = StemChoice {
            side: Side::Reference,
            part: StemPart::Vocals,
        };
        let full = StemChoice {
            side: Side::Reference,
            part: StemPart::Full,
        };
        assert_eq!(
            chosen_parts(&[vocals, vocals]).unwrap(),
            [Some(StemPart::Vocals), None]
        );
        assert!(chosen_parts(&[vocals, full]).is_err());
    }
}
