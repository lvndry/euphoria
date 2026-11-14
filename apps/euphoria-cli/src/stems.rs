// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria stems`: separating a track into vocals, drums, bass and the
//! rest, written as one WAV file per stem.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use euphoria_core::stems::{
    CachedSeparator, MixExport, Mixdown, SeparationObserver, SeparationStage, Stem, export_mix,
    publish_stems,
};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::batch::write_time_left;
use crate::output;
use crate::style::Theme;
use crate::terminal::Printable;
use crate::with_causes;

const SPINNER_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Debug, Args)]
pub struct StemsArguments {
    /// The audio file to separate.
    file: PathBuf,
    /// The folder to write the stems to, created when missing.
    #[arg(short, long, value_name = "DIR")]
    output: PathBuf,
    /// Replace stem files already in the folder.
    #[arg(long)]
    force: bool,
    /// Write one file, the acapella or the instrumental, named after the
    /// track, instead of the four stems.
    #[arg(long, value_enum, value_name = "MIX")]
    mix: Option<MixArgument>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MixArgument {
    /// The vocals alone.
    Acapella,
    /// Drums, bass and the rest, without the vocals.
    Instrumental,
}

impl From<MixArgument> for Mixdown {
    fn from(argument: MixArgument) -> Self {
        match argument {
            MixArgument::Acapella => Mixdown::Acapella,
            MixArgument::Instrumental => Mixdown::Instrumental,
        }
    }
}

fn mix_file_name(file: &Path, mixdown: Mixdown) -> String {
    let name = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "track".to_string());
    format!("{name} ({}).wav", mixdown.label())
}

pub fn run(arguments: &StemsArguments) -> Result<ExitCode> {
    let mixdown = arguments.mix.map(Mixdown::from);
    let destinations: Vec<(Stem, PathBuf)> = match mixdown {
        Some(_) => Vec::new(),
        None => Stem::ALL
            .iter()
            .map(|stem| (*stem, arguments.output.join(format!("{}.wav", stem.name()))))
            .collect(),
    };
    let mix_destination = mixdown.map(|mixdown| {
        arguments
            .output
            .join(mix_file_name(&arguments.file, mixdown))
    });
    if !arguments.force
        && let Some((_, existing)) = destinations.iter().find(|(_, path)| path.exists())
    {
        bail!(
            "{} already exists: add --force to replace the stems in {}",
            Printable(existing.display()),
            Printable(arguments.output.display())
        );
    }
    if !arguments.file.is_file() {
        bail!("{} is not a file", Printable(arguments.file.display()));
    }

    let input = std::fs::canonicalize(&arguments.file)
        .with_context(|| format!("cannot use {}", Printable(arguments.file.display())))?;
    for destination in destinations
        .iter()
        .map(|(_, path)| path)
        .chain(mix_destination.as_ref())
    {
        if std::fs::canonicalize(destination).ok().as_ref() == Some(&input) {
            bail!(
                "{} is the original input, which cannot be replaced by a stem",
                Printable(destination.display())
            );
        }
    }

    let mut separator = CachedSeparator::open_default().map_err(with_causes)?;
    let progress = SeparationProgress::new(&arguments.file);
    let started = Instant::now();
    let separated = separator.stems(&arguments.file, &progress);
    let was_cached = !progress.separated_any();
    progress.finish();
    let files = separated.map_err(with_causes)?;

    std::fs::create_dir_all(&arguments.output)
        .with_context(|| format!("cannot create {}", Printable(arguments.output.display())))?;
    if let (Some(mixdown), Some(destination)) = (mixdown, &mix_destination) {
        let mix =
            export_mix(&files, mixdown, destination, arguments.force, None).map_err(with_causes)?;
        write_mix_summary(mixdown, &mix, was_cached, started.elapsed())?;
        return Ok(ExitCode::SUCCESS);
    }
    publish_stems(&files, &destinations, arguments.force).map_err(with_causes)?;
    write_summary(&destinations, was_cached, started.elapsed())?;
    Ok(ExitCode::SUCCESS)
}

fn write_mix_summary(
    mixdown: Mixdown,
    mix: &MixExport,
    was_cached: bool,
    elapsed: Duration,
) -> Result<()> {
    let standard_output_theme = Theme::stdout();
    let mut standard_output = std::io::stdout().lock();
    writeln!(
        standard_output,
        "{}  {}",
        standard_output_theme.bold(format!("{:<12}", mixdown.label())),
        Printable(mix.path.display())
    )?;
    standard_output.flush()?;
    let theme = Theme::stderr();
    let mut standard_error = std::io::stderr().lock();
    if let Some(gain) = mix.gain {
        output::write_note(
            &mut standard_error,
            &theme,
            format_args!(
                "the sum would have clipped, so it was scaled down by {:.1} dB.",
                -20.0 * gain.log10()
            ),
        )?;
    }
    if was_cached {
        output::write_note(
            &mut standard_error,
            &theme,
            "these stems were separated before and came from the cache.",
        )?;
    } else {
        output::write_note(
            &mut standard_error,
            &theme,
            format_args!("separated in {}.", format_elapsed(elapsed)),
        )?;
    }
    Ok(())
}

fn write_summary(
    destinations: &[(Stem, PathBuf)],
    was_cached: bool,
    elapsed: Duration,
) -> Result<()> {
    let standard_output_theme = Theme::stdout();
    let mut standard_output = std::io::stdout().lock();
    for (stem, path) in destinations {
        writeln!(
            standard_output,
            "{}  {}",
            standard_output_theme.bold(format!("{:<6}", stem.label())),
            Printable(path.display())
        )?;
    }
    standard_output.flush()?;
    let theme = Theme::stderr();
    let mut standard_error = std::io::stderr().lock();
    if was_cached {
        output::write_note(
            &mut standard_error,
            &theme,
            "these stems were separated before and came from the cache.",
        )?;
    } else {
        output::write_note(
            &mut standard_error,
            &theme,
            format_args!(
                "separated in {}. The stems are 16 bit WAV files at 44.1 kHz.",
                format_elapsed(elapsed)
            ),
        )?;
    }
    Ok(())
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// The progress of one separation on standard error: a spinner while the
/// model loads and the file decodes, a bar while the model downloads and
/// while the segments are separated. Nothing is drawn off a terminal.
struct SeparationProgress {
    bar: ProgressBar,
    track_name: String,
    separated_any: std::sync::atomic::AtomicBool,
}

impl SeparationProgress {
    fn new(file: &Path) -> Self {
        let target = if std::io::stderr().is_terminal() {
            ProgressDrawTarget::stderr()
        } else {
            ProgressDrawTarget::hidden()
        };
        let bar = ProgressBar::with_draw_target(None, target);
        let track_name = file
            .file_name()
            .map(|name| Printable(name.to_string_lossy().into_owned()).to_string())
            .unwrap_or_default();
        let progress = Self {
            bar,
            track_name,
            separated_any: std::sync::atomic::AtomicBool::new(false),
        };
        progress.spin("hashing");
        progress
    }

    fn separated_any(&self) -> bool {
        self.separated_any
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn spin(&self, stage: &str) {
        if let Ok(style) = ProgressStyle::with_template("{spinner:.cyan} {msg}") {
            self.bar.set_style(style.tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "));
        }
        self.bar.enable_steady_tick(SPINNER_INTERVAL);
        self.bar.set_message(format!(
            "{}  {}",
            self.track_name,
            Theme::stderr().dim(stage)
        ));
    }

    fn bar_with(&self, template: &str, prefix: &'static str, length: u64) {
        if let Ok(style) = ProgressStyle::with_template(template) {
            self.bar.set_style(
                style
                    .progress_chars("━━─")
                    .with_key("left", write_time_left),
            );
        }
        self.bar.set_prefix(prefix);
        self.bar.set_length(length);
    }

    fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

impl SeparationObserver for SeparationProgress {
    fn stage(&self, stage: SeparationStage) {
        match stage {
            SeparationStage::Separating => {}
            other => self.spin(other.label()),
        }
    }

    fn downloading(&self, received: u64, total: u64) {
        if self.bar.length() != Some(total) {
            self.bar_with(
                "{prefix:.bold} {bytes}/{total_bytes}  {bar:32.cyan/dim}  {binary_bytes_per_sec}",
                "Downloading the model",
                total,
            );
        }
        self.bar.set_position(received);
    }

    fn separated(&self, done: usize, total: usize) {
        if done == 0 {
            self.separated_any
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self.bar.reset();
            self.bar_with(
                "{prefix:.bold} {pos}/{len} segments  {bar:32.cyan/dim}  {elapsed} so far{left}",
                "Separating",
                total as u64,
            );
        }
        self.bar.set_position(done as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_is_shown_in_seconds_then_minutes() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "42s");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2:05");
    }

    #[test]
    fn a_mix_is_named_after_the_track_and_the_mix() {
        assert_eq!(
            mix_file_name(Path::new("/music/Low Tide.mp3"), Mixdown::Acapella),
            "Low Tide (Acapella).wav"
        );
        assert_eq!(
            mix_file_name(Path::new("/music/Low Tide.mp3"), Mixdown::Instrumental),
            "Low Tide (Instrumental).wav"
        );
    }
}
