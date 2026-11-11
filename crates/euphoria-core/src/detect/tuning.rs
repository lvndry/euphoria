// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures how far a track is tuned from concert pitch, A = 440 Hz.
//!
//! The strongest partials of each frame are located to a fraction of a
//! frequency bin, and each one's distance to the nearest note of the 440 Hz
//! scale is collected in a histogram that wraps around at a quarter tone.
//! Music piles its partials up around one distance, which is the tuning
//! offset. Noise spreads them evenly.

use std::f32::consts::TAU;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::AudioBuffer;

/// Frames are the largest power of two number of samples that fits in this
/// length, which separates notes a semitone apart from 200 Hz upwards.
const FRAME_SECONDS: f32 = 0.5;
/// Frames are spread evenly over the track, so the cost does not grow with
/// its length.
const MAXIMUM_FRAME_COUNT: usize = 256;
/// Below this the frequency bins are too wide to tell a semitone apart.
const LOWEST_FREQUENCY_HZ: f32 = 200.0;
/// Above this partials are mostly overtones, which sit away from the notes
/// of the scale.
const HIGHEST_FREQUENCY_HZ: f32 = 2_000.0;
const PEAKS_PER_FRAME: usize = 8;
/// Peaks weaker than this share of the strongest peak of their frame are
/// left out.
const PEAK_FLOOR: f32 = 0.1;
/// A frame whose strongest partial is below this amplitude, about -80 dBFS,
/// is silence.
const SILENT_PARTIAL_AMPLITUDE: f32 = 1.0e-4;
const CONCERT_PITCH_HZ: f32 = 440.0;
const CENTS_PER_SEMITONE: f32 = 100.0;
const HISTOGRAM_BINS_PER_CENT: usize = 2;
const HISTOGRAM_BIN_COUNT: usize = CENTS_PER_SEMITONE as usize * HISTOGRAM_BINS_PER_CENT;
/// Width of the smoothing applied to the histogram before its peak is read.
const SMOOTHING_CENTS: f32 = 3.0;
/// The offset is the weighted mean of the partials this close to the peak.
const PEAK_HALF_WIDTH_CENTS: f32 = 10.0;
/// The least share of the partials that must lie within
/// [`PEAK_HALF_WIDTH_CENTS`] of the peak for the track to have a tuning.
///
/// Evenly spread partials put 0.2 there. Synthesized white and pink noise
/// measure 0.21 to 0.30, and synthesized chords above 0.7 even under noise
/// as loud as the chords, so the value sits halfway. On two minute previews
/// of dance music, whose drums spread the partials, it is also where the
/// two halves of a preview measure within 10 cents of each other for 99 in
/// 100 previews.
const MINIMUM_PEAK_SHARE: f32 = 0.5;

/// How far a track is tuned from concert pitch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// The offset in cents, from -50 to 50. Positive is sharp: at +30 the A
    /// above middle C sounds at 447.7 Hz.
    pub cents: f32,
    /// The share of the measured partials that agree with the offset, from
    /// 0 to 1.
    pub agreement: f32,
}

/// Measures the tuning of a track, or returns `None` when too few of its
/// partials agree on one: in silence, in noise, in a track that is mostly
/// drums, and in audio shorter than one frame.
pub fn measure_tuning(audio: &AudioBuffer) -> Option<Tuning> {
    strongest_offset(audio).filter(|tuning| tuning.agreement >= MINIMUM_PEAK_SHARE)
}

/// The offset most partials agree on, however few of them do.
pub(crate) fn strongest_offset(audio: &AudioBuffer) -> Option<Tuning> {
    let histogram = partial_offsets(audio)?;
    let total: f32 = histogram.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let peak_bin = strongest_bin(&smoothed(&histogram))?;

    let half_width = (PEAK_HALF_WIDTH_CENTS * HISTOGRAM_BINS_PER_CENT as f32) as isize;
    let mut weight_near_peak = 0.0;
    let mut weighted_distance = 0.0;
    for distance in -half_width..=half_width {
        let bin = (peak_bin as isize + distance).rem_euclid(HISTOGRAM_BIN_COUNT as isize);
        let weight = histogram[bin as usize];
        weight_near_peak += weight;
        weighted_distance += weight * distance as f32;
    }
    let agreement = weight_near_peak / total;
    let mean_bin = peak_bin as f32 + weighted_distance / weight_near_peak;
    Some(Tuning {
        cents: wrap_cents(cents_of_bin(mean_bin)),
        agreement,
    })
}

/// The centre of a histogram bin, which may be fractional, in cents from the
/// nearest note.
fn cents_of_bin(bin: f32) -> f32 {
    (bin + 0.5) / HISTOGRAM_BINS_PER_CENT as f32 - CENTS_PER_SEMITONE / 2.0
}

/// Brings an offset into the range from -50 up to 50 cents.
fn wrap_cents(cents: f32) -> f32 {
    (cents + CENTS_PER_SEMITONE / 2.0).rem_euclid(CENTS_PER_SEMITONE) - CENTS_PER_SEMITONE / 2.0
}

/// How far a frequency is from the nearest note of the 440 Hz scale, in
/// cents from -50 up to 50.
fn cents_from_nearest_note(frequency_hz: f32) -> f32 {
    wrap_cents(12.0 * CENTS_PER_SEMITONE * (frequency_hz / CONCERT_PITCH_HZ).log2())
}

/// A histogram of the distance from each strong partial to its nearest note,
/// weighted by the amplitude of the partial. `None` when the audio is shorter
/// than one frame.
fn partial_offsets(audio: &AudioBuffer) -> Option<[f32; HISTOGRAM_BIN_COUNT]> {
    let samples = audio.samples();
    let sample_rate = audio.sample_rate().get() as f32;
    let frame_length = frame_length(sample_rate);
    if samples.len() < frame_length {
        return None;
    }
    let transform = FftPlanner::<f32>::new().plan_fft_forward(frame_length);
    let window = hann_window(frame_length);
    let mut frame = vec![Complex::default(); frame_length];
    let mut scratch = vec![Complex::default(); transform.get_inplace_scratch_len()];

    let bin_width = sample_rate / frame_length as f32;
    let last_bin = frame_length / 2 - 1;
    let first_band_bin = ((LOWEST_FREQUENCY_HZ / bin_width).ceil() as usize).max(1);
    let last_band_bin = ((HIGHEST_FREQUENCY_HZ / bin_width).floor() as usize).min(last_bin - 1);
    if first_band_bin > last_band_bin {
        return None;
    }
    // A sine of amplitude 1.0 has this magnitude under a Hann window.
    let silent_magnitude = SILENT_PARTIAL_AMPLITUDE * frame_length as f32 / 4.0;

    let last_start = samples.len() - frame_length;
    let frame_count = (last_start / (frame_length / 2) + 1).min(MAXIMUM_FRAME_COUNT);
    let mut magnitudes = vec![0.0_f32; last_band_bin + 2];
    let mut peaks: Vec<(f32, usize)> = Vec::new();
    let mut histogram = [0.0_f32; HISTOGRAM_BIN_COUNT];

    for frame_index in 0..frame_count {
        let start = if frame_count == 1 {
            0
        } else {
            (last_start as u64 * frame_index as u64 / (frame_count as u64 - 1)) as usize
        };
        for ((slot, sample), weight) in frame
            .iter_mut()
            .zip(&samples[start..start + frame_length])
            .zip(&window)
        {
            *slot = Complex::new(sample * weight, 0.0);
        }
        transform.process_with_scratch(&mut frame, &mut scratch);
        for (magnitude, value) in magnitudes.iter_mut().zip(&frame).skip(first_band_bin - 1) {
            *magnitude = value.norm();
        }

        peaks.clear();
        for bin in first_band_bin..=last_band_bin {
            let magnitude = magnitudes[bin];
            if magnitude > magnitudes[bin - 1] && magnitude >= magnitudes[bin + 1] {
                peaks.push((magnitude, bin));
            }
        }
        peaks.sort_unstable_by(|left, right| right.0.total_cmp(&left.0));
        let Some(&(strongest, _)) = peaks.first() else {
            continue;
        };
        if strongest < silent_magnitude || !strongest.is_finite() {
            continue;
        }
        for &(magnitude, bin) in peaks.iter().take(PEAKS_PER_FRAME) {
            if magnitude < PEAK_FLOOR * strongest {
                break;
            }
            let frequency = (bin as f32 + peak_position(&magnitudes, bin)) * bin_width;
            let cents = cents_from_nearest_note(frequency);
            let histogram_bin =
                ((cents + CENTS_PER_SEMITONE / 2.0) * HISTOGRAM_BINS_PER_CENT as f32) as usize;
            histogram[histogram_bin.min(HISTOGRAM_BIN_COUNT - 1)] += magnitude;
        }
    }
    Some(histogram)
}

fn frame_length(sample_rate: f32) -> usize {
    let longest = (FRAME_SECONDS * sample_rate) as usize;
    // The largest power of two that is no more than `longest`.
    (longest.max(2) + 1).next_power_of_two() / 2
}

fn hann_window(length: usize) -> Vec<f32> {
    (0..length)
        .map(|index| 0.5 - 0.5 * (TAU * index as f32 / length as f32).cos())
        .collect()
}

/// Where the top of a spectral peak lies relative to its highest bin, from
/// -0.5 to 0.5 bins, read from a parabola through the logarithms of the bin
/// and its two neighbours.
fn peak_position(magnitudes: &[f32], bin: usize) -> f32 {
    let floor = f32::MIN_POSITIVE;
    let below = magnitudes[bin - 1].max(floor).ln();
    let centre = magnitudes[bin].max(floor).ln();
    let above = magnitudes[bin + 1].max(floor).ln();
    let curvature = below - 2.0 * centre + above;
    if curvature >= 0.0 {
        return 0.0;
    }
    (0.5 * (below - above) / curvature).clamp(-0.5, 0.5)
}

/// The histogram after a circular Gaussian blur.
fn smoothed(histogram: &[f32; HISTOGRAM_BIN_COUNT]) -> [f32; HISTOGRAM_BIN_COUNT] {
    let deviation_bins = SMOOTHING_CENTS * HISTOGRAM_BINS_PER_CENT as f32;
    let reach = (3.0 * deviation_bins).ceil() as isize;
    let kernel: Vec<f32> = (-reach..=reach)
        .map(|distance| (-0.5 * (distance as f32 / deviation_bins).powi(2)).exp())
        .collect();
    let mut smoothed = [0.0_f32; HISTOGRAM_BIN_COUNT];
    for (bin, value) in smoothed.iter_mut().enumerate() {
        *value = kernel
            .iter()
            .zip(-reach..=reach)
            .map(|(weight, distance)| {
                let source = (bin as isize + distance).rem_euclid(HISTOGRAM_BIN_COUNT as isize);
                weight * histogram[source as usize]
            })
            .sum();
    }
    smoothed
}

fn strongest_bin(histogram: &[f32; HISTOGRAM_BIN_COUNT]) -> Option<usize> {
    histogram
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(bin, _)| bin)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::detect::synthesis::{
        SAMPLE_RATE, buffer, chord_progression, mixed, pink_noise, white_noise,
    };
    use crate::key::Key;

    fn a_minor() -> Key {
        "A minor".parse().unwrap()
    }

    #[test]
    fn a_progression_rendered_off_pitch_reports_its_offset() {
        for cents in [-45.0, -30.0, -12.0, 0.0, 5.0, 30.0, 48.0] {
            let audio = buffer(chord_progression(a_minor(), cents, 12.0));
            let tuning = measure_tuning(&audio).expect("the progression has a tuning");
            assert!(
                (tuning.cents - cents).abs() < 1.0,
                "rendered at {cents}, measured {}",
                tuning.cents
            );
            assert!(tuning.agreement > 0.9, "{}", tuning.agreement);
        }
    }

    #[test]
    fn the_offset_is_found_under_noise_as_loud_as_the_music() {
        let chords = chord_progression(a_minor(), 30.0, 12.0);
        let noisy = mixed(&chords, &pink_noise(chords.len(), 1.0));
        let tuning = measure_tuning(&buffer(noisy)).expect("the music has a tuning");
        assert!((tuning.cents - 30.0).abs() < 1.0, "{}", tuning.cents);
    }

    #[test]
    fn noise_has_no_tuning() {
        for seconds in [1, 5, 30] {
            let sample_count = seconds * SAMPLE_RATE as usize;
            for noise in [
                white_noise(sample_count, 0.5),
                pink_noise(sample_count, 0.5),
            ] {
                let audio = buffer(noise);
                let strongest = strongest_offset(&audio).unwrap();
                assert!(
                    strongest.agreement < 0.35,
                    "{seconds} seconds: {}",
                    strongest.agreement
                );
                assert_eq!(measure_tuning(&audio), None);
            }
        }
    }

    #[test]
    fn silence_and_audio_shorter_than_a_frame_have_no_tuning() {
        assert_eq!(
            measure_tuning(&buffer(vec![0.0; 3 * SAMPLE_RATE as usize])),
            None
        );
        assert_eq!(measure_tuning(&buffer(vec![0.5, -0.5, 0.5])), None);
        assert_eq!(measure_tuning(&buffer(Vec::new())), None);
    }

    #[test]
    fn a_long_track_is_measured_from_frames_spread_over_all_of_it() {
        let mut samples = chord_progression(a_minor(), 0.0, 80.0);
        samples.extend(chord_progression(a_minor(), 30.0, 240.0));
        let tuning = measure_tuning(&buffer(samples)).expect("the track has a tuning");
        assert!((tuning.cents - 30.0).abs() < 1.0, "{}", tuning.cents);
    }

    #[test]
    fn the_offset_is_measured_at_other_sample_rates() {
        for sample_rate in [8_000, 48_000, 96_000] {
            let rendered = chord_progression(a_minor(), 30.0, 6.0);
            let pitch_ratio = sample_rate as f32 / SAMPLE_RATE as f32;
            // Played back at another rate the progression is transposed, so
            // the expected offset moves with it.
            let expected = wrap_cents(30.0 + 1_200.0 * pitch_ratio.log2());
            let audio = AudioBuffer::new(rendered, NonZeroU32::new(sample_rate).unwrap());
            let Some(tuning) = measure_tuning(&audio) else {
                panic!("no tuning at {sample_rate} Hz");
            };
            let distance = wrap_cents(tuning.cents - expected).abs();
            assert!(distance < 2.0, "{sample_rate} Hz: {}", tuning.cents);
        }
    }

    #[test]
    fn frequencies_are_measured_against_the_nearest_note() {
        let sharp_by = |cents: f32| CONCERT_PITCH_HZ * 2.0_f32.powf(cents / 1_200.0);
        assert!(cents_from_nearest_note(440.0).abs() < 0.01);
        assert!(cents_from_nearest_note(220.0).abs() < 0.01);
        assert!((cents_from_nearest_note(sharp_by(30.0)) - 30.0).abs() < 0.01);
        assert!((cents_from_nearest_note(sharp_by(-30.0)) + 30.0).abs() < 0.01);
        assert!((cents_from_nearest_note(sharp_by(130.0)) - 30.0).abs() < 0.01);
        assert!((cents_from_nearest_note(sharp_by(70.0)) + 30.0).abs() < 0.01);
    }

    #[test]
    fn frames_are_the_largest_power_of_two_within_half_a_second() {
        assert_eq!(frame_length(8_000.0), 2_048);
        assert_eq!(frame_length(22_050.0), 8_192);
        assert_eq!(frame_length(44_100.0), 16_384);
        assert_eq!(frame_length(48_000.0), 16_384);
        assert_eq!(frame_length(384_000.0), 131_072);
    }
}
