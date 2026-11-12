// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Encodes and decodes the blobs of performance data in an Engine DJ
//! library, following libdjinterop
//! (<https://github.com/xsco/libdjinterop>,
//! `src/djinterop/engine/v2/track_data_blob.cpp`, `beat_data_blob.cpp`,
//! `quick_cues_blob.cpp` and `src/djinterop/engine/encode_decode_utils.cpp`).
//!
//! Every decoder checks each length before it reads, so a damaged blob is
//! an error and never a panic, and keeps the bytes after the fields it
//! knows so that encoding gives them back.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

/// The number of hot cue pads a track has in Engine DJ.
pub const QUICK_CUE_COUNT: usize = 8;

/// The sample offset of a hot cue or main cue that is not set.
pub const EMPTY_QUICK_CUE_OFFSET: f64 = -1.0;

const LENGTH_HEADER: usize = 4;
const TRACK_DATA_LENGTH: usize = 44;
const BEAT_DATA_HEADER_LENGTH: usize = 17;
const BEAT_DATA_MINIMUM_LENGTH: usize = 33;
const MARKER_LENGTH: usize = 24;
const QUICK_CUE_FIXED_LENGTH: usize = 13;
const MAIN_CUE_LENGTH: usize = 17;
const MAXIMUM_LABEL_LENGTH: usize = 255;

/// Reads fixed-size fields from a payload, failing instead of reading past
/// its end.
struct Reader<'payload> {
    payload: &'payload [u8],
    position: usize,
    what: &'static str,
}

impl<'payload> Reader<'payload> {
    fn new(payload: &'payload [u8], what: &'static str) -> Self {
        Self {
            payload,
            position: 0,
            what,
        }
    }

    fn take(&mut self, length: usize) -> Result<&'payload [u8], String> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.payload.len())
            .ok_or_else(|| format!("the {} is cut short", self.what))?;
        let bytes = &self.payload[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], String> {
        let mut array = [0; LENGTH];
        array.copy_from_slice(self.take(LENGTH)?);
        Ok(array)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.array::<1>()?[0])
    }

    fn rest(&mut self) -> Vec<u8> {
        let rest = self.payload[self.position..].to_vec();
        self.position = self.payload.len();
        rest
    }

    fn remaining(&self) -> usize {
        self.payload.len() - self.position
    }

    /// A count of items of `item_length` bytes each, rejected when it is
    /// negative or more than the bytes left could hold.
    fn count(&mut self, item_length: usize) -> Result<usize, String> {
        let count = i64::from_be_bytes(self.array()?);
        usize::try_from(count)
            .ok()
            .filter(|count| {
                count
                    .checked_mul(item_length)
                    .is_some_and(|length| length <= self.remaining())
            })
            .ok_or_else(|| format!("the {} holds a count of {count} it cannot", self.what))
    }
}

/// Compresses `payload` the way Engine DJ stores its blobs: a big endian
/// `u32` with the length of the payload, then the zlib stream of it (RFC
/// 1950, with header and Adler-32) at the default compression, Qt's
/// `qCompress` format.
pub fn compress(payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let mut blob = length.to_be_bytes().to_vec();
    let mut encoder = ZlibEncoder::new(blob.split_off(LENGTH_HEADER), Compression::default());
    let compressed = encoder
        .write_all(payload)
        .and_then(|()| encoder.finish())
        .unwrap_or_default();
    blob.extend_from_slice(&compressed);
    blob
}

/// Decompresses a blob made by [`compress`]. An empty blob gives an empty
/// payload, and so does a blob whose length header is 0.
///
/// # Errors
///
/// A blob of one to three bytes, a zlib stream that fails, or a stream whose
/// length differs from its header, as a short reason.
pub fn decompress(blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.is_empty() {
        return Ok(Vec::new());
    }
    let mut reader = Reader::new(blob, "blob");
    let declared_length = u32::from_be_bytes(reader.array()?) as usize;
    if declared_length == 0 {
        return Ok(Vec::new());
    }
    let mut restored = Vec::new();
    ZlibDecoder::new(&blob[LENGTH_HEADER..])
        .take(declared_length as u64 + 1)
        .read_to_end(&mut restored)
        .map_err(|error| format!("the zlib stream is not valid ({error})"))?;
    if restored.len() != declared_length {
        return Err(format!(
            "the blob holds {} bytes, not the {declared_length} its header says",
            restored.len()
        ));
    }
    Ok(restored)
}

/// The `trackData` blob: the sample rate, length and key Engine DJ found for
/// a track, and its loudness.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackData {
    /// The sample rate of the track, in hertz.
    pub sample_rate: f64,
    /// The length of the track in samples of one channel.
    pub samples: i64,
    /// The key, numbered as `Track.key` is.
    pub key: i32,
    /// The average loudness of the low, middle and high frequencies.
    pub average_loudness: [f64; 3],
    /// The bytes after the 44 libdjinterop describes. Newer Engine DJ
    /// writes 68-byte payloads, and the 24 extra bytes are kept.
    pub extra: Vec<u8>,
}

impl TrackData {
    /// Reads a decompressed `trackData` payload.
    ///
    /// # Errors
    ///
    /// When the payload is shorter than 44 bytes.
    pub fn decode(payload: &[u8]) -> Result<Self, String> {
        let mut reader = Reader::new(payload, "track data");
        Ok(Self {
            sample_rate: f64::from_be_bytes(reader.array()?),
            samples: i64::from_be_bytes(reader.array()?),
            key: i32::from_be_bytes(reader.array()?),
            average_loudness: [
                f64::from_be_bytes(reader.array()?),
                f64::from_be_bytes(reader.array()?),
                f64::from_be_bytes(reader.array()?),
            ],
            extra: reader.rest(),
        })
    }

    /// The payload, all big endian, then the extra bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(TRACK_DATA_LENGTH + self.extra.len());
        payload.extend_from_slice(&self.sample_rate.to_be_bytes());
        payload.extend_from_slice(&self.samples.to_be_bytes());
        payload.extend_from_slice(&self.key.to_be_bytes());
        for loudness in self.average_loudness {
            payload.extend_from_slice(&loudness.to_be_bytes());
        }
        payload.extend_from_slice(&self.extra);
        payload
    }
}

/// One marker of a beat grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatGridMarker {
    /// Where the beat is, in samples of one channel.
    pub sample_offset: f64,
    /// The number of the beat, counted from the first downbeat at 0.
    pub beat_number: i64,
    /// The number of beats to the next marker, 0 on the last.
    pub beats_to_next_marker: i32,
    /// A field libdjinterop names but does not explain, written as 0.
    pub unknown: i32,
}

/// The `beatData` blob: the beat grid Engine DJ found and the one the user
/// adjusted.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatData {
    /// The sample rate of the track, in hertz.
    pub sample_rate: f64,
    /// The length of the track in samples of one channel.
    pub samples: f64,
    /// 1 when the track has a beat grid.
    pub is_beatgrid_set: u8,
    /// The grid analysis found.
    pub default_grid: Vec<BeatGridMarker>,
    /// The grid in use, after the user's changes.
    pub adjusted_grid: Vec<BeatGridMarker>,
    /// The bytes after the grids, usually 9 zero bytes.
    pub extra: Vec<u8>,
}

impl BeatData {
    /// Reads a decompressed `beatData` payload: a big endian header and
    /// marker counts, and little endian markers.
    ///
    /// # Errors
    ///
    /// When the payload is cut short, or a grid's count is negative or
    /// larger than the bytes left could hold.
    pub fn decode(payload: &[u8]) -> Result<Self, String> {
        if payload.len() < BEAT_DATA_MINIMUM_LENGTH {
            return Err("the beat data is cut short".to_string());
        }
        let mut reader = Reader::new(payload, "beat data");
        let sample_rate = f64::from_be_bytes(reader.array()?);
        let samples = f64::from_be_bytes(reader.array()?);
        let is_beatgrid_set = reader.u8()?;
        debug_assert_eq!(reader.position, BEAT_DATA_HEADER_LENGTH);
        let default_grid = read_grid(&mut reader)?;
        let adjusted_grid = read_grid(&mut reader)?;
        Ok(Self {
            sample_rate,
            samples,
            is_beatgrid_set,
            default_grid,
            adjusted_grid,
            extra: reader.rest(),
        })
    }

    /// The payload in Engine DJ's layout.
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(
            BEAT_DATA_MINIMUM_LENGTH
                + MARKER_LENGTH * (self.default_grid.len() + self.adjusted_grid.len())
                + self.extra.len(),
        );
        payload.extend_from_slice(&self.sample_rate.to_be_bytes());
        payload.extend_from_slice(&self.samples.to_be_bytes());
        payload.push(self.is_beatgrid_set);
        write_grid(&mut payload, &self.default_grid);
        write_grid(&mut payload, &self.adjusted_grid);
        payload.extend_from_slice(&self.extra);
        payload
    }
}

fn read_grid(reader: &mut Reader<'_>) -> Result<Vec<BeatGridMarker>, String> {
    let count = reader.count(MARKER_LENGTH)?;
    let mut markers = Vec::with_capacity(count);
    for _ in 0..count {
        markers.push(BeatGridMarker {
            sample_offset: f64::from_le_bytes(reader.array()?),
            beat_number: i64::from_le_bytes(reader.array()?),
            beats_to_next_marker: i32::from_le_bytes(reader.array()?),
            unknown: i32::from_le_bytes(reader.array()?),
        });
    }
    Ok(markers)
}

fn write_grid(payload: &mut Vec<u8>, grid: &[BeatGridMarker]) {
    payload.extend_from_slice(&(grid.len() as i64).to_be_bytes());
    for marker in grid {
        payload.extend_from_slice(&marker.sample_offset.to_le_bytes());
        payload.extend_from_slice(&marker.beat_number.to_le_bytes());
        payload.extend_from_slice(&marker.beats_to_next_marker.to_le_bytes());
        payload.extend_from_slice(&marker.unknown.to_le_bytes());
    }
}

/// One hot cue pad.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickCue {
    /// The cue's name, at most 255 bytes of UTF-8.
    pub label: String,
    /// Where the cue is, in samples of one channel, or
    /// [`EMPTY_QUICK_CUE_OFFSET`] for a pad that is not set.
    pub sample_offset: f64,
    /// The pad's colour as alpha, red, green and blue.
    pub colour: [u8; 4],
}

/// The `quickCues` blob: the hot cue pads and the main cue.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickCues {
    /// The pads in order, eight in every library seen.
    pub cues: Vec<QuickCue>,
    /// The main cue in use, in samples.
    pub adjusted_main_cue: f64,
    /// 1 when the user moved the main cue.
    pub is_main_cue_adjusted: u8,
    /// The main cue analysis placed, in samples.
    pub default_main_cue: f64,
    /// The bytes after the main cue.
    pub extra: Vec<u8>,
}

impl QuickCues {
    /// Reads a decompressed `quickCues` payload.
    ///
    /// # Errors
    ///
    /// When the payload is cut short or its count is negative or larger
    /// than the bytes left could hold.
    pub fn decode(payload: &[u8]) -> Result<Self, String> {
        let mut reader = Reader::new(payload, "quick cues");
        let count = reader.count(QUICK_CUE_FIXED_LENGTH)?;
        let mut cues = Vec::with_capacity(count);
        for _ in 0..count {
            let label_length = usize::from(reader.u8()?);
            let label = String::from_utf8_lossy(reader.take(label_length)?).into_owned();
            cues.push(QuickCue {
                label,
                sample_offset: f64::from_be_bytes(reader.array()?),
                colour: reader.array()?,
            });
        }
        if reader.remaining() < MAIN_CUE_LENGTH {
            return Err("the quick cues are cut short".to_string());
        }
        Ok(Self {
            cues,
            adjusted_main_cue: f64::from_be_bytes(reader.array()?),
            is_main_cue_adjusted: reader.u8()?,
            default_main_cue: f64::from_be_bytes(reader.array()?),
            extra: reader.rest(),
        })
    }

    /// The payload in Engine DJ's layout. A label longer than 255 bytes is
    /// cut at the last character that fits.
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(self.cues.len() as i64).to_be_bytes());
        for cue in &self.cues {
            let mut end = cue.label.len().min(MAXIMUM_LABEL_LENGTH);
            while !cue.label.is_char_boundary(end) {
                end -= 1;
            }
            payload.push(end as u8);
            payload.extend_from_slice(&cue.label.as_bytes()[..end]);
            payload.extend_from_slice(&cue.sample_offset.to_be_bytes());
            payload.extend_from_slice(&cue.colour);
        }
        payload.extend_from_slice(&self.adjusted_main_cue.to_be_bytes());
        payload.push(self.is_main_cue_adjusted);
        payload.extend_from_slice(&self.default_main_cue.to_be_bytes());
        payload.extend_from_slice(&self.extra);
        payload
    }
}

/// A constant beat grid as Engine DJ keeps one: a marker four beats before
/// the first downbeat and one at or after the end of the track, as
/// libdjinterop's `normalize_beatgrid` (`src/djinterop/engine/engine.cpp`)
/// places them. Positions are in samples at `sample_rate`.
pub fn constant_beat_grid(
    sample_rate: f64,
    samples: i64,
    first_downbeat_seconds: f64,
    bpm: f64,
) -> Vec<BeatGridMarker> {
    let samples_per_beat = sample_rate * 60.0 / bpm;
    let first_downbeat = first_downbeat_seconds * sample_rate;
    let last_beat = ((samples as f64 - first_downbeat) / samples_per_beat)
        .ceil()
        .max(1.0) as i64;
    vec![
        BeatGridMarker {
            sample_offset: first_downbeat - 4.0 * samples_per_beat,
            beat_number: -4,
            beats_to_next_marker: i32::try_from(last_beat + 4).unwrap_or(i32::MAX),
            unknown: 0,
        },
        BeatGridMarker {
            sample_offset: first_downbeat + last_beat as f64 * samples_per_beat,
            beat_number: last_beat,
            beats_to_next_marker: 0,
            unknown: 0,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_writes_the_length_then_zlib() {
        let blob = compress(b"abc");
        assert!(blob.starts_with(&[0, 0, 0, 3, 0x78]), "{blob:?}");
        assert_eq!(decompress(&blob).expect("the round trip"), b"abc");
        assert_eq!(decompress(&[]).expect("an empty blob"), Vec::<u8>::new());
        assert_eq!(
            decompress(&[0, 0, 0, 0]).expect("a length of 0"),
            Vec::<u8>::new()
        );
        assert!(decompress(&[0, 0]).is_err());
        assert!(decompress(&[0, 0, 0, 9, 1, 2, 3]).is_err());
        let lying = [vec![0, 0, 0, 2], blob[4..].to_vec()].concat();
        assert!(decompress(&lying).is_err());
    }

    #[test]
    fn track_data_has_the_libdjinterop_layout() {
        let track_data = TrackData {
            sample_rate: 44100.0,
            samples: 1_000_000,
            key: 7,
            average_loudness: [0.5, 0.25, 0.125],
            extra: Vec::new(),
        };
        let expected = [
            44100.0f64.to_be_bytes().to_vec(),
            1_000_000i64.to_be_bytes().to_vec(),
            7i32.to_be_bytes().to_vec(),
            0.5f64.to_be_bytes().to_vec(),
            0.25f64.to_be_bytes().to_vec(),
            0.125f64.to_be_bytes().to_vec(),
        ]
        .concat();
        assert_eq!(track_data.encode(), expected);
        assert_eq!(
            TrackData::decode(&expected).expect("the 44 bytes"),
            track_data
        );
        assert!(TrackData::decode(&expected[..43]).is_err());
        let extended = [expected, vec![7u8; 24]].concat();
        assert_eq!(extended.len(), 68);
        let round_trip = TrackData::decode(&extended).expect("the 68 bytes");
        assert_eq!(round_trip.extra, vec![7; 24]);
        assert_eq!(round_trip.encode(), extended);
    }

    #[test]
    fn beat_data_mixes_big_and_little_endian() {
        let marker = BeatGridMarker {
            sample_offset: 100.0,
            beat_number: 3,
            beats_to_next_marker: 12,
            unknown: 9,
        };
        let beat_data = BeatData {
            sample_rate: 48000.0,
            samples: 480000.0,
            is_beatgrid_set: 1,
            default_grid: vec![marker, marker],
            adjusted_grid: vec![marker, marker],
            extra: vec![0; 9],
        };
        let payload = beat_data.encode();
        assert_eq!(payload.len(), 33 + 24 * 4 + 9);
        assert_eq!(&payload[0..8], 48000f64.to_be_bytes());
        assert_eq!(payload[16], 1);
        assert_eq!(&payload[17..25], 2i64.to_be_bytes());
        assert_eq!(&payload[25..33], marker.sample_offset.to_le_bytes());
        assert_eq!(&payload[33..41], marker.beat_number.to_le_bytes());
        assert_eq!(
            BeatData::decode(&payload).expect("the round trip"),
            beat_data
        );
        let lying = [
            48000.0f64.to_be_bytes().to_vec(),
            480000.0f64.to_be_bytes().to_vec(),
            vec![1],
            1_000_000i64.to_be_bytes().to_vec(),
        ]
        .concat();
        assert!(BeatData::decode(&lying).is_err());
        assert!(BeatData::decode(&payload[..32]).is_err());
    }

    #[test]
    fn quick_cues_round_trip() {
        let mut cues = vec![QuickCue {
            label: "Drop".to_string(),
            sample_offset: 1323000.0,
            colour: [0xFF, 0xEF, 0x81, 0x30],
        }];
        for _ in 0..7 {
            cues.push(QuickCue {
                label: String::new(),
                sample_offset: -1.0,
                colour: [0; 4],
            });
        }
        let quick_cues = QuickCues {
            cues,
            adjusted_main_cue: -1.0,
            is_main_cue_adjusted: 0,
            default_main_cue: -1.0,
            extra: Vec::new(),
        };
        let payload = quick_cues.encode();
        assert_eq!(&payload[0..8], 8i64.to_be_bytes());
        assert_eq!(payload[8], 4);
        assert_eq!(&payload[9..13], b"Drop");
        assert_eq!(
            QuickCues::decode(&payload).expect("the round trip"),
            quick_cues
        );
        assert!(QuickCues::decode(&payload[..payload.len() - 1]).is_err());
        let long_label = QuickCues {
            cues: vec![QuickCue {
                label: "x".repeat(300),
                sample_offset: 1.0,
                colour: [0; 4],
            }],
            adjusted_main_cue: -1.0,
            is_main_cue_adjusted: 0,
            default_main_cue: -1.0,
            extra: Vec::new(),
        };
        let long_payload = long_label.encode();
        assert_eq!(long_payload[8], 255);
    }

    #[test]
    fn the_grid_spans_the_track() {
        let grid = constant_beat_grid(44100.0, 44100 * 60, 0.5, 120.0);
        let [first, last] = grid.as_slice() else {
            panic!("two markers");
        };
        assert_eq!(
            first.sample_offset,
            0.5 * 44100.0 - 4.0 * 22050.0,
            "the first marker, four beats before the downbeat"
        );
        assert_eq!(first.beat_number, -4);
        assert_eq!(first.beats_to_next_marker, 123);
        assert_eq!(last.beat_number, 119);
        assert_eq!(
            last.sample_offset,
            22050.0 + 119.0 * 22050.0,
            "the last marker, at the end of the track"
        );
        assert!(last.sample_offset >= 2_646_000.0);
    }
}
