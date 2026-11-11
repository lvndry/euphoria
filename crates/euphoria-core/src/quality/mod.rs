// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How good a file sounds for DJ use: what it is encoded with, where its
//! content really stops, how loud and how clipped it is, and a plain
//! verdict. `docs/concepts/audio-quality.md` describes the method and
//! `bench/results/quality.md` the measurements the thresholds come from.

mod cutoff;

use std::path::Path;

use ebur128::{EbuR128, Mode};
use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_ADPCM_G722, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MONKEYS_AUDIO, CODEC_ID_PCM_S32LE,
    CODEC_ID_TTA, CODEC_ID_WAVPACK,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters};

use crate::audio::{AudioBuffer, DecodeObserver, decode_mono_observed};
use crate::error::EuphoriaError;
pub(crate) use cutoff::spectral_cutoff;

/// A sample at least this close to full scale counts towards clipping.
const FULL_SCALE: f32 = 0.999;
/// Clipping is a run of at least this many consecutive samples at full
/// scale in one channel. A single sample at full scale is a peak, not a
/// flattened waveform.
const SHORTEST_CLIPPED_RUN: u32 = 3;

/// A lossy file at or above this average bit rate, in kbit/s, with a full
/// spectrum, is good for any system.
pub const GOOD_BITRATE_KBPS: u32 = 224;
/// A lossy file at or above this average bit rate, in kbit/s, and below
/// [`GOOD_BITRATE_KBPS`], is acceptable.
pub const ACCEPTABLE_BITRATE_KBPS: u32 = 160;
/// A lossless file whose content stops below this frequency, in Hz, was made
/// from a lossy source. Lossy encoders at 320 kbit/s keep content up to
/// about 20 kHz.
pub const LOSSLESS_LOWEST_CUTOFF_HZ: f32 = 19_000.0;

/// The verdict for DJ use, from best to worst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityVerdict {
    /// A lossless file with a full spectrum.
    Lossless,
    /// High bit rate lossy audio with the spectrum that bit rate keeps.
    Good,
    /// Medium bit rate lossy audio. Fine in most rooms.
    Acceptable,
    /// Low bit rate lossy audio. The top end and the detail suffer on a
    /// large sound system.
    WeakOnABigSystem,
    /// The file's bit rate promises more than its content holds: the sound
    /// stops where a lower quality file's would, so it was most likely
    /// made from one.
    LikelyReencoded,
}

impl QualityVerdict {
    /// A short name for a table column.
    pub fn label(self) -> &'static str {
        match self {
            QualityVerdict::Lossless => "Lossless",
            QualityVerdict::Good => "Good",
            QualityVerdict::Acceptable => "Acceptable",
            QualityVerdict::WeakOnABigSystem => "Weak on a big system",
            QualityVerdict::LikelyReencoded => "Likely re-encoded",
        }
    }
}

/// What Euphoria found out about how a file is encoded and how it sounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityReport {
    /// The codec, such as `mp3`, `aac`, `flac`, `alac` or `pcm_s16le`.
    pub codec: String,
    /// Whether the codec keeps every bit of the audio.
    pub lossless: bool,
    /// The average bit rate of the audio data in kbit/s, measured from the
    /// size of every audio packet and the length of the audio. Tags and
    /// artwork are not counted.
    pub bitrate_kbps: Option<u32>,
    /// Samples per second.
    pub sample_rate: u32,
    /// Bits per sample of a lossless file. Lossy codecs have none.
    pub bit_depth: Option<u32>,
    /// The number of channels.
    pub channels: Option<u32>,
    /// The frequency in Hz where the content stops, or `None` when it runs
    /// to the top of the spectrum. A lossy encoder removes everything above
    /// a frequency that depends on its bit rate.
    pub cutoff_hz: Option<f32>,
    /// The share of all samples, over every channel, that lie in runs of at
    /// least three consecutive samples at full scale.
    pub clipped_share: f32,
    /// Integrated loudness in LUFS, measured as EBU R128 describes over
    /// every channel.
    pub integrated_loudness_lufs: Option<f32>,
    /// The highest true peak of any channel, in dB relative to full scale.
    pub true_peak_dbtp: Option<f32>,
    /// The verdict for DJ use.
    pub verdict: QualityVerdict,
}

impl QualityReport {
    /// One sentence that explains the verdict.
    pub fn reason(&self) -> String {
        let bitrate = self.bitrate_kbps.map_or_else(
            || "an unknown bit rate".to_string(),
            |rate| format!("{rate} kbit/s"),
        );
        let cutoff = |hz: f32| format!("{:.1} kHz", hz / 1000.0);
        match (self.verdict, self.cutoff_hz) {
            (QualityVerdict::Lossless, _) => {
                format!(
                    "Lossless {} with a full spectrum.",
                    self.codec.to_uppercase()
                )
            }
            (QualityVerdict::LikelyReencoded, Some(hz)) if self.lossless => format!(
                "A lossless file, but its sound stops at {}, as a lossy file's does: it was most likely made from one.",
                cutoff(hz)
            ),
            (QualityVerdict::LikelyReencoded, Some(hz)) => format!(
                "{bitrate}, but the sound stops at {}, where a {} kbit/s file's would: it was most likely made from one.",
                cutoff(hz),
                typical_bitrate_for_cutoff(hz)
            ),
            (QualityVerdict::Good, _) => {
                format!("{bitrate} with the full spectrum that bit rate keeps.")
            }
            (QualityVerdict::Acceptable, _) => {
                format!(
                    "{bitrate}. Fine in most rooms; a 320 kbit/s or lossless copy holds more detail."
                )
            }
            (QualityVerdict::WeakOnABigSystem, _) => format!(
                "{bitrate}. Fine for practice and small rooms; on a club system the top end and detail suffer."
            ),
            (QualityVerdict::LikelyReencoded, None) => format!("{bitrate}."),
        }
    }
}

/// The lowest cutoff, in Hz, that a lossy file of `bitrate_kbps` has when it
/// was encoded from a full band source. Measured with LAME, Apple's AAC
/// encoder and FFmpeg's AAC encoder, with a margin; see
/// `bench/results/quality.md`.
pub fn expected_cutoff_hz(bitrate_kbps: u32) -> f32 {
    match bitrate_kbps {
        0..=111 => 14_500.0,
        112..=159 => 15_500.0,
        160..=223 => 17_500.0,
        224..=287 => 18_500.0,
        _ => 19_000.0,
    }
}

/// The bit rate whose files typically stop near `cutoff_hz`, for the
/// explanation of a re-encoded file.
fn typical_bitrate_for_cutoff(cutoff_hz: f32) -> u32 {
    match cutoff_hz {
        hz if hz < 15_000.0 => 96,
        hz if hz < 17_500.0 => 128,
        hz if hz < 18_500.0 => 160,
        _ => 192,
    }
}

/// The verdict for a file with these properties.
pub fn verdict(
    lossless: bool,
    bitrate_kbps: Option<u32>,
    cutoff_hz: Option<f32>,
) -> QualityVerdict {
    if lossless {
        return match cutoff_hz {
            Some(hz) if hz < LOSSLESS_LOWEST_CUTOFF_HZ => QualityVerdict::LikelyReencoded,
            _ => QualityVerdict::Lossless,
        };
    }
    let Some(bitrate) = bitrate_kbps else {
        return QualityVerdict::Acceptable;
    };
    if bitrate < ACCEPTABLE_BITRATE_KBPS {
        return QualityVerdict::WeakOnABigSystem;
    }
    if cutoff_hz.is_some_and(|hz| hz < expected_cutoff_hz(bitrate)) {
        return QualityVerdict::LikelyReencoded;
    }
    if bitrate >= GOOD_BITRATE_KBPS {
        QualityVerdict::Good
    } else {
        QualityVerdict::Acceptable
    }
}

/// Decodes the file at `path` and reports its quality.
pub fn analyze_quality(path: &Path) -> Result<QualityReport, EuphoriaError> {
    decode_with_quality(path).map(|(_, report)| report)
}

/// Decodes the file at `path` once, returning the mono mixdown that the
/// other analyses read and the quality report measured on the way.
pub fn decode_with_quality(path: &Path) -> Result<(AudioBuffer, QualityReport), EuphoriaError> {
    let mut probe = QualityProbe::default();
    let audio = decode_mono_observed(path, &mut probe)?;
    let report = probe.finish(&audio);
    Ok((audio, report))
}

/// Collects what the quality report needs while a file is decoded.
#[derive(Default)]
struct QualityProbe {
    codec: String,
    lossless: bool,
    bit_depth: Option<u32>,
    channels: Option<u32>,
    payload_bytes: u64,
    clipping: ClippingCounter,
    meter: Option<EbuR128>,
    /// Set when the loudness meter could not be made or fed, so that a
    /// partial measurement is not reported as a whole one.
    meter_failed: bool,
}

impl DecodeObserver for QualityProbe {
    fn stream(&mut self, parameters: &AudioCodecParameters) {
        let id = parameters.codec;
        self.codec = symphonia::default::get_codecs()
            .get_audio_decoder(id)
            .map_or_else(
                || "unknown".to_string(),
                |decoder| decoder.codec.info.short_name.to_string(),
            );
        self.lossless = is_lossless(id);
        self.bit_depth = parameters.bits_per_sample.filter(|_| self.lossless);
        self.channels = parameters
            .channels
            .as_ref()
            .and_then(|channels| u32::try_from(channels.count()).ok());
    }

    fn packet(&mut self, bytes: usize) {
        self.payload_bytes += bytes as u64;
    }

    fn samples(&mut self, interleaved: &[f32], channel_count: usize, sample_rate: u32) {
        self.clipping.add(interleaved, channel_count);
        if self.meter_failed {
            return;
        }
        if self.meter.is_none() {
            let channels = u32::try_from(channel_count).unwrap_or(u32::MAX);
            match EbuR128::new(channels, sample_rate, Mode::I | Mode::TRUE_PEAK) {
                Ok(meter) => self.meter = Some(meter),
                Err(_) => {
                    self.meter_failed = true;
                    return;
                }
            }
        }
        let fed = self
            .meter
            .as_mut()
            .is_some_and(|meter| meter.add_frames_f32(interleaved).is_ok());
        if !fed {
            self.meter_failed = true;
        }
    }
}

impl QualityProbe {
    fn finish(self, audio: &AudioBuffer) -> QualityReport {
        let duration = f64::from(audio.duration_seconds());
        let bitrate_kbps = (duration > 0.0 && self.payload_bytes > 0)
            .then(|| (self.payload_bytes as f64 * 8.0 / duration / 1000.0).round() as u32);
        let cutoff_hz = spectral_cutoff(audio.samples(), audio.sample_rate());
        let (integrated_loudness_lufs, true_peak_dbtp) = match (&self.meter, self.meter_failed) {
            (Some(meter), false) => (integrated_loudness(meter), true_peak(meter, self.channels)),
            _ => (None, None),
        };
        QualityReport {
            verdict: verdict(self.lossless, bitrate_kbps, cutoff_hz),
            codec: self.codec,
            lossless: self.lossless,
            bitrate_kbps,
            sample_rate: audio.sample_rate().get(),
            bit_depth: self.bit_depth,
            channels: self.channels,
            cutoff_hz,
            clipped_share: self.clipping.share(),
            integrated_loudness_lufs,
            true_peak_dbtp,
        }
    }
}

fn integrated_loudness(meter: &EbuR128) -> Option<f32> {
    meter
        .loudness_global()
        .ok()
        .filter(|lufs| lufs.is_finite())
        .map(|lufs| lufs as f32)
}

fn true_peak(meter: &EbuR128, channels: Option<u32>) -> Option<f32> {
    let highest = (0..channels.unwrap_or(1))
        .filter_map(|channel| meter.true_peak(channel).ok())
        .fold(0.0_f64, f64::max);
    (highest > 0.0).then(|| (20.0 * highest.log10()) as f32)
}

/// PCM codecs, which Symphonia numbers from the first PCM codec up to the
/// first ADPCM codec, and the lossless compressed codecs keep every bit.
fn is_lossless(codec: AudioCodecId) -> bool {
    (CODEC_ID_PCM_S32LE..CODEC_ID_ADPCM_G722).contains(&codec)
        || [
            CODEC_ID_FLAC,
            CODEC_ID_ALAC,
            CODEC_ID_WAVPACK,
            CODEC_ID_MONKEYS_AUDIO,
            CODEC_ID_TTA,
        ]
        .contains(&codec)
}

/// Counts samples in runs at full scale, one run per channel, across the
/// chunks a decoder hands out.
#[derive(Default)]
struct ClippingCounter {
    runs: Vec<u32>,
    clipped: u64,
    total: u64,
}

impl ClippingCounter {
    fn add(&mut self, interleaved: &[f32], channel_count: usize) {
        let channel_count = channel_count.max(1);
        if self.runs.len() != channel_count {
            self.runs = vec![0; channel_count];
        }
        for (index, sample) in interleaved.iter().enumerate() {
            let run = &mut self.runs[index % channel_count];
            if sample.abs() >= FULL_SCALE {
                *run += 1;
                if *run == SHORTEST_CLIPPED_RUN {
                    self.clipped += u64::from(SHORTEST_CLIPPED_RUN);
                } else if *run > SHORTEST_CLIPPED_RUN {
                    self.clipped += 1;
                }
            } else {
                *run = 0;
            }
        }
        self.total += interleaved.len() as u64;
    }

    fn share(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.clipped as f64 / self.total as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_files_are_lossless_unless_their_sound_stops_early() {
        assert_eq!(verdict(true, Some(900), None), QualityVerdict::Lossless);
        assert_eq!(
            verdict(true, Some(900), Some(20_500.0)),
            QualityVerdict::Lossless
        );
        assert_eq!(
            verdict(true, Some(900), Some(16_000.0)),
            QualityVerdict::LikelyReencoded
        );
    }

    #[test]
    fn lossy_files_are_judged_by_bit_rate_when_their_spectrum_matches_it() {
        assert_eq!(
            verdict(false, Some(320), Some(20_000.0)),
            QualityVerdict::Good
        );
        assert_eq!(verdict(false, Some(256), None), QualityVerdict::Good);
        assert_eq!(
            verdict(false, Some(192), Some(18_800.0)),
            QualityVerdict::Acceptable
        );
        assert_eq!(
            verdict(false, Some(128), Some(16_500.0)),
            QualityVerdict::WeakOnABigSystem
        );
        assert_eq!(
            verdict(false, Some(96), Some(15_500.0)),
            QualityVerdict::WeakOnABigSystem
        );
    }

    #[test]
    fn a_high_bit_rate_with_an_early_cutoff_is_likely_re_encoded() {
        assert_eq!(
            verdict(false, Some(320), Some(16_500.0)),
            QualityVerdict::LikelyReencoded
        );
        assert_eq!(
            verdict(false, Some(256), Some(17_000.0)),
            QualityVerdict::LikelyReencoded
        );
        assert_eq!(
            verdict(false, Some(192), Some(16_000.0)),
            QualityVerdict::LikelyReencoded
        );
    }

    #[test]
    fn a_low_bit_rate_is_weak_whatever_its_cutoff() {
        assert_eq!(
            verdict(false, Some(128), Some(11_000.0)),
            QualityVerdict::WeakOnABigSystem
        );
    }

    #[test]
    fn the_expected_cutoff_never_falls_as_the_bit_rate_rises() {
        let mut previous = 0.0;
        for bitrate in (32..=400).step_by(8) {
            let expected = expected_cutoff_hz(bitrate);
            assert!(expected >= previous, "{bitrate}");
            previous = expected;
        }
    }

    #[test]
    fn clipping_counts_only_runs_of_three_or_more_per_channel() {
        let mut counter = ClippingCounter::default();
        // Left: one peak, then a run of four. Right: a run of two.
        let left = [1.0, 0.5, 1.0, 1.0, 1.0, -1.0, 0.2, 0.0];
        let right = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 1.0];
        let interleaved: Vec<f32> = left
            .iter()
            .zip(&right)
            .flat_map(|(left, right)| [*left, *right])
            .collect();
        counter.add(&interleaved[..6], 2);
        counter.add(&interleaved[6..], 2);
        assert_eq!(counter.clipped, 4);
        assert_eq!(counter.total, 16);
        assert!((counter.share() - 0.25).abs() < 1.0e-6);
    }

    #[test]
    fn codec_ids_tell_lossless_from_lossy() {
        use symphonia::core::codecs::audio::well_known::{
            CODEC_ID_AAC, CODEC_ID_ADPCM_IMA_WAV, CODEC_ID_MP3, CODEC_ID_PCM_F32LE,
            CODEC_ID_PCM_S16LE, CODEC_ID_VORBIS,
        };
        for lossless in [
            CODEC_ID_PCM_S16LE,
            CODEC_ID_PCM_F32LE,
            CODEC_ID_FLAC,
            CODEC_ID_ALAC,
        ] {
            assert!(is_lossless(lossless), "{lossless}");
        }
        for lossy in [
            CODEC_ID_MP3,
            CODEC_ID_AAC,
            CODEC_ID_VORBIS,
            CODEC_ID_ADPCM_IMA_WAV,
        ] {
            assert!(!is_lossless(lossy), "{lossy}");
        }
    }

    #[test]
    fn every_verdict_has_a_reason() {
        let mut report = QualityReport {
            codec: "mp3".to_string(),
            lossless: false,
            bitrate_kbps: Some(320),
            sample_rate: 44_100,
            bit_depth: None,
            channels: Some(2),
            cutoff_hz: Some(16_400.0),
            clipped_share: 0.0,
            integrated_loudness_lufs: Some(-8.0),
            true_peak_dbtp: Some(0.3),
            verdict: QualityVerdict::LikelyReencoded,
        };
        assert_eq!(
            report.reason(),
            "320 kbit/s, but the sound stops at 16.4 kHz, where a 128 kbit/s file's would: it was most likely made from one."
        );
        for verdict in [
            QualityVerdict::Lossless,
            QualityVerdict::Good,
            QualityVerdict::Acceptable,
            QualityVerdict::WeakOnABigSystem,
        ] {
            report.verdict = verdict;
            assert!(!report.reason().is_empty());
        }
    }
}
