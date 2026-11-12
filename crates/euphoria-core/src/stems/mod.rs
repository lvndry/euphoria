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
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;

use crate::cache::ContentHash;
use crate::error::EuphoriaError;
use segments::{OverlapWindow, SegmentPlan};
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
/// Environment variable setting how many threads separation runs on.
pub const STEMS_THREADS_VARIABLE: &str = "EUPHORIA_STEMS_THREADS";

// ORT permits only one environment to be built in a process, even after
// its last handle is dropped. Keep it alive and share it across separators.
static ENVIRONMENT: Mutex<Option<ort::environment::Environment>> = Mutex::new(None);

fn environment() -> Result<ort::environment::Environment, EuphoriaError> {
    let mut shared = ENVIRONMENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(environment) = shared.as_ref() {
        return Ok(environment.clone());
    }
    let environment = ort::init()
        .with_name("euphoria")
        .with_telemetry(false)
        .build()
        .map_err(runtime_error("cannot start ONNX Runtime"))?;
    *shared = Some(environment.clone());
    Ok(environment)
}

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
    /// Waiting for a separation in another window or command to finish,
    /// since two at once would need twice the memory.
    Waiting,
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
            SeparationStage::Waiting => "waiting for another separation to finish",
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
        let environment = environment()?;
        let builder = Session::builder(&environment)
            .map_err(runtime_error("cannot start ONNX Runtime"))?
            // The ort crate otherwise lets ONNX Runtime pick the most power
            // efficient device, which on a Mac is Core ML: there this model
            // needs about 16 GB and its stems are wrong.
            .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)
            .map_err(runtime_error("cannot start ONNX Runtime"))?;
        let builder = builder
            .with_intra_threads(separation_threads())
            .map_err(runtime_error("cannot start ONNX Runtime"))?;
        // ONNX Runtime's memory pattern and CPU arena keep the peak of
        // every intermediate tensor allocated; without them a segment
        // needs about 2 GB less.
        let builder = builder
            .with_memory_pattern(false)
            .map_err(runtime_error("cannot start ONNX Runtime"))?;
        let mut builder = builder
            .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(false).build()])
            .map_err(runtime_error("cannot start ONNX Runtime"))?;
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
        let mut collected = CollectedStems::default();
        let frames = self.separate_into(left, right, observer, &mut collected)?;
        Ok(Stems {
            frames,
            interleaved: collected.interleaved,
        })
    }

    /// Separates like [`StemSeparator::separate`] and hands the stems to
    /// `sink` in order as soon as their blend is final, so that only about
    /// one segment of output is held at a time. Returns the frames written.
    pub(crate) fn separate_into(
        &mut self,
        left: &[f32],
        right: &[f32],
        observer: &dyn SeparationObserver,
        sink: &mut dyn StemSink,
    ) -> Result<usize, EuphoriaError> {
        if left.len() != right.len() {
            return Err(EuphoriaError::Separation {
                reason: "the left and right channels must have the same number of samples".into(),
                source: None,
            });
        }
        observer.stage(SeparationStage::Separating);
        let frames = left.len();
        if frames == 0 {
            return Err(EuphoriaError::Separation {
                reason: "there is no audio to separate".into(),
                source: None,
            });
        }
        let (mean, deviation) = mono_mean_and_deviation(left, right);
        let normalise = |sample: f32| (sample - mean) / deviation;
        let restore = |sample: f32| sample * deviation + mean;

        let plan = SegmentPlan::new(frames);
        let source_channels = Stem::ALL.len() * STEMS_CHANNELS;
        let mut window = OverlapWindow::new(source_channels);
        let mut finished = vec![Vec::new(); source_channels];
        let mut blocks: [Vec<f32>; 4] = Default::default();
        let mut spectrogram = Vec::new();
        let mut segment_waveform = vec![vec![0.0f32; SEGMENT_SAMPLES]; STEMS_CHANNELS];
        let mut separated_segment = vec![0.0f32; SEGMENT_SAMPLES];
        observer.separated(0, plan.len());
        for (done, segment) in plan.segments().enumerate() {
            for (channel, waveform) in segment_waveform.iter_mut().enumerate() {
                let samples = if channel == 0 { left } else { right };
                segment.read_padded(samples, waveform, normalise);
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
                    window.add(&segment, source_channel, &separated_segment);
                }
            }
            drop(outputs);
            window.take_until(segment.final_until(), &mut finished);
            for (stem_index, block) in blocks.iter_mut().enumerate() {
                let left_channel = &finished[stem_index * STEMS_CHANNELS];
                let right_channel = &finished[stem_index * STEMS_CHANNELS + 1];
                block.clear();
                for (left_sample, right_sample) in left_channel.iter().zip(right_channel) {
                    block.push(restore(*left_sample));
                    block.push(restore(*right_sample));
                }
            }
            sink.write(&blocks)?;
            observer.separated(done + 1, plan.len());
        }
        Ok(frames)
    }
}

/// Receives separated audio in order, a block of frames at a time: for
/// every stem, in the order of [`Stem::ALL`], left and right interleaved.
pub(crate) trait StemSink {
    fn write(&mut self, blocks: &[Vec<f32>; 4]) -> Result<(), EuphoriaError>;
}

/// Keeps every block in memory, for [`StemSeparator::separate`].
#[derive(Default)]
struct CollectedStems {
    interleaved: [Vec<f32>; 4],
}

impl StemSink for CollectedStems {
    fn write(&mut self, blocks: &[Vec<f32>; 4]) -> Result<(), EuphoriaError> {
        for (collected, block) in self.interleaved.iter_mut().zip(blocks) {
            collected.extend_from_slice(block);
        }
        Ok(())
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
        let content_hash = || {
            ContentHash::of_file(path).map_err(|source| EuphoriaError::Open {
                path: path.to_path_buf(),
                source,
            })
        };
        if let Some(files) = self.cache.lookup(&content_hash()?) {
            return Ok(files);
        }
        let _turn = self.cache.separation_turn(observer)?;
        let hash = content_hash()?;
        if let Some(files) = self.cache.lookup(&hash) {
            return Ok(files);
        }
        let separator = match &mut self.separator {
            Some(separator) => separator,
            empty => empty.insert(StemSeparator::load_from(&self.weights_path, observer)?),
        };
        observer.stage(SeparationStage::Decoding);
        let [left, right] = input::decode_stereo(path, STEMS_SAMPLE_RATE)?;
        self.cache.store_streamed(&hash, left.len(), |sink| {
            separator.separate_into(&left, &right, observer, sink)?;
            observer.stage(SeparationStage::Saving);
            if content_hash()? != hash {
                return Err(EuphoriaError::Separation {
                    reason: format!(
                        "{} changed while it was being separated; try again",
                        path.display()
                    ),
                    source: None,
                });
            }
            Ok(())
        })
    }
}

/// How many threads the network runs on: the value of
/// `EUPHORIA_STEMS_THREADS` when it is a positive number, otherwise every
/// core but one, so that the app and the music playing stay responsive.
fn separation_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    threads_for(std::env::var(STEMS_THREADS_VARIABLE).ok().as_deref(), cores)
}

fn threads_for(chosen: Option<&str>, cores: usize) -> usize {
    chosen
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or_else(|| cores.saturating_sub(1).max(1))
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

    fn synthetic_separator() -> StemSeparator {
        let environment = environment().unwrap();
        let session = Session::builder(&environment)
            .unwrap()
            .with_auto_device(ort::session::builder::AutoDevicePolicy::PreferCPU)
            .unwrap()
            .commit_from_memory(include_bytes!("fixtures/zero_stems.onnx"))
            .unwrap();
        StemSeparator {
            session,
            _environment: environment,
            transforms: SpectrogramTransforms::new(),
        }
    }

    #[test]
    fn mismatched_stereo_channels_are_rejected_instead_of_truncated() {
        let mut separator = synthetic_separator();
        for (left, right) in [(&[0.0, 0.5][..], &[0.0][..]), (&[0.0][..], &[0.0, 0.5][..])] {
            assert!(separator.separate(left, right, &Unobserved).is_err());
        }
    }

    #[test]
    fn separation_leaves_a_core_free_unless_told_otherwise() {
        assert_eq!(threads_for(None, 10), 9);
        assert_eq!(threads_for(None, 1), 1);
        assert_eq!(threads_for(Some("4"), 10), 4);
        assert_eq!(threads_for(Some("0"), 10), 9);
        assert_eq!(threads_for(Some("many"), 4), 3);
    }

    #[test]
    fn streamed_and_collected_stems_are_the_same() {
        let left: Vec<f32> = (0..700_000)
            .map(|index| (index as f32 * 0.003).sin() * 0.4)
            .collect();
        let right: Vec<f32> = left.iter().map(|sample| sample * 0.5 + 0.01).collect();
        let mut separator = synthetic_separator();
        let collected = separator.separate(&left, &right, &Unobserved).unwrap();
        let folder = crate::cache::test_support::TemporaryFolder::new();
        let cache = StemCache::new(folder.path().to_path_buf(), DEFAULT_STEM_CACHE_LIMIT_BYTES);
        let files = cache
            .store_streamed(&ContentHash::from_bytes([7; 32]), left.len(), |sink| {
                separator
                    .separate_into(&left, &right, &Unobserved, sink)
                    .map(|_| ())
            })
            .unwrap();
        let expected = folder.path().join("expected");
        std::fs::create_dir_all(&expected).unwrap();
        for stem in Stem::ALL {
            assert_eq!(collected.interleaved(stem).len(), left.len() * 2);
            let expected_file = expected.join(format!("{}.wav", stem.name()));
            wav::write_wav(
                &expected_file,
                collected.interleaved(stem),
                2,
                STEMS_SAMPLE_RATE,
            )
            .unwrap();
            assert_eq!(
                std::fs::read(files.path(stem)).unwrap(),
                std::fs::read(&expected_file).unwrap(),
                "{stem:?}"
            );
        }
    }

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

    #[test]
    fn source_replaced_during_separation_is_not_cached_under_its_old_hash() {
        struct ReplaceSource {
            stage: SeparationStage,
            path: PathBuf,
        }
        impl SeparationObserver for ReplaceSource {
            fn stage(&self, stage: SeparationStage) {
                if stage == self.stage {
                    wav::write_wav(&self.path, &[0.5; 1024], 2, STEMS_SAMPLE_RATE).unwrap();
                }
            }
        }
        for stage in [SeparationStage::Decoding, SeparationStage::Saving] {
            let folder = crate::cache::test_support::TemporaryFolder::new();
            let directory = folder.path();
            let path = directory.join("track.wav");
            wav::write_wav(&path, &[0.0; 1024], 2, STEMS_SAMPLE_RATE).unwrap();
            let hash = ContentHash::of_file(&path).unwrap();
            let mut separator = CachedSeparator::new(
                StemCache::new(directory.join("cache"), DEFAULT_STEM_CACHE_LIMIT_BYTES),
                directory.join("unused-weights"),
            );
            separator.separator = Some(synthetic_separator());
            let result = separator.stems(
                &path,
                &ReplaceSource {
                    stage,
                    path: path.clone(),
                },
            );
            assert!(result.is_err(), "source changed during {stage:?}");
            assert!(separator.cache.lookup(&hash).is_none());
            let current_hash = ContentHash::of_file(&path).unwrap();
            assert!(separator.cache.lookup(&current_hash).is_none());
            let files = separator.stems(&path, &Unobserved).unwrap();
            assert_eq!(separator.cache.lookup(&current_hash), Some(files));
        }
    }
}
