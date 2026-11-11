// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `tags` command: writes the analysis into the files' own tags after
//! showing what would change, and undoes the last such write.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use euphoria_core::tags::{
    DjSoftware, KeyNotation, LabelPlacement, LoggedWrite, TagWriteOptions, WriteLog, WritePlan,
    default_write_log_directory, plan_write, undo_last_write,
};
use euphoria_core::{
    CachedBatch, EuphoriaError, TrackAnalysis, default_cache_path, find_audio_files,
};

use crate::batch::{self, AnalysisOptions};
use crate::style::Theme;
use crate::view::KeyNotationChoice;
use crate::{output, with_causes};

#[derive(Debug, Subcommand)]
pub enum TagsCommand {
    /// Write the key, tempo and energy into the tags of audio files, after
    /// showing what would change.
    #[command(after_help = crate::help::tags_examples())]
    Write(WriteArguments),
    /// Put back the tags that the last `tags write` replaced.
    Undo,
}

#[derive(Debug, Args)]
pub struct WriteArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Write without asking for confirmation.
    #[arg(long, short = 'y', conflicts_with = "dry_run")]
    yes: bool,
    /// Show what would change and write nothing.
    #[arg(long)]
    dry_run: bool,
    /// How the key is written, in the key field and in the label.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    /// Leave the tempo field alone.
    #[arg(long, conflicts_with = "tempo")]
    no_tempo: bool,
    /// Write the tempo even with the Serato preset, which leaves it out so
    /// that Serato detects the tempo itself.
    #[arg(long)]
    tempo: bool,
    /// Where the key and energy label, such as "8A - Energy 7", goes. The
    /// text already in that field is kept.
    #[arg(long, value_enum, default_value_t, value_name = "WHERE")]
    label: LabelChoice,
    /// The DJ program whose tag fields are written.
    #[arg(long, value_enum, default_value_t, value_name = "PROGRAM")]
    preset: PresetChoice,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum LabelChoice {
    /// In front of the comment.
    #[default]
    CommentStart,
    /// After the comment.
    CommentEnd,
    /// In front of the grouping.
    Grouping,
    /// Nowhere: the energy is not written.
    None,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum PresetChoice {
    /// Every field a common program reads.
    #[default]
    Generic,
    Rekordbox,
    Serato,
    Traktor,
    EngineDj,
    #[value(name = "virtualdj")]
    VirtualDj,
    Mixxx,
}

impl PresetChoice {
    fn dj_software(self) -> DjSoftware {
        match self {
            PresetChoice::Generic => DjSoftware::Generic,
            PresetChoice::Rekordbox => DjSoftware::Rekordbox,
            PresetChoice::Serato => DjSoftware::Serato,
            PresetChoice::Traktor => DjSoftware::Traktor,
            PresetChoice::EngineDj => DjSoftware::EngineDj,
            PresetChoice::VirtualDj => DjSoftware::VirtualDj,
            PresetChoice::Mixxx => DjSoftware::Mixxx,
        }
    }
}

impl WriteArguments {
    fn options(&self) -> TagWriteOptions {
        let preset = self.preset.dj_software();
        TagWriteOptions {
            key_notation: match self.key_notation {
                KeyNotationChoice::Camelot => KeyNotation::Camelot,
                KeyNotationChoice::Musical => KeyNotation::Musical,
                KeyNotationChoice::OpenKey => KeyNotation::OpenKey,
            },
            write_bpm: !self.no_tempo && (self.tempo || preset.writes_bpm_by_default()),
            label: match self.label {
                LabelChoice::CommentStart => LabelPlacement::CommentStart,
                LabelChoice::CommentEnd => LabelPlacement::CommentEnd,
                LabelChoice::Grouping => LabelPlacement::Grouping,
                LabelChoice::None => LabelPlacement::None,
            },
            preset,
        }
    }
}

pub fn run(command: TagsCommand) -> Result<ExitCode> {
    match command {
        TagsCommand::Write(arguments) => write(&arguments),
        TagsCommand::Undo => undo(),
    }
}

/// A file something went wrong with, and what.
pub struct Failure {
    pub path: PathBuf,
    pub error: EuphoriaError,
}

fn write(arguments: &WriteArguments) -> Result<ExitCode> {
    let audio_files = find_audio_files(&arguments.paths).map_err(with_causes)?;
    let theme = Theme::stderr();
    if audio_files.is_empty() {
        output::write_no_audio_files(&mut std::io::stderr().lock(), &theme)?;
        return Ok(ExitCode::SUCCESS);
    }
    let CachedBatch {
        results,
        store_error,
        ..
    } = batch::analyze(&audio_files, &arguments.analysis)?;

    // Planning only reads the files.
    let options = arguments.options();
    let mut plans: Vec<WritePlan> = Vec::new();
    let mut analyses: Vec<TrackAnalysis> = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut unchanged_count = 0;
    for result in results {
        let planned = result.outcome.and_then(|analysis| {
            plan_write(&result.path, &analysis, &options).map(|plan| (plan, analysis))
        });
        match planned {
            Ok((plan, _)) if plan.is_empty() => unchanged_count += 1,
            Ok((plan, analysis)) => {
                plans.push(plan);
                analyses.push(analysis);
            }
            Err(error) => failures.push(Failure {
                path: result.path,
                error,
            }),
        }
    }

    let mut standard_output = std::io::stdout().lock();
    output::write_tag_preview(&mut standard_output, &Theme::stdout(), &plans)?;
    standard_output.flush()?;
    drop(standard_output);
    let mut standard_error = std::io::stderr().lock();
    output::write_tag_failures(&mut standard_error, &theme, &failures)?;
    output::write_tag_plan_summary(
        &mut standard_error,
        &theme,
        plans.len(),
        unchanged_count,
        failures.len(),
    )?;

    let decision = if plans.is_empty() || arguments.dry_run {
        Decision::Stop
    } else if arguments.yes {
        Decision::Write
    } else if !std::io::stdin().is_terminal() {
        Decision::CannotAsk
    } else {
        write!(
            standard_error,
            "{} ",
            theme.bold(format_args!(
                "Write these changes to {}? [y/N]",
                output::counted_files(plans.len())
            ))
        )?;
        standard_error.flush()?;
        if is_yes(&mut std::io::stdin().lock())? {
            Decision::Write
        } else {
            Decision::Stop
        }
    };

    let mut failed = !failures.is_empty();
    match decision {
        Decision::Stop => writeln!(standard_error, "Nothing was written.")?,
        Decision::CannotAsk => {
            failed = true;
            output::write_error(
                &mut standard_error,
                &theme,
                "nothing was written: standard input is not a terminal, so there is nobody to \
                 confirm the changes.",
            )?;
            output::write_hint(
                &mut standard_error,
                &theme,
                "Run again with --yes to write them, or with --dry-run to only preview them.",
            )?;
        }
        Decision::Write => {
            let log_directory = default_write_log_directory().map_err(with_causes)?;
            let mut log = WriteLog::new(&log_directory);
            let mut write_failures = Vec::new();
            let mut unrecorded = Vec::new();
            let mut written = Vec::new();
            for (plan, analysis) in plans.iter().zip(&analyses) {
                let path = plan.path().to_path_buf();
                match log.apply(plan) {
                    Ok(LoggedWrite::Recorded) => written.push((path, analysis)),
                    Ok(LoggedWrite::NotRecorded(error)) => {
                        written.push((path.clone(), analysis));
                        unrecorded.push(Failure { path, error });
                    }
                    Err(error) => write_failures.push(Failure { path, error }),
                }
            }
            if arguments.analysis.uses_cache() && !written.is_empty() {
                let recorded = batch::open_cache().and_then(|mut cache| {
                    written
                        .iter()
                        .try_for_each(|(path, analysis)| cache.record_retagged_file(path, analysis))
                });
                if let Err(error) = recorded {
                    output::write_cache_record_warning(&mut standard_error, &theme, &error)?;
                }
            }
            output::write_tag_failures(&mut standard_error, &theme, &write_failures)?;
            output::write_unrecorded_tag_writes(&mut standard_error, &theme, &unrecorded)?;
            output::write_tag_write_summary(
                &mut standard_error,
                &theme,
                plans.len() - write_failures.len(),
                write_failures.len(),
            )?;
            failed |= !write_failures.is_empty() || !unrecorded.is_empty();
        }
    }

    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

enum Decision {
    Write,
    Stop,
    /// There is nobody to ask, and nobody said yes beforehand.
    CannotAsk,
}

/// Reads one line and tells whether it says yes. Anything else, including
/// an empty line and the end of the input, is a no.
fn is_yes(input: &mut impl BufRead) -> std::io::Result<bool> {
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn undo() -> Result<ExitCode> {
    let log_directory = default_write_log_directory().map_err(with_causes)?;
    let Some(undone) = undo_last_write(&log_directory).map_err(with_causes)? else {
        eprintln!("There is no tag write to undo.");
        return Ok(ExitCode::SUCCESS);
    };
    let mut standard_output = std::io::stdout().lock();
    output::write_restored_files(&mut standard_output, &undone.restored)?;
    // The restored files have the bytes they had before the write, which
    // the cache usually knows; noting where they are spares hashing them on
    // the next run. No cache is created for this, and one that cannot be
    // used changes nothing.
    let has_cache = default_cache_path().is_ok_and(|path| path.exists());
    if has_cache
        && !undone.restored.is_empty()
        && let Ok(mut cache) = batch::open_cache()
    {
        for path in &undone.restored {
            if cache.recognise_file(path).is_err() {
                break;
            }
        }
    }
    standard_output.flush()?;
    drop(standard_output);

    let failures: Vec<Failure> = undone
        .failed
        .into_iter()
        .map(|(path, error)| Failure { path, error })
        .collect();
    let mut standard_error = std::io::stderr().lock();
    let theme = Theme::stderr();
    output::write_tag_failures(&mut standard_error, &theme, &failures)?;
    output::write_tag_undo_summary(
        &mut standard_error,
        &theme,
        undone.restored.len(),
        undone.untouched.len(),
        failures.len(),
    )?;
    Ok(if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_preset_name_selects_its_own_program() {
        let expected = [
            ("generic", DjSoftware::Generic),
            ("rekordbox", DjSoftware::Rekordbox),
            ("serato", DjSoftware::Serato),
            ("traktor", DjSoftware::Traktor),
            ("engine-dj", DjSoftware::EngineDj),
            ("virtualdj", DjSoftware::VirtualDj),
            ("mixxx", DjSoftware::Mixxx),
        ];
        for (name, program) in expected {
            let choice = PresetChoice::from_str(name, false).unwrap();
            assert_eq!(choice.dj_software(), program, "{name}");
        }
        let names: Vec<String> = PresetChoice::value_variants()
            .iter()
            .filter_map(|choice| choice.to_possible_value())
            .map(|value| value.get_name().to_string())
            .collect();
        assert_eq!(names.len(), expected.len());
        for program in DjSoftware::ALL {
            assert!(
                expected.iter().any(|(_, mapped)| *mapped == program),
                "{program:?} has no preset name"
            );
        }
    }

    #[test]
    fn only_a_clear_yes_confirms() {
        for answer in ["y\n", "Y\n", "yes\n", " Yes \r\n"] {
            assert!(is_yes(&mut answer.as_bytes()).unwrap(), "{answer:?}");
        }
        for answer in ["", "\n", "n\n", "no\n", "yep\n", "y es\n", "oui\n"] {
            assert!(!is_yes(&mut answer.as_bytes()).unwrap(), "{answer:?}");
        }
    }
}
