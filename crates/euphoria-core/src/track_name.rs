// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::meta::{MetadataRevision, StandardTag, Tag};

use crate::error::{DecodeFailure, EuphoriaError};

/// Longer `INFO` values are skipped. Real titles and artist names are far
/// shorter, and the size field of a damaged file can claim gigabytes.
const LONGEST_INFO_VALUE: u32 = 4096;

/// What a file's own tags say about the track.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackName {
    /// The track title, or `None` when the file has none.
    pub title: Option<String>,
    /// The artist, or `None` when the file has none.
    pub artist: Option<String>,
    /// The genre, or `None` when the file has none.
    pub genre: Option<String>,
}

/// Reads the title, artist and genre stored in an audio file, without decoding its
/// audio.
///
/// Reads ID3v2 and ID3v1 tags (MP3, and the ID3 chunk of AIFF files), Vorbis
/// comments (FLAC), MP4 metadata (M4A) and the `INFO` list of WAV files. An
/// ID3 chunk in a WAV file is not read. When a file carries several kinds of
/// tags, the first one in the file that has the value wins. A value that is
/// empty or only white space counts as missing.
///
/// # Errors
///
/// Returns [`EuphoriaError::Open`] when the file cannot be opened and
/// [`EuphoriaError::Decode`] when it is not an audio file the decoder
/// recognises or reading it fails partway.
pub fn read_track_name(path: &Path) -> Result<TrackName, EuphoriaError> {
    let mut format = crate::audio::open_format(path)?;
    let mut metadata = format.metadata();
    let mut tags = TrackName::default();
    loop {
        if let Some(revision) = metadata.current() {
            fill_from_revision(&mut tags, revision);
        }
        if metadata.pop().is_none() {
            break;
        }
    }
    if tags.title.is_none() || tags.artist.is_none() {
        fill_from_wav_info_list(&mut tags, path)?;
    }
    Ok(tags)
}

fn fill_from_revision(tags: &mut TrackName, revision: &MetadataRevision) {
    let media_tags = revision.media.tags.iter();
    let track_tags = revision
        .per_track
        .iter()
        .flat_map(|per_track| per_track.metadata.tags.iter());
    for tag in media_tags.chain(track_tags) {
        fill_from_tag(tags, tag);
    }
}

fn fill_from_tag(tags: &mut TrackName, tag: &Tag) {
    let (slot, value) = match &tag.std {
        Some(StandardTag::TrackTitle(title)) => (&mut tags.title, title),
        Some(StandardTag::Artist(artist)) => (&mut tags.artist, artist),
        Some(StandardTag::Genre(genre)) => (&mut tags.genre, genre),
        _ => return,
    };
    fill_slot(slot, value);
}

fn fill_slot(slot: &mut Option<String>, value: &str) {
    let value = value.trim();
    if slot.is_none() && !value.is_empty() {
        *slot = Some(value.to_string());
    }
}

/// Fills what is still missing from the `INFO` list of a WAV file. Symphonia
/// parses that list and then drops it, so it is read here. Files that are not
/// WAV are left alone.
fn fill_from_wav_info_list(tags: &mut TrackName, path: &Path) -> Result<(), EuphoriaError> {
    let file = File::open(path).map_err(|source| EuphoriaError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    read_wav_info_list(tags, &mut BufReader::new(file)).map_err(|error| EuphoriaError::Decode {
        path: path.to_path_buf(),
        failure: DecodeFailure::Read,
        source: Some(Box::new(error)),
    })
}

/// Walks the chunks of a WAV file up to its first `INFO` list. A file that
/// ends inside a chunk gives what was read before the cut.
fn read_wav_info_list(tags: &mut TrackName, reader: &mut (impl Read + Seek)) -> io::Result<()> {
    match walk_wav_chunks(tags, reader) {
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
        outcome => outcome,
    }
}

fn walk_wav_chunks(tags: &mut TrackName, reader: &mut (impl Read + Seek)) -> io::Result<()> {
    let mut file_header = [0_u8; 12];
    reader.read_exact(&mut file_header)?;
    if &file_header[..4] != b"RIFF" || &file_header[8..] != b"WAVE" {
        return Ok(());
    }
    loop {
        let (chunk_id, chunk_size) = read_chunk_header(reader)?;
        let mut unread = padded(chunk_size);
        if &chunk_id == b"LIST" && chunk_size >= 4 {
            let mut list_kind = [0_u8; 4];
            reader.read_exact(&mut list_kind)?;
            unread -= 4;
            if &list_kind == b"INFO" {
                return read_info_values(tags, reader, unread);
            }
        }
        reader.seek(SeekFrom::Current(unread))?;
    }
}

fn read_info_values(
    tags: &mut TrackName,
    reader: &mut (impl Read + Seek),
    list_size: i64,
) -> io::Result<()> {
    let mut unread = list_size;
    while unread >= 8 {
        let (value_id, value_size) = read_chunk_header(reader)?;
        unread -= 8 + padded(value_size);
        let slot = match &value_id {
            b"INAM" if value_size <= LONGEST_INFO_VALUE => &mut tags.title,
            b"IART" if value_size <= LONGEST_INFO_VALUE => &mut tags.artist,
            _ => {
                reader.seek(SeekFrom::Current(padded(value_size)))?;
                continue;
            }
        };
        let mut value = vec![0_u8; padded(value_size) as usize];
        reader.read_exact(&mut value)?;
        value.truncate(value_size as usize);
        fill_slot(slot, &info_text(&value));
    }
    Ok(())
}

fn read_chunk_header(reader: &mut impl Read) -> io::Result<([u8; 4], u32)> {
    let mut chunk_id = [0_u8; 4];
    let mut chunk_size = [0_u8; 4];
    reader.read_exact(&mut chunk_id)?;
    reader.read_exact(&mut chunk_size)?;
    Ok((chunk_id, u32::from_le_bytes(chunk_size)))
}

/// A chunk's size on disk: chunks are padded to an even number of bytes.
fn padded(chunk_size: u32) -> i64 {
    i64::from(chunk_size) + i64::from(chunk_size % 2)
}

/// The text of an `INFO` value, which ends at its first zero byte. The format
/// names no encoding: UTF-8 is tried first, and anything else is read as
/// Latin-1, the usual alternative.
fn info_text(value: &[u8]) -> String {
    let end = value
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(value.len());
    let value = &value[..end];
    match std::str::from_utf8(value) {
        Ok(text) => text.to_string(),
        Err(_) => value.iter().copied().map(char::from).collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn chunk(chunk_id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = chunk_id.to_vec();
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
        if body.len() % 2 == 1 {
            bytes.push(0);
        }
        bytes
    }

    fn wav(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((body.len() as u32 + 4).to_le_bytes());
        bytes.extend(b"WAVE");
        bytes.extend(body);
        bytes
    }

    fn info_list(values: &[Vec<u8>]) -> Vec<u8> {
        let mut body = b"INFO".to_vec();
        body.extend(values.concat());
        chunk(b"LIST", &body)
    }

    fn tags_of(file: Vec<u8>) -> TrackName {
        let mut tags = TrackName::default();
        read_wav_info_list(&mut tags, &mut Cursor::new(file)).unwrap();
        tags
    }

    #[test]
    fn reads_an_info_list_that_follows_the_samples() {
        let file = wav(&[
            chunk(b"fmt ", &[0; 16]),
            chunk(b"data", &[128; 7]),
            chunk(b"LIST", b"adtlignored"),
            info_list(&[
                chunk(b"ISFT", b"Some encoder\0"),
                chunk(b"IART", b"Saffron\0"),
                chunk(b"INAM", b"Low Tide\0\0"),
            ]),
        ]);
        assert_eq!(
            tags_of(file),
            TrackName {
                title: Some("Low Tide".to_string()),
                artist: Some("Saffron".to_string()),
                genre: None,
            }
        );
    }

    #[test]
    fn reads_text_that_is_not_utf8_as_latin_1() {
        let file = wav(&[info_list(&[
            chunk(b"INAM", b"D\xE9j\xE0 Vu"),
            chunk(b"IART", "Beyoncé".as_bytes()),
        ])]);
        let tags = tags_of(file);
        assert_eq!(tags.title.as_deref(), Some("Déjà Vu"));
        assert_eq!(tags.artist.as_deref(), Some("Beyoncé"));
    }

    #[test]
    fn keeps_values_found_earlier_and_skips_blank_and_oversized_ones() {
        let oversized = vec![b'a'; LONGEST_INFO_VALUE as usize + 1];
        let file = wav(&[info_list(&[
            chunk(b"INAM", b"From the list"),
            chunk(b"IART", &oversized),
            chunk(b"IART", b"  \0"),
        ])]);
        let mut tags = TrackName {
            title: Some("From ID3".to_string()),
            artist: None,
            genre: None,
        };
        read_wav_info_list(&mut tags, &mut Cursor::new(file)).unwrap();
        assert_eq!(tags.title.as_deref(), Some("From ID3"));
        assert_eq!(tags.artist, None);
    }

    #[test]
    fn a_wav_without_an_info_list_and_a_file_that_is_not_wav_have_no_tags() {
        let untagged = wav(&[chunk(b"fmt ", &[0; 16]), chunk(b"data", &[128; 8])]);
        assert_eq!(tags_of(untagged), TrackName::default());
        let not_wav = b"fLaC and then twelve bytes or more".to_vec();
        assert_eq!(tags_of(not_wav), TrackName::default());
    }

    #[test]
    fn a_file_cut_inside_a_value_keeps_the_values_before_the_cut() {
        let mut file = wav(&[info_list(&[
            chunk(b"IART", b"Saffron\0"),
            chunk(b"INAM", b"Low Tide"),
        ])]);
        file.truncate(file.len() - 3);
        let tags = tags_of(file);
        assert_eq!(tags.artist.as_deref(), Some("Saffron"));
        assert_eq!(tags.title, None);
    }
}
