// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Separating a track into four stems, vocals, drums, bass and the rest,
//! with the Demucs htdemucs model run through ONNX Runtime.
//!
//! The track is decoded to stereo at 44.1 kHz, normalised, and cut into
//! segments of 7.8 seconds that overlap by a quarter. Each segment goes
//! through the network, and the segments are blended back together with
//! triangular weights, as Demucs does without random shifts.

mod cache;
mod input;
mod model;
mod segments;
mod spectrogram;
mod wav;
mod weights;

use std::path::{Path, PathBuf};

use ort::session::Session;
use ort::value::Tensor;

use crate::cache::ContentHash;
use crate::error::EuphoriaError;
use segments::SegmentPlan;
use spectrogram::{
    FRAMES, FREQUENCY_BINS, SEGMENT_SAMPLES, SPECTROGRAM_CHANNELS, SpectrogramTransforms,
};

pub use cache::{DEFAULT_STEM_CACHE_LIMIT_BYTES, STEM_CACHE_LIMIT_VARIABLE, StemCache, StemFiles};
pub use model::{WEIGHTS_BYTES, WEIGHTS_SHA256, WEIGHTS_URL, default_weights_path};

/// The sample rate of separated stems, the rate htdemucs was trained at.
pub const STEMS_SAMPLE_RATE: u32 = 44_100;
/// Separated stems are stereo.
pub const STEMS_CHANNELS: usize = 2;
/// Names the model whose stems are cached, so that stems from another
/// model are never served in their place.
pub const STEMS_MODEL: &str = "htdemucs-955717e8";

/// One of the four parts a track is separated into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stem {
    /// Drums and percussion.
    Drums,
    /// Bass guitar and bass synths.
    Bass,
    /// Everything that is not vocals, drums or bass.
    Other,
    /// Lead and backing vocals.
    Vocals,
}

impl Stem {
    /// Every stem, in the order the model returns them.
    pub const ALL: [Stem; 4] = [Stem::Drums, Stem::Bass, Stem::Other, Stem::Vocals];

    /// The lowercase name used for files, such as `vocals`.
    pub fn name(self) -> &'static str {
        match self {
            Stem::Drums => "drums",
            Stem::Bass => "bass",
            Stem::Other => "other",
            Stem::Vocals => "vocals",
        }
    }

    /// The name shown to people, such as `Vocals`.
    pub fn label(self) -> &'static str {
        match self {
            Stem::Drums => "Drums",
            Stem::Bass => "Bass",
            Stem::Other => "Other",
            Stem::Vocals => "Vocals",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The four stems of a track, stereo at [`STEMS_SAMPLE_RATE`].
#[derive(Debug, Clone, PartialEq)]
pub struct Stems {
    frames: usize,
    interleaved: [Vec<f32>; 4],
}

impl Stems {
    /// Frames per stem, one sample per channel each.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The samples of one stem, left and right interleaved.
    pub fn interleaved(&self, stem: Stem) -> &[f32] {
        &self.interleaved[stem.index()]
    }

    /// Length in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.frames as f64 / f64::from(STEMS_SAMPLE_RATE)
    }
}

/// What a separation is doing, for progress reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeparationStage {
    /// Reading the model, and downloading it the first time.
    LoadingModel,
    /// Decoding the audio file.
    Decoding,
    /// Running the network over the segments of the track.
    Separating,
    /// Writing the stems to disk.
    Saving,
}

impl SeparationStage {
    /// A few words saying what the stage does.
    pub fn label(self) -> &'static str {
        match self {
            SeparationStage::LoadingModel => "loading the model",
            SeparationStage::Decoding => "decoding",
            SeparationStage::Separating => "separating",
            SeparationStage::Saving => "saving",
        }
    }
}

/// Hears how a separation is going. Every method does nothing unless
/// implemented.
pub trait SeparationObserver {
    /// A new stage started.
    fn stage(&self, _stage: SeparationStage) {}
    /// Bytes of the model received so far, out of `total`, during its
    /// first download.
    fn downloading(&self, _received: u64, _total: u64) {}
    /// `done` segments of `total` are separated.
    fn separated(&self, _done: usize, _total: usize) {}
}

/// An observer that hears nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unobserved;

impl SeparationObserver for Unobserved {}

/// The htdemucs network loaded and ready to separate tracks. Loading takes
/// a few seconds and about 200 MB of memory, so keep one for many tracks.
pub struct StemSeparator {
    session: Session,
    _environment: ort::environment::Environment,
    transforms: SpectrogramTransforms,
}

impl std::fmt::Debug for StemSeparator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StemSeparator")
            .finish_non_exhaustive()
    }
}

impl StemSeparator {
    /// Loads the model from Euphoria's data folder, downloading the weights
    /// there first if they are missing.
    pub fn load(observer: &dyn SeparationObserver) -> Result<Self, EuphoriaError> {
        Self::load_from(&default_weights_path()?, observer)
    }

    /// Loads the model with the weights at `weights_path`, downloading them
    /// there first if the file is missing.
    pub fn load_from(
        weights_path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<Self, EuphoriaError> {
        observer.stage(SeparationStage::LoadingModel);
        let bytes = model::load_weights(weights_path, &|received, total| {
            observer.downloading(received, total)
        })?;
        let tensors =
            weights::read_safetensors(&bytes).map_err(|reason| EuphoriaError::Separation {
                reason: format!(
                    "cannot read the weights at {}: {reason}",
                    weights_path.display()
                ),
                source: None,
            })?;
        drop(bytes);
        let environment = ort::init()
            .with_name("euphoria")
            .with_telemetry(false)
            .build()
            .map_err(runtime_error("cannot start ONNX Runtime"))?;
        let mut builder =
            Session::builder(&environment).map_err(runtime_error("cannot start ONNX Runtime"))?;
        for tensor in tensors {
            let value = Tensor::from_array((tensor.shape, tensor.values))
                .map_err(runtime_error("cannot hold the weights"))?;
            builder = builder
                .with_external_initializer(&tensor.name, value.into_dyn())
                .map_err(|error| {
                    runtime_error("cannot give the weights to the network")(error.into())
                })?;
        }
        let session = builder
            .commit_from_memory(model::GRAPH)
            .map_err(runtime_error("cannot load the network"))?;
        Ok(Self {
            session,
            _environment: environment,
            transforms: SpectrogramTransforms::new(),
        })
    }

    /// Decodes the file at `path` and separates it.
    pub fn separate_file(
        &mut self,
        path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<Stems, EuphoriaError> {
        observer.stage(SeparationStage::Decoding);
        let [left, right] = input::decode_stereo(path, STEMS_SAMPLE_RATE)?;
        self.separate(&left, &right, observer)
    }

    /// Separates stereo audio at [`STEMS_SAMPLE_RATE`], given as one slice
    /// per channel of equal length.
    pub fn separate(
        &mut self,
        left: &[f32],
        right: &[f32],
        observer: &dyn SeparationObserver,
    ) -> Result<Stems, EuphoriaError> {
        observer.stage(SeparationStage::Separating);
        let frames = left.len().min(right.len());
        if frames == 0 {
            return Err(EuphoriaError::Separation {
                reason: "there is no audio to separate".into(),
                source: None,
            });
        }
        let (mean, deviation) = mono_mean_and_deviation(&left[..frames], &right[..frames]);
        let normalised: [Vec<f32>; 2] = [left, right].map(|channel| {
            channel[..frames]
                .iter()
                .map(|sample| (sample - mean) / deviation)
                .collect()
        });

        let plan = SegmentPlan::new(frames);
        let mut accumulated = vec![vec![0.0f32; frames]; Stem::ALL.len() * STEMS_CHANNELS];
        let mut spectrogram = Vec::new();
        let mut segment_waveform = vec![vec![0.0f32; SEGMENT_SAMPLES]; STEMS_CHANNELS];
        let mut separated_segment = vec![0.0f32; SEGMENT_SAMPLES];
        observer.separated(0, plan.len());
        for (done, segment) in plan.segments().enumerate() {
            for (channel, waveform) in segment_waveform.iter_mut().enumerate() {
                segment.read_padded(&normalised[channel], waveform);
            }
            self.transforms.spectrogram(
                [&segment_waveform[0], &segment_waveform[1]],
                &mut spectrogram,
            );
            let spectrogram_input = Tensor::from_array((
                [1usize, SPECTROGRAM_CHANNELS, FREQUENCY_BINS, FRAMES],
                std::mem::take(&mut spectrogram),
            ))
            .map_err(runtime_error("cannot hold the spectrogram"))?;
            let waveform_input = Tensor::from_array((
                [1usize, STEMS_CHANNELS, SEGMENT_SAMPLES],
                segment_waveform.concat(),
            ))
            .map_err(runtime_error("cannot hold the waveform"))?;
            let outputs = self
                .session
                .run(ort::inputs![
                    "spectrogram" => spectrogram_input,
                    "waveform" => waveform_input,
                ])
                .map_err(runtime_error("the network failed on a segment"))?;
            let (_, frequency_branch) = outputs["frequency_branch"]
                .try_extract_tensor::<f32>()
                .map_err(runtime_error("the network returned no spectrogram"))?;
            let (_, time_branch) = outputs["time_branch"]
                .try_extract_tensor::<f32>()
                .map_err(runtime_error("the network returned no waveform"))?;
            let plane = FREQUENCY_BINS * FRAMES;
            for stem in Stem::ALL {
                for channel in 0..STEMS_CHANNELS {
                    let source_channel = stem.index() * STEMS_CHANNELS + channel;
                    let time_start = source_channel * SEGMENT_SAMPLES;
                    separated_segment
                        .copy_from_slice(&time_branch[time_start..time_start + SEGMENT_SAMPLES]);
                    let real_start = (stem.index() * SPECTROGRAM_CHANNELS + channel * 2) * plane;
                    self.transforms.add_inverse(
                        &frequency_branch[real_start..real_start + plane],
                        &frequency_branch[real_start + plane..real_start + 2 * plane],
                        &mut separated_segment,
                    );
                    segment.add_weighted(&separated_segment, &mut accumulated[source_channel]);
                }
            }
            observer.separated(done + 1, plan.len());
        }

        let total_weight = plan.total_weight();
        let mut channels = accumulated.into_iter();
        let interleaved = Stem::ALL.map(|_| {
            let left = channels.next().unwrap_or_default();
            let right = channels.next().unwrap_or_default();
            let restore = |sample: f32, weight: f32| sample / weight * deviation + mean;
            let mut samples = Vec::with_capacity(frames * STEMS_CHANNELS);
            for index in 0..frames {
                samples.push(restore(left[index], total_weight[index]));
                samples.push(restore(right[index], total_weight[index]));
            }
            samples
        });
        Ok(Stems {
            frames,
            interleaved,
        })
    }
}

/// Separates tracks and keeps their stems in a [`StemCache`], loading the
/// model only when a track is not cached yet.
#[derive(Debug)]
pub struct CachedSeparator {
    cache: StemCache,
    weights_path: PathBuf,
    separator: Option<StemSeparator>,
}

impl CachedSeparator {
    /// Uses the cache and the model in Euphoria's data folder.
    pub fn open_default() -> Result<Self, EuphoriaError> {
        Ok(Self::new(
            StemCache::open_default()?,
            default_weights_path()?,
        ))
    }

    /// Uses `cache`, and the model weights at `weights_path`.
    pub fn new(cache: StemCache, weights_path: PathBuf) -> Self {
        Self {
            cache,
            weights_path,
            separator: None,
        }
    }

    /// The cache the stems are kept in.
    pub fn cache(&self) -> &StemCache {
        &self.cache
    }

    /// The stems of the file at `path`, from the cache when it holds them,
    /// otherwise separated and saved there first.
    pub fn stems(
        &mut self,
        path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<StemFiles, EuphoriaError> {
        let hash = ContentHash::of_file(path).map_err(|source| EuphoriaError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        if let Some(files) = self.cache.lookup(&hash) {
            return Ok(files);
        }
        let separator = match &mut self.separator {
            Some(separator) => separator,
            empty => empty.insert(StemSeparator::load_from(&self.weights_path, observer)?),
        };
        let stems = separator.separate_file(path, observer)?;
        observer.stage(SeparationStage::Saving);
        self.cache.store(&hash, &stems)
    }
}

/// The mean and the standard deviation of the mono mix, which Demucs
/// normalises the whole track by. Silence gets a deviation of one, so that
/// it is not divided by zero.
fn mono_mean_and_deviation(left: &[f32], right: &[f32]) -> (f32, f32) {
    let count = left.len() as f64;
    let mono = || {
        left.iter()
            .zip(right)
            .map(|(left, right)| (f64::from(*left) + f64::from(*right)) / 2.0)
    };
    let mean = mono().sum::<f64>() / count;
    let squares: f64 = mono().map(|sample| (sample - mean).powi(2)).sum();
    let deviation = if count > 1.0 {
        (squares / (count - 1.0)).sqrt()
    } else {
        0.0
    };
    let deviation = if deviation > 1e-8 { deviation } else { 1.0 };
    (mean as f32, deviation as f32)
}

fn runtime_error(reason: &'static str) -> impl Fn(ort::Error) -> EuphoriaError {
    move |error| EuphoriaError::Separation {
        reason: reason.to_string(),
        source: Some(Box::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_named_in_the_order_the_model_returns_them() {
        let names: Vec<&str> = Stem::ALL.iter().map(|stem| stem.name()).collect();
        assert_eq!(names, ["drums", "bass", "other", "vocals"]);
        for (index, stem) in Stem::ALL.into_iter().enumerate() {
            assert_eq!(stem.index(), index);
        }
    }

    #[test]
    fn normalisation_uses_the_unbiased_deviation_of_the_mono_mix() {
        let (mean, deviation) = mono_mean_and_deviation(&[1.0, 3.0], &[1.0, 3.0]);
        assert_eq!(mean, 2.0);
        assert!((deviation - 2.0f32.sqrt()).abs() < 1e-6);
        assert_eq!(mono_mean_and_deviation(&[0.0; 4], &[0.0; 4]), (0.0, 1.0));
    }
}
