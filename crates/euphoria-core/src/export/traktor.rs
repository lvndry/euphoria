// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes a library as a Traktor collection file (NML), which Traktor Pro
//! reads with "Import Another Collection".
//!
//! The layout follows the NML that Traktor Pro 4 writes, as found in a real
//! `collection.nml` published by the traktor-nml-utils project, and the NML
//! writer of dj-data-converter, which was checked in Traktor: a
//! `COLLECTION` of `ENTRY` elements, and a `PLAYLISTS` tree of `NODE`
//! elements whose playlists name their tracks by volume, folder and file.
//!
//! Times in `CUE_V2` are in milliseconds. A beat grid is a cue of type 4
//! with a `GRID` child giving its tempo; Traktor holds one tempo per track.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::rekordbox::{
    CueKind, CuePoint, ExistingFile, Library, MAX_FOLDER_DEPTH, PlaylistNode, Track, export_error,
    is_xml_character, push_attribute, tempo_text, write_file,
};
use crate::error::{EuphoriaError, ExportProblem};
use crate::key::{Key, Mode};
use crate::tags::KeyNotation;

/// The number of hot cue pads a track has in Traktor, numbered 0 to 7 in
/// the file and 1 to 8 on screen.
pub const HOT_CUE_PADS: u8 = 8;

/// The volume name a Mac gives its startup disk unless the user renamed it.
pub const DEFAULT_STARTUP_VOLUME: &str = "Macintosh HD";

/// The folder separator of NML paths.
const SEPARATOR: &str = "/:";

/// The NML version Traktor Pro 3 writes. Traktor Pro 4 writes 20 and reads
/// both; dj-data-converter writes 19.
const NML_VERSION: &str = "19";

const SECONDS_PER_DAY: u64 = 86_400;

/// When the entries of an export were last changed, as Traktor stores it:
/// a calendar date and the seconds since midnight, in UTC. The writer
/// rejects invalid dates or times before writing an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModifiedTime {
    /// Year, such as 2026.
    pub year: i64,
    /// Month, 1 to 12.
    pub month: u32,
    /// Day of the month, 1 to 31.
    pub day: u32,
    /// Seconds since midnight, below 86 400.
    pub seconds_of_day: u32,
}

impl ModifiedTime {
    /// The time `moment`, or the Unix epoch for a moment before it.
    pub fn at(moment: SystemTime) -> Self {
        let since_epoch = moment
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        let days = i64::try_from(since_epoch / SECONDS_PER_DAY).unwrap_or(i64::MAX);
        let (year, month, day) = civil_date(days);
        Self {
            year,
            month,
            day,
            seconds_of_day: u32::try_from(since_epoch % SECONDS_PER_DAY).unwrap_or(0),
        }
    }

    /// Now.
    pub fn now() -> Self {
        Self::at(SystemTime::now())
    }

    /// True when the fields name a date and a moment of that day.
    pub fn is_valid(self) -> bool {
        self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= Self::days_in(self.year, self.month)
            && u64::from(self.seconds_of_day) < SECONDS_PER_DAY
    }

    /// The number of days the month of `year` has.
    fn days_in(year: i64, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }
}

/// The calendar date `days` after 1 January 1970, from Howard Hinnant's
/// `civil_from_days`.
fn civil_date(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

/// Choices that change what a Traktor export contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraktorOptions {
    /// The spelling of keys in the Key Text column. Traktor's own Key column
    /// is filled from the key itself, whatever this is.
    pub key_notation: KeyNotation,
    /// The name of the Mac volume that holds paths outside `/Volumes`,
    /// usually [`DEFAULT_STARTUP_VOLUME`]. Traktor names every file by its
    /// volume, and a name that differs from the real one makes the track
    /// show as missing.
    pub startup_volume: String,
    /// The time written as each entry's last change.
    pub modified: ModifiedTime,
}

impl Default for TraktorOptions {
    fn default() -> Self {
        Self {
            key_notation: KeyNotation::default(),
            startup_volume: DEFAULT_STARTUP_VOLUME.to_string(),
            modified: ModifiedTime::now(),
        }
    }
}

/// Where Traktor finds a file: a volume, the folders on it, and the file
/// name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NmlLocation {
    /// A drive letter with its colon on Windows, such as `C:`, and a volume
    /// name on a Mac, such as `Macintosh HD`.
    pub volume: String,
    /// The folders from the root of the volume, each followed by `/:` and
    /// the whole starting with it, such as `/:Music/:House/:`.
    pub directory: String,
    /// The file name.
    pub file: String,
}

impl NmlLocation {
    /// The text a playlist uses to name the track: volume, folders and
    /// file run together, such as `C:/:Music/:a.mp3`.
    pub fn primary_key(&self) -> String {
        format!("{}{}{}", self.volume, self.directory, self.file)
    }
}

/// The NML location of an absolute path.
///
/// A Windows path with a drive letter keeps the letter as its volume. A
/// Unix path under `/Volumes/<name>` is on the volume `<name>`. Any other
/// absolute Unix path is on `startup_volume`.
///
/// # Errors
///
/// Returns [`ExportProblem::NotAbsolutePath`] for a path that is neither,
/// or that names no file, and [`ExportProblem::PathNotUnicode`] for one
/// that is not valid Unicode, or [`ExportProblem::PathNotXml`] when the
/// location contains characters XML cannot represent.
pub fn nml_location(path: &Path, startup_volume: &str) -> Result<NmlLocation, ExportProblem> {
    let text = path.to_str().ok_or(ExportProblem::PathNotUnicode)?;
    let bytes = text.as_bytes();
    let has_drive_letter = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let (volume, rest): (String, Vec<&str>) = if has_drive_letter {
        (
            text[..2].to_string(),
            text[3..]
                .split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .collect(),
        )
    } else {
        let after_root = text
            .strip_prefix('/')
            .ok_or(ExportProblem::NotAbsolutePath)?;
        let parts: Vec<&str> = after_root
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        match parts.as_slice() {
            ["Volumes", volume, rest @ ..] if !rest.is_empty() => {
                // The volume is a component of the path, so the same rule as
                // for the folders applies: a volume named . or .. names no
                // file on the volume it seems to be on.
                if matches!(*volume, "." | "..") {
                    return Err(ExportProblem::NotAbsolutePath);
                }
                (volume.to_string(), rest.to_vec())
            }
            _ => (startup_volume.to_string(), parts),
        }
    };
    let (file, folders) = rest.split_last().ok_or(ExportProblem::NotAbsolutePath)?;
    if matches!(volume.as_str(), "." | "..")
        || folders.iter().any(|folder| matches!(*folder, "." | ".."))
        || matches!(*file, "." | "..")
    {
        return Err(ExportProblem::NotAbsolutePath);
    }
    let mut directory = String::from(SEPARATOR);
    for folder in folders {
        directory.push_str(folder);
        directory.push_str(SEPARATOR);
    }
    let location = NmlLocation {
        volume,
        directory,
        file: (*file).to_string(),
    };
    if [&location.volume, &location.directory, &location.file]
        .into_iter()
        .any(|part| !part.chars().all(is_xml_character))
    {
        return Err(ExportProblem::PathNotXml);
    }
    Ok(location)
}

/// The number Traktor stores for a key in `MUSICAL_KEY`: 0 to 11 for the
/// major keys from C upwards by semitone, and 12 to 23 for the minor keys
/// from C minor. Checked against the pairs of `INFO KEY` and `MUSICAL_KEY`
/// in a real Traktor Pro 4 collection. Mixxx's Traktor reader is one
/// semitone off from this.
pub fn musical_key_value(key: Key) -> u8 {
    match key.mode() {
        Mode::Major => key.tonic(),
        Mode::Minor => 12 + key.tonic(),
    }
}

/// The key a `MUSICAL_KEY` value stands for, or `None` above 23.
pub fn key_from_musical_key_value(value: u8) -> Option<Key> {
    match value {
        0..=11 => Some(Key::new(value, Mode::Major)),
        12..=23 => Some(Key::new(value - 12, Mode::Minor)),
        _ => None,
    }
}

/// Writes `library` as a Traktor NML document.
///
/// Each cue point becomes a `CUE_V2` of type 0: a hot cue on its pad, a
/// memory cue on none. A memory cue at the same time and with the same name
/// as a hot cue is left out, since Traktor lists hot cues among its cues
/// already; dj-data-converter does the same. A track with a tempo and a
/// first beat gets a grid marker at that beat.
///
/// Metadata follows [`super::rekordbox::push_attribute`]: a title, comment,
/// cue name or playlist name that contains characters XML 1.0 cannot
/// represent refuses the export, instead of being silently changed. Paths
/// are checked separately and are never changed to remove characters.
///
/// # Errors
///
/// Returns [`EuphoriaError::Export`] when a track path is not an absolute
/// local path, is not Unicode, contains characters XML cannot represent,
/// or names the same file as another track; when a tempo is not a positive
/// number at six-decimal precision; when a hot cue pad is not below
/// [`HOT_CUE_PADS`]; when the modification date or time is invalid; or
/// when a playlist names a path that is not in the collection. Returns [`EuphoriaError::ExportFolderDepth`] when folders are
/// nested deeper than [`MAX_FOLDER_DEPTH`]. Returns
/// [`EuphoriaError::ExportWrite`] when `writer` fails.
pub fn write_nml<W: std::io::Write>(
    mut writer: W,
    library: &Library,
    options: &TraktorOptions,
) -> Result<(), EuphoriaError> {
    let document = render(library, options)?;
    writer
        .write_all(document.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|source| EuphoriaError::ExportWrite { source })
}

/// Writes `library` as a Traktor NML file at `destination`, through a
/// temporary file moved into place once complete, as
/// [`super::rekordbox::write_xml_file`] does.
///
/// # Errors
///
/// The errors of [`write_nml`], and [`EuphoriaError::ExportDestinationExists`]
/// or [`EuphoriaError::ExportFile`] as for the rekordbox export.
pub fn write_nml_file(
    destination: &Path,
    library: &Library,
    options: &TraktorOptions,
    existing: ExistingFile,
) -> Result<(), EuphoriaError> {
    let document = render(library, options)?;
    write_file(destination, document.as_bytes(), existing)
}

fn render(library: &Library, options: &TraktorOptions) -> Result<String, EuphoriaError> {
    let mut primary_keys = std::collections::HashMap::with_capacity(library.tracks.len());
    let mut seen_keys = HashSet::with_capacity(library.tracks.len());
    let mut document = String::new();
    document.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
    document.push_str("<NML");
    push_attribute(&mut document, "VERSION", NML_VERSION)?;
    document.push_str(">\n");
    document.push_str("  <COLLECTION");
    push_attribute(&mut document, "ENTRIES", &library.tracks.len().to_string())?;
    document.push_str(">\n");
    for track in &library.tracks {
        let location = nml_location(&track.path, &options.startup_volume)
            .map_err(|problem| export_error(&track.path, problem))?;
        let primary_key = location.primary_key();
        if !seen_keys.insert(primary_key.clone()) {
            return Err(export_error(&track.path, ExportProblem::DuplicatePath));
        }
        push_entry(&mut document, track, &location, options)?;
        primary_keys.insert(track.path.as_path(), primary_key);
    }
    document.push_str("  </COLLECTION>\n");

    document.push_str("  <PLAYLISTS>\n");
    document.push_str("    <NODE TYPE=\"FOLDER\" NAME=\"$ROOT\">\n");
    document.push_str("      <SUBNODES");
    push_attribute(&mut document, "COUNT", &library.playlists.len().to_string())?;
    document.push_str(">\n");
    let mut node_path = Vec::new();
    for node in &library.playlists {
        push_playlist_node(&mut document, node, 1, &primary_keys, &mut node_path)?;
    }
    document.push_str("      </SUBNODES>\n");
    document.push_str("    </NODE>\n");
    document.push_str("  </PLAYLISTS>\n");
    document.push_str("</NML>\n");
    Ok(document)
}

fn push_entry(
    document: &mut String,
    track: &Track,
    location: &NmlLocation,
    options: &TraktorOptions,
) -> Result<(), EuphoriaError> {
    let tempo = match track.bpm {
        Some(bpm) => Some(
            tempo_text(bpm, 6)
                .ok_or_else(|| export_error(&track.path, ExportProblem::InvalidTempo))?,
        ),
        None => None,
    };
    if track
        .cue_points
        .iter()
        .any(|cue_point| matches!(cue_point.kind, CueKind::Hot(pad) if pad >= HOT_CUE_PADS))
    {
        return Err(export_error(&track.path, ExportProblem::InvalidHotCue));
    }

    let modified = options.modified;
    if !modified.is_valid() {
        return Err(export_error(
            &track.path,
            ExportProblem::InvalidModifiedTime,
        ));
    }
    document.push_str("    <ENTRY");
    push_attribute(
        document,
        "MODIFIED_DATE",
        &format!("{}/{}/{}", modified.year, modified.month, modified.day),
    )?;
    push_attribute(
        document,
        "MODIFIED_TIME",
        &modified.seconds_of_day.to_string(),
    )?;
    if let Some(title) = &track.title {
        push_attribute(document, "TITLE", title)?;
    }
    if let Some(artist) = &track.artist {
        push_attribute(document, "ARTIST", artist)?;
    }
    document.push_str(">\n");

    document.push_str("      <LOCATION");
    push_attribute(document, "DIR", &location.directory)?;
    push_attribute(document, "FILE", &location.file)?;
    push_attribute(document, "VOLUME", &location.volume)?;
    document.push_str("/>\n");

    document.push_str("      <INFO");
    let seconds = track.duration.as_secs_f64();
    push_attribute(document, "PLAYTIME", &format!("{:.0}", seconds.round()))?;
    push_attribute(document, "PLAYTIME_FLOAT", &format!("{seconds:.6}"))?;
    if let Some(key) = track.key {
        push_attribute(document, "KEY", &options.key_notation.spell(key))?;
    }
    if let Some(comment) = &track.comment {
        push_attribute(document, "COMMENT", comment)?;
    }
    document.push_str("/>\n");

    if let Some(tempo) = &tempo {
        document.push_str("      <TEMPO");
        push_attribute(document, "BPM", tempo)?;
        push_attribute(document, "BPM_QUALITY", "100.000000")?;
        document.push_str("/>\n");
    }
    if let Some(key) = track.key {
        document.push_str("      <MUSICAL_KEY");
        push_attribute(document, "VALUE", &musical_key_value(key).to_string())?;
        document.push_str("/>\n");
    }

    if let Some((tempo, first_beat)) = tempo.as_ref().zip(track.first_beat) {
        push_cue(
            document,
            "Beat Marker",
            CueType::Grid(tempo),
            first_beat,
            -1,
        )?;
    }
    for cue_point in cues_to_write(&track.cue_points) {
        let hot_cue = match cue_point.kind {
            CueKind::Memory => -1,
            CueKind::Hot(pad) => i16::from(pad),
        };
        push_cue(
            document,
            &cue_point.name,
            CueType::Cue,
            cue_point.time,
            hot_cue,
        )?;
    }
    document.push_str("    </ENTRY>\n");
    Ok(())
}

/// The cue points of a track without the memory cues that repeat a hot cue.
fn cues_to_write(cue_points: &[CuePoint]) -> impl Iterator<Item = &CuePoint> {
    cue_points.iter().filter(move |cue_point| {
        cue_point.kind != CueKind::Memory
            || !cue_points.iter().any(|other| {
                matches!(other.kind, CueKind::Hot(_))
                    && other.time == cue_point.time
                    && other.name == cue_point.name
            })
    })
}

#[derive(Clone, Copy)]
enum CueType<'tempo> {
    Cue,
    Grid(&'tempo str),
}

fn push_cue(
    document: &mut String,
    name: &str,
    cue_type: CueType<'_>,
    start: Duration,
    hot_cue: i16,
) -> Result<(), EuphoriaError> {
    document.push_str("      <CUE_V2");
    push_attribute(document, "NAME", name)?;
    push_attribute(document, "DISPL_ORDER", "0")?;
    push_attribute(
        document,
        "TYPE",
        match cue_type {
            CueType::Cue => "0",
            CueType::Grid(_) => "4",
        },
    )?;
    push_attribute(document, "START", &milliseconds(start))?;
    push_attribute(document, "LEN", "0.000000")?;
    push_attribute(document, "REPEATS", "-1")?;
    push_attribute(document, "HOTCUE", &hot_cue.to_string())?;
    match cue_type {
        CueType::Cue => document.push_str("/>\n"),
        CueType::Grid(tempo) => {
            document.push_str(">\n        <GRID");
            push_attribute(document, "BPM", tempo)?;
            document.push_str("/>\n      </CUE_V2>\n");
        }
    }
    Ok(())
}

fn milliseconds(position: Duration) -> String {
    format!("{:.6}", position.as_secs_f64() * 1000.0)
}

fn push_playlist_node<'library>(
    document: &mut String,
    node: &'library PlaylistNode,
    depth: usize,
    primary_keys: &std::collections::HashMap<&Path, String>,
    node_path: &mut Vec<&'library str>,
) -> Result<(), EuphoriaError> {
    let indent = "    ".repeat(depth + 1);
    match node {
        PlaylistNode::Folder(folder) => {
            if depth > MAX_FOLDER_DEPTH {
                return Err(EuphoriaError::ExportFolderDepth {
                    folder: folder.name.clone(),
                    maximum: MAX_FOLDER_DEPTH,
                });
            }
            document.push_str(&indent);
            document.push_str("<NODE TYPE=\"FOLDER\"");
            push_attribute(document, "NAME", &folder.name)?;
            document.push_str(">\n");
            document.push_str(&indent);
            document.push_str("  <SUBNODES");
            push_attribute(document, "COUNT", &folder.children.len().to_string())?;
            document.push_str(">\n");
            node_path.push(&folder.name);
            for child in &folder.children {
                push_playlist_node(document, child, depth + 1, primary_keys, node_path)?;
            }
            node_path.pop();
            document.push_str(&indent);
            document.push_str("  </SUBNODES>\n");
        }
        PlaylistNode::Playlist(playlist) => {
            node_path.push(&playlist.name);
            let uuid = playlist_uuid(node_path);
            node_path.pop();
            document.push_str(&indent);
            document.push_str("<NODE TYPE=\"PLAYLIST\"");
            push_attribute(document, "NAME", &playlist.name)?;
            document.push_str(">\n");
            document.push_str(&indent);
            document.push_str("  <PLAYLIST");
            push_attribute(document, "ENTRIES", &playlist.tracks.len().to_string())?;
            push_attribute(document, "TYPE", "LIST")?;
            push_attribute(document, "UUID", &uuid)?;
            document.push_str(">\n");
            for path in &playlist.tracks {
                let primary_key = primary_keys
                    .get(path.as_path())
                    .ok_or_else(|| export_error(path, ExportProblem::NotInCollection))?;
                document.push_str(&indent);
                document.push_str("    <ENTRY><PRIMARYKEY TYPE=\"TRACK\"");
                push_attribute(document, "KEY", primary_key)?;
                document.push_str("/></ENTRY>\n");
            }
            document.push_str(&indent);
            document.push_str("  </PLAYLIST>\n");
        }
    }
    document.push_str(&indent);
    document.push_str("</NODE>\n");
    Ok(())
}

/// 32 hexadecimal digits that name a playlist, the same for the same
/// folders and name in every export, so a playlist exported again is the
/// same playlist to Traktor.
fn playlist_uuid(node_path: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"euphoria traktor playlist");
    for name in node_path {
        hasher.update(&(name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
    }
    hasher.finalize().to_hex()[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_match_the_calendar() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(19_782), (2024, 2, 29));
        assert_eq!(civil_date(20_403), (2025, 11, 11));
        let moment = UNIX_EPOCH + Duration::from_secs(20_403 * SECONDS_PER_DAY + 3_725);
        assert_eq!(
            ModifiedTime::at(moment),
            ModifiedTime {
                year: 2025,
                month: 11,
                day: 11,
                seconds_of_day: 3_725
            }
        );
    }

    #[test]
    fn every_key_value_round_trips_and_matches_the_real_collection() {
        for value in 0..24 {
            let key = key_from_musical_key_value(value).unwrap();
            assert_eq!(musical_key_value(key), value);
        }
        assert_eq!(key_from_musical_key_value(24), None);
        // Pairs of INFO KEY and MUSICAL_KEY VALUE in the Traktor Pro 4
        // collection of traktor-nml-utils.
        for (open_key, value) in [("12m", 14), ("5d", 4), ("9d", 8), ("6d", 11), ("10m", 12)] {
            let key: Key = open_key.parse().unwrap();
            assert_eq!(musical_key_value(key), value, "{open_key}");
        }
    }

    #[test]
    fn locations_name_the_volume_folders_and_file() {
        let location = |path: &str| nml_location(Path::new(path), "Macintosh HD");
        assert_eq!(
            location("/Users/dj/Music/a.mp3").unwrap(),
            NmlLocation {
                volume: "Macintosh HD".to_string(),
                directory: "/:Users/:dj/:Music/:".to_string(),
                file: "a.mp3".to_string(),
            }
        );
        assert_eq!(
            location("/Volumes/USB Stick/Sets/b.flac")
                .unwrap()
                .primary_key(),
            "USB Stick/:Sets/:b.flac"
        );
        assert_eq!(
            location(r"C:\Music\Sets\c.wav").unwrap().primary_key(),
            "C:/:Music/:Sets/:c.wav"
        );
        assert_eq!(location("/a.mp3").unwrap().directory, "/:");
        assert_eq!(
            location("relative/a.mp3"),
            Err(ExportProblem::NotAbsolutePath)
        );
        assert_eq!(location("/"), Err(ExportProblem::NotAbsolutePath));
        assert_eq!(
            location("/Music/../a.mp3"),
            Err(ExportProblem::NotAbsolutePath)
        );
    }

    #[test]
    fn a_memory_cue_that_repeats_a_hot_cue_is_left_out() {
        let at = |seconds, kind, name: &str| CuePoint {
            name: name.to_string(),
            time: Duration::from_secs(seconds),
            kind,
        };
        let cues = [
            at(1, CueKind::Memory, "Start"),
            at(1, CueKind::Hot(0), "Start"),
            at(2, CueKind::Memory, "Mine"),
            at(3, CueKind::Memory, "Drop"),
            at(3, CueKind::Hot(1), "Other"),
        ];
        let written: Vec<&CuePoint> = cues_to_write(&cues).collect();
        assert_eq!(written, [&cues[1], &cues[2], &cues[3], &cues[4]]);
    }
}
