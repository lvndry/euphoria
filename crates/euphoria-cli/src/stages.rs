// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The progress shown on a terminal while files are analyzed: one line per
//! file in progress with its current stage, under the bar that counts files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use euphoria_core::{AnalysisObserver, AnalysisStage};
use indicatif::{MultiProgress, ProgressBar, ProgressState, ProgressStyle};

use crate::style::Theme;
use crate::terminal::Printable;
use crate::view::shorten_middle;

/// How often the spinner of a file moves, so a long stage never looks
/// frozen.
const SPINNER_INTERVAL: Duration = Duration::from_millis(120);
/// How long a file is analyzed before its time is shown.
const SHOW_TIME_AFTER: Duration = Duration::from_secs(5);
/// Longer file names are shortened in the middle.
const LONGEST_NAME: usize = 48;

/// One line per file being analyzed, shown in `progress`.
pub struct StageLines {
    progress: MultiProgress,
    style: ProgressStyle,
    theme: Theme,
    lines: Mutex<HashMap<PathBuf, ProgressBar>>,
}

impl StageLines {
    /// Lines are indented when they sit under the bar that counts files.
    pub fn new(progress: MultiProgress, indented: bool) -> Self {
        let template = if indented {
            "  {spinner:.cyan} {msg}{file_time:.dim}"
        } else {
            "{spinner:.cyan} {msg}{file_time:.dim}"
        };
        let style = ProgressStyle::with_template(template)
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ")
            .with_key("file_time", write_file_time);
        Self {
            progress,
            style,
            theme: Theme::stderr(),
            lines: Mutex::new(HashMap::new()),
        }
    }

    fn message(&self, path: &Path, stage: &str) -> String {
        format!("{}  {}", short_name(path), self.theme.dim(stage))
    }

    /// Shows `message` on the line of `path`, adding the line when the file
    /// has none yet. A file served from the cache never reports a stage, so
    /// it never gets a line.
    fn show(&self, path: &Path, message: String) {
        let Ok(mut lines) = self.lines.lock() else {
            return;
        };
        let line = lines.entry(path.to_path_buf()).or_insert_with(|| {
            let line = self.progress.add(ProgressBar::new_spinner());
            line.set_style(self.style.clone());
            line.enable_steady_tick(SPINNER_INTERVAL);
            line
        });
        line.set_message(message);
    }

    /// Takes away the line of `path`, which has finished.
    pub fn finish(&self, path: &Path) {
        let line = self
            .lines
            .lock()
            .ok()
            .and_then(|mut lines| lines.remove(path));
        if let Some(line) = line {
            line.finish_and_clear();
            self.progress.remove(&line);
        }
    }
}

impl AnalysisObserver for StageLines {
    fn stage(&self, path: &Path, stage: AnalysisStage) {
        self.show(path, self.message(path, stage.label()));
    }

    fn decoded(&self, path: &Path, share: f32) {
        let percent = (share * 100.0).round() as u32;
        let stage = format!("{} {percent}%", AnalysisStage::Decoding.label());
        self.show(path, self.message(path, &stage));
    }
}

/// Writes how long the file has been analyzed, once that is long enough to
/// be worth saying.
fn write_file_time(state: &ProgressState, writer: &mut dyn std::fmt::Write) {
    if let Some(text) = file_time(state.elapsed()) {
        let _ = writer.write_str(&text);
    }
}

fn file_time(elapsed: Duration) -> Option<String> {
    if elapsed < SHOW_TIME_AFTER {
        return None;
    }
    let seconds = elapsed.as_secs();
    Some(if seconds < 60 {
        format!("  {seconds}s")
    } else {
        format!("  {}:{:02}", seconds / 60, seconds % 60)
    })
}

/// The file name of `path`, shortened in the middle when it is long, with
/// characters that could act on the terminal made visible.
fn short_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    shorten_middle(&Printable(name).to_string(), LONGEST_NAME)
}

/// After a while, says once that a debug build is slow, so that someone
/// running one does not take it for a hang. Not built into release builds.
#[cfg(debug_assertions)]
pub struct DebugBuildHint {
    stop: Option<std::sync::mpsc::Sender<()>>,
    waiter: Option<std::thread::JoinHandle<()>>,
}

#[cfg(debug_assertions)]
impl DebugBuildHint {
    /// How long a run of a debug build goes before the hint is shown.
    const AFTER: Duration = Duration::from_secs(10);

    pub fn start(progress: MultiProgress) -> Self {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let waiter = std::thread::spawn(move || {
            if stopped.recv_timeout(Self::AFTER) == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                let mut note = Vec::new();
                let _ = crate::output::write_note(
                    &mut note,
                    &Theme::stderr(),
                    "this is a debug build of euphoria, which analyzes many times slower \
                     than a release build. Use `cargo run --release` or an installed euphoria.",
                );
                let _ = progress.println(String::from_utf8_lossy(&note).trim_end());
            }
        });
        Self {
            stop: Some(stop),
            waiter: Some(waiter),
        }
    }
}

#[cfg(debug_assertions)]
impl Drop for DebugBuildHint {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(waiter) = self.waiter.take() {
            let _ = waiter.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_are_shortened_in_the_middle() {
        let long = format!("{}.flac", "a".repeat(30) + &"b".repeat(30));
        let shown = short_name(Path::new(&format!("music/{long}")));
        assert_eq!(shown.chars().count(), LONGEST_NAME);
        assert!(
            shown.starts_with("aaaa") && shown.ends_with("bb.flac"),
            "{shown}"
        );
        assert!(shown.contains('…'));
        assert_eq!(short_name(Path::new("music/short.mp3")), "short.mp3");
    }

    #[test]
    fn names_cannot_act_on_the_terminal() {
        let shown = short_name(Path::new("music/evil\r\u{1b}[2K.mp3"));
        assert!(!shown.chars().any(char::is_control), "{shown:?}");
    }

    #[test]
    fn the_time_shows_only_after_a_while() {
        assert_eq!(file_time(Duration::from_secs(0)), None);
        assert_eq!(file_time(Duration::from_millis(4_900)), None);
        assert_eq!(file_time(Duration::from_secs(5)).as_deref(), Some("  5s"));
        assert_eq!(file_time(Duration::from_secs(59)).as_deref(), Some("  59s"));
        assert_eq!(
            file_time(Duration::from_secs(125)).as_deref(),
            Some("  2:05")
        );
    }
}
