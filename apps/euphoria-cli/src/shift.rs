// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `shift` command: writes a copy of a track in another key.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Args;
use euphoria_core::shift::{ExistingOutput, ShiftedFormat, write_shifted_copy};
use euphoria_core::tags::{KeyNotation, TagValues, TagWriteOptions};
use euphoria_core::{Key, is_supported_audio_file};

use crate::batch::{self, AnalysisOptions};
use crate::style::Theme;
use crate::terminal::Printable;
use crate::view::KeyNotationChoice;
use crate::with_causes;

#[derive(Debug, Args)]
pub struct ShiftArguments {
    /// The track to shift. It is only read, never changed.
    file: PathBuf,
    /// How many semitones to move the key, from -6 to 6: 2 moves A minor to
    /// B minor, -3 moves it to F# minor.
    #[arg(
        long,
        short = 's',
        allow_negative_numbers = true,
        value_parser = clap::value_parser!(i8).range(-6..=6),
        value_name = "N"
    )]
    semitones: i8,
    /// The file to write, ending in .wav or .flac.
    #[arg(long, short = 'o', value_name = "FILE")]
    output: PathBuf,
    /// Replace the output file if it exists.
    #[arg(long)]
    force: bool,
    /// How the shifted key is written into the copy's tags.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

pub fn run(arguments: &ShiftArguments) -> Result<ExitCode> {
    if arguments.semitones == 0 {
        bail!("a shift of 0 semitones leaves the track as it is, so nothing was written");
    }
    let input = std::path::absolute(&arguments.file)
        .with_context(|| format!("cannot use {}", arguments.file.display()))?;
    if !input.is_file() {
        bail!("{} is not a file", input.display());
    }
    if !is_supported_audio_file(&input) {
        bail!("{} is not an audio file Euphoria reads", input.display());
    }
    let output = std::path::absolute(&arguments.output)
        .with_context(|| format!("cannot use {} as the output", arguments.output.display()))?;
    if ShiftedFormat::of_path(&output).is_none() {
        bail!(
            "{} must end in .wav or .flac, the formats a shifted copy is written in",
            output.display()
        );
    }
    if output.exists() && !arguments.force {
        bail!("{} exists; pass --force to replace it", output.display());
    }

    // The key of the original, from the cache when it was analyzed before,
    // with the user's correction when there is one.
    let files = vec![input.clone()];
    let batch = batch::analyze(&files, &arguments.analysis, batch::Display::Stages)?;
    let user_edits = batch::user_edits(
        &batch.results,
        &arguments.analysis,
        batch::CorrectionCheck::ReadFile,
    )?;
    let results = batch::corrected(batch.results, &user_edits);
    let values = match results.into_iter().next().map(|result| result.outcome) {
        Some(Ok(analysis)) => TagValues::from(&analysis),
        Some(Err(error)) => return Err(with_causes(error)),
        None => TagValues::default(),
    };

    let tag_options = TagWriteOptions {
        key_notation: match arguments.key_notation {
            KeyNotationChoice::Camelot => KeyNotation::Camelot,
            KeyNotationChoice::Musical => KeyNotation::Musical,
            KeyNotationChoice::OpenKey => KeyNotation::OpenKey,
        },
        ..TagWriteOptions::default()
    };
    let existing = if arguments.force {
        ExistingOutput::Replace
    } else {
        ExistingOutput::Keep
    };
    let copy = write_shifted_copy(
        &input,
        &output,
        arguments.semitones,
        values,
        &tag_options,
        existing,
    )
    .map_err(with_causes)?;

    let theme = Theme::stderr();
    let mut standard_error = std::io::stderr().lock();
    writeln!(
        standard_error,
        "{}",
        summary(
            &theme,
            arguments.semitones,
            values.key,
            copy.key,
            arguments.key_notation,
            &copy.path
        )
    )?;
    Ok(ExitCode::SUCCESS)
}

/// One line saying what was written: how far the track moved, its key
/// before and after, and where the copy is.
fn summary(
    theme: &Theme,
    semitones: i8,
    original: Option<Key>,
    shifted: Option<Key>,
    notation: KeyNotationChoice,
    path: &std::path::Path,
) -> String {
    let direction = if semitones > 0 { "up" } else { "down" };
    let amount = semitones.unsigned_abs();
    let unit = if amount == 1 { "semitone" } else { "semitones" };
    let keys = match (original, shifted) {
        (Some(original), Some(shifted)) => format!(
            ", from {} to {}",
            theme.key(original, notation.short(original)),
            theme.key(shifted, theme.bold(notation.short(shifted)))
        ),
        _ => ", with no key to tag: none was found in the original".to_string(),
    };
    format!(
        "Shifted {direction} {amount} {unit}{keys}, to {}",
        theme.bold(Printable(path.display()))
    )
}
