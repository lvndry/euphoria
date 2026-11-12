// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cutting a track into the overlapping segments the network separates and
//! blending their outputs back, as `demucs.apply.apply_model` does with
//! `split=True, overlap=0.25, shifts=0`.

use super::spectrogram::SEGMENT_SAMPLES;

/// The share of a segment that overlaps the next one.
const OVERLAP: f64 = 0.25;
/// How far apart segments start.
const STRIDE: usize = ((1.0 - OVERLAP) * SEGMENT_SAMPLES as f64) as usize;

/// The segments of a track of a given length, with the triangular weights
/// that blend them.
pub(crate) struct SegmentPlan {
    frames: usize,
    weight: Vec<f32>,
}

/// One segment: where it starts in the track and how many frames of the
/// track it covers, at most [`SEGMENT_SAMPLES`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Segment<'plan> {
    offset: usize,
    length: usize,
    frames: usize,
    weight: &'plan [f32],
}

impl SegmentPlan {
    pub(crate) fn new(frames: usize) -> Self {
        let half = SEGMENT_SAMPLES / 2;
        let peak = half.max(SEGMENT_SAMPLES - half) as f32;
        let weight = (1..=half)
            .chain((1..=SEGMENT_SAMPLES - half).rev())
            .map(|step| step as f32 / peak)
            .collect();
        Self { frames, weight }
    }

    /// How many segments the track is cut into.
    pub(crate) fn len(&self) -> usize {
        self.frames.div_ceil(STRIDE)
    }

    pub(crate) fn segments(&self) -> impl Iterator<Item = Segment<'_>> {
        (0..self.frames).step_by(STRIDE).map(|offset| Segment {
            offset,
            length: SEGMENT_SAMPLES.min(self.frames - offset),
            frames: self.frames,
            weight: &self.weight,
        })
    }

    /// The sum of the weights every frame of the track receives, which the
    /// blended output is divided by.
    pub(crate) fn total_weight(&self) -> Vec<f32> {
        let mut total = vec![0.0f32; self.frames];
        for segment in self.segments() {
            for (index, weight) in segment.weight[..segment.length].iter().enumerate() {
                total[segment.offset + index] += weight;
            }
        }
        total
    }
}

impl Segment<'_> {
    /// Frames of padding before the track's audio in the network's input.
    /// A segment shorter than the network's input is centred in it, with
    /// the audio around it filling the padding where there is any.
    fn padding_before(&self) -> usize {
        (SEGMENT_SAMPLES - self.length) / 2
    }

    /// Fills `output`, [`SEGMENT_SAMPLES`] long, with the network's input
    /// for this segment from one channel of the track: its audio, centred,
    /// and zeros beyond the ends of the track.
    pub(crate) fn read_padded(&self, channel: &[f32], output: &mut [f32]) {
        let start = self.offset as isize - self.padding_before() as isize;
        for (index, value) in output.iter_mut().enumerate() {
            let position = start + index as isize;
            *value = if position >= 0 && (position as usize) < self.frames {
                channel[position as usize]
            } else {
                0.0
            };
        }
    }

    /// Adds the weighted output of the network for this segment, which is
    /// [`SEGMENT_SAMPLES`] long, to one channel of the track.
    pub(crate) fn add_weighted(&self, separated: &[f32], channel: &mut [f32]) {
        let trimmed = &separated[self.padding_before()..self.padding_before() + self.length];
        let destination = &mut channel[self.offset..self.offset + self.length];
        for ((sum, sample), weight) in destination.iter_mut().zip(trimmed).zip(self.weight) {
            *sum += sample * weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_start_every_three_quarters_of_a_segment() {
        assert_eq!(STRIDE, 257_985);
        let plan = SegmentPlan::new(600_000);
        let starts: Vec<(usize, usize)> = plan
            .segments()
            .map(|segment| (segment.offset, segment.length))
            .collect();
        assert_eq!(
            starts,
            vec![
                (0, SEGMENT_SAMPLES),
                (257_985, SEGMENT_SAMPLES - 1965),
                (515_970, 84_030)
            ]
        );
        assert_eq!(plan.len(), 3);
    }

    #[test]
    fn the_weight_is_a_triangle_peaking_at_one() {
        let plan = SegmentPlan::new(10);
        assert_eq!(plan.weight.len(), SEGMENT_SAMPLES);
        assert_eq!(plan.weight[0], 1.0 / 171_990.0);
        assert_eq!(plan.weight[171_989], 1.0);
        assert_eq!(plan.weight[171_990], 1.0);
        assert_eq!(plan.weight[SEGMENT_SAMPLES - 1], 1.0 / 171_990.0);
    }

    #[test]
    fn a_short_segment_is_centred_with_the_audio_around_it() {
        let channel: Vec<f32> = (0..600_000).map(|index| index as f32).collect();
        let plan = SegmentPlan::new(channel.len());
        let last = plan.segments().last().unwrap();
        let mut input = vec![0.0; SEGMENT_SAMPLES];
        last.read_padded(&channel, &mut input);
        let before = (SEGMENT_SAMPLES - 84_030) / 2;
        assert_eq!(input[before], 515_970.0);
        assert_eq!(input[0], (515_970 - before) as f32);
        assert_eq!(input[before + 84_030 - 1], 599_999.0);
        assert_eq!(input[before + 84_030], 0.0);
    }

    #[test]
    fn blending_an_unchanged_input_gives_it_back() {
        let channel: Vec<f32> = (0..700_000)
            .map(|index| (index as f32 * 0.001).sin())
            .collect();
        let plan = SegmentPlan::new(channel.len());
        let mut blended = vec![0.0; channel.len()];
        let mut input = vec![0.0; SEGMENT_SAMPLES];
        for segment in plan.segments() {
            segment.read_padded(&channel, &mut input);
            segment.add_weighted(&input, &mut blended);
        }
        let total = plan.total_weight();
        for index in [0, 1, 257_985, 400_000, 699_999] {
            assert!(
                (blended[index] / total[index] - channel[index]).abs() < 1e-4,
                "{index}"
            );
        }
    }
}
