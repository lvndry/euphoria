// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering the set of the set builder as one mix file: the same plan and
//! the same render as `euphoria set render`, off the main thread, with a
//! progress bar and Cancel, then a CUE sheet, a tracklist and chapter lines
//! beside the mix.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use euphoria_core::harmony::transition::PlanTrack;
use euphoria_core::set_render::{
    MixFormat, MixInput, RenderOptions, RenderProgress, RenderTrack, chapters, cue_sheet,
    format_mix_time, plan_mix, render_mix, tracklist,
};
use euphoria_core::shift::ExistingOutput;
use euphoria_core::{EuphoriaError, LockedTransition, locked_blend};
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::transition_plan::{PlanSide, plan_side, signals_of};

/// The button that renders the set as one mix.
pub(crate) const RENDER_ACTION: &str = "Render mix\u{2026}";

/// How often the progress bar is redrawn while a mix renders.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// What a render running in the background shares with the window.
#[derive(Default)]
pub(crate) struct RenderShared {
    done_milliseconds: AtomicU64,
    total_milliseconds: AtomicU64,
    cancelled: AtomicBool,
}

impl RenderShared {
    /// How far the render has got, from 0 to 1, or `None` while the tracks
    /// are still being read and planned.
    pub(crate) fn fraction(&self) -> Option<f32> {
        let total = self.total_milliseconds.load(Ordering::Relaxed);
        (total > 0).then(|| {
            (self.done_milliseconds.load(Ordering::Relaxed) as f32 / total as f32).min(1.0)
        })
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

struct SharedProgress(Arc<RenderShared>);

impl RenderProgress for SharedProgress {
    fn rendered(&mut self, done_seconds: f64, total_seconds: f64) {
        self.0
            .total_milliseconds
            .store((total_seconds * 1000.0) as u64, Ordering::Relaxed);
        self.0
            .done_milliseconds
            .store((done_seconds * 1000.0) as u64, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// A render of the set that is running.
pub(crate) struct MixRenderRun {
    pub(crate) shared: Arc<RenderShared>,
    pub(crate) output: PathBuf,
}

/// Everything a render needs, taken from the window before it starts.
struct MixRenderJob {
    title: String,
    sides: Vec<PlanSide>,
    tracks: Vec<RenderTrack>,
    locks: Vec<LockedTransition>,
    output: PathBuf,
    signals_directory: Option<PathBuf>,
}

/// What a finished render wrote.
struct MixRenderDone {
    output: PathBuf,
    length_seconds: f64,
}

/// The files written beside the mix at `output`: its CUE sheet, tracklist
/// and chapters.
pub(crate) fn side_files(output: &Path) -> [PathBuf; 3] {
    let stem = output
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let beside = |suffix: &str| output.with_file_name(format!("{stem}{suffix}"));
    [
        beside(".cue"),
        beside(".tracklist.txt"),
        beside(".chapters.txt"),
    ]
}

/// `output` with the extension of a mix: kept when it is .wav or .flac,
/// .wav otherwise.
pub(crate) fn mix_output(output: PathBuf) -> (PathBuf, MixFormat) {
    match MixFormat::of_path(&output) {
        Some(format) => (output, format),
        None => (output.with_extension("wav"), MixFormat::Wav),
    }
}

impl MixRenderJob {
    fn run(self, shared: Arc<RenderShared>) -> Result<MixRenderDone, EuphoriaError> {
        let directory = self.signals_directory.as_deref();
        let signals: Vec<_> = self
            .sides
            .iter()
            .map(|side| signals_of(side, directory))
            .collect();
        if shared.is_cancelled() {
            return Err(EuphoriaError::MixCancelled);
        }
        let inputs: Vec<MixInput<'_>> = self
            .sides
            .iter()
            .zip(&signals)
            .zip(&self.tracks)
            .enumerate()
            .map(|(index, ((side, signals), track))| MixInput {
                track: track.clone(),
                plan: PlanTrack {
                    analysis: &side.analysis,
                    sections: &side.sections,
                    energy: &signals.energy,
                    stems: signals.stems.as_ref(),
                },
                locked_exit: self.tracks.get(index + 1).and_then(|next| {
                    locked_blend(&self.locks, (&track.path, None), (&next.path, None))
                }),
            })
            .collect();
        let plan = plan_mix(&inputs);
        let (output, format) = mix_output(self.output.clone());
        let options = RenderOptions {
            format,
            existing: if output == self.output {
                ExistingOutput::Replace
            } else {
                ExistingOutput::Keep
            },
        };
        let rendered = render_mix(
            &plan,
            &output,
            &options,
            &mut SharedProgress(shared.clone()),
        )?;
        let audio_file_name = output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let [cue_path, tracklist_path, chapters_path] = side_files(&output);
        let texts = [
            (
                cue_path,
                cue_sheet(
                    &self.title,
                    &audio_file_name,
                    format,
                    &plan.tracks,
                    &rendered.timeline,
                ),
            ),
            (
                tracklist_path,
                tracklist(&self.title, &plan.tracks, &rendered.timeline),
            ),
            (chapters_path, chapters(&plan.tracks, &rendered.timeline)),
        ];
        for (path, text) in texts {
            std::fs::write(&path, text)
                .map_err(|source| EuphoriaError::ExportFile { path, source })?;
        }
        Ok(MixRenderDone {
            output,
            length_seconds: rendered.timeline.length_seconds,
        })
    }
}

impl EuphoriaApp {
    /// Asks where to save the set as one mix, then renders it there.
    pub(crate) fn choose_mix_render(&mut self, cx: &mut Context<'_, Self>) {
        let Some(planned) = self.set_builder.planned.as_ref() else {
            return;
        };
        if self.set_builder.render.is_some() {
            return;
        }
        let directory = planned
            .paths
            .first()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let suggested = format!(
            "{}.wav",
            crate::stem_export::file_name_part(&self.set_name())
        );
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
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
            this.update(cx, |this, cx| this.begin_mix_render(destination, cx))
                .ok();
        })
        .detach();
    }

    /// Renders the set to `output` in the background. The save dialog has
    /// already asked whether to replace a file there.
    pub(crate) fn begin_mix_render(&mut self, output: PathBuf, cx: &mut Context<'_, Self>) {
        if self.set_builder.render.is_some() {
            return;
        }
        let Some(planned) = self.set_builder.planned.as_ref() else {
            return;
        };
        let library = self.session.library();
        let mut sides = Vec::with_capacity(planned.paths.len());
        let mut tracks = Vec::with_capacity(planned.paths.len());
        for path in &planned.paths {
            let side = library
                .track_by_path(path)
                .and_then(|track| Some((track, plan_side(track, self)?)));
            let Some((track, side)) = side else {
                self.notice = Some(format!(
                    "The mix cannot be rendered: {} is not analyzed yet.",
                    path.display()
                ));
                cx.notify();
                return;
            };
            tracks.push(RenderTrack {
                path: path.clone(),
                artist: track.artist.clone(),
                title: Some(track.title().to_string()),
                duration_seconds: f64::from(side.analysis.duration_seconds),
                grid: side.analysis.beat_grid,
            });
            sides.push(side);
        }
        let job = MixRenderJob {
            title: self.set_name(),
            sides,
            tracks,
            locks: self.session.locked_transitions().to_vec(),
            output: output.clone(),
            signals_directory: self.planner.signals_directory.clone(),
        };
        let shared = Arc::new(RenderShared::default());
        self.set_builder.render = Some(MixRenderRun {
            shared: shared.clone(),
            output,
        });
        let background_job = self.begin_background_job("Rendering the mix", cx);
        let ticking = shared.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
                let running = this
                    .update(cx, |this, cx| {
                        let running = this
                            .set_builder
                            .render
                            .as_ref()
                            .is_some_and(|run| Arc::ptr_eq(&run.shared, &ticking));
                        if running {
                            cx.notify();
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { job.run(shared) })
                .await;
            this.update(cx, |this, cx| {
                this.background_jobs.finish(background_job);
                this.set_builder.render = None;
                this.finish_mix_render(outcome, cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn finish_mix_render(
        &mut self,
        outcome: Result<MixRenderDone, EuphoriaError>,
        cx: &mut Context<'_, Self>,
    ) {
        match outcome {
            Ok(done) => {
                let name = done
                    .output
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.show_toast(
                    format!(
                        "Saved the mix as {name} ({}), with its CUE sheet, tracklist and chapters",
                        format_mix_time(done.length_seconds)
                    ),
                    Some(crate::toast::ToastAction::Reveal(done.output)),
                    cx,
                );
            }
            Err(EuphoriaError::MixCancelled) => {
                self.show_toast("Rendering the mix was cancelled".to_string(), None, cx);
            }
            Err(error) => {
                self.notice = Some(format!("Could not render the mix: {}", error.chain()));
            }
        }
        cx.notify();
    }

    /// Stops the render that is running. Nothing is written.
    pub(crate) fn cancel_mix_render(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(run) = &self.set_builder.render {
            run.shared.cancel();
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mix_is_wav_unless_its_name_says_flac_or_mp3() {
        assert_eq!(
            mix_output(PathBuf::from("/sets/friday.flac")),
            (PathBuf::from("/sets/friday.flac"), MixFormat::Flac)
        );
        assert_eq!(
            mix_output(PathBuf::from("/sets/friday.mp3")),
            (
                PathBuf::from("/sets/friday.mp3"),
                MixFormat::Mp3 {
                    bitrate_kbps: euphoria_core::set_render::DEFAULT_MP3_BITRATE_KBPS
                }
            )
        );
        assert_eq!(
            mix_output(PathBuf::from("/sets/friday")),
            (PathBuf::from("/sets/friday.wav"), MixFormat::Wav)
        );
        assert_eq!(
            side_files(Path::new("/sets/friday.wav")),
            [
                PathBuf::from("/sets/friday.cue"),
                PathBuf::from("/sets/friday.tracklist.txt"),
                PathBuf::from("/sets/friday.chapters.txt"),
            ]
        );
    }

    #[test]
    fn progress_is_unknown_until_the_render_starts() {
        let shared = RenderShared::default();
        assert_eq!(shared.fraction(), None);
        let mut progress = SharedProgress(Arc::new(RenderShared::default()));
        progress.rendered(30.0, 120.0);
        assert_eq!(progress.0.fraction(), Some(0.25));
        assert!(!progress.is_cancelled());
        progress.0.cancel();
        assert!(progress.is_cancelled());
    }
}
