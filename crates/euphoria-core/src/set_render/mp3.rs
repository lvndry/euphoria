// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An MP3 writer for mixes: LAME, built from the source vendored in the
//! mp3lame-sys crate, encodes constant bitrate frames a block at a time.
//! The file starts with an ID3v2 tag that names the mix and holds one
//! chapter per track, and LAME's info frame, rewritten once the stream is
//! complete, tells players the exact length.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};

use mp3lame_encoder::{Bitrate, Builder, Encoder, FlushNoGap, InterleavedPcm, Quality};

/// A chapter of the mix: a track, from where it starts to where the next
/// one does.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Chapter {
    pub(super) title: String,
    pub(super) start_milliseconds: u32,
    pub(super) end_milliseconds: u32,
}

/// The most chapters the table of contents can list.
const MOST_CHAPTERS: usize = 255;

/// The largest MP3 frame LAME writes, in bytes, for the flush buffer.
const LARGEST_FRAME_BYTES: usize = 7_200;

pub(super) struct Mp3Writer {
    file: BufWriter<File>,
    encoder: Encoder,
    audio_offset: u64,
    encoded: Vec<u8>,
    samples: u64,
}

impl Mp3Writer {
    pub(super) fn new(
        file: File,
        sample_rate: u32,
        bitrate_kbps: u16,
        title: &str,
        chapters: &[Chapter],
    ) -> Result<Self, String> {
        let bitrate = bitrate_of(bitrate_kbps)
            .ok_or_else(|| format!("{bitrate_kbps} kbps is not an MP3 bitrate"))?;
        let mut builder = Builder::new().ok_or("the MP3 encoder could not be created")?;
        let failed =
            |error: mp3lame_encoder::BuildError| format!("the MP3 encoder refused: {error}");
        builder.set_num_channels(2).map_err(failed)?;
        builder.set_sample_rate(sample_rate).map_err(failed)?;
        builder.set_brate(bitrate).map_err(failed)?;
        builder.set_quality(Quality::Best).map_err(failed)?;
        builder.set_to_write_vbr_tag(true).map_err(failed)?;
        let encoder = builder.build().map_err(failed)?;
        let mut file = BufWriter::new(file);
        let tag = id3_tag(title, chapters);
        file.write_all(&tag).map_err(|error| error.to_string())?;
        Ok(Self {
            file,
            encoder,
            audio_offset: tag.len() as u64,
            encoded: Vec::new(),
            samples: 0,
        })
    }

    pub(super) fn write(&mut self, interleaved: &[f32]) -> Result<(), String> {
        self.encoded.clear();
        self.encoded
            .reserve(mp3lame_encoder::max_required_buffer_size(
                interleaved.len() / 2,
            ));
        self.encoder
            .encode_to_vec(InterleavedPcm(interleaved), &mut self.encoded)
            .map_err(|error| format!("the MP3 encoder failed: {error}"))?;
        self.file
            .write_all(&self.encoded)
            .map_err(|error| error.to_string())?;
        self.samples += interleaved.len() as u64;
        Ok(())
    }

    /// Flushes the encoder, writes the info frame over the one LAME left
    /// at the start of the stream, and returns the samples encoded,
    /// counting every channel.
    pub(super) fn finish(mut self) -> Result<u64, String> {
        self.encoded.clear();
        self.encoded.reserve(LARGEST_FRAME_BYTES);
        self.encoder
            .flush_to_vec::<FlushNoGap>(&mut self.encoded)
            .map_err(|error| format!("the MP3 encoder failed: {error}"))?;
        self.file
            .write_all(&self.encoded)
            .map_err(|error| error.to_string())?;
        let mut info = Vec::new();
        if self.encoder.lame_tag_encode_to_vec(&mut info).is_some() {
            self.file
                .seek(SeekFrom::Start(self.audio_offset))
                .map_err(|error| error.to_string())?;
            self.file
                .write_all(&info)
                .map_err(|error| error.to_string())?;
        }
        self.file.flush().map_err(|error| error.to_string())?;
        Ok(self.samples)
    }
}

fn bitrate_of(kbps: u16) -> Option<Bitrate> {
    Some(match kbps {
        96 => Bitrate::Kbps96,
        112 => Bitrate::Kbps112,
        128 => Bitrate::Kbps128,
        160 => Bitrate::Kbps160,
        192 => Bitrate::Kbps192,
        224 => Bitrate::Kbps224,
        256 => Bitrate::Kbps256,
        320 => Bitrate::Kbps320,
        _ => return None,
    })
}

/// An ID3v2.3 tag with the title of the mix, a table of contents and one
/// chapter frame per track, as podcast players and some music players
/// show them.
pub(super) fn id3_tag(title: &str, chapters: &[Chapter]) -> Vec<u8> {
    let mut frames = Vec::new();
    frames.extend(text_frame(b"TIT2", title));
    let listed = &chapters[..chapters.len().min(MOST_CHAPTERS)];
    if !listed.is_empty() {
        let mut contents = b"toc\0".to_vec();
        contents.push(0x03);
        contents.push(listed.len() as u8);
        for index in 0..listed.len() {
            contents.extend(chapter_id(index));
        }
        frames.extend(frame(b"CTOC", &contents));
        for (index, chapter) in listed.iter().enumerate() {
            let mut contents = chapter_id(index);
            contents.extend(chapter.start_milliseconds.to_be_bytes());
            contents.extend(chapter.end_milliseconds.to_be_bytes());
            contents.extend(u32::MAX.to_be_bytes());
            contents.extend(u32::MAX.to_be_bytes());
            contents.extend(text_frame(b"TIT2", &chapter.title));
            frames.extend(frame(b"CHAP", &contents));
        }
    }
    let mut tag = b"ID3".to_vec();
    tag.extend([3, 0, 0]);
    tag.extend(syncsafe(frames.len() as u32));
    tag.extend(frames);
    tag
}

fn chapter_id(index: usize) -> Vec<u8> {
    format!("chp{index}\0").into_bytes()
}

/// A text frame in UTF-16 with a byte order mark, which every ID3v2.3
/// reader understands.
fn text_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
    let mut contents = vec![1, 0xFF, 0xFE];
    for unit in text.replace(['\r', '\n'], " ").encode_utf16() {
        contents.extend(unit.to_le_bytes());
    }
    frame(id, &contents)
}

fn frame(id: &[u8; 4], contents: &[u8]) -> Vec<u8> {
    let mut frame = id.to_vec();
    frame.extend((contents.len() as u32).to_be_bytes());
    frame.extend([0, 0]);
    frame.extend(contents);
    frame
}

fn syncsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_lists_every_chapter_in_a_table_of_contents() {
        let chapters = [
            Chapter {
                title: "One".to_string(),
                start_milliseconds: 0,
                end_milliseconds: 64_000,
            },
            Chapter {
                title: "Two".to_string(),
                start_milliseconds: 64_000,
                end_milliseconds: 128_500,
            },
        ];
        let tag = id3_tag("Friday", &chapters);
        assert_eq!(&tag[..5], b"ID3\x03\x00");
        let size = tag[6..10]
            .iter()
            .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
        assert_eq!(size, tag.len() - 10);
        let starts: Vec<usize> = tag
            .windows(4)
            .enumerate()
            .filter(|(_, window)| *window == b"CTOC" || *window == b"CHAP")
            .map(|(index, _)| index + 10)
            .collect();
        assert_eq!(starts.len(), 3);
        assert_eq!(&tag[starts[0]..starts[0] + 6], b"toc\0\x03\x02");
        let second = starts[2];
        assert_eq!(&tag[second..second + 5], b"chp1\0");
        let end = u32::from_be_bytes(tag[second + 9..second + 13].try_into().unwrap());
        assert_eq!(end, 128_500);
    }

    #[test]
    fn only_lame_bitrates_are_taken() {
        for kbps in super::super::MP3_BITRATES {
            assert!(bitrate_of(kbps).is_some(), "{kbps}");
        }
        assert!(bitrate_of(300).is_none());
    }
}
