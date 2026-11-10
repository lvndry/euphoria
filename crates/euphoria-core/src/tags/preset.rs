// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::TagFormat;
use super::native::MP4_TEMPO;

pub(super) const ID3_KEY: &str = "TKEY";
pub(super) const ID3_BPM: &str = "TBPM";
pub(super) const MP4_INITIAL_KEY: &str = "----:com.apple.iTunes:initialkey";
pub(super) const MP4_KEY: &str = "----:com.apple.iTunes:KEY";
pub(super) const MP4_FREEFORM_BPM: &str = "----:com.apple.iTunes:BPM";
pub(super) const VORBIS_INITIAL_KEY: &str = "INITIALKEY";
pub(super) const VORBIS_KEY: &str = "KEY";
pub(super) const VORBIS_BPM: &str = "BPM";
pub(super) const VORBIS_TEMPO: &str = "TEMPO";

/// The DJ program a file is tagged for. Each preset writes the fields that
/// program is known or reported to read; `docs/reference/tags.md` lists them
/// with how well each one is confirmed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum DjSoftware {
    /// AlphaTheta rekordbox.
    Rekordbox,
    /// Serato DJ.
    Serato,
    /// Native Instruments Traktor.
    Traktor,
    /// Engine DJ.
    EngineDj,
    /// No program in particular: every field a common reader looks at,
    /// including the alternative names.
    #[default]
    Generic,
}

impl DjSoftware {
    /// The fields the key is written to in a tag of `format`.
    pub fn key_fields(self, format: TagFormat) -> &'static [&'static str] {
        match (format, self) {
            (TagFormat::Id3v2, _) => &[ID3_KEY],
            (TagFormat::Mp4, _) => &[MP4_INITIAL_KEY],
            (TagFormat::VorbisComments, DjSoftware::Generic) => &[VORBIS_INITIAL_KEY, VORBIS_KEY],
            (TagFormat::VorbisComments, _) => &[VORBIS_INITIAL_KEY],
        }
    }

    /// The fields the tempo is written to in a tag of `format`.
    pub fn bpm_fields(self, format: TagFormat) -> &'static [&'static str] {
        match (format, self) {
            (TagFormat::Id3v2, _) => &[ID3_BPM],
            (TagFormat::Mp4, DjSoftware::Generic) => &[MP4_TEMPO, MP4_FREEFORM_BPM],
            (TagFormat::Mp4, _) => &[MP4_TEMPO],
            (TagFormat::VorbisComments, _) => &[VORBIS_BPM],
        }
    }

    /// Whether the tempo keeps up to two decimals in fields that can hold
    /// them. The other presets write whole numbers, the only form every
    /// program is known to accept.
    pub fn writes_fractional_bpm(self) -> bool {
        self == DjSoftware::Generic
    }
}
