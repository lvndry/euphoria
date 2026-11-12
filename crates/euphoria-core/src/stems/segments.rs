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
}

impl Segment<'_> {
    /// Frames of padding before the track's audio in the network's input.
    /// A segment shorter than the network's input is centred in it, with
    /// the audio around it filling the padding where there is any.
    fn padding_before(&self) -> usize {
        (SEGMENT_SAMPLES - self.length) / 2
    }

    /// Fills `output`, [`SEGMENT_SAMPLES`] long, with the network's input
    /// for this segment from one channel of the track: its audio passed
    /// through `normalise`, centred, and zeros beyond the ends of the track.
    pub(crate) fn read_padded(
        &self,
        channel: &[f32],
        output: &mut [f32],
        normalise: impl Fn(f32) -> f32,
    ) {
        let start = self.offset as isize - self.padding_before() as isize;
        for (index, value) in output.iter_mut().enumerate() {
            let position = start + index as isize;
            *value = if position >= 0 && (position as usize) < self.frames {
                normalise(channel[position as usize])
            } else {
                0.0
            };
        }
    }

    /// The frame before which no later segment adds anything, so that the
    /// blend of every earlier frame is final once this segment is added.
    pub(crate) fn final_until(&self) -> usize {
        (self.offset + STRIDE).min(self.frames)
    }
}

/// The blended output of the frames that segments still add to, one
/// buffer per channel of every stem. Frames whose blend is final are taken
/// out, so the window never holds much more than one segment, whatever the
/// length of the track.
pub(crate) struct OverlapWindow {
    start: usize,
    channels: Vec<Vec<f32>>,
    weights: Vec<f32>,
}

impl OverlapWindow {
    pub(crate) fn new(channel_count: usize) -> Self {
        Self {
            start: 0,
            channels: vec![Vec::new(); channel_count],
            weights: Vec::new(),
        }
    }

    /// Adds the output of the network for `segment`, which is
    /// [`SEGMENT_SAMPLES`] long, to `channel`, weighted. Every channel of a
    /// segment must be added before [`OverlapWindow::take_until`].
    pub(crate) fn add(&mut self, segment: &Segment<'_>, channel: usize, separated: &[f32]) {
        let first = segment.offset - self.start;
        let end = first + segment.length;
        let buffer = &mut self.channels[channel];
        if buffer.len() < end {
            buffer.resize(end, 0.0);
        }
        let trimmed =
            &separated[segment.padding_before()..segment.padding_before() + segment.length];
        for ((sum, sample), weight) in buffer[first..end]
            .iter_mut()
            .zip(trimmed)
            .zip(segment.weight)
        {
            *sum += sample * weight;
        }
        if channel == 0 {
            if self.weights.len() < end {
                self.weights.resize(end, 0.0);
            }
            for (total, weight) in self.weights[first..end].iter_mut().zip(segment.weight) {
                *total += weight;
            }
        }
    }

    /// Moves the blended frames before `until` into `output`, one vector
    /// per channel, each frame divided by the weight it received.
    pub(crate) fn take_until(&mut self, until: usize, output: &mut [Vec<f32>]) {
        let count = until.saturating_sub(self.start).min(self.weights.len());
        for (buffer, taken) in self.channels.iter_mut().zip(output.iter_mut()) {
            taken.clear();
            taken.extend(
                buffer
                    .drain(..count.min(buffer.len()))
                    .zip(&self.weights)
                    .map(|(sum, weight)| sum / weight),
            );
        }
        self.weights.drain(..count);
        self.start += count;
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
        last.read_padded(&channel, &mut input, |sample| sample);
        let before = (SEGMENT_SAMPLES - 84_030) / 2;
        assert_eq!(input[before], 515_970.0);
        assert_eq!(input[0], (515_970 - before) as f32);
        assert_eq!(input[before + 84_030 - 1], 599_999.0);
        assert_eq!(input[before + 84_030], 0.0);
    }

    #[test]
    fn blending_an_unchanged_input_gives_it_back_a_window_at_a_time() {
        let channel: Vec<f32> = (0..700_000)
            .map(|index| (index as f32 * 0.001).sin())
            .collect();
        let plan = SegmentPlan::new(channel.len());
        let mut window = OverlapWindow::new(1);
        let mut input = vec![0.0; SEGMENT_SAMPLES];
        let mut taken = vec![Vec::new()];
        let mut blended = Vec::new();
        let mut largest_window = 0;
        for segment in plan.segments() {
            segment.read_padded(&channel, &mut input, |sample| sample);
            window.add(&segment, 0, &input);
            largest_window = largest_window.max(window.weights.len());
            window.take_until(segment.final_until(), &mut taken);
            blended.extend_from_slice(&taken[0]);
        }
        assert_eq!(blended.len(), channel.len());
        assert!(largest_window <= SEGMENT_SAMPLES, "{largest_window}");
        for index in [0, 1, 257_985, 400_000, 699_999] {
            assert!((blended[index] - channel[index]).abs() < 1e-4, "{index}");
        }
    }

    #[test]
    fn normalisation_applies_to_the_audio_and_not_to_the_padding() {
        let channel = vec![1.0f32; 1000];
        let plan = SegmentPlan::new(channel.len());
        let segment = plan.segments().next().unwrap();
        let mut input = vec![0.0; SEGMENT_SAMPLES];
        segment.read_padded(&channel, &mut input, |sample| sample * 3.0 - 1.0);
        assert_eq!(input[segment.padding_before()], 2.0);
        assert_eq!(input[0], 0.0);
        assert_eq!(input[SEGMENT_SAMPLES - 1], 0.0);
    }
}
