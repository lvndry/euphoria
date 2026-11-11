// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `tags set` command: edits tag fields by hand across many files, with
//! the preview, confirmation and undo of `tags write`.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, anyhow, bail};
use clap::Args;
use euphoria_core::tags::{
    FieldEdit, LoggedWrite, TagField, TextChange, WriteLog, WritePlan, default_write_log_directory,
    plan_edit,
};
use euphoria_core::{CachedAnalyzer, TrackAnalysis, default_cache_path, find_audio_files};

use crate::batch::open_cache;
use crate::tags::{Decision, Failure, decide, report_nothing_written};
use crate::{output, with_causes};

#[derive(Debug, Args)]
pub struct SetArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Give a field a value, such as `genre=Deep House` or `year=2024`.
    /// An empty value, such as `album=`, removes the field. May be given
    /// more than once.
    #[arg(long = "field", value_name = "FIELD=VALUE")]
    fields: Vec<String>,
    /// Remove a field. A comment or grouping keeps the key and energy
    /// label Euphoria wrote in it.
    #[arg(long, value_name = "FIELD")]
    clear: Vec<String>,
    /// The field to find and replace in, with --find and --with.
    #[arg(long, value_name = "FIELD", requires_all = ["find", "with"])]
    replace: Option<String>,
    /// The text to find in the field --replace names.
    #[arg(long, value_name = "TEXT", requires = "replace")]
    find: Option<String>,
    /// The text that takes its place. May be empty.
    #[arg(long, value_name = "TEXT", requires = "replace")]
    with: Option<String>,
    /// Find only text with the same capital letters.
    #[arg(long, requires = "replace")]
    match_case: bool,
    /// Put text in front of a field, such as `title=[Edit] `.
    #[arg(long, value_name = "FIELD=TEXT")]
    add_prefix: Vec<String>,
    /// Put text at the end of a field.
    #[arg(long, value_name = "FIELD=TEXT")]
    add_suffix: Vec<String>,
    /// Take text off the front of a field, where it is there.
    #[arg(long, value_name = "FIELD=TEXT")]
    remove_prefix: Vec<String>,
    /// Take text off the end of a field, where it is there.
    #[arg(long, value_name = "FIELD=TEXT")]
    remove_suffix: Vec<String>,
    /// Write without asking for confirmation.
    #[arg(long, short = 'y', conflicts_with = "dry_run")]
    yes: bool,
    /// Show what would change and write nothing.
    #[arg(long)]
    dry_run: bool,
}

/// Makes the change of one of the prefix and suffix options.
type AffixChange = fn(String) -> TextChange;

/// The field a command line name stands for.
fn field_named(name: &str) -> Result<TagField> {
    TagField::from_name(name).ok_or_else(|| {
        let names: Vec<&str> = TagField::ALL.iter().map(|field| field.name()).collect();
        anyhow!(
            "{name:?} is not a field that can be edited. The fields are {}.",
            names.join(", ")
        )
    })
}

/// A `FIELD=TEXT` argument.
fn field_and_text(argument: &str, option: &str) -> Result<(TagField, String)> {
    let Some((name, text)) = argument.split_once('=') else {
        bail!("{option} takes FIELD=TEXT, such as genre=House, not {argument:?}");
    };
    Ok((field_named(name)?, text.to_string()))
}

impl SetArguments {
    /// The edits, in the order the options are listed in `--help`.
    fn edits(&self) -> Result<Vec<FieldEdit>> {
        let mut edits = Vec::new();
        for argument in &self.fields {
            let (field, text) = field_and_text(argument, "--field")?;
            edits.push(FieldEdit {
                field,
                change: TextChange::Set(text),
            });
        }
        for name in &self.clear {
            edits.push(FieldEdit {
                field: field_named(name)?,
                change: TextChange::Clear,
            });
        }
        if let (Some(name), Some(find), Some(with)) = (&self.replace, &self.find, &self.with) {
            if find.is_empty() {
                bail!("--find needs some text to look for");
            }
            edits.push(FieldEdit {
                field: field_named(name)?,
                change: TextChange::Replace {
                    find: find.clone(),
                    replace: with.clone(),
                    match_case: self.match_case,
                },
            });
        }
        let affixes: [(&[String], &str, AffixChange); 4] = [
            (&self.add_prefix, "--add-prefix", TextChange::AddPrefix),
            (&self.add_suffix, "--add-suffix", TextChange::AddSuffix),
            (
                &self.remove_prefix,
                "--remove-prefix",
                TextChange::RemovePrefix,
            ),
            (
                &self.remove_suffix,
                "--remove-suffix",
                TextChange::RemoveSuffix,
            ),
        ];
        for (arguments, option, change) in affixes {
            for argument in arguments {
                let (field, text) = field_and_text(argument, option)?;
                if text.is_empty() {
                    bail!("{option} needs some text, as in {}=TEXT", field.name());
                }
                edits.push(FieldEdit {
                    field,
                    change: change(text),
                });
            }
        }
        if edits.is_empty() {
            bail!(
                "nothing to change: give at least one of --field, --clear, --replace, \
                 --add-prefix, --add-suffix, --remove-prefix or --remove-suffix"
            );
        }
        Ok(edits)
    }
}

pub fn set(arguments: &SetArguments) -> Result<ExitCode> {
    let edits = arguments.edits()?;
    let audio_files = find_audio_files(&arguments.paths).map_err(with_causes)?;
    if audio_files.is_empty() {
        eprintln!("No audio files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let mut plans: Vec<WritePlan> = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut unchanged_count = 0;
    for path in audio_files {
        match plan_edit(&path, &edits) {
            Ok(plan) if plan.is_empty() => unchanged_count += 1,
            Ok(plan) => plans.push(plan),
            Err(error) => failures.push(Failure { path, error }),
        }
    }

    let mut standard_output = std::io::stdout().lock();
    output::write_tag_preview(&mut standard_output, &plans)?;
    standard_output.flush()?;
    drop(standard_output);
    let mut standard_error = std::io::stderr().lock();
    output::write_tag_failures(&mut standard_error, &failures)?;
    output::write_tag_plan_summary(
        &mut standard_error,
        plans.len(),
        unchanged_count,
        failures.len(),
    )?;

    let decision = decide(
        plans.len(),
        arguments.dry_run,
        arguments.yes,
        &mut standard_error,
    )?;
    let mut failed = !failures.is_empty();
    if !matches!(decision, Decision::Write) {
        failed |= report_nothing_written(decision, &mut standard_error)?;
        return Ok(exit_code(failed));
    }

    // The analyses the cache holds for the files as they are now, so that
    // the files are still known once their tags change and are not
    // analyzed again.
    let cache = default_cache_path()
        .is_ok_and(|path| path.exists())
        .then(|| open_cache().and_then(CachedAnalyzer::new).ok())
        .flatten();
    let known: Vec<Option<TrackAnalysis>> = plans
        .iter()
        .map(|plan| {
            cache
                .as_ref()
                .and_then(|cache| cache.stored_analysis(plan.path()))
        })
        .collect();

    let log_directory = default_write_log_directory().map_err(with_causes)?;
    let mut log = WriteLog::new(&log_directory);
    let mut write_failures = Vec::new();
    let mut unrecorded = Vec::new();
    for (plan, analysis) in plans.iter().zip(&known) {
        let path = plan.path().to_path_buf();
        let written = match log.apply(plan) {
            Ok(LoggedWrite::Recorded) => true,
            Ok(LoggedWrite::NotRecorded(error)) => {
                unrecorded.push(Failure {
                    path: path.clone(),
                    error,
                });
                true
            }
            Err(error) => {
                write_failures.push(Failure { path, error });
                false
            }
        };
        if let (true, Some(cache), Some(analysis)) = (written, &cache, analysis)
            && let Err(error) = cache.record_retagged_file(plan.path(), analysis)
        {
            output::write_cache_record_warning(&mut standard_error, &error)?;
        }
    }
    output::write_tag_failures(&mut standard_error, &write_failures)?;
    output::write_unrecorded_tag_writes(&mut standard_error, &unrecorded)?;
    output::write_tag_write_summary(
        &mut standard_error,
        plans.len() - write_failures.len(),
        write_failures.len(),
    )?;
    failed |= !write_failures.is_empty() || !unrecorded.is_empty();
    Ok(exit_code(failed))
}

fn exit_code(failed: bool) -> ExitCode {
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
