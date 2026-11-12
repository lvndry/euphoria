// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the htdemucs weights from the safetensors file Demucs publishes:
//! an eight byte header length, a JSON header naming every tensor with its
//! type, shape and byte range, then the raw little endian data.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One tensor of the weights, converted to 32 bit floats.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WeightTensor {
    pub(crate) name: String,
    pub(crate) shape: Vec<i64>,
    pub(crate) values: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct TensorHeader {
    dtype: String,
    shape: Vec<i64>,
    data_offsets: [usize; 2],
}

/// Reads every tensor of a safetensors file held in memory. Only 16 and 32
/// bit floats are accepted, which is what the Demucs files hold.
pub(crate) fn read_safetensors(bytes: &[u8]) -> Result<Vec<WeightTensor>, String> {
    let header_length_bytes: [u8; 8] = bytes
        .get(..8)
        .and_then(|slice| slice.try_into().ok())
        .ok_or("the file is shorter than its header")?;
    let header_length = usize::try_from(u64::from_le_bytes(header_length_bytes))
        .map_err(|_| "the header length does not fit in memory")?;
    let header_end = 8usize
        .checked_add(header_length)
        .filter(|end| *end <= bytes.len())
        .ok_or("the header runs past the end of the file")?;
    let mut entries: BTreeMap<String, serde_json::Value> =
        serde_json::from_slice(&bytes[8..header_end]).map_err(|error| error.to_string())?;
    entries.remove("__metadata__");
    let data = &bytes[header_end..];

    let mut tensors = Vec::with_capacity(entries.len());
    for (name, entry) in entries {
        let header: TensorHeader =
            serde_json::from_value(entry).map_err(|error| format!("tensor {name}: {error}"))?;
        let [start, end] = header.data_offsets;
        let raw = data
            .get(start..end)
            .filter(|_| start <= end)
            .ok_or_else(|| format!("tensor {name} runs past the end of the file"))?;
        let element_count = header
            .shape
            .iter()
            .try_fold(1usize, |count, dimension| {
                usize::try_from(*dimension)
                    .ok()
                    .and_then(|dimension| count.checked_mul(dimension))
            })
            .ok_or_else(|| format!("tensor {name} has an impossible shape"))?;
        let values: Vec<f32> = match header.dtype.as_str() {
            "F16" => raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| half_to_single(u16::from_le_bytes(*pair)))
                .collect(),
            "F32" => raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|quad| f32::from_le_bytes(*quad))
                .collect(),
            other => return Err(format!("tensor {name} has the unsupported type {other}")),
        };
        if values.len() != element_count {
            return Err(format!(
                "tensor {name} holds {} values for a shape of {element_count}",
                values.len()
            ));
        }
        tensors.push(WeightTensor {
            name,
            shape: header.shape,
            values,
        });
    }
    Ok(tensors)
}

/// Converts an IEEE 754 half precision number to single precision exactly.
fn half_to_single(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let single_bits = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, _) => {
            let shift = mantissa.leading_zeros() - 21;
            let normalised = (mantissa << shift) & 0x3ff;
            sign | ((127 - 15 + 1 - shift) << 23) | (normalised << 13)
        }
        (0x1f, 0) => sign | 0x7f80_0000,
        (0x1f, _) => sign | 0x7fc0_0000 | (mantissa << 13),
        _ => sign | ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(single_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_precision_values_convert_exactly() {
        assert_eq!(half_to_single(0x0000), 0.0);
        assert_eq!(half_to_single(0x3c00), 1.0);
        assert_eq!(half_to_single(0xc000), -2.0);
        assert_eq!(half_to_single(0x3555), 0.333_251_95);
        assert_eq!(half_to_single(0x7bff), 65504.0);
        assert_eq!(half_to_single(0x0001), 2.0f32.powi(-24));
        assert_eq!(half_to_single(0x03ff), 1023.0 * 2.0f32.powi(-24));
        assert_eq!(half_to_single(0x7c00), f32::INFINITY);
        assert!(half_to_single(0x7e00).is_nan());
    }

    fn safetensors_file(header: &str, data: &[u8]) -> Vec<u8> {
        let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn reads_half_and_single_precision_tensors() {
        let header = r#"{"__metadata__":{"klass":"x"},"a":{"dtype":"F16","shape":[2],"data_offsets":[0,4]},"b":{"dtype":"F32","shape":[1,1],"data_offsets":[4,8]}}"#;
        let mut data = vec![0x00, 0x3c, 0x00, 0xc0];
        data.extend_from_slice(&0.5f32.to_le_bytes());
        let tensors = read_safetensors(&safetensors_file(header, &data)).unwrap();
        assert_eq!(
            tensors,
            vec![
                WeightTensor {
                    name: "a".into(),
                    shape: vec![2],
                    values: vec![1.0, -2.0],
                },
                WeightTensor {
                    name: "b".into(),
                    shape: vec![1, 1],
                    values: vec![0.5],
                },
            ]
        );
    }

    #[test]
    fn refuses_truncated_or_inconsistent_files() {
        assert!(read_safetensors(&[1, 0]).is_err());
        let past_end = r#"{"a":{"dtype":"F16","shape":[4],"data_offsets":[0,8]}}"#;
        assert!(read_safetensors(&safetensors_file(past_end, &[0; 4])).is_err());
        let wrong_shape = r#"{"a":{"dtype":"F16","shape":[3],"data_offsets":[0,4]}}"#;
        assert!(read_safetensors(&safetensors_file(wrong_shape, &[0; 4])).is_err());
        let integers = r#"{"a":{"dtype":"I64","shape":[1],"data_offsets":[0,8]}}"#;
        assert!(read_safetensors(&safetensors_file(integers, &[0; 8])).is_err());
    }
}
