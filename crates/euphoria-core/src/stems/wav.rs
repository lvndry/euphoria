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
pub(crate) fn write_wav(
    path: &Path,
    interleaved: &[f32],
    channels: u16,
    sample_rate: u32,
) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    write_wav_to(&mut writer, interleaved, channels, sample_rate)?;
    writer
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()
}

fn write_wav_to(
    writer: &mut impl Write,
    interleaved: &[f32],
    channels: u16,
    sample_rate: u32,
) -> io::Result<()> {
    let too_long = || io::Error::new(io::ErrorKind::InvalidInput, "too much audio for a WAV file");
    let data_bytes =
        u32::try_from(interleaved.len() * usize::from(BYTES_PER_SAMPLE)).map_err(|_| too_long())?;
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
    writer.write_all(&data_bytes.to_le_bytes())?;
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
        write_wav_to(&mut bytes, &[0.0, 1.0, -2.0, 0.5], 2, 44_100).unwrap();
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 44);
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            44_100 * 4
        );
        let samples: Vec<i16> = bytes[44..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(samples, [0, 32767, -32767, 16384]);
    }
}
