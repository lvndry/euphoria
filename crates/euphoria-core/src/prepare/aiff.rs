// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A 24 bit AIFF writer: the sound is written as it comes and the sizes
//! are patched in at the end.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::EuphoriaError;
use crate::shift::{BITS_PER_SAMPLE, to_integer, write_error};

const BYTES_PER_SAMPLE: u64 = BITS_PER_SAMPLE as u64 / 8;
const HEADER_BYTES: u64 = 54;

pub(super) struct AiffWriter {
    path: PathBuf,
    file: BufWriter<File>,
    channels: u16,
    samples: u64,
}

impl AiffWriter {
    pub(super) fn new(
        file: File,
        path: &Path,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, EuphoriaError> {
        let mut writer = Self {
            path: path.to_path_buf(),
            file: BufWriter::new(file),
            channels,
            samples: 0,
        };
        let header = header(channels, 0, sample_rate);
        writer
            .file
            .write_all(&header)
            .map_err(|error| write_error(path, error.to_string()))?;
        Ok(writer)
    }

    pub(super) fn write(&mut self, interleaved: &[f32]) -> Result<(), EuphoriaError> {
        let total = self.samples + interleaved.len() as u64;
        if total * BYTES_PER_SAMPLE + HEADER_BYTES + 1 > u64::from(u32::MAX) {
            return Err(write_error(
                &self.path,
                "the AIFF would exceed its 4 GiB limit; use FLAC for a longer copy".to_string(),
            ));
        }
        let mut bytes = Vec::with_capacity(interleaved.len() * 3);
        for sample in interleaved {
            let value = to_integer(*sample);
            bytes.extend_from_slice(&value.to_be_bytes()[1..]);
        }
        self.file
            .write_all(&bytes)
            .map_err(|error| write_error(&self.path, error.to_string()))?;
        self.samples = total;
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<u64, EuphoriaError> {
        let failure = |error: std::io::Error| write_error(&self.path, error.to_string());
        let data_bytes = self.samples * BYTES_PER_SAMPLE;
        if data_bytes % 2 == 1 {
            self.file.write_all(&[0]).map_err(failure)?;
        }
        let frames = self.samples / u64::from(self.channels);
        let padded = data_bytes + data_bytes % 2;
        self.file.seek(SeekFrom::Start(4)).map_err(failure)?;
        let form_size = (HEADER_BYTES - 8 + padded) as u32;
        self.file
            .write_all(&form_size.to_be_bytes())
            .map_err(failure)?;
        self.file.seek(SeekFrom::Start(22)).map_err(failure)?;
        self.file
            .write_all(&(frames as u32).to_be_bytes())
            .map_err(failure)?;
        self.file.seek(SeekFrom::Start(42)).map_err(failure)?;
        let chunk_size = (8 + data_bytes) as u32;
        self.file
            .write_all(&chunk_size.to_be_bytes())
            .map_err(failure)?;
        self.file.flush().map_err(failure)?;
        Ok(self.samples)
    }
}

fn header(channels: u16, frames: u32, sample_rate: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_BYTES as usize);
    bytes.extend_from_slice(b"FORM");
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(b"AIFF");
    bytes.extend_from_slice(b"COMM");
    bytes.extend_from_slice(&18_u32.to_be_bytes());
    bytes.extend_from_slice(&channels.to_be_bytes());
    bytes.extend_from_slice(&frames.to_be_bytes());
    bytes.extend_from_slice(&BITS_PER_SAMPLE.to_be_bytes());
    bytes.extend_from_slice(&extended_precision(sample_rate));
    bytes.extend_from_slice(b"SSND");
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes
}

/// A whole number as the 80 bit IEEE extended float that AIFF stores sample
/// rates in.
fn extended_precision(value: u32) -> [u8; 10] {
    let value = u64::from(value.max(1));
    let exponent = 63 - value.leading_zeros();
    let mantissa = value << (63 - exponent);
    let biased = (16383 + exponent) as u16;
    let mut bytes = [0; 10];
    bytes[..2].copy_from_slice(&biased.to_be_bytes());
    bytes[2..].copy_from_slice(&mantissa.to_be_bytes());
    bytes
}
