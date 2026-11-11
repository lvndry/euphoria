// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `corrections` command: the keys and tempos you set by hand, read
//! from the cache and written as JSON Lines for training a key model.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use euphoria_core::export::rekordbox::ExistingFile;
use euphoria_core::{
    Cache, PathInclusion, StoredCorrection, default_cache_path, render_corrections_jsonl,
    write_corrections_file,
};

use crate::export::check_output_file;
use crate::output::{self, counted};
use crate::style::Theme;
use crate::terminal::Printable;
use crate::with_causes;

#[derive(Debug, Subcommand)]
pub enum CorrectionsCommand {
    /// Write every key and tempo you corrected as JSON Lines, one object
    /// per track, named by the hash of its content.
    #[command(after_help = crate::help::corrections_examples())]
    Export(ExportArguments),
}

#[derive(Debug, Args)]
pub struct ExportArguments {
    /// The file to write. Without it, the lines go to standard output.
    #[arg(long, short = 'o', value_name = "FILE")]
    output: Option<PathBuf>,
    /// Replace the output file if it exists.
    #[arg(long, requires = "output")]
    force: bool,
    /// Add the path each file was last seen at. Leave it off for an export
    /// that says nothing about where your music is.
    #[arg(long)]
    include_paths: bool,
}

pub fn run(command: CorrectionsCommand) -> Result<ExitCode> {
    match command {
        CorrectionsCommand::Export(arguments) => export(&arguments),
    }
}

fn export(arguments: &ExportArguments) -> Result<ExitCode> {
    let paths = if arguments.include_paths {
        PathInclusion::Include
    } else {
        PathInclusion::Omit
    };
    let existing = if arguments.force {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    };
    let output_file = arguments
        .output
        .as_ref()
        .map(|output| {
            std::path::absolute(output)
                .with_context(|| format!("cannot use {} as the output", output.display()))
        })
        .transpose()?;
    if let Some(output_file) = &output_file {
        check_output_file(output_file, existing)?;
    }

    let corrections = stored_corrections()?;
    match &output_file {
        Some(output_file) => {
            write_corrections_file(output_file, &corrections, paths, existing)
                .map_err(with_causes)?;
        }
        None => {
            let document = render_corrections_jsonl(&corrections, paths)?;
            let mut standard_output = std::io::stdout().lock();
            standard_output.write_all(document.as_bytes())?;
            standard_output.flush()?;
        }
    }

    let theme = Theme::stderr();
    let mut standard_error = std::io::stderr().lock();
    if corrections.is_empty() {
        output::write_note(
            &mut standard_error,
            &theme,
            "no corrections yet. Correct a key or a tempo in the desktop app, then export again.",
        )?;
    }
    write!(
        standard_error,
        "Exported {}",
        theme.bold(counted(corrections.len(), "correction", "corrections"))
    )?;
    if let Some(output_file) = &output_file {
        write!(
            standard_error,
            " to {}",
            theme.bold(Printable(output_file.display()))
        )?;
    }
    writeln!(standard_error)?;
    Ok(ExitCode::SUCCESS)
}

/// Every correction in the cache, or none when there is no cache yet. A
/// missing cache is not created.
fn stored_corrections() -> Result<Vec<StoredCorrection>> {
    let cache_path = default_cache_path().map_err(with_causes)?;
    let cache_exists = cache_path
        .try_exists()
        .with_context(|| format!("cannot look for the cache at {}", cache_path.display()))?;
    if !cache_exists {
        return Ok(Vec::new());
    }
    Cache::open(&cache_path)
        .and_then(|cache| cache.stored_corrections())
        .map_err(with_causes)
}
