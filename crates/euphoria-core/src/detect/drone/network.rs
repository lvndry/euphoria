// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The drone-v1 network, run on the CPU with no inference runtime: three
//! blocks of two 3x3 convolutions, each followed by batch normalization and
//! an ELU, with a 2x2 max pool after every block; then the mean over time and
//! one linear layer to the 24 keys.
//!
//! Batch normalization is folded into the convolution it follows. Each
//! convolution is nine matrix products, one per filter tap, over a frame
//! with a border of zeros, so no unrolled copy of the input is made. The
//! track is read in stretches of frames with enough context on either side
//! that every output is the one the whole track at once would give, which
//! keeps memory flat for long tracks.

use std::collections::HashMap;

use serde::Deserialize;

use super::cqt::CORE_BINS;
use crate::error::EuphoriaError;

/// The weights of drone-v1, exported from the published checkpoint. See
/// `models/README.md` for their provenance and licence.
const WEIGHTS: &[u8] = include_bytes!("../../../models/drone-v1.safetensors");

/// The feature contract the weights were trained with.
const FEATURE_SPEC: &str = "cqt24-8oct-c1-pad4-hop4096-v2";
/// The order of the 24 outputs: tonic times two, plus one for minor.
const LABEL_SCHEMA: &str = "tonic-times-two-plus-minor-v1";

/// The number of keys the network scores.
pub(super) const KEY_COUNT: usize = 24;
/// The fewest frames the network accepts: one column after three halvings.
pub(super) const MINIMUM_FRAMES: usize = 8;
/// Frames read at once, a multiple of eight so the pools stay aligned.
const STRETCH_FRAMES: usize = 256;
/// Frames of context read on either side of a stretch. An output after the
/// last pool depends on 16 frames before its own eight and 15 after.
const CONTEXT_FRAMES: usize = 24;
/// The batch normalization epsilon PyTorch uses by default.
const NORMALIZATION_EPSILON: f32 = 1e-5;

/// One convolution with its batch normalization folded in.
struct Convolution {
    input_channels: usize,
    output_channels: usize,
    /// Weights by tap, then output channel, then input channel.
    tap_weights: Vec<Vec<f32>>,
    bias: Vec<f32>,
}

/// The loaded network.
pub(super) struct Network {
    convolutions: [Convolution; 6],
    head_weights: Vec<f32>,
    head_bias: Vec<f32>,
}

#[derive(Deserialize)]
struct TensorEntry {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: [usize; 2],
}

fn invalid(reason: impl Into<String>) -> EuphoriaError {
    EuphoriaError::Detection {
        detector: super::DETECTOR_NAME,
        reason: reason.into(),
        source: None,
    }
}

/// The float32 tensors of a safetensors file, by name, with their shapes,
/// and its metadata.
type Tensors = HashMap<String, (Vec<usize>, Vec<f32>)>;

fn read_safetensors(bytes: &[u8]) -> Result<(Tensors, HashMap<String, String>), EuphoriaError> {
    let header_length = bytes
        .get(..8)
        .and_then(|prefix| <[u8; 8]>::try_from(prefix).ok())
        .map(u64::from_le_bytes)
        .and_then(|length| usize::try_from(length).ok())
        .ok_or_else(|| invalid("the weights file has no header"))?;
    let header_end = 8_usize
        .checked_add(header_length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| invalid("the weights header runs past the file"))?;
    let mut header: HashMap<String, serde_json::Value> =
        serde_json::from_slice(&bytes[8..header_end])
            .map_err(|error| invalid(format!("the weights header is not JSON: {error}")))?;
    let metadata: HashMap<String, String> = header
        .remove("__metadata__")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| invalid(format!("the weights metadata is not text: {error}")))?
        .unwrap_or_default();
    let data = &bytes[header_end..];
    let mut tensors = HashMap::new();
    for (name, value) in header {
        let entry: TensorEntry = serde_json::from_value(value)
            .map_err(|error| invalid(format!("tensor {name} is malformed: {error}")))?;
        if entry.dtype != "F32" {
            return Err(invalid(format!("tensor {name} is {}, not F32", entry.dtype)));
        }
        let [start, end] = entry.data_offsets;
        let element_count: usize = entry.shape.iter().product();
        let raw = data
            .get(start..end)
            .filter(|raw| raw.len() == element_count * 4)
            .ok_or_else(|| invalid(format!("tensor {name} does not fit its offsets")))?;
        let values = raw
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        tensors.insert(name, (entry.shape, values));
    }
    Ok((tensors, metadata))
}

impl Network {
    /// The network Euphoria ships.
    pub(super) fn shipped() -> Result<Self, EuphoriaError> {
        Self::from_safetensors(WEIGHTS)
    }

    fn from_safetensors(bytes: &[u8]) -> Result<Self, EuphoriaError> {
        let (mut tensors, metadata) = read_safetensors(bytes)?;
        for (field, expected) in [("feature_spec", FEATURE_SPEC), ("label_schema", LABEL_SCHEMA)] {
            if metadata.get(field).map(String::as_str) != Some(expected) {
                return Err(invalid(format!("the weights do not declare {field} {expected}")));
            }
        }
        let mut take = |name: &str, shape: &[usize]| -> Result<Vec<f32>, EuphoriaError> {
            let (found_shape, values) = tensors
                .remove(name)
                .ok_or_else(|| invalid(format!("tensor {name} is missing")))?;
            if found_shape != shape {
                return Err(invalid(format!(
                    "tensor {name} has shape {found_shape:?}, not {shape:?}"
                )));
            }
            Ok(values)
        };
        let channels = [(1, 16), (16, 16), (16, 32), (32, 32), (32, 64), (64, 64)];
        let layer_indices = [(0, 1), (3, 4), (7, 8), (10, 11), (14, 15), (17, 18)];
        let mut convolutions = Vec::with_capacity(6);
        for ((input_channels, output_channels), (convolution, normalization)) in
            channels.into_iter().zip(layer_indices)
        {
            let weight = take(
                &format!("net.{convolution}.weight"),
                &[output_channels, input_channels, 3, 3],
            )?;
            let bias = take(&format!("net.{convolution}.bias"), &[output_channels])?;
            let scale = take(&format!("net.{normalization}.weight"), &[output_channels])?;
            let shift = take(&format!("net.{normalization}.bias"), &[output_channels])?;
            let mean = take(&format!("net.{normalization}.running_mean"), &[output_channels])?;
            let variance = take(&format!("net.{normalization}.running_var"), &[output_channels])?;
            let gains: Vec<f32> = scale
                .iter()
                .zip(&variance)
                .map(|(scale, variance)| scale / (variance + NORMALIZATION_EPSILON).sqrt())
                .collect();
            let tap_weights = (0..9)
                .map(|tap| {
                    (0..output_channels)
                        .flat_map(|output| {
                            let weight = &weight;
                            let gain = gains[output];
                            (0..input_channels).map(move |input| {
                                weight[(output * input_channels + input) * 9 + tap] * gain
                            })
                        })
                        .collect()
                })
                .collect();
            let bias = (0..output_channels)
                .map(|output| (bias[output] - mean[output]) * gains[output] + shift[output])
                .collect();
            convolutions.push(Convolution {
                input_channels,
                output_channels,
                tap_weights,
                bias,
            });
        }
        let head_inputs = 64 * (CORE_BINS / 8);
        let head_weights = take("head.weight", &[KEY_COUNT, head_inputs])?;
        let head_bias = take("head.bias", &[KEY_COUNT])?;
        let convolutions: [Convolution; 6] = convolutions
            .try_into()
            .map_err(|_| invalid("the network needs six convolutions"))?;
        Ok(Self {
            convolutions,
            head_weights,
            head_bias,
        })
    }

    /// The scores of the 24 keys, before softmax, for features of
    /// [`CORE_BINS`] rows stored row after row, or `None` when there are
    /// fewer than [`MINIMUM_FRAMES`] frames.
    pub(super) fn logits(&self, features: &[f32], frame_count: usize) -> Option<[f32; KEY_COUNT]> {
        if frame_count < MINIMUM_FRAMES || features.len() != CORE_BINS * frame_count {
            return None;
        }
        let final_columns = frame_count / 8;
        let final_rows = CORE_BINS / 8;
        let channels = 64;
        let mut sums = vec![0.0_f32; channels * final_rows];
        let mut stretch_start = 0;
        while stretch_start < frame_count {
            let stretch_end = (stretch_start + STRETCH_FRAMES).min(frame_count);
            let read_start = stretch_start.saturating_sub(CONTEXT_FRAMES);
            let read_end = (stretch_end + CONTEXT_FRAMES).min(frame_count);
            let pooled = self.blocks(features, frame_count, read_start..read_end);
            let first_column = stretch_start / 8 - read_start / 8;
            let last_column = stretch_end.min(final_columns * 8) / 8 - read_start / 8;
            for (channel_row, sum) in sums.iter_mut().enumerate() {
                let row = pooled.row(channel_row / final_rows, channel_row % final_rows);
                *sum += row[first_column..last_column].iter().sum::<f32>();
            }
            stretch_start = stretch_end;
        }
        let means: Vec<f32> = sums
            .iter()
            .map(|sum| sum / final_columns as f32)
            .collect();
        Some(std::array::from_fn(|key| {
            let weights = &self.head_weights[key * means.len()..(key + 1) * means.len()];
            self.head_bias[key]
                + weights
                    .iter()
                    .zip(&means)
                    .map(|(weight, mean)| weight * mean)
                    .sum::<f32>()
        }))
    }

    /// The three blocks run on the frames in `columns`, as if those were
    /// the whole track.
    fn blocks(
        &self,
        features: &[f32],
        frame_count: usize,
        columns: std::ops::Range<usize>,
    ) -> Activations {
        let mut activations = Activations::zeroed(1, CORE_BINS, columns.len());
        for row in 0..CORE_BINS {
            let source = &features[row * frame_count + columns.start..row * frame_count + columns.end];
            activations.row_mut(0, row).copy_from_slice(source);
        }
        for pair in self.convolutions.chunks_exact(2) {
            activations = pair[0].apply(&activations);
            activations = pair[1].apply(&activations);
            activations = activations.max_pooled();
        }
        activations
    }
}

/// Feature maps by channel, each with a border of one zero all around.
struct Activations {
    channels: usize,
    rows: usize,
    columns: usize,
    values: Vec<f32>,
}

impl Activations {
    fn zeroed(channels: usize, rows: usize, columns: usize) -> Self {
        Self {
            channels,
            rows,
            columns,
            values: vec![0.0; channels * (rows + 2) * (columns + 2)],
        }
    }

    fn stride(&self) -> usize {
        self.columns + 2
    }

    fn plane(&self) -> usize {
        (self.rows + 2) * self.stride()
    }

    fn offset(&self, channel: usize, row: usize) -> usize {
        channel * self.plane() + (row + 1) * self.stride() + 1
    }

    fn row(&self, channel: usize, row: usize) -> &[f32] {
        let start = self.offset(channel, row);
        &self.values[start..start + self.columns]
    }

    fn row_mut(&mut self, channel: usize, row: usize) -> &mut [f32] {
        let start = self.offset(channel, row);
        let columns = self.columns;
        &mut self.values[start..start + columns]
    }

    /// The 2x2 maximum, dropping a last odd row or column as PyTorch does.
    fn max_pooled(&self) -> Self {
        let mut pooled = Self::zeroed(self.channels, self.rows / 2, self.columns / 2);
        for channel in 0..self.channels {
            for row in 0..pooled.rows {
                let upper = self.row(channel, 2 * row);
                let lower = self.row(channel, 2 * row + 1);
                let output = pooled.row_mut(channel, row);
                for (column, value) in output.iter_mut().enumerate() {
                    *value = upper[2 * column]
                        .max(upper[2 * column + 1])
                        .max(lower[2 * column])
                        .max(lower[2 * column + 1]);
                }
            }
        }
        pooled
    }
}

impl Convolution {
    /// The convolution with zero padding of one, then the ELU.
    fn apply(&self, input: &Activations) -> Activations {
        debug_assert_eq!(input.channels, self.input_channels);
        let mut output = Activations::zeroed(self.output_channels, input.rows, input.columns);
        let stride = input.stride();
        let plane = input.plane();
        let first = stride + 1;
        let span = (input.rows - 1) * stride + input.columns;
        for (tap, weights) in self.tap_weights.iter().enumerate() {
            let row_shift = tap / 3;
            let column_shift = tap % 3;
            let source_start = first + row_shift * stride + column_shift - stride - 1;
            // SAFETY: the weights hold output channels times input channels
            // values read with those strides. Every source index is
            // `source_start + channel * plane + position` with `position`
            // below `span`, which stays inside the input plane because the
            // shift moves at most one row and one column into its border.
            // Every destination index is `first + channel * plane + position`,
            // inside the output plane, which has the same shape. The input
            // and output are distinct allocations.
            unsafe {
                matrixmultiply::sgemm(
                    self.output_channels,
                    self.input_channels,
                    span,
                    1.0,
                    weights.as_ptr(),
                    self.input_channels as isize,
                    1,
                    input.values.as_ptr().add(source_start),
                    plane as isize,
                    1,
                    if tap == 0 { 0.0 } else { 1.0 },
                    output.values.as_mut_ptr().add(first),
                    plane as isize,
                    1,
                );
            }
        }
        for channel in 0..self.output_channels {
            let bias = self.bias[channel];
            for row in 0..output.rows {
                for value in output.row_mut(channel, row) {
                    let shifted = *value + bias;
                    *value = if shifted > 0.0 { shifted } else { shifted.exp_m1() };
                }
            }
            let plane_start = channel * plane;
            for row in 0..output.rows {
                let row_start = plane_start + (row + 1) * stride;
                output.values[row_start] = 0.0;
                output.values[row_start + stride - 1] = 0.0;
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_weights_load() {
        let network = Network::shipped().unwrap();
        assert_eq!(network.head_weights.len(), KEY_COUNT * 64 * 24);
        let parameters: usize = network
            .convolutions
            .iter()
            .map(|convolution| convolution.tap_weights.iter().map(Vec::len).sum::<usize>() + convolution.bias.len())
            .sum::<usize>()
            + network.head_weights.len()
            + network.head_bias.len();
        let normalization_parameters = 2 * (16 + 16 + 32 + 32 + 64 + 64);
        assert_eq!(parameters + normalization_parameters, 109_128);
    }

    #[test]
    fn too_few_frames_give_no_scores() {
        let network = Network::shipped().unwrap();
        assert!(network.logits(&vec![0.0; CORE_BINS * 7], 7).is_none());
        assert!(network.logits(&vec![0.0; CORE_BINS * 8], 8).is_some());
    }

    #[test]
    fn reading_in_stretches_gives_the_scores_of_reading_at_once() {
        let network = Network::shipped().unwrap();
        for frame_count in [8, 37, 290] {
            let features: Vec<f32> = (0..CORE_BINS * frame_count)
                .map(|index| ((index * 2_654_435_761) % 1_000) as f32 / 500.0)
                .collect();
            let streamed = network.logits(&features, frame_count).unwrap();
            let whole = network.blocks(&features, frame_count, 0..frame_count);
            let columns = frame_count / 8;
            let means: Vec<f32> = (0..64 * 24)
                .map(|index| whole.row(index / 24, index % 24)[..columns].iter().sum::<f32>() / columns as f32)
                .collect();
            for key in 0..KEY_COUNT {
                let expected: f32 = network.head_bias[key]
                    + network.head_weights[key * means.len()..(key + 1) * means.len()]
                        .iter()
                        .zip(&means)
                        .map(|(weight, mean)| weight * mean)
                        .sum::<f32>();
                assert!((streamed[key] - expected).abs() < 1e-3, "{frame_count} {key}");
            }
        }
    }

    #[test]
    fn malformed_weights_are_refused() {
        assert!(Network::from_safetensors(b"").is_err());
        assert!(Network::from_safetensors(&[16, 0, 0, 0, 0, 0, 0, 0, b'{', b'}']).is_err());
        let mut truncated = WEIGHTS.to_vec();
        truncated.truncate(WEIGHTS.len() - 4);
        assert!(Network::from_safetensors(&truncated).is_err());
    }
}
