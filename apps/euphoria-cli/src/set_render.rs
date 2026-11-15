// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `set render` command: plays a set into one continuous mix, each
//! track blended into the next at a section start, and writes it as a WAV
//! or FLAC file with a tracklist, a CUE sheet and chapters beside it.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use euphoria_core::export::read_m3u_file;
use euphoria_core::export::rekordbox::ExistingFile;
use euphoria_core::harmony::transition::{
    AutomaticCues, PlanSection, PlanTrack, SectionSource, sections_from_cues,
};
use euphoria_core::plan_signals::{SignalsCache, TrackSignals};
use euphoria_core::set_render::{
    MixFormat, MixInput, MixMove, MixPlan, MixTimeline, RenderOptions, RenderProgress, RenderTrack,
    chapters, cue_sheet, format_mix_time, mix_timeline, plan_mix, render_mix, tracklist,
};
use euphoria_core::shift::ExistingOutput;
use euphoria_core::{
    ContentHash, FileResult, LockedTransition, PlaylistStore, TrackAnalysis,
    default_playlists_path, is_supported_audio_file, locked_blend, read_track_name,
};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::batch::{self, AnalysisOptions};
use crate::export::{check_output_file, write_missing_files};
use crate::playlists::find_playlist;
use crate::style::Theme;
use crate::terminal::Printable;
use crate::with_causes;

#[derive(Debug, Args)]
pub struct RenderArguments {
    /// The set in play order: the name of a playlist built in the desktop
    /// app, an M3U or M3U8 playlist file, or audio files.
    #[arg(required = true, value_name = "SET")]
    sources: Vec<PathBuf>,
    /// The file to write the mix to, ending in .wav or .flac.
    #[arg(
        long,
        short = 'o',
        visible_alias = "output",
        value_name = "FILE",
        required_unless_present = "dry_run"
    )]
    out: Option<PathBuf>,
    /// The file format. Defaults to the extension of --out; a name without
    /// one gets it added.
    #[arg(long, value_enum, value_name = "FORMAT")]
    format: Option<FormatChoice>,
    /// Also write a timestamped tracklist beside the mix, as
    /// NAME.tracklist.txt.
    #[arg(long)]
    tracklist: bool,
    /// Also write a CUE sheet beside the mix, as NAME.cue.
    #[arg(long)]
    cue: bool,
    /// Also write chapter lines for a video description beside the mix, as
    /// NAME.chapters.txt.
    #[arg(long)]
    chapters: bool,
    /// Show where each track comes in and why, without rendering.
    #[arg(long)]
    dry_run: bool,
    /// Replace the mix and the files beside it if they exist.
    #[arg(long)]
    force: bool,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

/// A file format for `--format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FormatChoice {
    /// 24 bit WAV. Past 4 GB it is written as RF64.
    Wav,
    /// 24 bit FLAC, lossless and about half the size.
    Flac,
}

impl FormatChoice {
    fn format(self) -> MixFormat {
        match self {
            FormatChoice::Wav => MixFormat::Wav,
            FormatChoice::Flac => MixFormat::Flac,
        }
    }
}

/// What a file written beside the mix holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideKind {
    Tracklist,
    CueSheet,
    Chapters,
}

impl SideKind {
    fn name(self) -> &'static str {
        match self {
            SideKind::Tracklist => "tracklist",
            SideKind::CueSheet => "CUE sheet",
            SideKind::Chapters => "chapters",
        }
    }
}

/// A file written beside the mix.
struct SideFile {
    path: PathBuf,
    kind: SideKind,
}

pub fn run(arguments: &RenderArguments) -> Result<ExitCode> {
    let output = match &arguments.out {
        Some(out) => Some(output_path(out, arguments.format)?),
        None => None,
    };
    let side_files = output
        .as_ref()
        .map(|(path, _)| side_files(path, arguments))
        .unwrap_or_default();
    if let Some((path, _)) = &output
        && !arguments.dry_run
    {
        let existing = if arguments.force {
            ExistingFile::Replace
        } else {
            ExistingFile::Keep
        };
        check_output_file(path, existing)?;
        for side in &side_files {
            check_output_file(&side.path, existing)?;
        }
    }

    let (title, files) = set_files(&arguments.sources, &arguments.analysis, output.as_ref())?;
    if files.is_empty() {
        bail!("the set has no tracks");
    }
    if let Some((path, _)) = &output
        && files.iter().any(|file| crate::set::same_file(file, path))
    {
        bail!(
            "{} is a track of the set, which is never changed",
            Printable(path.display())
        );
    }
    let tracks = analyze(&files, &arguments.analysis)?;
    let plan = plan(&tracks, &locked_transitions()?);
    let timeline = mix_timeline(&plan).map_err(|reason| anyhow::anyhow!(reason))?;

    let mut standard_output = std::io::stdout().lock();
    write_plan(
        &mut standard_output,
        &Theme::stdout(),
        &title,
        &plan,
        &timeline,
    )?;
    standard_output.flush()?;
    drop(standard_output);

    let Some((output, format)) = output else {
        return Ok(ExitCode::SUCCESS);
    };
    if arguments.dry_run {
        return Ok(ExitCode::SUCCESS);
    }
    let options = RenderOptions {
        format,
        existing: if arguments.force {
            ExistingOutput::Replace
        } else {
            ExistingOutput::Keep
        },
    };
    let mut progress = RenderBar::new();
    let rendered = render_mix(&plan, &output, &options, &mut progress);
    progress.bar.finish_and_clear();
    let rendered = rendered.map_err(with_causes)?;

    let mut report = std::io::stderr().lock();
    writeln!(
        report,
        "Wrote the mix to {} ({}, {} kHz, 24 bit {})",
        Printable(output.display()),
        format_mix_time(rendered.timeline.length_seconds),
        f64::from(rendered.sample_rate) / 1000.0,
        format.extension().to_uppercase()
    )?;
    let audio_file_name = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    for side in &side_files {
        let text = match side.kind {
            SideKind::CueSheet => cue_sheet(
                &title,
                &audio_file_name,
                format,
                &plan.tracks,
                &rendered.timeline,
            ),
            SideKind::Tracklist => tracklist(&title, &plan.tracks, &rendered.timeline),
            SideKind::Chapters => chapters(&plan.tracks, &rendered.timeline),
        };
        std::fs::write(&side.path, text)
            .with_context(|| format!("cannot write {}", Printable(side.path.display())))?;
        writeln!(
            report,
            "Wrote the {} to {}",
            side.kind.name(),
            Printable(side.path.display())
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

/// The absolute path of the mix and its format, from `--out` and
/// `--format`.
fn output_path(out: &Path, chosen: Option<FormatChoice>) -> Result<(PathBuf, MixFormat)> {
    let absolute = std::path::absolute(out)
        .with_context(|| format!("cannot use {} as the output", out.display()))?;
    let named = MixFormat::of_path(&absolute);
    match (named, chosen.map(FormatChoice::format)) {
        (Some(named), Some(chosen)) if named != chosen => bail!(
            "{} ends in .{} but --format is {}",
            absolute.display(),
            named.extension(),
            chosen.extension()
        ),
        (Some(named), _) => Ok((absolute, named)),
        (None, Some(chosen)) if absolute.extension().is_none() => {
            Ok((absolute.with_extension(chosen.extension()), chosen))
        }
        (None, _) => bail!(
            "{} must end in .wav or .flac: a mix is written as WAV or FLAC",
            absolute.display()
        ),
    }
}

fn side_files(output: &Path, arguments: &RenderArguments) -> Vec<SideFile> {
    let stem = output
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let beside = |suffix: &str| output.with_file_name(format!("{stem}{suffix}"));
    let mut files = Vec::new();
    if arguments.tracklist {
        files.push(SideFile {
            path: beside(".tracklist.txt"),
            kind: SideKind::Tracklist,
        });
    }
    if arguments.cue {
        files.push(SideFile {
            path: beside(".cue"),
            kind: SideKind::CueSheet,
        });
    }
    if arguments.chapters {
        files.push(SideFile {
            path: beside(".chapters.txt"),
            kind: SideKind::Chapters,
        });
    }
    files
}

/// The name of the set and its audio files in play order.
fn set_files(
    sources: &[PathBuf],
    analysis: &AnalysisOptions,
    output: Option<&(PathBuf, MixFormat)>,
) -> Result<(String, Vec<PathBuf>)> {
    if let [source] = sources {
        if !source.exists() {
            let Some(name) = source.to_str() else {
                bail!("{} does not exist", source.display());
            };
            let playlist = find_playlist(name, analysis.uses_cache())
                .with_context(|| format!("{name:?} is neither a file nor a playlist"))?;
            write_missing_files(&playlist.missing)?;
            return Ok((playlist.name, playlist.files));
        }
        let is_playlist_file = source
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("m3u") || extension.eq_ignore_ascii_case("m3u8")
            });
        if is_playlist_file {
            let files = read_m3u_file(source)
                .with_context(|| format!("cannot read {}", Printable(source.display())))?;
            let missing: Vec<&PathBuf> = files.iter().filter(|file| !file.is_file()).collect();
            if let Some(first) = missing.first() {
                bail!(
                    "{} of the playlist's tracks are missing, such as {}",
                    missing.len(),
                    Printable(first.display())
                );
            }
            let name = source
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Set".to_string());
            return Ok((name, files));
        }
    }
    for source in sources {
        if source.is_dir() {
            bail!(
                "{} is a folder, which has no play order: give a playlist, an M3U8 file or audio \
                 files in order",
                Printable(source.display())
            );
        }
        if !source.is_file() {
            bail!("{} does not exist", Printable(source.display()));
        }
        if !is_supported_audio_file(source) {
            bail!(
                "{} is not an audio file Euphoria reads",
                Printable(source.display())
            );
        }
    }
    let name = output
        .and_then(|(path, _)| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Mix".to_string());
    Ok((name, sources.to_vec()))
}

struct Loaded {
    path: PathBuf,
    content: Option<ContentHash>,
    analysis: TrackAnalysis,
    sections: Vec<PlanSection>,
    signals: TrackSignals,
    artist: Option<String>,
    title: Option<String>,
}

fn analyze(files: &[PathBuf], options: &AnalysisOptions) -> Result<Vec<Loaded>> {
    let files: Vec<PathBuf> = files
        .iter()
        .map(|file| file.canonicalize().unwrap_or_else(|_| file.clone()))
        .collect();
    let batch = batch::analyze(&files, options, batch::Display::Stages)?;
    let edits = batch::user_edits(&batch.results, options, batch::CorrectionCheck::Quick)?;
    let results = batch::corrected(batch.results, &edits);
    let cache = SignalsCache::open_default().map_err(with_causes)?;
    let mut loaded = Vec::with_capacity(results.len());
    for FileResult { path, outcome } in results {
        let analysis = outcome.map_err(|error| {
            with_causes(error).context(format!("cannot analyze {}", Printable(path.display())))
        })?;
        let signals = crate::plan::signals_of(&cache, &path, &analysis)?;
        let edited_cues = edits
            .get(&path)
            .and_then(|edits| edits.as_ref().ok())
            .and_then(|edits| edits.cues.clone());
        let sections = match (edited_cues, analysis.beat_grid) {
            (Some(cues), Some(grid)) => sections_from_cues(&cues, &grid),
            _ => AutomaticCues.sections(&analysis),
        };
        let name = read_track_name(&path).ok();
        loaded.push(Loaded {
            artist: name.as_ref().and_then(|name| name.artist.clone()),
            title: name.and_then(|name| name.title),
            content: ContentHash::of_file(&path).ok(),
            path,
            analysis,
            sections,
            signals,
        });
    }
    Ok(loaded)
}

/// The transitions the user locked in the desktop app, or none when no
/// playlists database is kept yet.
fn locked_transitions() -> Result<Vec<LockedTransition>> {
    let path = default_playlists_path().map_err(with_causes)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let store = PlaylistStore::open(&path).map_err(with_causes)?;
    store.load_locked_transitions().map_err(with_causes)
}

fn plan(tracks: &[Loaded], locks: &[LockedTransition]) -> MixPlan {
    let inputs: Vec<MixInput<'_>> = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| MixInput {
            track: RenderTrack {
                path: track.path.clone(),
                artist: track.artist.clone(),
                title: track.title.clone(),
                duration_seconds: f64::from(track.analysis.duration_seconds),
                grid: track.analysis.beat_grid,
            },
            plan: PlanTrack {
                analysis: &track.analysis,
                sections: &track.sections,
                energy: &track.signals.energy,
                stems: track.signals.stems.as_ref(),
            },
            locked_exit: tracks.get(index + 1).and_then(|next| {
                locked_blend(
                    locks,
                    (&track.path, track.content.as_ref()),
                    (&next.path, next.content.as_ref()),
                )
            }),
        })
        .collect();
    plan_mix(&inputs)
}

fn write_plan(
    writer: &mut impl Write,
    theme: &Theme,
    title: &str,
    plan: &MixPlan,
    timeline: &MixTimeline,
) -> std::io::Result<()> {
    let count = plan.tracks.len();
    writeln!(
        writer,
        "{}  {} {}, {}",
        theme.bold(Printable(title)),
        count,
        if count == 1 { "track" } else { "tracks" },
        format_mix_time(timeline.length_seconds)
    )?;
    writeln!(writer)?;
    let width = count.to_string().len();
    for (index, (track, placement)) in plan.tracks.iter().zip(&timeline.placements).enumerate() {
        writeln!(
            writer,
            "{:>width$}. {:>8}  {}",
            index + 1,
            format_mix_time(placement.start_seconds),
            Printable(track.display_name())
        )?;
        if let Some(transition) = plan.transitions.get(index) {
            let reason = match transition.mix {
                MixMove::Blend(_) => theme.dim(&transition.reason),
                MixMove::Sequence => theme.caution(&transition.reason),
            };
            writeln!(writer, "{:width$}  {:>8}  ↳ {reason}", "", "")?;
        }
    }
    Ok(())
}

/// The progress bar of a render on standard error.
struct RenderBar {
    bar: ProgressBar,
}

impl RenderBar {
    fn new() -> Self {
        let target = if std::io::stderr().is_terminal() {
            ProgressDrawTarget::stderr()
        } else {
            ProgressDrawTarget::hidden()
        };
        let bar = ProgressBar::with_draw_target(Some(0), target);
        if let Ok(style) =
            ProgressStyle::with_template("Rendering {bar:30.cyan/dim} {percent:>3}%  {eta} left")
        {
            bar.set_style(style.progress_chars("━━─"));
        }
        Self { bar }
    }
}

impl RenderProgress for RenderBar {
    fn rendered(&mut self, done_seconds: f64, total_seconds: f64) {
        self.bar.set_length(total_seconds.ceil() as u64);
        self.bar.set_position(done_seconds as u64);
    }
}
