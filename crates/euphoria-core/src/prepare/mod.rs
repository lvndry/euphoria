// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preparing tracks for a club system: every track brought to one loudness,
//! kept under a true-peak ceiling by a transparent limiter, and optionally
//! brought back to concert pitch. Prepared tracks are always new files; an
//! original is only read.
//!
//! [`plan_track`] measures a track and works out what preparing it would do,
//! without writing anything. [`prepare_track`] carries out exactly that
//! plan, so the numbers of a dry run are the numbers of the real run.

mod aiff;
mod limiter;
pub mod manifest;
mod preview;
pub mod replaygain;

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use ebur128::{Channel, EbuR128, Mode};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::TagExt;
use serde::Serialize;

use self::aiff::AiffWriter;
use self::limiter::TruePeakLimiter;
pub use self::manifest::{Manifest, RevertReport, RunMode, manifests_in, revert, tag_track};
pub use self::preview::{ComparisonClip, comparison_clip};
pub use self::replaygain::{
    REPLAYGAIN_REFERENCE_LUFS, ReplayGainFields, read_replaygain, write_replaygain,
};
use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;
use crate::shift::{
    ExistingOutput, PitchShifter, ShiftedFormat, ShiftedWriter, create_temporary_file, install,
    same_file,
};

/// The loudness targets offered by default, in LUFS: streaming, broadcast
/// and two club levels.
pub const LOUDNESS_PRESETS_LUFS: [f32; 4] = [-14.0, -11.0, -9.0, -7.0];

/// The default suffix added to the name of a prepared file.
pub const DEFAULT_SUFFIX: &str = " (prepared)";

/// The smallest tuning offset worth correcting, in cents: below it the
/// shift cannot be heard and only costs sound quality.
pub const MINIMUM_TUNING_CORRECTION_CENTS: f32 = 3.0;

/// The largest tuning offset an analysis reports, in cents.
const MAXIMUM_TUNING_CORRECTION_CENTS: f32 = 50.0;

/// The limiter aims this far under the ceiling, in dB, so the ceiling holds
/// when another meter interpolates between samples a little differently.
pub(crate) const LIMITER_MARGIN_DB: f32 = 0.15;

/// Consecutive frames at full scale that count as clipping.
const CLIPPED_RUN_FRAMES: u64 = 3;
const CLIPPED_LEVEL: f32 = 0.9999;

/// What preparing does to the sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrepareSettings {
    /// The integrated loudness to bring every track to, in LUFS.
    pub target_lufs: f32,
    /// The true-peak ceiling, in dBTP.
    pub true_peak_ceiling_db: f32,
    /// The most the limiter may turn peaks down, in dB. A track that would
    /// need more gets less gain instead.
    pub maximum_limiting_db: f32,
    /// Shifts tracks measured off A = 440 Hz back to concert pitch.
    pub correct_tuning: bool,
}

impl Default for PrepareSettings {
    fn default() -> Self {
        Self {
            target_lufs: -9.0,
            true_peak_ceiling_db: -1.0,
            maximum_limiting_db: 3.0,
            correct_tuning: false,
        }
    }
}

impl PrepareSettings {
    /// Checks that every value is in a range that makes sense.
    pub fn validate(&self) -> Result<(), String> {
        if !(-30.0..=0.0).contains(&self.target_lufs) {
            return Err(format!(
                "a loudness target of {} LUFS is outside -30 to 0",
                self.target_lufs
            ));
        }
        if !(-6.0..=0.0).contains(&self.true_peak_ceiling_db) {
            return Err(format!(
                "a true-peak ceiling of {} dBTP is outside -6 to 0",
                self.true_peak_ceiling_db
            ));
        }
        if !(0.0..=12.0).contains(&self.maximum_limiting_db) {
            return Err(format!(
                "a limiting allowance of {} dB is outside 0 to 12",
                self.maximum_limiting_db
            ));
        }
        Ok(())
    }
}

/// The audio file formats a prepared track is written in, always at 24 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// WAV.
    Wav,
    /// AIFF.
    Aiff,
    /// FLAC.
    Flac,
}

impl OutputFormat {
    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Flac => "flac",
        }
    }
}

/// What a lossy source becomes: a lossless file, never another lossy one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LossyOutput {
    /// 24 bit FLAC.
    #[default]
    Flac,
    /// 24 bit WAV.
    Wav,
}

/// The format a prepared copy of `input` is written in: the format of the
/// source when it is WAV, AIFF or FLAC, else the lossless format chosen
/// for lossy sources.
pub fn output_format(input: &Path, lossy: LossyOutput) -> OutputFormat {
    let extension = input
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("wav" | "wave") => OutputFormat::Wav,
        Some("aif" | "aiff") => OutputFormat::Aiff,
        Some("flac") => OutputFormat::Flac,
        _ => match lossy {
            LossyOutput::Flac => OutputFormat::Flac,
            LossyOutput::Wav => OutputFormat::Wav,
        },
    }
}

/// Why a track is not prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The track is silent, so it has no loudness to match.
    Silent,
    /// The track is shorter than the 400 milliseconds loudness needs.
    TooShort,
}

impl SkipReason {
    /// A sentence saying why.
    pub fn description(self) -> &'static str {
        match self {
            Self::Silent => "the track is silent, so it has no loudness to match",
            Self::TooShort => "the track is shorter than the 0.4 seconds loudness needs",
        }
    }
}

/// Runs of samples at full scale found in a source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ClippingReport {
    /// Passages of at least three consecutive frames at full scale.
    pub runs: u32,
    /// Frames inside those passages.
    pub clipped_frames: u64,
    /// The longest passage, in frames.
    pub longest_run_frames: u64,
    /// Where the first passage starts, in seconds.
    pub first_run_seconds: Option<f32>,
}

/// Everything measured about a track and what preparing it will do.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackPlan {
    /// Sample rate of the source, kept in the copy.
    pub sample_rate: u32,
    /// Channels of the source, kept in the copy.
    pub channels: u16,
    /// Length in frames.
    pub frames: u64,
    /// Length in seconds.
    pub duration_seconds: f32,
    /// Integrated loudness of the source, in LUFS.
    pub measured_lufs: Option<f32>,
    /// True peak of the source, in dBTP.
    pub true_peak_dbtp: Option<f32>,
    /// Sample peak of the source, in dBFS.
    pub sample_peak_db: Option<f32>,
    /// Passages already clipped in the source. They are reported, not
    /// repaired.
    pub clipping: ClippingReport,
    /// The loudness asked for, in LUFS.
    pub target_lufs: f32,
    /// The gain that reaches the target, in dB.
    pub wanted_gain_db: f32,
    /// The gain applied, in dB: the wanted gain, or less when the limiter
    /// would have to do more than it is allowed.
    pub gain_db: f32,
    /// How far the limiter turns the highest peak down, in dB.
    pub limiting_db: f32,
    /// True when the gain was capped so the limiter stays within its
    /// allowance, and the target is not reached.
    pub gain_capped: bool,
    /// The loudness the prepared track is expected to have, in LUFS.
    pub predicted_lufs: Option<f32>,
    /// How far the source is tuned from A = 440 Hz, in cents, as measured
    /// by an analysis.
    pub tuning_cents: Option<f32>,
    /// The pitch shift that will be applied to reach concert pitch, in
    /// cents, or `None` when tuning is left alone.
    pub tuning_correction_cents: Option<f32>,
    /// The format of the prepared file.
    pub output_format: OutputFormat,
    /// Why the track will not be prepared, when it will not.
    pub skipped: Option<SkipReason>,
}

impl TrackPlan {
    /// True when the plan leaves the track as it is: no gain and no
    /// pitch shift.
    pub fn changes_nothing(&self) -> bool {
        self.skipped.is_none() && self.gain_db == 0.0 && self.tuning_correction_cents.is_none()
    }
}

/// What measuring a track found, before any settings: measure once, then
/// plan for as many settings as needed.
#[derive(Debug, Clone)]
pub struct TrackMeasurement {
    path: PathBuf,
    sample_rate: u32,
    channels: u16,
    frames: u64,
    lufs: Option<f32>,
    true_peak: f32,
    sample_peak: f32,
    clipping: ClippingReport,
}

pub(crate) fn failure(path: &Path, reason: impl Into<String>) -> EuphoriaError {
    EuphoriaError::Prepare {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// Reads the track at `input` once, measuring its loudness, peaks and
/// clipping.
pub fn measure_track(input: &Path) -> Result<TrackMeasurement, EuphoriaError> {
    let mut stream = AudioStream::open(input)?;
    let sample_rate = stream.sample_rate().get();
    let channels = stream.channel_count().get();
    let channel_count = usize::from(channels);
    let meter_error = |error: ebur128::Error| failure(input, format!("cannot measure it: {error}"));
    let mut meter = EbuR128::new(u32::from(channels), sample_rate, Mode::I | Mode::TRUE_PEAK)
        .map_err(meter_error)?;
    if channels == 1 {
        meter
            .set_channel(0, Channel::DualMono)
            .map_err(meter_error)?;
    }
    let mut frames = 0_u64;
    let mut sample_peak = 0.0_f32;
    let mut run_frames = 0_u64;
    let mut clipping = ClippingReport::default();
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk)? {
        meter.add_frames_f32(&chunk).map_err(meter_error)?;
        for frame in chunk.chunks_exact(channel_count) {
            let level = frame
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            sample_peak = sample_peak.max(level);
            if level >= CLIPPED_LEVEL {
                run_frames += 1;
                if run_frames == CLIPPED_RUN_FRAMES {
                    clipping.runs += 1;
                    clipping.clipped_frames += CLIPPED_RUN_FRAMES;
                    if clipping.first_run_seconds.is_none() {
                        let start = frames + 1 - CLIPPED_RUN_FRAMES;
                        clipping.first_run_seconds = Some(start as f32 / sample_rate as f32);
                    }
                } else if run_frames > CLIPPED_RUN_FRAMES {
                    clipping.clipped_frames += 1;
                }
                clipping.longest_run_frames = clipping.longest_run_frames.max(run_frames);
            } else {
                run_frames = 0;
            }
            frames += 1;
        }
    }
    let loudness = meter.loudness_global().map_err(meter_error)?;
    let mut true_peak = 0.0_f64;
    for channel in 0..u32::from(channels) {
        true_peak = true_peak.max(meter.true_peak(channel).map_err(meter_error)?);
    }
    Ok(TrackMeasurement {
        path: input.to_path_buf(),
        sample_rate,
        channels,
        frames,
        lufs: loudness.is_finite().then_some(loudness as f32),
        true_peak: true_peak as f32,
        sample_peak,
        clipping,
    })
}

fn decibels(linear: f32) -> Option<f32> {
    (linear > 0.0).then(|| 20.0 * linear.log10())
}

/// Measures the track at `input` and works out what preparing it does,
/// without writing anything. `tuning_cents` is how far an analysis found it
/// tuned from A = 440 Hz; it is used only when the settings correct tuning.
pub fn plan_track(
    input: &Path,
    settings: &PrepareSettings,
    tuning_cents: Option<f32>,
    lossy: LossyOutput,
) -> Result<TrackPlan, EuphoriaError> {
    measure_track(input)?.plan(settings, tuning_cents, lossy)
}

impl TrackMeasurement {
    /// Works out what preparing the measured track does. The same
    /// measurement always gives the same plan for the same settings.
    pub fn plan(
        &self,
        settings: &PrepareSettings,
        tuning_cents: Option<f32>,
        lossy: LossyOutput,
    ) -> Result<TrackPlan, EuphoriaError> {
        settings
            .validate()
            .map_err(|reason| failure(&self.path, reason))?;
        let measured = self;
        let duration_seconds = measured.frames as f32 / measured.sample_rate as f32;
        let true_peak_dbtp = decibels(measured.true_peak);
        let tuning_correction_cents = if settings.correct_tuning {
            tuning_cents
                .filter(|cents| cents.is_finite() && cents.abs() >= MINIMUM_TUNING_CORRECTION_CENTS)
                .map(|cents| {
                    (-cents).clamp(
                        -MAXIMUM_TUNING_CORRECTION_CENTS,
                        MAXIMUM_TUNING_CORRECTION_CENTS,
                    )
                })
        } else {
            None
        };
        let skipped = match measured.lufs {
            Some(_) => None,
            None if measured.sample_peak == 0.0 => Some(SkipReason::Silent),
            None if duration_seconds < 0.4 => Some(SkipReason::TooShort),
            None => Some(SkipReason::Silent),
        };
        let (wanted_gain_db, gain_db, limiting_db, gain_capped) =
            match (measured.lufs, true_peak_dbtp) {
                (Some(lufs), Some(peak)) => {
                    let wanted = settings.target_lufs - lufs;
                    let allowed =
                        settings.true_peak_ceiling_db - peak + settings.maximum_limiting_db;
                    let gain = wanted.min(allowed);
                    let limiting = (peak + gain - settings.true_peak_ceiling_db).max(0.0);
                    (wanted, gain, limiting, wanted > allowed)
                }
                _ => (0.0, 0.0, 0.0, false),
            };
        Ok(TrackPlan {
            sample_rate: measured.sample_rate,
            channels: measured.channels,
            frames: measured.frames,
            duration_seconds,
            measured_lufs: measured.lufs,
            true_peak_dbtp,
            sample_peak_db: decibels(measured.sample_peak),
            clipping: measured.clipping,
            target_lufs: settings.target_lufs,
            wanted_gain_db,
            gain_db,
            limiting_db,
            gain_capped,
            predicted_lufs: measured.lufs.map(|lufs| lufs + gain_db),
            tuning_cents,
            tuning_correction_cents,
            output_format: output_format(&self.path, lossy),
            skipped,
        })
    }
}

/// A suffix for file names that is safe to put in one: no path separators.
pub fn check_suffix(suffix: &str) -> Result<(), String> {
    if suffix
        .chars()
        .any(|character| matches!(character, '/' | '\\' | '\0'))
    {
        return Err("the suffix cannot contain a path separator".to_string());
    }
    Ok(())
}

/// Where the prepared copy of `input` goes: next to the original, or in
/// `output_directory`, named like the original plus `suffix`. A name that
/// is taken, on disk or in `reserved`, gets a number, so nothing is ever
/// overwritten.
pub fn prepared_path(
    input: &Path,
    output_directory: Option<&Path>,
    suffix: &str,
    format: OutputFormat,
    reserved: &HashSet<PathBuf>,
) -> PathBuf {
    let directory = output_directory
        .map(Path::to_path_buf)
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let stem = input
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "track".to_string());
    let mut candidate = directory.join(format!("{stem}{suffix}.{}", format.extension()));
    let mut number = 2;
    while candidate.exists() || reserved.contains(&candidate) || same_file(&candidate, input) {
        candidate = directory.join(format!("{stem}{suffix} {number}.{}", format.extension()));
        number += 1;
    }
    candidate
}

/// What [`prepare_track`] wrote.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreparedTrack {
    /// The file written.
    pub output: PathBuf,
    /// The plan that was carried out.
    pub plan: TrackPlan,
    /// How far the limiter turned the highest peak down, in dB.
    pub limiter_reduction_db: f32,
    /// True when the tags and cover art of the original were copied.
    pub tags_copied: bool,
}

enum OutputWriter {
    Shifted(Box<ShiftedWriter>),
    Aiff(AiffWriter),
}

impl OutputWriter {
    fn create(
        file: File,
        path: &Path,
        format: OutputFormat,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, EuphoriaError> {
        Ok(match format {
            OutputFormat::Wav => Self::Shifted(Box::new(ShiftedWriter::create(
                file,
                path,
                ShiftedFormat::Wav,
                channels,
                sample_rate,
            )?)),
            OutputFormat::Flac => Self::Shifted(Box::new(ShiftedWriter::create(
                file,
                path,
                ShiftedFormat::Flac,
                channels,
                sample_rate,
            )?)),
            OutputFormat::Aiff => Self::Aiff(AiffWriter::new(file, path, channels, sample_rate)?),
        })
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<(), EuphoriaError> {
        match self {
            Self::Shifted(writer) => writer.write(interleaved),
            Self::Aiff(writer) => writer.write(interleaved),
        }
    }

    fn finish(self) -> Result<u64, EuphoriaError> {
        match self {
            Self::Shifted(writer) => writer.finish(),
            Self::Aiff(writer) => writer.finish(),
        }
    }
}

/// Writes the prepared copy of the track at `input` to `output`, as `plan`
/// says, and copies the tags and cover art of the original into it. The
/// original is only read. The copy is written under a temporary name and
/// moved into place once complete; a file already at `output` is never
/// replaced. `on_progress` is told how much of the track is done, from 0
/// to 1.
pub fn prepare_track(
    input: &Path,
    plan: &TrackPlan,
    output: &Path,
    settings: &PrepareSettings,
    on_progress: &mut dyn FnMut(f32),
) -> Result<PreparedTrack, EuphoriaError> {
    if let Some(reason) = plan.skipped {
        return Err(failure(input, reason.description()));
    }
    if same_file(input, output) {
        return Err(failure(
            input,
            "the output is the original file, which is never changed",
        ));
    }
    if ShiftedFormat::of_path(output).is_none()
        && output.extension().and_then(|extension| extension.to_str()) != Some("aiff")
    {
        return Err(failure(
            input,
            format!("{} must end in .wav, .aiff or .flac", output.display()),
        ));
    }
    let mut stream = AudioStream::open(input)?;
    let channels = plan.channels;
    let sample_rate = plan.sample_rate;
    if stream.sample_rate().get() != sample_rate || stream.channel_count().get() != channels {
        return Err(failure(input, "the track changed since it was measured"));
    }
    let channel_count = usize::from(channels);
    let mut shifter = match plan.tuning_correction_cents {
        Some(cents) => Some(
            PitchShifter::with_cents(channel_count, sample_rate, cents)
                .map_err(|reason| failure(input, reason))?,
        ),
        None => None,
    };
    let mut limiter = TruePeakLimiter::new(
        channel_count,
        sample_rate,
        settings.true_peak_ceiling_db - LIMITER_MARGIN_DB,
    );
    let gain = 10.0_f32.powf(plan.gain_db / 20.0);

    let (file, temporary) =
        create_temporary_file(output).map_err(|source| EuphoriaError::ExportFile {
            path: output.to_path_buf(),
            source,
        })?;
    let written = (|| -> Result<u64, EuphoriaError> {
        let mut writer =
            OutputWriter::create(file, &temporary, plan.output_format, channels, sample_rate)?;
        let mut chunk = Vec::new();
        let mut done_frames = 0_u64;
        let limit_and_write = |samples: &mut Vec<f32>,
                               writer: &mut OutputWriter,
                               limiter: &mut TruePeakLimiter|
         -> Result<(), EuphoriaError> {
            for sample in samples.iter_mut() {
                *sample *= gain;
            }
            writer.write(&limiter.push(samples))
        };
        while stream.read_chunk(&mut chunk)? {
            done_frames += (chunk.len() / channel_count) as u64;
            let mut shifted = match shifter.as_mut() {
                Some(shifter) => shifter
                    .process(&chunk)
                    .map_err(|reason| failure(input, reason))?,
                None => std::mem::take(&mut chunk),
            };
            limit_and_write(&mut shifted, &mut writer, &mut limiter)?;
            on_progress((done_frames as f32 / plan.frames.max(1) as f32).min(1.0));
        }
        if let Some(shifter) = shifter.take() {
            let mut rest = shifter.finish().map_err(|reason| failure(input, reason))?;
            limit_and_write(&mut rest, &mut writer, &mut limiter)?;
        }
        writer.write(&limiter.finish())?;
        writer.finish()
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }

    let tags_copied = match copy_tags(input, &temporary) {
        Ok(copied) => copied,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    };
    if let Err(error) = install(&temporary, output, ExistingOutput::Keep) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    on_progress(1.0);
    Ok(PreparedTrack {
        output: output.to_path_buf(),
        plan: plan.clone(),
        limiter_reduction_db: limiter.reduction_db(),
        tags_copied,
    })
}

/// Copies the tags, with their cover art, of `source` into `destination`,
/// in the kind of tag its format uses. Returns false when the source has
/// no tags.
fn copy_tags(source: &Path, destination: &Path) -> Result<bool, EuphoriaError> {
    fn read_error(source: &Path, error: impl std::fmt::Display) -> EuphoriaError {
        failure(source, format!("cannot read its tags: {error}"))
    }
    let original = Probe::open(source)
        .map_err(|error| read_error(source, error))?
        .read()
        .map_err(|error| read_error(source, error))?;
    let Some(tag) = original.primary_tag().or_else(|| original.first_tag()) else {
        return Ok(false);
    };
    let copy_file = Probe::open(destination)
        .map_err(|error| read_error(source, error))?
        .read()
        .map_err(|error| read_error(source, error))?;
    let mut tag = tag.clone();
    tag.re_map(copy_file.file_type().primary_tag_type());
    tag.save_to_path(destination, WriteOptions::default())
        .map_err(|error| failure(source, format!("cannot write the tags: {error}")))?;
    Ok(true)
}

#[cfg(test)]
mod tests;
