// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing stems as WAV files.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

const BYTES_PER_SAMPLE: u16 = 2;
const INTEGER_FORMAT: u16 = 1;

/// Writes interleaved samples to a 16 bit WAV file at `path`, replacing it.
/// Samples beyond full scale are clipped.
#[cfg(test)]
pub(crate) fn write_wav(
    path: &Path,
    interleaved: &[f32],
    channels: u16,
    sample_rate: u32,
) -> io::Result<()> {
    let frames = interleaved.len() / usize::from(channels.max(1));
    let mut writer = WavWriter::create(path, frames, channels, sample_rate)?;
    writer.append(interleaved)?;
    writer.finish()
}

/// A 16 bit WAV file written a block at a time, whose length is known
/// before the first block.
pub(crate) struct WavWriter {
    writer: BufWriter<File>,
    samples_left: usize,
}

impl WavWriter {
    /// Creates the file at `path` for `frames` frames of `channels`
    /// channels and writes its header.
    pub(crate) fn create(
        path: &Path,
        frames: usize,
        channels: u16,
        sample_rate: u32,
    ) -> io::Result<Self> {
        let samples = frames * usize::from(channels);
        let mut writer = BufWriter::new(File::create(path)?);
        write_header(&mut writer, samples, channels, sample_rate)?;
        Ok(Self {
            writer,
            samples_left: samples,
        })
    }

    /// Appends interleaved samples, clipping those beyond full scale.
    pub(crate) fn append(&mut self, interleaved: &[f32]) -> io::Result<()> {
        if interleaved.len() > self.samples_left {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "more audio than the WAV file was created for",
            ));
        }
        self.samples_left -= interleaved.len();
        write_samples(&mut self.writer, interleaved)
    }

    /// Writes what is buffered and makes sure it reaches the disk. Fails
    /// when fewer samples were appended than the file was created for.
    pub(crate) fn finish(self) -> io::Result<()> {
        if self.samples_left != 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "less audio than the WAV file was created for",
            ));
        }
        self.writer
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()
    }
}

fn write_header(
    writer: &mut impl Write,
    samples: usize,
    channels: u16,
    sample_rate: u32,
) -> io::Result<()> {
    let too_long = || io::Error::new(io::ErrorKind::InvalidInput, "too much audio for a WAV file");
    let data_bytes =
        u32::try_from(samples * usize::from(BYTES_PER_SAMPLE)).map_err(|_| too_long())?;
    let riff_bytes = data_bytes.checked_add(36).ok_or_else(too_long)?;
    let block_align = channels * BYTES_PER_SAMPLE;
    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_bytes.to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16u32.to_le_bytes())?;
    writer.write_all(&INTEGER_FORMAT.to_le_bytes())?;
    writer.write_all(&channels.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&(sample_rate * u32::from(block_align)).to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&(BYTES_PER_SAMPLE * 8).to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_bytes.to_le_bytes())
}

fn write_samples(writer: &mut impl Write, interleaved: &[f32]) -> io::Result<()> {
    for sample in interleaved {
        let scaled = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        writer.write_all(&scaled.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_canonical_header_and_clips_integers() {
        let mut bytes = Vec::new();
        write_header(&mut bytes, 4, 2, 44_100).unwrap();
        write_samples(&mut bytes, &[0.0, 1.0, -2.0, 0.5]).unwrap();
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 44);
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            44_100 * 4
        );
        let samples: Vec<i16> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(samples, [0, 32767, -32767, 16384]);
    }

    #[test]
    fn a_streamed_file_must_get_exactly_the_audio_it_was_created_for() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("stem.wav");
        let mut writer = WavWriter::create(&path, 3, 2, 44_100).unwrap();
        writer.append(&[0.1, 0.2, 0.3, 0.4]).unwrap();
        writer.append(&[0.5, 0.6]).unwrap();
        assert!(writer.append(&[0.7, 0.8]).is_err());
        writer.finish().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 44 + 12);

        let short = WavWriter::create(&folder.path().join("short.wav"), 3, 2, 44_100).unwrap();
        assert!(short.finish().is_err());
    }
}
