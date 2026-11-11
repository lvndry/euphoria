// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::TagFormat;
use super::mp4::TEMPO as MP4_TEMPO;

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
///
/// Where the evidence leaves a choice open, every preset takes the one that
/// cannot hurt: an M4A key goes into both names that programs are known to
/// read, and an alternative field that a file already has is updated so it
/// never contradicts the value written.
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
    /// Atomix VirtualDJ.
    VirtualDj,
    /// Mixxx, the one program whose tag reader can be read in its source.
    Mixxx,
    /// No program in particular: the fields every program above reads,
    /// with the tempo as a whole number.
    #[default]
    Generic,
}

impl DjSoftware {
    /// The fields the key is written to in a tag of `format`.
    ///
    /// For M4A both `initialkey` and `KEY` are written. rekordbox 7 holds
    /// only the name `KEY`, Mixxx reads either, and the other programs are
    /// reported to read `initialkey`; no program is known to misread a file
    /// that has both. For FLAC only `INITIALKEY` is created, because
    /// VirtualDJ is reported to read neither field when both exist.
    pub fn key_fields(self, format: TagFormat) -> &'static [&'static str] {
        match format {
            TagFormat::Id3v2 => &[ID3_KEY],
            TagFormat::Mp4 => &[MP4_INITIAL_KEY, MP4_KEY],
            TagFormat::VorbisComments => &[VORBIS_INITIAL_KEY],
        }
    }

    /// Key fields that are updated when the file already has them, and not
    /// created otherwise. A `KEY` left in a FLAC file by another program
    /// would otherwise disagree with the key written, and VirtualDJ prefers
    /// `KEY` when both exist.
    pub fn key_fields_if_present(self, format: TagFormat) -> &'static [&'static str] {
        match format {
            TagFormat::VorbisComments => &[VORBIS_KEY],
            TagFormat::Id3v2 | TagFormat::Mp4 => &[],
        }
    }

    /// The fields the tempo is written to in a tag of `format`.
    pub fn bpm_fields(self, format: TagFormat) -> &'static [&'static str] {
        match (format, self) {
            (TagFormat::Id3v2, _) => &[ID3_BPM],
            (TagFormat::Mp4, DjSoftware::Mixxx | DjSoftware::Generic) => {
                &[MP4_TEMPO, MP4_FREEFORM_BPM]
            }
            (TagFormat::Mp4, _) => &[MP4_TEMPO],
            (TagFormat::VorbisComments, _) => &[VORBIS_BPM],
        }
    }

    /// Tempo fields that are updated when the file already has them, and
    /// not created otherwise. Mixxx reads the freeform M4A tempo before
    /// `tmpo`, and `TEMPO` when a FLAC file has no `BPM`.
    pub fn bpm_fields_if_present(self, format: TagFormat) -> &'static [&'static str] {
        match (format, self) {
            (TagFormat::Id3v2, _) => &[],
            (TagFormat::Mp4, DjSoftware::Mixxx | DjSoftware::Generic) => &[],
            (TagFormat::Mp4, _) => &[MP4_FREEFORM_BPM],
            (TagFormat::VorbisComments, _) => &[VORBIS_TEMPO],
        }
    }

    /// Whether the tempo keeps up to two decimals in fields that can hold
    /// them. Only Mixxx is known to read a tempo with decimals; the other
    /// presets write whole numbers, the only form every program accepts.
    pub fn writes_fractional_bpm(self) -> bool {
        self == DjSoftware::Mixxx
    }

    /// Whether the tempo is written unless the user says otherwise. Serato
    /// does not run its own tempo detection on a file that already has a
    /// tempo, so a tempo written by Euphoria would stand in for Serato's;
    /// for Serato it is written only on request.
    pub fn writes_bpm_by_default(self) -> bool {
        self != DjSoftware::Serato
    }

    /// Every preset.
    pub const ALL: [DjSoftware; 7] = [
        DjSoftware::Rekordbox,
        DjSoftware::Serato,
        DjSoftware::Traktor,
        DjSoftware::EngineDj,
        DjSoftware::VirtualDj,
        DjSoftware::Mixxx,
        DjSoftware::Generic,
    ];
}
