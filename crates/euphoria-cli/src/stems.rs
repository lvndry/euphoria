// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria stems`: separating a track into vocals, drums, bass and the
//! rest, written as one WAV file per stem.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Args;
use euphoria_core::stems::{CachedSeparator, SeparationObserver, SeparationStage, Stem, StemFiles};
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
}

pub fn run(arguments: &StemsArguments) -> Result<ExitCode> {
    let destinations: Vec<(Stem, PathBuf)> = Stem::ALL
        .iter()
        .map(|stem| (*stem, arguments.output.join(format!("{}.wav", stem.name()))))
        .collect();
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
    for (_, destination) in &destinations {
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
    publish_stems(&files, &destinations, arguments.force)?;
    write_summary(&destinations, was_cached, started.elapsed())?;
    Ok(ExitCode::SUCCESS)
}

/// Stages every stem before replacing any output. A failed publication
/// removes the new files and restores the previous directory entries.
fn publish_stems(files: &StemFiles, destinations: &[(Stem, PathBuf)], force: bool) -> Result<()> {
    publish_stems_using(files, destinations, force, install_stem)
}

fn install_stem(
    temporary: tempfile::NamedTempFile,
    destination: &Path,
    force: bool,
) -> std::io::Result<()> {
    if force {
        temporary.persist(destination)
    } else {
        temporary.persist_noclobber(destination)
    }
    .map(|_| ())
    .map_err(|error| error.error)
}

fn publish_stems_using(
    files: &StemFiles,
    destinations: &[(Stem, PathBuf)],
    force: bool,
    mut install: impl FnMut(tempfile::NamedTempFile, &Path, bool) -> std::io::Result<()>,
) -> Result<()> {
    let mut staged = Vec::new();
    for (stem, destination) in destinations {
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        let mut source = std::fs::File::open(files.path(*stem))
            .with_context(|| format!("cannot read the cached {} stem", stem.name()))?;
        std::io::copy(&mut source, temporary.as_file_mut())?;
        temporary.as_file().sync_all()?;
        match std::fs::symlink_metadata(destination) {
            Ok(metadata) if !force || metadata.is_dir() => {
                bail!("cannot replace {}", Printable(destination.display()))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("cannot check {}", Printable(destination.display())));
            }
        }
        staged.push(temporary);
    }
    let parent = destinations
        .first()
        .and_then(|(_, path)| path.parent())
        .unwrap_or_else(|| Path::new("."));
    let backups = tempfile::Builder::new()
        .prefix(".euphoria-stems-")
        .tempdir_in(parent)?;
    let mut saved = Vec::new();
    let mut published = Vec::new();
    let result: Result<()> = (|| {
        if force {
            for (index, (_, destination)) in destinations.iter().enumerate() {
                match std::fs::symlink_metadata(destination) {
                    Ok(metadata) if metadata.is_dir() => {
                        bail!("cannot replace {}", Printable(destination.display()))
                    }
                    Ok(_) => {
                        let backup = backups.path().join(index.to_string());
                        std::fs::rename(destination, &backup)?;
                        saved.push((destination, backup));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        for ((_, destination), temporary) in destinations.iter().zip(staged) {
            install(temporary, destination, force)
                .with_context(|| format!("cannot write {}", Printable(destination.display())))?;
            published.push(destination);
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for destination in published.into_iter().rev() {
            if let Err(error) = std::fs::remove_file(destination) {
                rollback_errors.push(error);
            }
        }
        for (destination, backup) in saved.into_iter().rev() {
            if let Err(error) = std::fs::rename(backup, destination) {
                rollback_errors.push(error);
            }
        }
        if !rollback_errors.is_empty() {
            let retained = backups.keep();
            bail!(
                "{error:#}; could not completely restore the previous stems: {rollback_errors:?}; backups kept at {}",
                Printable(retained.display())
            );
        }
        return Err(error);
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
    fn an_output_created_after_the_preflight_check_is_not_replaced() {
        use euphoria_core::{
            ContentHash,
            stems::{STEMS_MODEL, StemCache},
        };
        let folder =
            std::env::temp_dir().join(format!("euphoria-stems-publish-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let cache = StemCache::new(folder.join("cache"), u64::MAX);
        let hash = ContentHash::of_file(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../euphoria-core/tests/fixtures/sine-440hz-stereo.wav"),
        )
        .unwrap();
        let entry = cache.directory().join(format!(
            "{STEMS_MODEL}-{}",
            hash.as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        std::fs::create_dir_all(&entry).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../euphoria-core/tests/fixtures/sine-440hz-stereo.wav");
        for stem in Stem::ALL {
            std::fs::copy(&fixture, entry.join(format!("{}.wav", stem.name()))).unwrap();
        }
        let files = cache.lookup(&hash).unwrap();
        let destination = folder.join("vocals.wav");
        assert!(!destination.exists());
        std::fs::write(&destination, b"created by another writer").unwrap();
        let result = publish_stems(&files, &[(Stem::Vocals, destination.clone())], false);
        let bytes = std::fs::read(&destination).unwrap();
        std::fs::remove_dir_all(folder).unwrap();
        assert!(
            result.is_err(),
            "a late output must not be silently overwritten"
        );
        assert_eq!(bytes, b"created by another writer");
    }

    #[test]
    fn a_late_publication_error_rolls_back_new_and_replaced_outputs() {
        use euphoria_core::{
            ContentHash,
            stems::{STEMS_MODEL, StemCache},
        };
        let folder = tempfile::tempdir().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../euphoria-core/tests/fixtures/sine-440hz-stereo.wav");
        let hash = ContentHash::of_file(&fixture).unwrap();
        let cache = StemCache::new(folder.path().join("cache"), u64::MAX);
        let hex: String = hash
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let entry = cache.directory().join(format!("{STEMS_MODEL}-{hex}"));
        std::fs::create_dir_all(&entry).unwrap();
        for stem in Stem::ALL {
            std::fs::copy(&fixture, entry.join(format!("{}.wav", stem.name()))).unwrap();
        }
        let files = cache.lookup(&hash).unwrap();
        for force in [false, true] {
            let output = folder.path().join(format!("output-{force}"));
            std::fs::create_dir(&output).unwrap();
            let destinations: Vec<_> = Stem::ALL
                .iter()
                .map(|stem| (*stem, output.join(format!("{}.wav", stem.name()))))
                .collect();
            if force {
                for (_, destination) in &destinations {
                    std::fs::write(destination, b"previous stem").unwrap();
                }
            }
            let mut installed = 0;
            let result = publish_stems_using(
                &files,
                &destinations,
                force,
                |temporary, destination, force| {
                    installed += 1;
                    if installed == 3 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "simulated publication failure",
                        ));
                    }
                    install_stem(temporary, destination, force)
                },
            );
            assert!(result.is_err());
            assert_eq!(installed, 3, "two outputs were installed before failure");
            for (_, destination) in destinations {
                if force {
                    assert_eq!(std::fs::read(destination).unwrap(), b"previous stem");
                } else {
                    assert!(!destination.exists());
                }
            }
            assert_eq!(
                std::fs::read_dir(output).unwrap().count(),
                if force { 4 } else { 0 }
            );
        }
    }

    #[test]
    fn elapsed_time_is_shown_in_seconds_then_minutes() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "42s");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2:05");
    }
}
