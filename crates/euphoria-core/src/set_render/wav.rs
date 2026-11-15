// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A 24 bit WAV writer with no length limit: a file that outgrows the 4 GiB
//! a RIFF header can count becomes RF64 when it is finished, through the
//! JUNK chunk reserved for that at the start.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};

use crate::shift::to_integer;

const BYTES_PER_SAMPLE: u16 = 3;
/// RIFF, its size and WAVE.
const RIFF_HEADER_BYTES: u64 = 12;
/// The JUNK chunk that becomes ds64: its 8 byte header and 28 bytes.
const RESERVED_BYTES: u64 = 8 + 28;
/// The fmt chunk: its 8 byte header and 16 bytes.
const FORMAT_BYTES: u64 = 8 + 16;
/// Where the samples start.
const DATA_OFFSET: u64 = RIFF_HEADER_BYTES + RESERVED_BYTES + FORMAT_BYTES + 8;

pub(super) struct WavWriter {
    file: BufWriter<File>,
    channels: u16,
    sample_rate: u32,
    data_bytes: u64,
}

impl WavWriter {
    pub(super) fn new(file: File, channels: u16, sample_rate: u32) -> std::io::Result<Self> {
        let mut writer = Self {
            file: BufWriter::new(file),
            channels,
            sample_rate,
            data_bytes: 0,
        };
        writer.write_header(false)?;
        Ok(writer)
    }

    pub(super) fn write(&mut self, interleaved: &[f32]) -> std::io::Result<()> {
        for sample in interleaved {
            let bytes = to_integer(*sample).to_le_bytes();
            self.file.write_all(&bytes[..3])?;
        }
        self.data_bytes += interleaved.len() as u64 * u64::from(BYTES_PER_SAMPLE);
        Ok(())
    }

    /// Completes the header and returns the samples written, counting every
    /// channel.
    pub(super) fn finish(mut self) -> std::io::Result<u64> {
        let large = DATA_OFFSET - 8 + self.data_bytes > u64::from(u32::MAX);
        self.file.seek(SeekFrom::Start(0))?;
        self.write_header(large)?;
        self.file.flush()?;
        Ok(self.data_bytes / u64::from(BYTES_PER_SAMPLE))
    }

    fn write_header(&mut self, large: bool) -> std::io::Result<()> {
        let riff_size = DATA_OFFSET - 8 + self.data_bytes;
        let block_align = self.channels * BYTES_PER_SAMPLE;
        let mut header = Vec::with_capacity(DATA_OFFSET as usize);
        if large {
            header.extend_from_slice(b"RF64");
            header.extend_from_slice(&u32::MAX.to_le_bytes());
            header.extend_from_slice(b"WAVE");
            header.extend_from_slice(b"ds64");
            header.extend_from_slice(&28_u32.to_le_bytes());
            header.extend_from_slice(&riff_size.to_le_bytes());
            header.extend_from_slice(&self.data_bytes.to_le_bytes());
            let frames = self.data_bytes / u64::from(block_align);
            header.extend_from_slice(&frames.to_le_bytes());
            header.extend_from_slice(&0_u32.to_le_bytes());
        } else {
            header.extend_from_slice(b"RIFF");
            header.extend_from_slice(&(riff_size as u32).to_le_bytes());
            header.extend_from_slice(b"WAVE");
            header.extend_from_slice(b"JUNK");
            header.extend_from_slice(&28_u32.to_le_bytes());
            header.extend_from_slice(&[0; 28]);
        }
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&16_u32.to_le_bytes());
        header.extend_from_slice(&1_u16.to_le_bytes());
        header.extend_from_slice(&self.channels.to_le_bytes());
        header.extend_from_slice(&self.sample_rate.to_le_bytes());
        let byte_rate = self.sample_rate * u32::from(block_align);
        header.extend_from_slice(&byte_rate.to_le_bytes());
        header.extend_from_slice(&block_align.to_le_bytes());
        header.extend_from_slice(&(BYTES_PER_SAMPLE * 8).to_le_bytes());
        header.extend_from_slice(b"data");
        let data_size = if large {
            u32::MAX
        } else {
            self.data_bytes as u32
        };
        header.extend_from_slice(&data_size.to_le_bytes());
        self.file.write_all(&header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_file_header_becomes_rf64_in_place() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("big.wav");
        let mut writer = WavWriter::new(File::create(&path).unwrap(), 2, 44_100).unwrap();
        writer.data_bytes = u64::from(u32::MAX);
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        writer.write_header(true).unwrap();
        writer.file.flush().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, DATA_OFFSET);
        assert_eq!(&bytes[0..4], b"RF64");
        assert_eq!(&bytes[12..16], b"ds64");
        let data_size = u64::from_le_bytes(bytes[28..36].try_into().unwrap());
        assert_eq!(data_size, u64::from(u32::MAX));
        assert_eq!(&bytes[48..52], b"fmt ");
        assert_eq!(&bytes[72..76], b"data");
    }
}
