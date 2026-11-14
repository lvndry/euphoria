// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saving the stems of the selected track: the four stems as WAV files, or
//! the acapella or the instrumental as one file, in the track's key or in
//! the one it is shifted to.

use std::path::{Path, PathBuf};

use euphoria_core::stems::{
    MixExport, Mixdown, Stem, StemFiles, StemShift, export_mix, export_stems,
};
use euphoria_core::tags::TagValues;
use gpui::{Context, PathPromptOptions};

use crate::app::EuphoriaApp;
use crate::library::Track;

/// What to save of the stems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StemExportKind {
    /// The four stems, one file each, in a folder.
    Stems,
    /// One file mixed from the stems.
    Mix(Mixdown),
}

/// Where an export writes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StemExportTarget {
    Stems(Vec<(Stem, PathBuf)>),
    Mix(Mixdown, PathBuf),
}

/// An export that is ready to run.
#[derive(Debug, Clone)]
pub(crate) struct StemExportRequest {
    pub(crate) title: String,
    pub(crate) files: StemFiles,
    pub(crate) target: StemExportTarget,
    pub(crate) shift: Option<StemShift>,
    /// Whether files already at the destinations may be replaced: when the
    /// save dialog has asked, or the user has answered the question here.
    pub(crate) replace: bool,
}

impl StemExportRequest {
    /// The destinations that are taken already.
    pub(crate) fn existing(&self) -> Vec<PathBuf> {
        let destinations: Vec<&Path> = match &self.target {
            StemExportTarget::Stems(destinations) => destinations
                .iter()
                .map(|(_, path)| path.as_path())
                .collect(),
            StemExportTarget::Mix(_, path) => vec![path.as_path()],
        };
        destinations
            .into_iter()
            .filter(|path| std::fs::symlink_metadata(path).is_ok())
            .map(Path::to_path_buf)
            .collect()
    }

    fn description(&self) -> String {
        let key = self
            .shift
            .map(|shift| format!(" shifted by {:+} semitones", shift.semitones))
            .unwrap_or_default();
        match &self.target {
            StemExportTarget::Stems(_) => format!("the stems of {}{key}", self.title),
            StemExportTarget::Mix(mixdown, _) => {
                format!(
                    "the {} of {}{key}",
                    mixdown.label().to_lowercase(),
                    self.title
                )
            }
        }
    }

    /// Writes the files. Runs for as long as shifting and writing the stems
    /// takes, so not on the main thread.
    pub(crate) fn run(&self) -> Result<StemExportDone, euphoria_core::EuphoriaError> {
        match &self.target {
            StemExportTarget::Stems(destinations) => {
                let done =
                    export_stems(&self.files, destinations, self.replace, self.shift.as_ref())?;
                Ok(StemExportDone::Stems {
                    folder: destinations
                        .first()
                        .and_then(|(_, path)| path.parent())
                        .map(Path::to_path_buf)
                        .unwrap_or_default(),
                    key: done.key,
                })
            }
            StemExportTarget::Mix(mixdown, destination) => {
                let export = export_mix(
                    &self.files,
                    *mixdown,
                    destination,
                    self.replace,
                    self.shift.as_ref(),
                )?;
                Ok(StemExportDone::Mix(export))
            }
        }
    }
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StemExportDone {
    Stems {
        folder: PathBuf,
        key: Option<euphoria_core::key::Key>,
    },
    Mix(MixExport),
}

impl StemExportDone {
    /// What the notice strip says once the export is saved.
    pub(crate) fn message(&self, request: &StemExportRequest) -> String {
        let key = |key: Option<euphoria_core::key::Key>| {
            key.map(|key| format!(", tagged {} ({})", key.camelot(), key))
                .unwrap_or_default()
        };
        match self {
            StemExportDone::Stems {
                folder,
                key: shifted,
            } => format!(
                "Saved {} to {}{}",
                request.description(),
                folder.display(),
                key(*shifted)
            ),
            StemExportDone::Mix(export) => {
                let scaled = export
                    .gain
                    .map(|gain| {
                        format!(
                            ". The sum would have clipped, so it was scaled down by {:.1} dB",
                            -20.0 * gain.log10()
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "Saved {} to {}{}{}",
                    request.description(),
                    export.path.display(),
                    key(export.key),
                    scaled
                )
            }
        }
    }
}

/// `title` as the start of a file name: without the characters file systems
/// refuse, and not hidden.
pub(crate) fn file_name_part(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|character| {
            if character.is_control() || "/\\:*?\"<>|".contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').trim();
    if trimmed.is_empty() {
        "track".to_string()
    } else {
        trimmed.to_string()
    }
}

fn shift_suffix(shift: Option<&StemShift>) -> String {
    shift
        .map(|shift| format!("{:+}", shift.semitones))
        .unwrap_or_default()
}

/// The file of `stem` in a folder of stems: `Title - vocals.wav`, or
/// `Title - vocals (+2).wav` when shifted by two semitones.
pub(crate) fn stem_file_name(title: &str, stem: Stem, shift: Option<&StemShift>) -> String {
    let title = file_name_part(title);
    match shift {
        Some(_) => format!("{title} - {} ({}).wav", stem.name(), shift_suffix(shift)),
        None => format!("{title} - {}.wav", stem.name()),
    }
}

/// The file of a mix: `Title (Acapella).wav`, or `Title (Acapella, +2).wav`
/// when shifted by two semitones.
pub(crate) fn mix_file_name(title: &str, mixdown: Mixdown, shift: Option<&StemShift>) -> String {
    let title = file_name_part(title);
    match shift {
        Some(_) => format!("{title} ({}, {}).wav", mixdown.label(), shift_suffix(shift)),
        None => format!("{title} ({}).wav", mixdown.label()),
    }
}

/// The name of the track to put in file names: its title in the tags, or
/// its file name without the extension.
fn track_title(track: &Track) -> String {
    let title = track.title();
    if title == track.file_name {
        Path::new(title)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| title.to_string())
    } else {
        title.to_string()
    }
}

/// What an export of the selected track starts from.
pub(crate) struct StemExportSource {
    pub(crate) title: String,
    pub(crate) files: StemFiles,
    pub(crate) shift: Option<StemShift>,
    /// The folder of the track, where the save dialog opens.
    pub(crate) directory: PathBuf,
}

impl EuphoriaApp {
    /// The stems of the selected track with the title for file names and
    /// the key change to make, when it is separated.
    pub(crate) fn stem_export_source(&self, shifted: bool) -> Option<StemExportSource> {
        let track = self.selected_track()?;
        let files = self.stems.ready_files(&track.path)?;
        let shift = (shifted && self.key_shift != 0).then(|| StemShift {
            semitones: self.key_shift,
            values: TagValues {
                key: track.key(),
                bpm: track.bpm(),
                energy: track.energy(),
            },
        });
        Some(StemExportSource {
            title: track_title(track),
            files,
            shift,
            directory: track
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
        })
    }

    /// Asks where to save an export of the stems of the selected track,
    /// shifted to the key the track is changed to when `shifted`.
    pub(crate) fn choose_stem_export(
        &mut self,
        kind: StemExportKind,
        shifted: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(StemExportSource {
            title,
            files,
            shift,
            directory,
        }) = self.stem_export_source(shifted)
        else {
            return;
        };
        match kind {
            StemExportKind::Stems => {
                let chosen = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Export here".into()),
                });
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(Some(paths))) = chosen.await else {
                        return;
                    };
                    let Some(folder) = paths.into_iter().next() else {
                        return;
                    };
                    this.update(cx, |this, cx| {
                        let request = stems_request(title, files, &folder, shift, false);
                        this.begin_stem_export(request, cx);
                    })
                    .ok();
                })
                .detach();
            }
            StemExportKind::Mix(mixdown) => {
                let suggested = mix_file_name(&title, mixdown, shift.as_ref());
                let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
                cx.spawn(async move |this, cx| {
                    let destination = match chosen.await {
                        Ok(Ok(Some(path))) => path,
                        Ok(Ok(None)) | Err(_) => return,
                        Ok(Err(error)) => {
                            this.update(cx, |this, cx| {
                                this.notice =
                                    Some(format!("Could not open the save dialog: {error:#}"));
                                cx.notify();
                            })
                            .ok();
                            return;
                        }
                    };
                    this.update(cx, |this, cx| {
                        let request = StemExportRequest {
                            title,
                            files,
                            target: StemExportTarget::Mix(mixdown, destination),
                            shift,
                            replace: true,
                        };
                        this.begin_stem_export(request, cx);
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Runs the export, unless it would replace files and the user has not
    /// said that it may: then it waits for the answer.
    pub(crate) fn begin_stem_export(
        &mut self,
        request: StemExportRequest,
        cx: &mut Context<'_, Self>,
    ) {
        if !request.replace && !request.existing().is_empty() {
            self.pending_stem_export = Some(request);
            cx.notify();
            return;
        }
        self.run_stem_export(request, cx);
    }

    /// Replaces the files that were in the way of the waiting export.
    pub(crate) fn confirm_pending_stem_export(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(mut request) = self.pending_stem_export.take() {
            request.replace = true;
            self.run_stem_export(request, cx);
        }
    }

    /// Drops the waiting export; nothing is written.
    pub(crate) fn cancel_pending_stem_export(&mut self, cx: &mut Context<'_, Self>) {
        self.pending_stem_export = None;
        cx.notify();
    }

    fn run_stem_export(&mut self, request: StemExportRequest, cx: &mut Context<'_, Self>) {
        self.notice = Some(format!("Saving {}…", request.description()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn({
                    let request = request.clone();
                    async move { request.run() }
                })
                .await;
            this.update(cx, |this, cx| {
                this.notice = Some(match outcome {
                    Ok(done) => done.message(&request),
                    Err(error) => format!(
                        "Could not save {}: {}",
                        request.description(),
                        error.chain()
                    ),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// An export of the four stems into `folder`.
pub(crate) fn stems_request(
    title: String,
    files: StemFiles,
    folder: &Path,
    shift: Option<StemShift>,
    replace: bool,
) -> StemExportRequest {
    let destinations = Stem::ALL
        .iter()
        .map(|stem| {
            (
                *stem,
                folder.join(stem_file_name(&title, *stem, shift.as_ref())),
            )
        })
        .collect();
    StemExportRequest {
        title,
        files,
        target: StemExportTarget::Stems(destinations),
        shift,
        replace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shifted(semitones: i8) -> StemShift {
        StemShift {
            semitones,
            values: TagValues::default(),
        }
    }

    #[test]
    fn file_names_carry_the_title_the_stem_and_the_shift() {
        assert_eq!(
            stem_file_name("Low Tide", Stem::Vocals, None),
            "Low Tide - vocals.wav"
        );
        assert_eq!(
            stem_file_name("Low Tide", Stem::Drums, Some(&shifted(-2))),
            "Low Tide - drums (-2).wav"
        );
        assert_eq!(
            mix_file_name("Low Tide", Mixdown::Acapella, None),
            "Low Tide (Acapella).wav"
        );
        assert_eq!(
            mix_file_name("Low Tide", Mixdown::Instrumental, Some(&shifted(3))),
            "Low Tide (Instrumental, +3).wav"
        );
    }

    #[test]
    fn a_title_never_makes_a_path_or_a_hidden_file() {
        assert_eq!(file_name_part("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(file_name_part("..secret"), "secret");
        assert_eq!(file_name_part("  "), "track");
    }
}
