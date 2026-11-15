// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `prepare` command: writes copies of tracks at one loudness, under a
//! true-peak ceiling, so a set plays evenly on a club system.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use euphoria_core::prepare::{
    DEFAULT_SUFFIX, LossyOutput, Manifest, PrepareSettings, ReplayGainFields, RevertReport,
    RunMode, TrackPlan, check_suffix, manifests_in, plan_track, prepare_track, prepared_path,
    revert, tag_track,
};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::Serialize;

use crate::batch::{self, AnalysisOptions};
use crate::output;
use crate::scan;
use crate::style::Theme;
use crate::terminal::Printable;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum LossyChoice {
    #[default]
    Flac,
    Wav,
}

#[derive(Debug, Args)]
pub struct PrepareArguments {
    /// Tracks, or folders of tracks, to prepare. They are only read, never
    /// changed.
    #[arg(
        required_unless_present = "revert",
        conflicts_with = "revert",
        value_name = "PATH"
    )]
    paths: Vec<PathBuf>,
    /// Measure the loudness and write it into the tags of the originals as
    /// ReplayGain track gain and peak, without changing the audio and
    /// without writing copies. Undo it with --revert.
    #[arg(long, conflicts_with_all = ["tune", "out", "revert"])]
    tag_only: bool,
    /// Undo a run: remove the files it wrote, and put back the tags a
    /// --tag-only run changed. Takes the manifest file the run left, or a
    /// folder to revert every run recorded in it. A file that changed since
    /// the run is kept.
    #[arg(long, value_name = "MANIFEST_OR_FOLDER")]
    revert: Option<PathBuf>,
    /// The integrated loudness to bring every track to, in LUFS: -14 for
    /// streaming, -11, -9 for the club, or -7 for the loudest.
    #[arg(
        long,
        default_value_t = -9.0,
        allow_negative_numbers = true,
        value_name = "LUFS"
    )]
    loudness: f32,
    /// The ceiling the true peak never passes, in dBTP.
    #[arg(
        long,
        default_value_t = -1.0,
        allow_negative_numbers = true,
        value_name = "DBTP"
    )]
    true_peak: f32,
    /// The most the limiter may turn peaks down, in dB. A track that would
    /// need more gets less gain instead, and says so.
    #[arg(long, default_value_t = 3.0, value_name = "DB")]
    max_limiting: f32,
    /// Shift tracks measured off A = 440 Hz back to concert pitch, with
    /// the tempo unchanged.
    #[arg(long)]
    tune: bool,
    /// Write the copies into this folder instead of next to the originals.
    #[arg(long, short = 'o', value_name = "DIR")]
    out: Option<PathBuf>,
    /// Added to the name of every copy, before the extension.
    #[arg(long, default_value = DEFAULT_SUFFIX, value_name = "TEXT")]
    suffix: String,
    /// What MP3, AAC, Ogg and other lossy tracks become: they are never
    /// written as lossy files.
    #[arg(long, value_enum, default_value_t, value_name = "FORMAT")]
    lossy_as: LossyChoice,
    /// Measure and print what would be done to each track, and write
    /// nothing.
    #[arg(long)]
    dry_run: bool,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

#[derive(Serialize)]
struct JsonTrack<'a> {
    input: &'a Path,
    output: Option<&'a Path>,
    status: &'static str,
    error: Option<String>,
    #[serde(flatten)]
    plan: Option<&'a TrackPlan>,
    limiter_reduction_db: Option<f32>,
    replaygain_track_gain: Option<&'a str>,
    replaygain_track_peak: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonSettings {
    target_lufs: f32,
    true_peak_ceiling_db: f32,
    maximum_limiting_db: f32,
    correct_tuning: bool,
    dry_run: bool,
}

#[derive(Serialize)]
struct JsonDocument<'a> {
    settings: JsonSettings,
    tracks: Vec<JsonTrack<'a>>,
    prepared: usize,
    skipped: usize,
    failed: usize,
}

#[derive(Default)]
struct RunState {
    reserved: HashSet<PathBuf>,
    manifests: BTreeMap<PathBuf, Manifest>,
}

struct Outcome {
    input: PathBuf,
    replaygain: Option<ReplayGainFields>,
    tagged: bool,
    plan: Option<TrackPlan>,
    output: Option<PathBuf>,
    written: bool,
    limiter_reduction_db: Option<f32>,
    error: Option<String>,
}

pub fn run(arguments: &PrepareArguments) -> Result<ExitCode> {
    if let Some(target) = &arguments.revert {
        return run_revert(target, arguments.json);
    }
    let settings = PrepareSettings {
        target_lufs: arguments.loudness,
        true_peak_ceiling_db: arguments.true_peak,
        maximum_limiting_db: arguments.max_limiting,
        correct_tuning: arguments.tune,
    };
    settings.validate().map_err(anyhow::Error::msg)?;
    check_suffix(&arguments.suffix).map_err(anyhow::Error::msg)?;
    let lossy = match arguments.lossy_as {
        LossyChoice::Flac => LossyOutput::Flac,
        LossyChoice::Wav => LossyOutput::Wav,
    };
    let out_directory = arguments
        .out
        .as_ref()
        .map(|directory| {
            std::path::absolute(directory)
                .with_context(|| format!("cannot use {}", directory.display()))
        })
        .transpose()?;
    if let Some(directory) = &out_directory
        && directory.exists()
        && !directory.is_dir()
    {
        bail!("{} is not a folder", directory.display());
    }

    let theme = Theme::stderr();
    let scan = scan::scan_quietly(&arguments.paths)?;
    let mut standard_error = std::io::stderr().lock();
    scan::write_notices(&mut standard_error, &theme, &scan)?;
    if scan.files.is_empty() {
        output::write_no_audio_files(&mut standard_error, &theme)?;
        return Ok(output::exit_status(false, true));
    }
    drop(standard_error);

    let tunings = if arguments.tune {
        measured_tunings(&scan.files, arguments)?
    } else {
        HashMap::new()
    };

    if !arguments.dry_run
        && let Some(directory) = &out_directory
    {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("cannot create {}", directory.display()))?;
    }

    let bar = progress_bar(scan.files.len());
    let mut state = RunState::default();
    let mut outcomes = Vec::new();
    for input in &scan.files {
        bar.set_message(file_label(input));
        let outcome = prepare_one(
            input,
            &settings,
            lossy,
            arguments,
            out_directory.as_deref(),
            tunings.get(input).copied().flatten(),
            &mut state,
        );
        outcomes.push(outcome);
        bar.inc(1);
    }
    bar.finish_and_clear();

    if !arguments.dry_run && !arguments.tag_only {
        for outcome in outcomes.iter().filter(|outcome| outcome.written) {
            if let Some(output) = &outcome.output {
                let folder = output.parent().map(Path::to_path_buf).unwrap_or_default();
                let manifest = state
                    .manifests
                    .entry(folder)
                    .or_insert_with(|| Manifest::new(RunMode::Copies, &settings));
                manifest
                    .record_copy(&outcome.input, output)
                    .map_err(crate::with_causes)?;
            }
        }
    }
    let mut manifest_paths = Vec::new();
    for (folder, manifest) in &state.manifests {
        if !manifest.entries.is_empty() {
            manifest_paths.push(manifest.save(folder).map_err(crate::with_causes)?);
        }
    }
    let failed = outcomes.iter().any(|outcome| outcome.error.is_some());
    if arguments.json {
        write_json(&settings, arguments.dry_run, &outcomes)?;
    } else {
        write_text(&outcomes, arguments.dry_run, &settings)?;
        for path in &manifest_paths {
            println!(
                "Run recorded in {}; undo it with: euphoria prepare --revert {}",
                Printable(path.display()),
                Printable(path.display())
            );
        }
    }
    Ok(output::exit_status(failed, false))
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| Printable(name.to_string_lossy().into_owned()).to_string())
        .unwrap_or_default()
}

fn progress_bar(count: usize) -> ProgressBar {
    let target = if std::io::stderr().is_terminal() {
        ProgressDrawTarget::stderr()
    } else {
        ProgressDrawTarget::hidden()
    };
    let bar = ProgressBar::with_draw_target(Some(count as u64), target);
    if let Ok(style) = ProgressStyle::with_template("{bar:30.cyan/blue} {pos}/{len} {msg}") {
        bar.set_style(style);
    }
    bar
}

fn measured_tunings(
    files: &[PathBuf],
    arguments: &PrepareArguments,
) -> Result<HashMap<PathBuf, Option<f32>>> {
    let analyzed = batch::analyze(files, &arguments.analysis, batch::Display::CountOnly)?;
    Ok(analyzed
        .results
        .into_iter()
        .map(|result| {
            let cents = result
                .outcome
                .ok()
                .and_then(|analysis| analysis.tuning_cents);
            (result.path, cents)
        })
        .collect())
}

fn prepare_one(
    input: &Path,
    settings: &PrepareSettings,
    lossy: LossyOutput,
    arguments: &PrepareArguments,
    out_directory: Option<&Path>,
    tuning_cents: Option<f32>,
    state: &mut RunState,
) -> Outcome {
    let mut outcome = Outcome {
        input: input.to_path_buf(),
        replaygain: None,
        tagged: false,
        plan: None,
        output: None,
        written: false,
        limiter_reduction_db: None,
        error: None,
    };
    let plan = match plan_track(input, settings, tuning_cents, lossy) {
        Ok(plan) => plan,
        Err(error) => {
            outcome.error = Some(crate::with_causes(error).to_string());
            return outcome;
        }
    };
    if arguments.tag_only {
        if plan.skipped.is_none() {
            outcome.replaygain = ReplayGainFields::from_plan(&plan);
            if let (false, Some(fields)) = (arguments.dry_run, &outcome.replaygain) {
                let manifest = state
                    .manifests
                    .entry(input.parent().map(Path::to_path_buf).unwrap_or_default())
                    .or_insert_with(|| Manifest::new(RunMode::TagOnly, settings));
                match tag_track(manifest, input, fields) {
                    Ok(()) => outcome.tagged = true,
                    Err(error) => outcome.error = Some(crate::with_causes(error).to_string()),
                }
            }
        }
        outcome.plan = Some(plan);
        return outcome;
    }
    if plan.skipped.is_none() {
        let output = prepared_path(
            input,
            out_directory,
            &arguments.suffix,
            plan.output_format,
            &state.reserved,
        );
        state.reserved.insert(output.clone());
        outcome.output = Some(output);
    }
    if !arguments.dry_run
        && let Some(output) = outcome.output.clone()
    {
        match prepare_track(input, &plan, &output, settings, &mut |_| {}) {
            Ok(prepared) => {
                outcome.written = true;
                outcome.limiter_reduction_db = Some(prepared.limiter_reduction_db);
            }
            Err(error) => outcome.error = Some(crate::with_causes(error).to_string()),
        }
    }
    outcome.plan = Some(plan);
    outcome
}

fn write_json(settings: &PrepareSettings, dry_run: bool, outcomes: &[Outcome]) -> Result<()> {
    let tracks: Vec<JsonTrack<'_>> = outcomes
        .iter()
        .map(|outcome| JsonTrack {
            input: &outcome.input,
            output: outcome.output.as_deref(),
            status: status(outcome, dry_run),
            error: outcome.error.clone(),
            plan: outcome.plan.as_ref(),
            limiter_reduction_db: outcome.limiter_reduction_db,
            replaygain_track_gain: outcome
                .replaygain
                .as_ref()
                .and_then(|fields| fields.gain.as_deref()),
            replaygain_track_peak: outcome
                .replaygain
                .as_ref()
                .and_then(|fields| fields.peak.as_deref()),
        })
        .collect();
    let count = |name: &str| tracks.iter().filter(|track| track.status == name).count();
    let document = JsonDocument {
        settings: JsonSettings {
            target_lufs: settings.target_lufs,
            true_peak_ceiling_db: settings.true_peak_ceiling_db,
            maximum_limiting_db: settings.maximum_limiting_db,
            correct_tuning: settings.correct_tuning,
            dry_run,
        },
        prepared: count("prepared") + count("tagged") + count("dry_run"),
        skipped: count("skipped"),
        failed: count("failed"),
        tracks,
    };
    let mut standard_output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut standard_output, &document)?;
    writeln!(standard_output)?;
    Ok(())
}

fn status(outcome: &Outcome, dry_run: bool) -> &'static str {
    if outcome.error.is_some() {
        "failed"
    } else if outcome
        .plan
        .as_ref()
        .is_some_and(|plan| plan.skipped.is_some())
    {
        "skipped"
    } else if dry_run {
        "dry_run"
    } else if outcome.tagged {
        "tagged"
    } else {
        "prepared"
    }
}

fn signed(value: f32) -> String {
    format!("{value:+.1}")
}

fn write_text(outcomes: &[Outcome], dry_run: bool, settings: &PrepareSettings) -> Result<()> {
    let theme = Theme::stdout();
    let mut standard_output = std::io::stdout().lock();
    let mut prepared = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for outcome in outcomes {
        writeln!(
            standard_output,
            "{}",
            theme.bold(Printable(outcome.input.display()))
        )?;
        if let Some(error) = &outcome.error {
            failed += 1;
            writeln!(
                standard_output,
                "  {} {}",
                theme.bad("failed"),
                Printable(error)
            )?;
            continue;
        }
        let Some(plan) = &outcome.plan else { continue };
        if let Some(reason) = plan.skipped {
            skipped += 1;
            writeln!(standard_output, "  skipped: {}", reason.description())?;
            continue;
        }
        prepared += 1;
        if let Some(lufs) = plan.measured_lufs {
            writeln!(
                standard_output,
                "  loudness  {lufs:.1} LUFS, gain {} dB, to {:.1} LUFS",
                signed(plan.gain_db),
                plan.predicted_lufs.unwrap_or(lufs),
            )?;
        }
        let peak = plan
            .true_peak_dbtp
            .map_or("none".to_string(), |peak| format!("{peak:.1} dBTP"));
        writeln!(
            standard_output,
            "  limiting  {:.1} dB, true peak {peak}, ceiling {:.1} dBTP",
            plan.limiting_db, settings.true_peak_ceiling_db
        )?;
        if plan.gain_capped {
            writeln!(
                standard_output,
                "  {}",
                theme.caution(format!(
                    "gain capped: reaching {:.1} LUFS needs {} dB, more than the {:.1} dB of limiting allowed",
                    plan.target_lufs,
                    signed(plan.wanted_gain_db),
                    settings.maximum_limiting_db
                ))
            )?;
        }
        match (plan.tuning_correction_cents, plan.tuning_cents) {
            (Some(correction), Some(measured)) => writeln!(
                standard_output,
                "  tuning    {measured:+.1} cents off 440 Hz, shifted {correction:+.1} cents"
            )?,
            (None, Some(measured)) if settings.correct_tuning => writeln!(
                standard_output,
                "  tuning    {measured:+.1} cents off 440 Hz, left as it is"
            )?,
            _ => {}
        }
        if plan.clipping.runs > 0 {
            writeln!(
                standard_output,
                "  {}",
                theme.caution(format!(
                    "clipping  {} passages at full scale, {} frames, first at {:.1} s; not repaired",
                    plan.clipping.runs,
                    plan.clipping.clipped_frames,
                    plan.clipping.first_run_seconds.unwrap_or(0.0)
                ))
            )?;
        }
        if let Some(fields) = &outcome.replaygain {
            writeln!(
                standard_output,
                "  replaygain  {} track gain, {} peak{}",
                fields.gain.as_deref().unwrap_or("none"),
                fields.peak.as_deref().unwrap_or("none"),
                if outcome.tagged { ", written" } else { "" }
            )?;
        }
        if let Some(path) = &outcome.output {
            let verb = if dry_run { "would write" } else { "wrote" };
            writeln!(
                standard_output,
                "  {verb}     {}",
                Printable(path.display())
            )?;
        }
    }
    let heading = if dry_run {
        format!("{prepared} to prepare")
    } else {
        format!("Prepared {prepared}")
    };
    let mut line = format!("{heading} of {}", outcomes.len());
    if skipped > 0 {
        line.push_str(&format!(", {skipped} skipped"));
    }
    if failed > 0 {
        line.push_str(&format!(", {failed} failed"));
    }
    writeln!(standard_output, "{line}")?;
    if dry_run {
        writeln!(standard_output, "Nothing was written.")?;
    }
    Ok(())
}

fn run_revert(target: &Path, json: bool) -> Result<ExitCode> {
    let manifests = if target.is_dir() {
        let mut found = manifests_in(target);
        found.reverse();
        found
    } else {
        vec![target.to_path_buf()]
    };
    if manifests.is_empty() {
        bail!("no prepare manifest in {}", target.display());
    }
    let mut report = RevertReport::default();
    for manifest in &manifests {
        let one = revert(manifest).map_err(crate::with_causes)?;
        report.removed.extend(one.removed);
        report.restored.extend(one.restored);
        report.kept.extend(one.kept);
    }
    let mut standard_output = std::io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut standard_output, &report)?;
        writeln!(standard_output)?;
    } else {
        for path in &report.removed {
            writeln!(standard_output, "removed   {}", Printable(path.display()))?;
        }
        for path in &report.restored {
            writeln!(standard_output, "restored  {}", Printable(path.display()))?;
        }
        for (path, reason) in &report.kept {
            writeln!(
                standard_output,
                "kept      {}: {}",
                Printable(path.display()),
                Printable(reason)
            )?;
        }
        writeln!(
            standard_output,
            "Removed {}, restored the tags of {}, kept {}",
            report.removed.len(),
            report.restored.len(),
            report.kept.len()
        )?;
    }
    Ok(
        if report
            .kept
            .iter()
            .any(|(_, reason)| reason != "it is already gone")
        {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        },
    )
}
