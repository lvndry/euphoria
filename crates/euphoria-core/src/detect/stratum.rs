// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use stratum_dsp::features::chroma::extractor::compute_stft;
use stratum_dsp::features::period::tempogram::{
    TempogramBandFusionConfig, TempogramCandidateDebug,
    estimate_bpm_tempogram_with_candidates_band_fusion,
};
use stratum_dsp::preprocessing::normalization::{NormalizationConfig, normalize};
use stratum_dsp::preprocessing::silence::{SilenceDetector, detect_and_trim};
use stratum_dsp::{AnalysisConfig, AnalysisResult, Key as StratumKey, analyze_audio};

use crate::audio::AudioBuffer;
use crate::detect::decimate::decimate;
use crate::detect::tempo_choice;
use crate::detect::{Analysis, KeyDetector, TempoDetector, TempoReading};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};
use crate::tempo_range::TempoRange;

/// Key and tempo detection through the pure Rust `stratum-dsp` crate.
///
/// Tempo detection is the cheap half. It reads at most the middle ten
/// minutes of a track, at less than twice 44.1 kHz, and computes one
/// spectrogram of them. [`TempoDetector::detect_tempo`] reports the tempo in
/// [`TempoRange::DEFAULT`], and [`StratumDetector::read_tempo`] the estimate
/// before it is placed in a range. Key detection analyzes the whole track and holds several larger
/// spectrograms of it in memory, so it costs several times more time and
/// memory.
#[derive(Debug, Default, Clone, Copy)]
pub struct StratumDetector;

/// Below this length `stratum-dsp` still answers, but with a key and tempo
/// that do not come from the audio.
const MINIMUM_ANALYSIS_SECONDS: f32 = 1.0;

/// Tempo analysis reads at most this much audio, taken from the middle of
/// the track, so its memory stops growing with the length of the file.
/// Nearly all single tracks are shorter and are analyzed whole. Shorter
/// excerpts cost less but change the estimate on about one track in ten.
const TEMPO_EXCERPT_SECONDS: usize = 600;

/// Tempo analysis decimates audio to the lowest whole fraction of its sample
/// rate that stays at or above this rate, the one the `stratum-dsp` frame
/// lengths are set for. A high resolution file then costs about what a
/// 44.1 kHz file does.
const TEMPO_SAMPLE_RATE_FLOOR: u32 = 44_100;

/// A tempogram candidate within this share of the estimate is the estimate,
/// and two tempos whose ratio is this close to a power of two are octaves of
/// each other. It is the tolerance of the standard tempo evaluation.
const SAME_TEMPO_TOLERANCE: f32 = 0.04;

impl StratumDetector {
    /// Returns both key and tempo, the same values as the two trait methods.
    /// The two are separate analysis passes, so this costs as much as calling
    /// both methods. Silent audio, or audio shorter than one second, yields
    /// an analysis with neither.
    pub fn analyze(&self, audio: &AudioBuffer) -> Result<Analysis, EuphoriaError> {
        Ok(Analysis {
            key: self.detect_key(audio)?,
            tempo: self.detect_tempo(audio)?,
        })
    }

    /// The tempo estimate before it is placed in a [`TempoRange`], with what
    /// its confidence is based on. `None` for silent audio and audio shorter
    /// than one second.
    pub fn read_tempo(&self, audio: &AudioBuffer) -> Result<Option<TempoReading>, EuphoriaError> {
        tempo_of_middle(audio, TEMPO_EXCERPT_SECONDS)
    }
}

impl KeyDetector for StratumDetector {
    fn name(&self) -> &'static str {
        "stratum-dsp"
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        if !is_analyzable(audio.samples(), audio.sample_rate().get()) {
            return Ok(None);
        }
        let result = run_analysis(audio.samples(), audio.sample_rate().get(), key_config())?;
        let key = match result.key {
            StratumKey::Major(tonic) => Key::new((tonic % 12) as u8, Mode::Major),
            StratumKey::Minor(tonic) => Key::new((tonic % 12) as u8, Mode::Minor),
        };
        Ok(Some(key))
    }
}

impl TempoDetector for StratumDetector {
    fn name(&self) -> &'static str {
        "stratum-dsp"
    }

    fn detect_tempo(&self, audio: &AudioBuffer) -> Result<Option<f32>, EuphoriaError> {
        Ok(self
            .read_tempo(audio)?
            .map(|reading| reading.tempo_in(TempoRange::DEFAULT)))
    }
}

/// The tempo of the middle `excerpt_seconds` of the audio, or of all of it
/// when it is shorter than that.
fn tempo_of_middle(
    audio: &AudioBuffer,
    excerpt_seconds: usize,
) -> Result<Option<TempoReading>, EuphoriaError> {
    let sample_rate = audio.sample_rate().get();
    let excerpt = middle_excerpt(audio.samples(), sample_rate, excerpt_seconds);
    if !is_analyzable(excerpt, sample_rate) {
        return Ok(None);
    }
    let factor = decimation_factor(sample_rate);
    let analysis_rate = sample_rate / factor as u32;
    let config = tempo_config();
    match tempogram_tempo(decimate(excerpt, factor), analysis_rate, &config) {
        Some(reading) => Ok(Some(reading)),
        None => {
            let samples = decimate(excerpt, factor);
            let estimate = valid_tempo(run_analysis(&samples, analysis_rate, config)?.bpm);
            Ok(estimate.map(|estimate| TempoReading {
                estimate,
                score_margin: None,
            }))
        }
    }
}

/// How many input samples become one sample of tempo analysis.
fn decimation_factor(sample_rate: u32) -> usize {
    (sample_rate / TEMPO_SAMPLE_RATE_FLOOR).max(1) as usize
}

/// A tempo a detector can report: a positive, finite number.
fn valid_tempo(tempo: f32) -> Option<f32> {
    Some(tempo).filter(|tempo| tempo.is_finite() && *tempo > 0.0)
}

/// How far the tempogram scored `estimate` above the best tempo that is not
/// an octave of it: how clearly the beat period was found, whatever octave it
/// is counted in.
fn octave_score_margin(estimate: f32, candidates: &[TempogramCandidateDebug]) -> f32 {
    let best_score = |keep: &dyn Fn(f32) -> bool| {
        candidates
            .iter()
            .filter(|candidate| keep(candidate.bpm))
            .map(|candidate| candidate.score)
            .fold(0.0_f32, f32::max)
    };
    let estimate_score =
        best_score(&|bpm| (bpm - estimate).abs() <= SAME_TEMPO_TOLERANCE * estimate);
    let rival_score = best_score(&|bpm| !are_octaves(estimate, bpm));
    estimate_score - rival_score
}

/// Whether one tempo is the other doubled or halved any number of times.
fn are_octaves(first: f32, second: f32) -> bool {
    let octaves = (second / first).log2();
    (octaves - octaves.round()).abs() < (1.0 + SAME_TEMPO_TOLERANCE).log2()
}

/// False for silence and for audio too short to analyze.
fn is_analyzable(samples: &[f32], sample_rate: u32) -> bool {
    let seconds = samples.len() as f32 / sample_rate as f32;
    seconds >= MINIMUM_ANALYSIS_SECONDS && !crate::audio::is_silent(samples)
}

fn run_analysis(
    samples: &[f32],
    sample_rate: u32,
    config: AnalysisConfig,
) -> Result<AnalysisResult, EuphoriaError> {
    analyze_audio(samples, sample_rate, config).map_err(|error| EuphoriaError::Detection {
        detector: "stratum-dsp",
        reason: "analysis failed".to_string(),
        source: Some(Box::new(error)),
    })
}

/// The middle `excerpt_seconds` of the samples, or all of them when the
/// audio is shorter than that.
fn middle_excerpt(samples: &[f32], sample_rate: u32, excerpt_seconds: usize) -> &[f32] {
    let excerpt_length = excerpt_seconds.saturating_mul(sample_rate as usize);
    if samples.len() <= excerpt_length {
        return samples;
    }
    let start = (samples.len() - excerpt_length) / 2;
    &samples[start..start + excerpt_length]
}

/// The tempogram estimate that `analyze_audio` reports under
/// [`tempo_config`], computed through the same `stratum-dsp` steps but
/// without the onset lists, beat grid and key that `analyze_audio` also
/// builds, with the margin its confidence is based on. Returns `None` when a
/// step fails or the estimate is not a valid tempo, which is when
/// `analyze_audio` falls back to its estimate from onset intervals.
///
/// Takes the samples by value and normalizes them in place, so the only
/// full length copy of the audio is the one the caller made.
fn tempogram_tempo(
    mut normalized: Vec<f32>,
    sample_rate: u32,
    config: &AnalysisConfig,
) -> Option<TempoReading> {
    let normalization = NormalizationConfig {
        method: config.normalization,
        ..NormalizationConfig::default()
    };
    normalize(&mut normalized, normalization, sample_rate as f32).ok()?;
    let silence_detector = SilenceDetector {
        threshold_db: config.min_amplitude_db,
        frame_size: config.frame_size,
        ..SilenceDetector::default()
    };
    let (trimmed, _silence_regions) =
        detect_and_trim(&normalized, sample_rate, silence_detector).ok()?;
    drop(normalized);
    let spectrogram = compute_stft(&trimmed, config.frame_size, config.hop_size).ok()?;
    drop(trimmed);
    if spectrogram.is_empty() {
        return None;
    }
    let (estimate, candidates) = estimate_bpm_tempogram_with_candidates_band_fusion(
        &spectrogram,
        sample_rate,
        config.hop_size as u32,
        config.min_bpm,
        config.max_bpm,
        config.bpm_resolution,
        usize::MAX,
        band_fusion_config(config),
    )
    .ok()?;
    let estimate = valid_tempo(estimate.bpm)?;
    let leading = tempo_choice::leading_candidates(&candidates);
    let pulse = tempo_choice::OnsetPulse::from_spectrogram(
        &spectrogram,
        sample_rate,
        config.frame_size,
        config.hop_size,
    );
    let estimate = tempo_choice::choose(&pulse, &leading).map_or(estimate, |chosen| chosen.tempo);
    Some(TempoReading {
        estimate,
        score_margin: Some(octave_score_margin(estimate, &candidates)),
    })
}

/// The tempogram settings `analyze_audio` derives from its configuration.
fn band_fusion_config(config: &AnalysisConfig) -> TempogramBandFusionConfig {
    TempogramBandFusionConfig {
        enabled: config.enable_tempogram_band_fusion,
        low_max_hz: config.tempogram_band_low_max_hz,
        mid_max_hz: config.tempogram_band_mid_max_hz,
        high_max_hz: config.tempogram_band_high_max_hz,
        w_full: config.tempogram_band_w_full,
        w_low: config.tempogram_band_w_low,
        w_mid: config.tempogram_band_w_mid,
        w_high: config.tempogram_band_w_high,
        seed_only: config.tempogram_band_seed_only,
        support_threshold: config.tempogram_band_support_threshold,
        consensus_bonus: config.tempogram_band_consensus_bonus,
        enable_mel: config.enable_tempogram_mel_novelty,
        mel_n_mels: config.tempogram_mel_n_mels,
        mel_fmin_hz: config.tempogram_mel_fmin_hz,
        mel_fmax_hz: config.tempogram_mel_fmax_hz,
        mel_max_filter_bins: config.tempogram_mel_max_filter_bins,
        w_mel: config.tempogram_mel_weight,
        novelty_w_spectral: config.tempogram_novelty_w_spectral,
        novelty_w_energy: config.tempogram_novelty_w_energy,
        novelty_w_hfc: config.tempogram_novelty_w_hfc,
        novelty_local_mean_window: config.tempogram_novelty_local_mean_window,
        novelty_smooth_window: config.tempogram_novelty_smooth_window,
        debug_track_id: config.debug_track_id,
        debug_gt_bpm: config.debug_gt_bpm,
        debug_top_n: config.debug_top_n,
        superflux_max_filter_bins: config.tempogram_superflux_max_filter_bins,
    }
}

/// The default tempo estimate without its multi-resolution pass, which
/// recomputes the spectrogram at three hop sizes for ambiguous tracks and
/// lowers accuracy on GiantSteps Tempo.
///
/// The key settings apply when `analyze_audio` runs as the fallback. It
/// always computes a key after the tempo, and they reduce that to one plain
/// chroma reading of the spectrogram the tempo estimate already uses. They
/// do not change the tempo.
fn tempo_config() -> AnalysisConfig {
    AnalysisConfig {
        enable_tempogram_multi_resolution: false,
        enable_key_stft_override: false,
        enable_key_harmonic_mask: false,
        enable_key_spectrogram_time_smoothing: false,
        enable_key_hpcp: false,
        enable_key_segment_voting: false,
        enable_key_frame_weighting: false,
        ..AnalysisConfig::default()
    }
}

/// The default key analysis, with a tempo estimate from onset intervals only.
/// The key does not depend on the tempo, and this estimate costs less than
/// the default one.
fn key_config() -> AnalysisConfig {
    AnalysisConfig {
        force_legacy_bpm: true,
        enable_onset_consensus: false,
        ..AnalysisConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    /// Decaying tone bursts, `tempo` to the minute.
    fn clicks(tempo: f32, seconds: f32, sample_rate: u32) -> Vec<f32> {
        let beat_seconds = 60.0 / tempo;
        (0..(seconds * sample_rate as f32) as usize)
            .map(|index| {
                let since_beat = (index as f32 / sample_rate as f32) % beat_seconds;
                (std::f32::consts::TAU * 800.0 * since_beat).sin() * (-since_beat / 0.03).exp()
            })
            .collect()
    }

    fn click_track() -> Vec<f32> {
        clicks(130.0, 6.0, 44_100)
    }

    fn buffer(samples: Vec<f32>, sample_rate: u32) -> AudioBuffer {
        AudioBuffer::new(samples, NonZeroU32::new(sample_rate).unwrap())
    }

    #[test]
    fn only_positive_finite_numbers_are_valid_tempos() {
        assert_eq!(valid_tempo(126.0), Some(126.0));
        for tempo in [0.0, -126.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(valid_tempo(tempo), None, "{tempo}");
        }
    }

    #[test]
    fn detected_tempos_are_folded_into_the_default_range() {
        let expectations = [
            (125.5, 125.5),
            (90.0, 90.0),
            (62.75, 125.5),
            (89.9, 179.8),
            (180.0, 90.0),
            (251.0, 125.5),
            (31.5, 126.0),
            (350.0, 175.0),
        ];
        for (tempo, folded) in expectations {
            let reading = TempoReading {
                estimate: tempo,
                score_margin: None,
            };
            assert_eq!(reading.tempo_in(TempoRange::DEFAULT), folded, "{tempo}");
        }
    }

    #[test]
    fn tempos_doubled_or_halved_are_octaves_of_each_other() {
        assert!(are_octaves(125.0, 125.0));
        assert!(are_octaves(125.0, 62.5));
        assert!(are_octaves(125.0, 250.0));
        assert!(are_octaves(125.0, 31.25));
        assert!(are_octaves(125.0, 127.0));
        assert!(!are_octaves(125.0, 83.3));
        assert!(!are_octaves(125.0, 187.5));
        assert!(!are_octaves(125.0, 140.0));
    }

    fn candidate(bpm: f32, score: f32) -> TempogramCandidateDebug {
        TempogramCandidateDebug {
            bpm,
            score,
            fft_norm: 0.0,
            autocorr_norm: 0.0,
            selected: false,
        }
    }

    #[test]
    fn the_margin_is_over_the_best_tempo_that_is_not_an_octave() {
        let candidates = [
            candidate(124.0, 0.9),
            candidate(62.0, 0.95),
            candidate(248.0, 0.7),
            candidate(186.0, 0.6),
            candidate(93.0, 0.4),
        ];
        let margin = octave_score_margin(124.0, &candidates);
        assert!((margin - 0.3).abs() < 1.0e-6, "{margin}");
        assert_eq!(octave_score_margin(124.0, &[]), 0.0);
        let alone = octave_score_margin(124.0, &[candidate(124.0, 0.8)]);
        assert!((alone - 0.8).abs() < 1.0e-6);
    }

    #[test]
    fn a_steady_click_track_has_a_tempo_with_high_confidence() {
        let reading = StratumDetector
            .read_tempo(&buffer(clicks(128.0, 20.0, 44_100), 44_100))
            .unwrap()
            .unwrap();
        assert!(
            (reading.tempo_in(TempoRange::DEFAULT) - 128.0).abs() < 0.04 * 128.0,
            "{reading:?}"
        );
        assert!(reading.score_margin.is_some());
        assert_eq!(reading.confidence(), crate::Confidence::High, "{reading:?}");
    }

    #[test]
    fn high_sample_rates_are_decimated_to_at_least_the_floor() {
        let expectations = [
            (8_000, 1),
            (44_100, 1),
            (48_000, 1),
            (88_199, 1),
            (88_200, 2),
            (96_000, 2),
            (176_400, 4),
            (192_000, 4),
            (384_000, 8),
        ];
        for (sample_rate, factor) in expectations {
            assert_eq!(decimation_factor(sample_rate), factor, "{sample_rate} Hz");
        }
    }

    #[test]
    fn a_failed_tempogram_falls_back_to_the_full_analysis() {
        let sample_rate = 44_100;
        let mut samples = vec![0.0; 2 * sample_rate as usize];
        for (index, sample) in samples.iter_mut().take(500).enumerate() {
            *sample = (std::f32::consts::TAU * 800.0 * index as f32 / sample_rate as f32).sin();
        }
        let config = tempo_config();
        assert_eq!(tempogram_tempo(samples.clone(), sample_rate, &config), None);

        let fallback = run_analysis(&samples, sample_rate, config).unwrap().bpm;
        let samples_again = samples.clone();
        let detected = StratumDetector
            .detect_tempo(&buffer(samples, sample_rate))
            .unwrap();

        assert_eq!(
            detected,
            valid_tempo(fallback).map(|tempo| TempoRange::DEFAULT.fold(tempo))
        );
        let reading = StratumDetector
            .read_tempo(&buffer(samples_again, sample_rate))
            .unwrap();
        assert_eq!(
            reading,
            valid_tempo(fallback).map(|estimate| TempoReading {
                estimate,
                score_margin: None,
            }),
            "a fallback estimate has no margin to base its confidence on"
        );
    }

    #[test]
    fn a_track_longer_than_the_excerpt_gets_the_tempo_of_its_middle() {
        let sample_rate = 44_100;
        let excerpt_seconds = 8;
        let middle_tempo = 132.0;
        let edge = clicks(100.0, 2.0 * excerpt_seconds as f32, sample_rate);
        let middle = clicks(middle_tempo, excerpt_seconds as f32, sample_rate);
        let whole: Vec<f32> = [edge.as_slice(), middle.as_slice(), edge.as_slice()].concat();
        assert_eq!(
            middle_excerpt(&whole, sample_rate, excerpt_seconds),
            middle.as_slice()
        );
        let whole = buffer(whole, sample_rate);

        let tempo_of = |seconds: usize| {
            let reading = tempo_of_middle(&whole, seconds).unwrap().unwrap();
            reading.tempo_in(TempoRange::DEFAULT)
        };
        let tempo_of_excerpt = tempo_of(excerpt_seconds);
        let tempo_of_whole = tempo_of(5 * excerpt_seconds);

        assert!(
            (tempo_of_excerpt - middle_tempo).abs() < 0.04 * middle_tempo,
            "{tempo_of_excerpt}"
        );
        assert!(
            (tempo_of_whole - 100.0).abs() < 4.0,
            "the edges should set the tempo of the whole track, not {tempo_of_whole}"
        );
    }

    #[test]
    fn the_tempogram_tempo_is_the_one_analyze_audio_reports() {
        let samples = click_track();
        let config = tempo_config();

        let direct =
            tempogram_tempo(samples.clone(), 44_100, &config).map(|reading| reading.estimate);
        let reported = run_analysis(&samples, 44_100, config).unwrap().bpm;

        assert_eq!(direct, Some(reported));
    }

    #[test]
    fn audio_no_longer_than_the_excerpt_is_used_whole() {
        let sample_rate = 10;
        let samples = vec![0.5; TEMPO_EXCERPT_SECONDS * sample_rate];
        assert_eq!(
            middle_excerpt(&samples, sample_rate as u32, TEMPO_EXCERPT_SECONDS).len(),
            samples.len()
        );
        assert_eq!(
            middle_excerpt(&samples[..7], sample_rate as u32, TEMPO_EXCERPT_SECONDS).len(),
            7
        );
    }

    #[test]
    fn longer_audio_is_cut_to_its_middle() {
        let sample_rate = 10;
        let excerpt_length = TEMPO_EXCERPT_SECONDS * sample_rate;
        let samples: Vec<f32> = (0..excerpt_length + 100)
            .map(|index| index as f32)
            .collect();

        let excerpt = middle_excerpt(&samples, sample_rate as u32, TEMPO_EXCERPT_SECONDS);

        assert_eq!(excerpt.len(), excerpt_length);
        assert_eq!(excerpt.first(), Some(&50.0));
        assert_eq!(excerpt.last(), Some(&(excerpt_length as f32 + 49.0)));
    }
}
