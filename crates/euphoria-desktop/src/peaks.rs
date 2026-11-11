// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reducing decoded audio to the columns drawn as a waveform: for each
//! column, how loud the bass, the middle and the treble are, so that kicks,
//! breakdowns and hi hats can be told apart at a glance.

use std::f32::consts::TAU;

/// Number of columns kept per track, enough for the widest waveform drawn.
pub(crate) const PEAK_COUNT: usize = 720;

/// Below this frequency a sound counts as bass: kick drums and bass lines.
const LOW_BAND_LIMIT_HERTZ: f32 = 200.0;

/// Above this frequency a sound counts as treble: hi hats, cymbals and air.
const HIGH_BAND_LIMIT_HERTZ: f32 = 2_500.0;

/// The lowest rate the bass and the middle are measured at, in samples per
/// second. It is more than four times the top of the middle band, so every
/// frequency of both bands is represented with room to spare.
const LOWEST_REDUCED_RATE: u32 = 11_025;

/// How many samples are averaged into one before the bass and the middle are
/// measured: as many as keep the rate at or above [`LOWEST_REDUCED_RATE`].
/// That is 4 at 44.1 kHz, 34 at 384 kHz, and 1 (no reduction) at the rates
/// below 22.05 kHz, down to the 8 kHz the decoder accepts.
fn reduction_for(sample_rate: u32) -> usize {
    (sample_rate / LOWEST_REDUCED_RATE).max(1) as usize
}

/// How loud each band is in one column of the waveform, each from 0.0 to
/// 1.0 on a scale shared by the whole track.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct BandPeaks {
    /// Bass, below 200 Hz.
    pub(crate) low: f32,
    /// The middle, from 200 Hz to 2.5 kHz.
    pub(crate) mid: f32,
    /// Treble, above 2.5 kHz.
    pub(crate) high: f32,
}

impl BandPeaks {
    /// The loudest of the three bands, which is how tall the column is.
    pub(crate) fn loudest(&self) -> f32 {
        self.low.max(self.mid).max(self.high)
    }

    fn raise_to(&mut self, other: &BandPeaks) {
        self.low = self.low.max(other.low);
        self.mid = self.mid.max(other.mid);
        self.high = self.high.max(other.high);
    }
}

/// Which side of its cutoff a filter lets through.
#[derive(Debug, Clone, Copy)]
enum Pass {
    Below,
    Above,
}

/// A second order Butterworth filter: a handful of multiplications per
/// sample, cheap enough to run over a whole track, and steep enough (12 dB
/// per octave) to keep a kick out of the treble and a hi hat out of the bass.
#[derive(Debug, Clone, Copy)]
struct Filter {
    input_gains: [f32; 3],
    feedback_gains: [f32; 2],
    earlier_inputs: [f32; 2],
    earlier_outputs: [f32; 2],
}

impl Filter {
    fn new(pass: Pass, cutoff_hertz: f32, sample_rate: f32) -> Self {
        // A cutoff at or above half the sample rate cannot be represented, so
        // it is held just under it.
        let cutoff_hertz = cutoff_hertz.min(sample_rate * 0.45);
        let angle = TAU * cutoff_hertz / sample_rate;
        let cosine = angle.cos();
        let damping = angle.sin() / std::f32::consts::SQRT_2;
        let scale = 1.0 / (1.0 + damping);
        let (edge_gain, middle_gain) = match pass {
            Pass::Below => ((1.0 - cosine) / 2.0, 1.0 - cosine),
            Pass::Above => ((1.0 + cosine) / 2.0, -(1.0 + cosine)),
        };
        Self {
            input_gains: [edge_gain * scale, middle_gain * scale, edge_gain * scale],
            feedback_gains: [-2.0 * cosine * scale, (1.0 - damping) * scale],
            earlier_inputs: [0.0; 2],
            earlier_outputs: [0.0; 2],
        }
    }

    fn filter(&mut self, sample: f32) -> f32 {
        let output = self.input_gains[0] * sample
            + self.input_gains[1] * self.earlier_inputs[0]
            + self.input_gains[2] * self.earlier_inputs[1]
            - self.feedback_gains[0] * self.earlier_outputs[0]
            - self.feedback_gains[1] * self.earlier_outputs[1];
        self.earlier_inputs = [sample, self.earlier_inputs[0]];
        self.earlier_outputs = [output, self.earlier_outputs[0]];
        output
    }
}

/// Splits `samples` into `column_count` equal spans and returns how loud the
/// bass, the middle and the treble are in each, scaled so that the loudest
/// band of the loudest column is 1.0. Silent audio gives all zeros.
///
/// Loudness is the root of the mean square of the band over the span and not
/// its highest sample: the brief peaks of hi hats and of a heavily limited
/// master reach as high as a kick, while the average tells them apart.
pub(crate) fn compute_band_peaks(
    samples: &[f32],
    sample_rate: u32,
    column_count: usize,
) -> Vec<BandPeaks> {
    if samples.is_empty() || column_count == 0 || sample_rate == 0 {
        return Vec::new();
    }
    // Only the treble needs every sample. The bass and the middle are found
    // from what is left once the treble is taken out, averaged a few samples
    // at a time, which costs a fraction as much and loses nothing below
    // 2.5 kHz.
    let reduction = reduction_for(sample_rate);
    let full_rate = sample_rate as f32;
    let reduced_rate = full_rate / reduction as f32;
    let mut treble = Filter::new(Pass::Above, HIGH_BAND_LIMIT_HERTZ, full_rate);
    let mut bass = Filter::new(Pass::Below, LOW_BAND_LIMIT_HERTZ, reduced_rate);
    let mut above_bass = Filter::new(Pass::Above, LOW_BAND_LIMIT_HERTZ, reduced_rate);
    let mut below_treble = Filter::new(Pass::Below, HIGH_BAND_LIMIT_HERTZ, reduced_rate);
    let mut columns = vec![BandPeaks::default(); column_count];

    let mut without_treble_sum = 0.0;
    let mut samples_in_sum = 0;
    let mut span_start = 0;
    for (column_index, column) in columns.iter_mut().enumerate() {
        let span_end = ((column_index + 1) * samples.len() / column_count).max(span_start);
        let mut squares = BandPeaks::default();
        let mut reduced_sample_count = 0_usize;
        for sample in &samples[span_start..span_end] {
            let high = treble.filter(*sample);
            squares.high += high * high;

            without_treble_sum += sample - high;
            samples_in_sum += 1;
            if samples_in_sum == reduction {
                let without_treble = without_treble_sum / reduction as f32;
                without_treble_sum = 0.0;
                samples_in_sum = 0;
                let low = bass.filter(without_treble);
                let mid = below_treble.filter(above_bass.filter(without_treble));
                squares.low += low * low;
                squares.mid += mid * mid;
                reduced_sample_count += 1;
            }
        }
        let root_mean = |sum_of_squares: f32, count: usize| {
            if count == 0 {
                0.0
            } else {
                (sum_of_squares / count as f32).sqrt()
            }
        };
        *column = BandPeaks {
            low: root_mean(squares.low, reduced_sample_count),
            mid: root_mean(squares.mid, reduced_sample_count),
            high: root_mean(squares.high, span_end - span_start),
        };
        span_start = span_end;
    }

    let loudest = columns
        .iter()
        .map(BandPeaks::loudest)
        .fold(0.0_f32, f32::max);
    if loudest > 0.0 {
        for column in &mut columns {
            column.low /= loudest;
            column.mid /= loudest;
            column.high /= loudest;
        }
    }
    columns
}

/// Reduces a track's `columns` to at most `bar_count` bars, each band as
/// loud as its loudest column in the bar. The columns keep their scale, so a
/// quiet passage stays short however few bars there are.
pub(crate) fn bars_from_columns(columns: &[BandPeaks], bar_count: usize) -> Vec<BandPeaks> {
    let bar_count = bar_count.min(columns.len());
    (0..bar_count)
        .map(|bar_index| {
            let start = bar_index * columns.len() / bar_count;
            let end = ((bar_index + 1) * columns.len() / bar_count).max(start + 1);
            let mut bar = BandPeaks::default();
            for column in &columns[start..end.min(columns.len())] {
                bar.raise_to(column);
            }
            bar
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 44_100;

    fn sine(frequency_hertz: f32, amplitude: f32, sample_count: usize) -> Vec<f32> {
        (0..sample_count)
            .map(|index| {
                amplitude * (TAU * frequency_hertz * index as f32 / SAMPLE_RATE as f32).sin()
            })
            .collect()
    }

    fn band(low: f32, mid: f32, high: f32) -> BandPeaks {
        BandPeaks { low, mid, high }
    }

    #[test]
    fn empty_audio_has_no_columns() {
        assert!(compute_band_peaks(&[], SAMPLE_RATE, 10).is_empty());
        assert!(compute_band_peaks(&[0.5], SAMPLE_RATE, 0).is_empty());
        assert!(compute_band_peaks(&[0.5], 0, 10).is_empty());
    }

    #[test]
    fn silence_stays_flat() {
        let columns = compute_band_peaks(&[0.0; 4_000], SAMPLE_RATE, 4);
        assert_eq!(columns, vec![BandPeaks::default(); 4]);
    }

    #[test]
    fn a_bass_note_is_loudest_in_the_low_band() {
        let columns = compute_band_peaks(&sine(60.0, 0.8, SAMPLE_RATE as usize), SAMPLE_RATE, 8);
        let settled = columns[4];
        assert!(settled.low > 0.8, "{settled:?}");
        assert!(settled.mid < settled.low / 2.0, "{settled:?}");
        assert!(settled.high < 0.05, "{settled:?}");
    }

    #[test]
    fn a_vocal_range_note_is_loudest_in_the_middle_band() {
        let columns = compute_band_peaks(&sine(800.0, 0.8, SAMPLE_RATE as usize), SAMPLE_RATE, 8);
        let settled = columns[4];
        assert!(settled.mid > settled.low * 2.0, "{settled:?}");
        assert!(settled.mid > settled.high * 2.0, "{settled:?}");
    }

    #[test]
    fn a_hi_hat_range_note_is_loudest_in_the_high_band() {
        let columns = compute_band_peaks(&sine(9_000.0, 0.8, SAMPLE_RATE as usize), SAMPLE_RATE, 8);
        let settled = columns[4];
        assert!(settled.high > 0.8, "{settled:?}");
        assert!(settled.mid < settled.high / 2.0, "{settled:?}");
        assert!(settled.low < 0.05, "{settled:?}");
    }

    /// One second of a tone at `sample_rate`, and the column in the middle of
    /// it once the filters have settled.
    fn settled_bands_of_tone(frequency_hertz: f32, sample_rate: u32) -> BandPeaks {
        let samples: Vec<f32> = (0..sample_rate)
            .map(|index| 0.8 * (TAU * frequency_hertz * index as f32 / sample_rate as f32).sin())
            .collect();
        compute_band_peaks(&samples, sample_rate, 8)[4]
    }

    #[test]
    fn the_bands_hold_at_every_sample_rate_the_decoder_accepts() {
        for sample_rate in [
            8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 96_000, 192_000, 384_000,
        ] {
            let bass = settled_bands_of_tone(60.0, sample_rate);
            assert!(bass.low > bass.mid * 2.0, "{sample_rate} Hz: {bass:?}");
            assert!(bass.low > bass.high * 10.0, "{sample_rate} Hz: {bass:?}");

            for middle_tone in [500.0, 1_500.0] {
                let middle = settled_bands_of_tone(middle_tone, sample_rate);
                assert!(
                    middle.mid > middle.low * 2.0 && middle.mid > middle.high * 1.5,
                    "{middle_tone} Hz at {sample_rate} Hz: {middle:?}"
                );
            }

            let treble = settled_bands_of_tone(3_500.0, sample_rate);
            assert!(
                treble.high > treble.mid * 1.5 && treble.high > treble.low * 10.0,
                "{sample_rate} Hz: {treble:?}"
            );
        }
    }

    #[test]
    fn the_reduction_keeps_the_middle_band_representable() {
        assert_eq!(reduction_for(8_000), 1);
        assert_eq!(reduction_for(22_049), 1);
        assert_eq!(reduction_for(22_050), 2);
        assert_eq!(reduction_for(44_100), 4);
        assert_eq!(reduction_for(48_000), 4);
        assert_eq!(reduction_for(384_000), 34);
        for sample_rate in (8_000..=384_000).step_by(1_000) {
            let reduced_rate = sample_rate as f32 / reduction_for(sample_rate) as f32;
            assert!(
                reduced_rate / 2.0 > HIGH_BAND_LIMIT_HERTZ * 1.5,
                "{sample_rate} Hz is reduced to {reduced_rate} Hz"
            );
        }
    }

    #[test]
    fn a_kick_a_gap_and_a_hat_show_up_in_different_bands_of_different_columns() {
        let one_second = SAMPLE_RATE as usize;
        let mut samples = sine(55.0, 0.9, one_second);
        samples.extend(vec![0.0; one_second]);
        samples.extend(sine(10_000.0, 0.3, one_second));
        let columns = compute_band_peaks(&samples, SAMPLE_RATE, 3);
        let (kick, hat) = (columns[0], columns[2]);
        assert!(kick.low > kick.high * 5.0, "{kick:?}");
        assert!(hat.high > hat.low * 5.0, "{hat:?}");
        assert!(kick.loudest() > hat.loudest() * 2.0);
    }

    #[test]
    fn the_loudest_band_of_the_track_is_full_height_and_quiet_audio_is_scaled_up() {
        let columns = compute_band_peaks(&sine(60.0, 0.01, SAMPLE_RATE as usize), SAMPLE_RATE, 16);
        let loudest = columns
            .iter()
            .map(BandPeaks::loudest)
            .fold(0.0_f32, f32::max);
        assert!((loudest - 1.0).abs() < 1.0e-6);
        assert!(columns.iter().all(|column| column.loudest() <= 1.0));
    }

    #[test]
    fn more_columns_than_samples_still_gives_every_column() {
        let columns = compute_band_peaks(&[0.5, 1.0], SAMPLE_RATE, 4);
        assert_eq!(columns.len(), 4);
    }

    #[test]
    fn bars_never_outnumber_columns() {
        let columns = [
            band(1.0, 0.5, 0.2),
            band(0.5, 0.2, 0.1),
            band(0.2, 0.1, 0.0),
        ];
        assert_eq!(bars_from_columns(&columns, 10).len(), 3);
        assert!(bars_from_columns(&[], 10).is_empty());
        assert!(bars_from_columns(&columns, 0).is_empty());
    }

    #[test]
    fn a_bar_takes_the_loudest_of_each_band_it_covers() {
        let columns = [
            band(1.0, 0.1, 0.0),
            band(0.2, 0.6, 0.0),
            band(0.1, 0.05, 0.3),
            band(0.0, 0.0, 0.1),
        ];
        assert_eq!(
            bars_from_columns(&columns, 2),
            vec![band(1.0, 0.6, 0.0), band(0.1, 0.05, 0.3)]
        );
    }

    #[test]
    fn bars_keep_the_scale_of_the_columns_they_cover() {
        let quiet = [band(0.2, 0.1, 0.05), band(0.1, 0.1, 0.0)];
        assert_eq!(bars_from_columns(&quiet, 1), vec![band(0.2, 0.1, 0.05)]);

        let mut columns = vec![band(0.1, 0.05, 0.0); 700];
        columns.extend([band(1.0, 0.5, 0.2); 20]);
        let bars = bars_from_columns(&columns, 72);
        assert!(bars[..60].iter().all(|bar| (bar.low - 0.1).abs() < 1.0e-6));
        assert_eq!(bars.last().map(BandPeaks::loudest), Some(1.0));
    }
}
