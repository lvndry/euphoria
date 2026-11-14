// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measuring how the stems of a separated track sound, for the mashup
//! finder: each stem's share of the energy, how much of the track it sounds
//! in, and which octaves its energy lies in.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use super::{STEMS_SAMPLE_RATE, Stem, StemFiles};
use crate::error::EuphoriaError;
use crate::harmony::{STEM_BAND_EDGES_HZ, STEM_BANDS, StemPresence, StemProfile};

/// The stretch of the track each measurement covers, in frames: a second.
const WINDOW_FRAMES: usize = STEMS_SAMPLE_RATE as usize;

/// Frames of each window whose spectrum is measured.
const SPECTRUM_FRAMES: usize = 4_096;

/// A stem sounds in a window when its level is at least this share of the
/// level of all the stems together, 12 dB below...
const ACTIVE_LEVEL_RATIO: f32 = 0.25;

/// ...and louder than this, 50 dB below full scale.
const SILENCE_LEVEL: f32 = 0.003_16;

/// Measures the stems in `files`, reading each file once.
pub fn profile_stems(files: &StemFiles) -> Result<StemProfile, EuphoriaError> {
    let mut readers = Vec::with_capacity(Stem::ALL.len());
    for stem in Stem::ALL {
        readers.push(StemWav::open(&files.path(stem))?);
    }
    let mut measure = Measure::new();
    let mut windows: Vec<Vec<Vec<f32>>> = vec![Vec::new(); Stem::ALL.len()];
    loop {
        let mut any = false;
        for (reader, window) in readers.iter_mut().zip(&mut windows) {
            reader.read_channels(window, WINDOW_FRAMES)?;
            any |= window.iter().any(|channel| !channel.is_empty());
        }
        if !any {
            break;
        }
        measure.add_window(&windows);
    }
    Ok(measure.profile())
}

/// Where each stem sounds in a track, in windows of a second.
#[derive(Debug, Clone, PartialEq)]
pub struct StemActivity {
    active: [Vec<bool>; 4],
}

impl StemActivity {
    /// How long each window is.
    pub const WINDOW_SECONDS: f32 = 1.0;

    /// How many windows the track has.
    pub fn windows(&self) -> usize {
        self.active[0].len()
    }

    /// Whether `stem` sounds in each window, from the start of the track.
    pub fn windows_of(&self, stem: Stem) -> &[bool] {
        &self.active[stem as usize]
    }

    /// Whether `stem` sounds `seconds` into the track, or `None` past its
    /// end.
    pub fn is_active_at(&self, stem: Stem, seconds: f64) -> Option<bool> {
        if seconds < 0.0 {
            return None;
        }
        let window = (seconds / f64::from(Self::WINDOW_SECONDS)) as usize;
        self.active[stem as usize].get(window).copied()
    }
}

/// Finds where each stem in `files` sounds, window by window, by the rule
/// [`profile_stems`] uses: a stem sounds in a window when it is louder than
/// 50 dB below full scale and within 12 dB of the level of all four stems
/// together.
pub fn stem_activity(files: &StemFiles) -> Result<StemActivity, EuphoriaError> {
    let mut readers = Vec::with_capacity(Stem::ALL.len());
    for stem in Stem::ALL {
        readers.push(StemWav::open(&files.path(stem))?);
    }
    let mut windows: Vec<Vec<Vec<f32>>> = vec![Vec::new(); Stem::ALL.len()];
    let mut active: [Vec<bool>; 4] = Default::default();
    loop {
        let mut any = false;
        for (reader, window) in readers.iter_mut().zip(&mut windows) {
            reader.read_channels(window, WINDOW_FRAMES)?;
            any |= window.iter().any(|channel| !channel.is_empty());
        }
        if !any {
            break;
        }
        let length = windows
            .iter()
            .flat_map(|channels| channels.iter().map(Vec::len))
            .max()
            .unwrap_or(0);
        let energies: Vec<f64> = windows
            .iter()
            .map(|channels| {
                let total: f64 = channels
                    .iter()
                    .flat_map(|channel| channel.iter())
                    .map(|sample| f64::from(sample * sample))
                    .sum();
                total / channels.len().max(1) as f64
            })
            .collect();
        let level = |energy: f64| (energy / length.max(1) as f64).sqrt() as f32;
        let all_level = level(energies.iter().sum());
        for (stem, energy) in energies.iter().enumerate() {
            let stem_level = level(*energy);
            active[stem]
                .push(stem_level >= SILENCE_LEVEL && stem_level >= all_level * ACTIVE_LEVEL_RATIO);
        }
    }
    Ok(StemActivity { active })
}

struct StemWav {
    path: std::path::PathBuf,
    reader: hound::WavReader<BufReader<File>>,
    channels: usize,
}

impl StemWav {
    fn open(path: &Path) -> Result<Self, EuphoriaError> {
        let reader = hound::WavReader::open(path).map_err(|error| wav_error(path, error))?;
        let spec = reader.spec();
        if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
            return Err(wav_error(path, hound::Error::Unsupported));
        }
        if spec.sample_rate != STEMS_SAMPLE_RATE {
            return Err(EuphoriaError::StemFiles {
                path: path.to_path_buf(),
                source: std::io::Error::other(format!(
                    "the stem is at {} Hz, not the {STEMS_SAMPLE_RATE} Hz stems are kept at",
                    spec.sample_rate
                )),
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            channels: usize::from(spec.channels.max(1)),
            reader,
        })
    }

    /// Reads up to `frames` frames into `channels`, one buffer per
    /// channel, so that channels in opposite phase do not cancel.
    fn read_channels(
        &mut self,
        channels: &mut Vec<Vec<f32>>,
        frames: usize,
    ) -> Result<(), EuphoriaError> {
        let count = self.channels;
        channels.resize_with(count, Vec::new);
        channels.iter_mut().for_each(Vec::clear);
        let mut samples = self.reader.samples::<i16>();
        'frames: for _ in 0..frames {
            for channel in channels.iter_mut() {
                match samples.next() {
                    Some(sample) => channel.push(
                        f32::from(sample.map_err(|error| wav_error(&self.path, error))?) / 32_768.0,
                    ),
                    None => break 'frames,
                }
            }
        }
        let complete = channels.iter().map(Vec::len).min().unwrap_or(0);
        channels
            .iter_mut()
            .for_each(|channel| channel.truncate(complete));
        Ok(())
    }
}

fn wav_error(path: &Path, error: hound::Error) -> EuphoriaError {
    EuphoriaError::StemFiles {
        path: path.to_path_buf(),
        source: std::io::Error::other(error.to_string()),
    }
}

/// The running sums the profile is made from.
struct Measure {
    fft: Arc<dyn Fft<f32>>,
    hann: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    band_of_bin: Vec<Option<usize>>,
    energy: [f64; 4],
    active_windows: [usize; 4],
    windows: usize,
    bands: [[f64; STEM_BANDS]; 4],
}

impl Measure {
    fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(SPECTRUM_FRAMES);
        let hann = (0..SPECTRUM_FRAMES)
            .map(|index| {
                let phase = std::f32::consts::TAU * index as f32 / SPECTRUM_FRAMES as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        let bin_hertz = STEMS_SAMPLE_RATE as f32 / SPECTRUM_FRAMES as f32;
        let band_of_bin = (0..SPECTRUM_FRAMES / 2)
            .map(|bin| {
                let hertz = bin as f32 * bin_hertz;
                STEM_BAND_EDGES_HZ
                    .windows(2)
                    .position(|edges| hertz >= edges[0] && hertz < edges[1])
            })
            .collect();
        Self {
            fft,
            hann,
            spectrum: vec![Complex::default(); SPECTRUM_FRAMES],
            band_of_bin,
            energy: [0.0; 4],
            active_windows: [0; 4],
            windows: 0,
            bands: [[0.0; STEM_BANDS]; 4],
        }
    }

    /// Adds one window of every stem, each as one buffer per channel.
    /// Levels and spectra are summed over the channels as powers.
    fn add_window(&mut self, windows: &[Vec<Vec<f32>>]) {
        let length = windows
            .iter()
            .flat_map(|channels| channels.iter().map(Vec::len))
            .max()
            .unwrap_or(0);
        if length == 0 {
            return;
        }
        self.windows += 1;
        let energies: Vec<f64> = windows
            .iter()
            .map(|channels| {
                let total: f64 = channels
                    .iter()
                    .flat_map(|channel| channel.iter())
                    .map(|sample| f64::from(sample * sample))
                    .sum();
                total / channels.len().max(1) as f64
            })
            .collect();
        let all: f64 = energies.iter().sum();
        let level = |energy: f64| (energy / length as f64).sqrt() as f32;
        let all_level = level(all);
        for (stem, channels) in windows.iter().enumerate() {
            self.energy[stem] += energies[stem];
            let stem_level = level(energies[stem]);
            if stem_level < SILENCE_LEVEL || stem_level < all_level * ACTIVE_LEVEL_RATIO {
                continue;
            }
            self.active_windows[stem] += 1;
            for channel in channels {
                self.add_spectrum(stem, channel);
            }
        }
    }

    /// Adds the octave energies of the middle of `window`, one channel, to
    /// `stem`'s.
    fn add_spectrum(&mut self, stem: usize, window: &[f32]) {
        let start = window.len().saturating_sub(SPECTRUM_FRAMES) / 2;
        for (index, value) in self.spectrum.iter_mut().enumerate() {
            let sample = window.get(start + index).copied().unwrap_or(0.0);
            *value = Complex::new(sample * self.hann[index], 0.0);
        }
        self.fft.process(&mut self.spectrum);
        for (bin, band) in self.band_of_bin.iter().enumerate() {
            if let Some(band) = band {
                self.bands[stem][*band] += f64::from(self.spectrum[bin].norm_sqr());
            }
        }
    }

    fn profile(&self) -> StemProfile {
        let total: f64 = self.energy.iter().sum();
        let presence = |stem: Stem| {
            let index = stem as usize;
            let band_total: f64 = self.bands[index].iter().sum();
            let mut bands = [0.0; STEM_BANDS];
            if band_total > 0.0 {
                for (share, energy) in bands.iter_mut().zip(&self.bands[index]) {
                    *share = (energy / band_total) as f32;
                }
            }
            StemPresence {
                share: if total > 0.0 {
                    (self.energy[index] / total) as f32
                } else {
                    0.0
                },
                active_fraction: if self.windows > 0 {
                    self.active_windows[index] as f32 / self.windows as f32
                } else {
                    0.0
                },
                bands,
            }
        };
        StemProfile {
            drums: presence(Stem::Drums),
            bass: presence(Stem::Bass),
            other: presence(Stem::Other),
            vocals: presence(Stem::Vocals),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stems::wav::write_wav;

    /// Writes four stems of `seconds` seconds: a tone at each given
    /// frequency, or silence for `None`, sounding over the given span of
    /// the track as fractions.
    fn write_stems(
        directory: &Path,
        seconds: usize,
        tones: [(Option<f32>, (f32, f32)); 4],
    ) -> StemFiles {
        let files = StemFiles::in_directory(directory.to_path_buf());
        let frames = seconds * STEMS_SAMPLE_RATE as usize;
        for (stem, (tone, (from, to))) in Stem::ALL.iter().zip(tones) {
            let mut interleaved = vec![0.0; frames * 2];
            if let Some(hertz) = tone {
                for frame in 0..frames {
                    let along = frame as f32 / frames as f32;
                    if along < from || along >= to {
                        continue;
                    }
                    let time = frame as f32 / STEMS_SAMPLE_RATE as f32;
                    let sample = 0.3 * (std::f32::consts::TAU * hertz * time).sin();
                    interleaved[frame * 2] = sample;
                    interleaved[frame * 2 + 1] = sample;
                }
            }
            write_wav(&files.path(*stem), &interleaved, 2, STEMS_SAMPLE_RATE).unwrap();
        }
        files
    }

    fn strongest_band(presence: &StemPresence) -> usize {
        presence
            .bands
            .iter()
            .enumerate()
            .max_by(|first, second| first.1.total_cmp(second.1))
            .map(|(band, _)| band)
            .unwrap()
    }

    #[test]
    fn activity_follows_where_each_stem_sounds_second_by_second() {
        let directory = tempfile::tempdir().unwrap();
        let files = write_stems(
            directory.path(),
            10,
            [
                (Some(120.0), (0.0, 1.0)),
                (None, (0.0, 1.0)),
                (Some(500.0), (0.0, 0.5)),
                (Some(2_000.0), (0.5, 1.0)),
            ],
        );
        let activity = stem_activity(&files).unwrap();
        assert_eq!(activity.windows(), 10);
        assert!(
            activity
                .windows_of(Stem::Drums)
                .iter()
                .all(|active| *active)
        );
        assert!(
            activity
                .windows_of(Stem::Bass)
                .iter()
                .all(|active| !*active)
        );
        assert_eq!(activity.is_active_at(Stem::Other, 2.5), Some(true));
        assert_eq!(activity.is_active_at(Stem::Other, 7.5), Some(false));
        assert_eq!(activity.is_active_at(Stem::Vocals, 2.5), Some(false));
        assert_eq!(activity.is_active_at(Stem::Vocals, 7.5), Some(true));
        assert_eq!(activity.is_active_at(Stem::Vocals, 10.5), None);
        assert_eq!(activity.is_active_at(Stem::Vocals, -1.0), None);
    }

    #[test]
    fn each_stem_is_placed_in_its_octave_and_its_span() {
        let directory = tempfile::tempdir().unwrap();
        let files = write_stems(
            directory.path(),
            10,
            [
                (Some(120.0), (0.0, 1.0)),
                (Some(60.0), (0.0, 1.0)),
                (Some(500.0), (0.0, 1.0)),
                (Some(2_000.0), (0.5, 1.0)),
            ],
        );
        let profile = profile_stems(&files).unwrap();
        assert_eq!(strongest_band(&profile.drums), 1);
        assert_eq!(strongest_band(&profile.bass), 0);
        assert_eq!(strongest_band(&profile.other), 3);
        assert_eq!(strongest_band(&profile.vocals), 5);
        assert!((profile.other.active_fraction - 1.0).abs() < 1e-6);
        assert!(
            (profile.vocals.active_fraction - 0.5).abs() < 0.11,
            "{}",
            profile.vocals.active_fraction
        );
        let shares =
            profile.drums.share + profile.bass.share + profile.other.share + profile.vocals.share;
        assert!((shares - 1.0).abs() < 1e-4);
        assert!(profile.has_vocals());
        assert!(profile.has_full_instrumental());
        for presence in [profile.drums, profile.bass, profile.other, profile.vocals] {
            assert!((presence.bands.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn a_silent_vocal_stem_means_an_instrumental() {
        let directory = tempfile::tempdir().unwrap();
        let files = write_stems(
            directory.path(),
            4,
            [
                (Some(120.0), (0.0, 1.0)),
                (Some(60.0), (0.0, 1.0)),
                (Some(500.0), (0.0, 1.0)),
                (None, (0.0, 1.0)),
            ],
        );
        let profile = profile_stems(&files).unwrap();
        assert_eq!(profile.vocals.active_fraction, 0.0);
        assert_eq!(profile.vocals.share, 0.0);
        assert_eq!(profile.vocals.bands, [0.0; STEM_BANDS]);
        assert!(!profile.has_vocals());
    }

    #[test]
    fn a_stem_in_opposite_phase_left_and_right_still_counts() {
        let directory = tempfile::tempdir().unwrap();
        let files = write_stems(
            directory.path(),
            4,
            [
                (Some(120.0), (0.0, 1.0)),
                (Some(60.0), (0.0, 1.0)),
                (Some(500.0), (0.0, 1.0)),
                (None, (0.0, 1.0)),
            ],
        );
        let frames = 4 * STEMS_SAMPLE_RATE as usize;
        let mut interleaved = vec![0.0; frames * 2];
        for frame in 0..frames {
            let time = frame as f32 / STEMS_SAMPLE_RATE as f32;
            let sample = 0.3 * (std::f32::consts::TAU * 2_000.0 * time).sin();
            interleaved[frame * 2] = sample;
            interleaved[frame * 2 + 1] = -sample;
        }
        write_wav(
            &files.path(Stem::Vocals),
            &interleaved,
            2,
            STEMS_SAMPLE_RATE,
        )
        .unwrap();
        let profile = profile_stems(&files).unwrap();
        assert!((profile.vocals.active_fraction - 1.0).abs() < 1e-6);
        assert_eq!(strongest_band(&profile.vocals), 5);
        assert!(profile.vocals.share > 0.2, "{}", profile.vocals.share);
    }

    #[test]
    fn a_stem_at_another_rate_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let files = write_stems(
            directory.path(),
            1,
            [
                (Some(120.0), (0.0, 1.0)),
                (Some(60.0), (0.0, 1.0)),
                (Some(500.0), (0.0, 1.0)),
                (None, (0.0, 1.0)),
            ],
        );
        write_wav(&files.path(Stem::Other), &[0.0; 96_000], 2, 48_000).unwrap();
        let refused = profile_stems(&files).unwrap_err();
        assert!(refused.chain().contains("48000 Hz"), "{}", refused.chain());
    }

    #[test]
    fn a_missing_stem_file_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let files = StemFiles::in_directory(directory.path().to_path_buf());
        assert!(profile_stems(&files).is_err());
    }
}
