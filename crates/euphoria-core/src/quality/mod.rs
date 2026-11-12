// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How good a file sounds for DJ use: what it is encoded with, where its
//! content really stops, how loud and how clipped it is, and a plain
//! verdict. `docs/concepts/audio-quality.md` describes the method and
//! `bench/results/quality.md` the measurements the thresholds come from.

mod cutoff;
mod peak;

use std::path::Path;

use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_ADPCM_G722, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MONKEYS_AUDIO, CODEC_ID_PCM_S32LE,
    CODEC_ID_TTA, CODEC_ID_VORBIS, CODEC_ID_WAVPACK,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters};

use crate::audio::{AudioBuffer, DecodeObserver, decode_mono_observed};
use crate::energy::measure_loudness;
use crate::error::EuphoriaError;
use cutoff::SpectralCutoffMeter;
use peak::TruePeakMeter;

/// A sample at least this close to full scale counts towards clipping.
const FULL_SCALE: f32 = 0.999;
/// Clipping is a run of at least this many consecutive samples at full
/// scale in one channel. A single sample at full scale is a peak, not a
/// flattened waveform.
const SHORTEST_CLIPPED_RUN: u32 = 3;

/// A lossy file at or above this average bit rate, in kbit/s, with the
/// spectrum its bit rate keeps, is good for any system. Apple's AAC encoder
/// set to 256 kbit/s averaged 208 kbit/s on the quietest source measured, so
/// the threshold sits below the nominal rate.
pub const GOOD_BITRATE_KBPS: u32 = 200;
/// A lossy file at or above this average bit rate, in kbit/s, and below
/// [`GOOD_BITRATE_KBPS`], is acceptable. Encoders set to 128 kbit/s measured
/// at most 129, and set to 192 kbit/s at least 155.
pub const ACCEPTABLE_BITRATE_KBPS: u32 = 144;
/// A Vorbis file at or above this average bit rate, in kbit/s, with the
/// spectrum its bit rate keeps, is good for any system. Vorbis is encoded
/// at a quality level and averages well below the level's nominal rate:
/// the reference encoder at quality 8, nominally 256 kbit/s, averaged 148
/// to 221 kbit/s on music, and at quality 6, nominally 192, 116 to 162.
pub const VORBIS_GOOD_BITRATE_KBPS: u32 = 145;
/// A Vorbis file at or above this average bit rate, in kbit/s, and below
/// [`VORBIS_GOOD_BITRATE_KBPS`], is acceptable. Quality 6 measured at least
/// 116 kbit/s, quality 4, nominally 128, at most 104.
pub const VORBIS_ACCEPTABLE_BITRATE_KBPS: u32 = 110;
/// A file with at least this share of its samples clipped gets a note in
/// its reason. Lossy encoding hides most clipping, so this is a lower bound.
pub const NOTICEABLE_CLIPPED_SHARE: f32 = 0.001;
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
    /// a frequency that depends on its bit rate. Channel powers are combined
    /// before any mono mixdown, so opposite phases cannot cancel content.
    pub cutoff_hz: Option<f32>,
    /// The share of all samples, over every channel, that lie in runs of at
    /// least three consecutive samples at full scale.
    pub clipped_share: f32,
    /// Integrated loudness in LUFS, measured as EBU R128 describes on the
    /// mono mixdown played on both speakers, as the energy rating measures
    /// it.
    pub integrated_loudness_lufs: Option<f32>,
    /// The highest true peak of any channel, in dB relative to full scale,
    /// estimated by interpolating four times between samples.
    pub true_peak_dbtp: Option<f32>,
    /// The verdict for DJ use.
    pub verdict: QualityVerdict,
}

impl QualityReport {
    /// One or two sentences that explain the verdict, and mention clipping
    /// when there is much of it.
    pub fn reason(&self) -> String {
        let mut reason = self.verdict_reason();
        if self.clipped_share >= NOTICEABLE_CLIPPED_SHARE {
            reason.push_str(" The master is clipped in places.");
        }
        reason
    }

    fn verdict_reason(&self) -> String {
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
        0..=143 => 14_500.0,
        // Apple's encoder at 192 kbit/s cut one source at 16.4 kHz, so in
        // this range only a source of 96 kbit/s or less can be told apart.
        144..=199 => 15_500.0,
        200..=287 => 18_000.0,
        _ => 19_500.0,
    }
}

/// The bit rate whose files typically stop near `cutoff_hz`, for the
/// explanation of a re-encoded file.
fn typical_bitrate_for_cutoff(cutoff_hz: f32) -> u32 {
    match cutoff_hz {
        hz if hz < 14_000.0 => 64,
        hz if hz < 15_800.0 => 96,
        hz if hz < 18_000.0 => 128,
        _ => 192,
    }
}

/// The lowest cutoff, in Hz, that a Vorbis file of `bitrate_kbps` has when
/// it was encoded from a full band source. The reference encoder stopped no
/// lower than 18.9 kHz at quality 6 and 19.6 kHz at quality 8, and mostly
/// kept the whole spectrum; see `bench/results/quality.md`.
pub fn expected_vorbis_cutoff_hz(bitrate_kbps: u32) -> f32 {
    if bitrate_kbps >= VORBIS_GOOD_BITRATE_KBPS {
        18_500.0
    } else {
        18_000.0
    }
}

/// The verdict for a Vorbis file with these properties. Vorbis gets lines of
/// its own because its bit rates run lower than those of MP3 and AAC for the
/// same quality.
pub fn vorbis_verdict(bitrate_kbps: Option<u32>, cutoff_hz: Option<f32>) -> QualityVerdict {
    let Some(bitrate) = bitrate_kbps else {
        return QualityVerdict::Acceptable;
    };
    if bitrate < VORBIS_ACCEPTABLE_BITRATE_KBPS {
        return QualityVerdict::WeakOnABigSystem;
    }
    if cutoff_hz.is_some_and(|hz| hz < expected_vorbis_cutoff_hz(bitrate)) {
        return QualityVerdict::LikelyReencoded;
    }
    if bitrate >= VORBIS_GOOD_BITRATE_KBPS {
        QualityVerdict::Good
    } else {
        QualityVerdict::Acceptable
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
    let (audio, pending) = decode_measuring_quality(path, |_| {})?;
    let loudness = measure_loudness(audio.samples(), audio.sample_rate())?;
    let report = pending.finish(&audio, loudness.map(|loudness| loudness.integrated_lufs));
    Ok((audio, report))
}

/// Decodes the file at `path` and measures everything in the quality report
/// except the loudness, which the caller usually has from another analysis
/// of the same audio.
pub(crate) fn decode_measuring_quality(
    path: &Path,
    on_decoded_share: impl FnMut(f32),
) -> Result<(AudioBuffer, PendingQuality), EuphoriaError> {
    let mut probe = ReportingProbe {
        probe: QualityProbe::default(),
        on_decoded_share,
    };
    let audio = decode_mono_observed(path, &mut probe)?;
    Ok((audio, PendingQuality(probe.probe)))
}

/// A quality probe that also passes on how far decoding has come.
struct ReportingProbe<Report: FnMut(f32)> {
    probe: QualityProbe,
    on_decoded_share: Report,
}

impl<Report: FnMut(f32)> DecodeObserver for ReportingProbe<Report> {
    fn stream(&mut self, parameters: &AudioCodecParameters) {
        self.probe.stream(parameters);
    }

    fn packet(&mut self, bytes: usize) {
        self.probe.packet(bytes);
    }

    fn samples(&mut self, interleaved: &[f32], channel_count: usize, sample_rate: u32) {
        self.probe.samples(interleaved, channel_count, sample_rate);
    }

    fn decoded_share(&mut self, share: f32) {
        (self.on_decoded_share)(share);
    }
}

/// A quality report that waits for the loudness.
pub(crate) struct PendingQuality(QualityProbe);

impl PendingQuality {
    /// The report, with `integrated_loudness_lufs` measured on `audio`.
    pub(crate) fn finish(
        self,
        audio: &AudioBuffer,
        integrated_loudness_lufs: Option<f32>,
    ) -> QualityReport {
        self.0.finish(audio, integrated_loudness_lufs)
    }
}

/// Collects what the quality report needs while a file is decoded.
#[derive(Default)]
struct QualityProbe {
    codec: String,
    is_vorbis: bool,
    lossless: bool,
    bit_depth: Option<u32>,
    channels: Option<u32>,
    payload_bytes: u64,
    clipping: ClippingCounter,
    true_peak: TruePeakMeter,
    cutoff: SpectralCutoffMeter,
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
        self.is_vorbis = id == CODEC_ID_VORBIS;
        let bit_depth = match parameters.bits_per_sample {
            None if id == CODEC_ID_ALAC => alac_bit_depth(parameters.extra_data.as_deref()),
            known => known,
        };
        self.bit_depth = bit_depth.filter(|_| self.lossless);
        self.channels = parameters
            .channels
            .as_ref()
            .and_then(|channels| u32::try_from(channels.count()).ok());
    }

    fn packet(&mut self, bytes: usize) {
        self.payload_bytes += bytes as u64;
    }

    fn samples(&mut self, interleaved: &[f32], channel_count: usize, sample_rate: u32) {
        self.cutoff.add(interleaved, channel_count, sample_rate);
        self.clipping.add(interleaved, channel_count);
        self.true_peak.add(interleaved, channel_count);
    }
}

impl QualityProbe {
    fn finish(self, audio: &AudioBuffer, integrated_loudness_lufs: Option<f32>) -> QualityReport {
        let duration = f64::from(audio.duration_seconds());
        let bitrate_kbps = (duration > 0.0 && self.payload_bytes > 0)
            .then(|| (self.payload_bytes as f64 * 8.0 / duration / 1000.0).round() as u32);
        let cutoff_hz = self.cutoff.finish();
        QualityReport {
            verdict: if self.is_vorbis {
                vorbis_verdict(bitrate_kbps, cutoff_hz)
            } else {
                verdict(self.lossless, bitrate_kbps, cutoff_hz)
            },
            codec: self.codec,
            lossless: self.lossless,
            bitrate_kbps,
            sample_rate: audio.sample_rate().get(),
            bit_depth: self.bit_depth,
            channels: self.channels,
            cutoff_hz,
            clipped_share: self.clipping.share(),
            integrated_loudness_lufs,
            true_peak_dbtp: self.true_peak.decibels(),
        }
    }
}

/// The bit depth an ALAC stream declares in its magic cookie, which may
/// follow a `frma` atom and an `alac` atom header. Symphonia reads the
/// cookie but does not pass the depth on.
fn alac_bit_depth(cookie: Option<&[u8]>) -> Option<u32> {
    let mut cookie = cookie?;
    for atom in [b"frma", b"alac"] {
        if cookie.get(4..8) == Some(&atom[..]) {
            cookie = cookie.get(12..)?;
        }
    }
    const DEPTH_OFFSET: usize = 5;
    const COOKIE_LENGTHS: [usize; 2] = [24, 48];
    COOKIE_LENGTHS
        .contains(&cookie.len())
        .then(|| u32::from(cookie[DEPTH_OFFSET]))
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
    fn vorbis_files_are_judged_by_their_own_bit_rates() {
        // Medians and extremes the reference encoder reached on music; see
        // bench/results/quality.md.
        for (bitrate, cutoff, expected) in [
            (104, Some(18_271.0), QualityVerdict::WeakOnABigSystem),
            (116, Some(18_895.0), QualityVerdict::Acceptable),
            (131, None, QualityVerdict::Acceptable),
            (148, Some(19_628.0), QualityVerdict::Good),
            (201, Some(20_844.0), QualityVerdict::Good),
            // A 128 kbit/s MP3 encoded again as Vorbis at quality 9.
            (228, Some(17_743.0), QualityVerdict::LikelyReencoded),
            (130, Some(16_000.0), QualityVerdict::LikelyReencoded),
        ] {
            assert_eq!(
                vorbis_verdict(Some(bitrate), cutoff),
                expected,
                "{bitrate} kbit/s, cutoff {cutoff:?}"
            );
        }
        assert_eq!(vorbis_verdict(None, None), QualityVerdict::Acceptable);
    }

    #[test]
    fn alac_bit_depth_is_read_from_the_magic_cookie() {
        let mut cookie = vec![0, 0, 16, 0, 0, 24, 40, 10, 14, 2, 0, 255];
        cookie.extend([0; 12]);
        assert_eq!(alac_bit_depth(Some(&cookie)), Some(24));
        let mut wrapped = b"\x00\x00\x00\x0cfrma".to_vec();
        wrapped.extend(b"alac\x00\x00\x00\x24alac\x00\x00\x00\x00");
        wrapped.extend(&cookie);
        assert_eq!(alac_bit_depth(Some(&wrapped)), Some(24));
        assert_eq!(alac_bit_depth(Some(&cookie[..20])), None);
        assert_eq!(alac_bit_depth(None), None);
    }

    /// A full-band source and its 14 kHz lowpass, split with one transform.
    fn split_noise() -> (Vec<f32>, Vec<f32>) {
        use rustfft::{FftPlanner, num_complex::Complex};
        let mut state = 0x2545_f491_u32;
        let noise: Vec<f32> = (0..1 << 17)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.25
            })
            .collect();
        let length = noise.len();
        let mut spectrum: Vec<_> = noise
            .iter()
            .map(|sample| Complex::new(*sample, 0.0))
            .collect();
        let mut planner = FftPlanner::<f32>::new();
        planner.plan_fft_forward(length).process(&mut spectrum);
        for (bin, value) in spectrum.iter_mut().enumerate() {
            if bin.min(length - bin) as f32 * 44_100.0 / length as f32 > 14_000.0 {
                *value = Complex::default();
            }
        }
        planner.plan_fft_inverse(length).process(&mut spectrum);
        let low = spectrum
            .into_iter()
            .map(|value| value.re / length as f32)
            .collect();
        (noise, low)
    }

    fn write_wav(path: &Path, channels: &[&[f32]]) {
        let channel_count = channels.len() as u16;
        let data_bytes = (channels[0].len() * channels.len() * 2) as u32;
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data_bytes).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(channel_count.to_le_bytes());
        bytes.extend(44_100_u32.to_le_bytes());
        bytes.extend((44_100 * u32::from(channel_count) * 2).to_le_bytes());
        bytes.extend((channel_count * 2).to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(data_bytes.to_le_bytes());
        for index in 0..channels[0].len() {
            for channel in channels {
                bytes.extend(((channel[index] * 32_767.0) as i16).to_le_bytes());
            }
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn opposite_phase_highs_do_not_make_a_full_band_file_look_re_encoded() {
        let folder = crate::cache::test_support::TemporaryFolder::new();
        let (full, low) = split_noise();
        let opposite: Vec<f32> = low
            .iter()
            .zip(&full)
            .map(|(low, full)| 2.0 * low - full)
            .collect();
        let lowpass = folder.path().join("lowpass.wav");
        write_wav(&lowpass, &[&low]);
        let report = analyze_quality(&lowpass).unwrap();
        assert_eq!(report.verdict, QualityVerdict::LikelyReencoded);
        assert!((report.cutoff_hz.unwrap() - 14_000.0).abs() < 300.0);
        for (name, channels) in [
            ("full.wav", vec![full.as_slice()]),
            ("same-phase.wav", vec![full.as_slice(), full.as_slice()]),
            (
                "opposed-highs.wav",
                vec![full.as_slice(), opposite.as_slice()],
            ),
        ] {
            let path = folder.path().join(name);
            write_wav(&path, &channels);
            let report = analyze_quality(&path).unwrap();
            assert_eq!(report.cutoff_hz, None, "{name}: {report:?}");
            assert_eq!(report.verdict, QualityVerdict::Lossless, "{name}");
        }
        let analysis = crate::analyze_file(&folder.path().join("opposed-highs.wav")).unwrap();
        assert_eq!(analysis.quality.unwrap().verdict, QualityVerdict::Lossless);
    }

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
        let cases = [
            (320, Some(20_000.0), QualityVerdict::Good),
            (256, None, QualityVerdict::Good),
            (208, Some(18_600.0), QualityVerdict::Good),
            (192, Some(18_800.0), QualityVerdict::Acceptable),
            (155, Some(16_400.0), QualityVerdict::Acceptable),
            (128, Some(16_500.0), QualityVerdict::WeakOnABigSystem),
            (96, Some(15_500.0), QualityVerdict::WeakOnABigSystem),
        ];
        for (bitrate, cutoff, expected) in cases {
            assert_eq!(
                verdict(false, Some(bitrate), cutoff),
                expected,
                "{bitrate} {cutoff:?}"
            );
        }
    }

    #[test]
    fn a_high_bit_rate_with_an_early_cutoff_is_likely_re_encoded() {
        for (bitrate, cutoff) in [
            (320, 16_500.0),
            (320, 18_900.0),
            (256, 17_000.0),
            (192, 15_200.0),
        ] {
            assert_eq!(
                verdict(false, Some(bitrate), Some(cutoff)),
                QualityVerdict::LikelyReencoded,
                "{bitrate} {cutoff}"
            );
        }
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
        report.clipped_share = 0.002;
        assert!(
            report
                .reason()
                .ends_with(" The master is clipped in places.")
        );
        report.clipped_share = 0.0;
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
