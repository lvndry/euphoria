// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hearing the selected track in another key, seeing what it would mix with
//! there, and exporting a copy shifted to that key.

use std::path::{Path, PathBuf};

use euphoria_core::shift::{ExistingOutput, MAXIMUM_SHIFT_SEMITONES, write_shifted_copy};
use euphoria_core::tags::{TagValues, TagWriteOptions};
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::library::Track;

/// The name suggested for a copy of `track` shifted by `semitones`: its file
/// name with the shift, as FLAC, which keeps every bit of the shifted sound.
pub(crate) fn suggested_name(track: &Track, semitones: i8) -> String {
    let stem = Path::new(&track.file_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| track.title().to_string());
    format!("{stem} ({semitones:+}).flac")
}

impl EuphoriaApp {
    /// Moves the key of the selected track by `semitones` from its own key,
    /// within the shifts allowed: what plays changes key at once, and the
    /// tracks it mixes with are found for the new key.
    pub(crate) fn set_key_shift(&mut self, semitones: i8, cx: &mut Context<'_, Self>) {
        let semitones = semitones.clamp(-MAXIMUM_SHIFT_SEMITONES, MAXIMUM_SHIFT_SEMITONES);
        if semitones == self.key_shift {
            return;
        }
        self.key_shift = semitones;
        self.apply_key_shift_to_playback();
        self.refresh_mixes();
        cx.notify();
    }

    /// Asks where to save a copy of the selected track in the shifted key,
    /// then writes it there off the main thread.
    pub(crate) fn export_shifted_copy(&mut self, cx: &mut Context<'_, Self>) {
        let semitones = self.key_shift;
        let Some(track) = self.selected_track() else {
            return;
        };
        if semitones == 0 {
            return;
        }
        let source = track.path.clone();
        let values = TagValues {
            key: track.key(),
            bpm: track.bpm(),
            energy: track.energy(),
        };
        let directory = source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested_name(track, semitones)));
        cx.spawn(async move |this, cx| {
            let destination = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.notice = Some(format!("Could not open the save dialog: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    // The save dialog has already asked whether to replace a
                    // file of that name.
                    write_shifted_copy(
                        &source,
                        &destination,
                        semitones,
                        values,
                        &TagWriteOptions::default(),
                        ExistingOutput::Replace,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.notice = Some(match outcome {
                    Ok(copy) => format!(
                        "Saved a copy {} {} {} to {}{}",
                        if semitones > 0 { "up" } else { "down" },
                        semitones.unsigned_abs(),
                        if semitones.unsigned_abs() == 1 {
                            "semitone"
                        } else {
                            "semitones"
                        },
                        copy.path.display(),
                        copy.key
                            .map(|key| format!(", in {} ({})", key.camelot(), key))
                            .unwrap_or_default()
                    ),
                    Err(error) => format!("The shifted copy could not be saved: {}", error.chain()),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_suggested_name_carries_the_shift_and_ends_in_flac() {
        let mut library = crate::library::Library::default();
        library.add_files(vec![PathBuf::from("/music/Low Tide.mp3")]);
        let track = library.track(0).unwrap();
        assert_eq!(suggested_name(track, 2), "Low Tide (+2).flac");
        assert_eq!(suggested_name(track, -3), "Low Tide (-3).flac");
    }
}
