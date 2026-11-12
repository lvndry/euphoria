// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes a library as a rekordbox XML collection, the file rekordbox reads
//! through its "rekordbox xml" bridge.
//!
//! The document holds a `COLLECTION` of `TRACK` elements and a `PLAYLISTS`
//! tree of `NODE` elements. Tracks are numbered from 1 in the order given, and
//! playlists refer to them by that number.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{EuphoriaError, ExportProblem};
use crate::key::{Key, Mode};

const LOCATION_PREFIX: &str = "file://localhost/";

/// How many names are tried for the temporary file of an export before
/// giving up. Each is random, so even one collision is unlikely.
const TEMPORARY_NAME_ATTEMPTS: usize = 16;

/// The number of hot cue pads a track has in rekordbox: 16, lettered A to P.
/// Its export mode and the players it prepares drives for use the first 8.
pub const HOT_CUE_PADS: u8 = 16;

/// How many folders deep a playlist tree may go. A folder at the top level is
/// at depth 1.
pub const MAX_FOLDER_DEPTH: usize = 32;

// Spelled as on the Camelot wheel, which uses flats except for F sharp.
const TONIC_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// How a key is spelled in the `Tonality` attribute. rekordbox shows this
/// text as it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyNotation {
    /// Camelot codes such as "8A".
    #[default]
    Camelot,
    /// Short musical names such as "Am", "F#m" or "Db".
    Musical,
}

/// Choices that change what the export contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    /// The spelling of keys.
    pub key_notation: KeyNotation,
}

/// Everything written to one rekordbox XML file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Library {
    /// The tracks of the collection. Each path appears once.
    pub tracks: Vec<Track>,
    /// The playlists and folders shown at the top level in rekordbox.
    pub playlists: Vec<PlaylistNode>,
}

/// One audio file and what is known about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Absolute path of the audio file, either Unix style (`/Music/a.mp3`) or
    /// Windows style with a drive letter (`C:\Music\a.mp3`).
    pub path: PathBuf,
    /// Track title. The attribute is left out when this is `None`.
    pub title: Option<String>,
    /// Artist name. The attribute is left out when this is `None`.
    pub artist: Option<String>,
    /// Length of the audio, written in whole seconds.
    pub duration: Duration,
    /// Detected key. The attribute is left out when this is `None`.
    pub key: Option<Key>,
    /// Tempo in beats per minute. The attribute is left out when this is `None`.
    /// It is written with two decimals, so the smallest tempo is 0.01.
    pub bpm: Option<f64>,
    /// Text for the comments column. The attribute is left out when this is
    /// `None`.
    pub comment: Option<String>,
    /// Position of the first beat. Together with `bpm` it becomes a beat grid
    /// with a constant tempo, counted in 4/4 with this beat as the first of a bar.
    pub first_beat: Option<Duration>,
    /// Cue points, written in the order given.
    pub cue_points: Vec<CuePoint>,
}

impl Track {
    /// A track with only a path and a duration, and nothing else known.
    pub fn new(path: impl Into<PathBuf>, duration: Duration) -> Self {
        Self {
            path: path.into(),
            title: None,
            artist: None,
            duration,
            key: None,
            bpm: None,
            comment: None,
            first_beat: None,
            cue_points: Vec::new(),
        }
    }
}

/// A marked position in a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuePoint {
    /// Label shown next to the cue. It may be empty.
    pub name: String,
    /// Position from the start of the track.
    pub time: Duration,
    /// Memory cue or hot cue.
    pub kind: CueKind,
}

/// Where rekordbox stores a cue point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CueKind {
    /// A memory cue, listed along the waveform.
    Memory,
    /// A hot cue on a pad, where 0 is pad A, 1 is pad B and so on, up to
    /// 15 for pad P. The number is below [`HOT_CUE_PADS`].
    Hot(u8),
}

/// An entry of the playlist tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistNode {
    /// A folder holding playlists and other folders.
    Folder(Folder),
    /// A playlist.
    Playlist(Playlist),
}

/// A named group of playlists and folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Folder name.
    pub name: String,
    /// The playlists and folders inside, in display order.
    pub children: Vec<PlaylistNode>,
}

/// A named, ordered list of tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    /// Playlist name.
    pub name: String,
    /// Paths of the tracks in play order. Each one is the `path` of a track
    /// in [`Library::tracks`].
    pub tracks: Vec<PathBuf>,
}

/// Writes `library` as a rekordbox XML document.
///
/// The library is checked before anything is written, so a library that
/// cannot be exported leaves `writer` untouched.
///
/// Titles, artists, comments, cue names, playlist names and folder names lose the
/// characters XML 1.0 cannot carry: the control characters U+0000 to U+001F
/// other than tab, line feed and carriage return, and U+FFFE and U+FFFF. The
/// rest of the text is written unchanged. Paths lose nothing, because every
/// such byte of a path is percent encoded.
///
/// # Errors
///
/// Returns [`EuphoriaError::Export`] when a track path is not an absolute
/// local path or appears twice, when a tempo is not a number that is at least
/// 0.01 once rounded to two decimals, when a hot cue pad is not below
/// [`HOT_CUE_PADS`], or when a playlist names a path that is not in the
/// collection. Returns [`EuphoriaError::ExportFolderDepth`] when folders are
/// nested deeper than [`MAX_FOLDER_DEPTH`]. Returns
/// [`EuphoriaError::ExportWrite`] when `writer` fails.
pub fn write_xml<W: Write>(
    mut writer: W,
    library: &Library,
    options: &ExportOptions,
) -> Result<(), EuphoriaError> {
    let document = render(library, options)?;
    writer
        .write_all(document.as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|source| EuphoriaError::ExportWrite { source })
}

/// What to do when the file an export is written to already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingFile {
    /// Leave it as it is and fail.
    Keep,
    /// Replace it with the export.
    Replace,
}

/// Writes `library` as a rekordbox XML file at `destination`.
///
/// The document is written to a temporary file next to `destination` and
/// moved into place once it is complete, so `destination` holds either the
/// whole export or what it held before. The library is checked first, as in
/// [`write_xml`].
///
/// # Errors
///
/// Returns the errors of [`write_xml`] for a library that cannot be
/// exported. Returns [`EuphoriaError::ExportDestinationExists`] when
/// `destination` exists and `existing` is [`ExistingFile::Keep`]. Returns
/// [`EuphoriaError::ExportFile`] when the file cannot be created, written or
/// moved into place.
pub fn write_xml_file(
    destination: &Path,
    library: &Library,
    options: &ExportOptions,
    existing: ExistingFile,
) -> Result<(), EuphoriaError> {
    let document = render(library, options)?;
    write_file(destination, document.as_bytes(), existing)
}

/// Writes `contents` to `destination` through a temporary file next to it
/// that is moved into place once complete, so `destination` holds either
/// all of `contents` or what it held before.
///
/// # Errors
///
/// Returns [`EuphoriaError::ExportDestinationExists`] when `destination`
/// exists and `existing` is [`ExistingFile::Keep`], and
/// [`EuphoriaError::ExportFile`] when the file cannot be created, written or
/// moved into place.
pub(crate) fn write_file(
    destination: &Path,
    contents: &[u8],
    existing: ExistingFile,
) -> Result<(), EuphoriaError> {
    let file_error = |source: io::Error| EuphoriaError::ExportFile {
        path: destination.to_path_buf(),
        source,
    };
    // Looked at before writing so that nothing is created when the answer is
    // already known. The install below is what guarantees it.
    if existing == ExistingFile::Keep && is_present(destination).map_err(file_error)? {
        return Err(EuphoriaError::ExportDestinationExists {
            path: destination.to_path_buf(),
        });
    }

    let (mut temporary_file, temporary_path) =
        create_temporary_file(destination).map_err(file_error)?;
    let installed = temporary_file
        .write_all(contents)
        .and_then(|()| temporary_file.sync_all())
        .map_err(file_error)
        .and_then(|()| install(&temporary_path, destination, existing));
    drop(temporary_file);
    if installed.is_err() {
        // The export has failed and that failure is what the caller needs to
        // see, whether or not the temporary file could be removed.
        let _ = std::fs::remove_file(&temporary_path);
    }
    installed
}

/// Creates an empty file next to `destination` under a name nothing else
/// uses. The file is always a new one: a name that is taken, even by a
/// symbolic link, is passed over, so no existing file is opened or emptied.
fn create_temporary_file(destination: &Path) -> io::Result<(File, PathBuf)> {
    let file_name = destination.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "the path does not name a file")
    })?;
    for _ in 0..TEMPORARY_NAME_ATTEMPTS {
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".{:016x}.tmp", unpredictable_number()));
        let temporary_path = destination.with_file_name(temporary_name);
        match File::options()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((file, temporary_path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every temporary file name that was tried is taken",
    ))
}

/// A number another program cannot guess, from the random keys the standard
/// library gives each hash map.
fn unpredictable_number() -> u64 {
    RandomState::new().build_hasher().finish()
}

/// Gives the finished temporary file the name `destination` in one step.
///
/// With [`ExistingFile::Replace`] it takes the place of whatever is there.
/// With [`ExistingFile::Keep`] it only takes a name that is free, so a file
/// that another program created at `destination` a moment before is kept.
fn install(
    temporary_path: &Path,
    destination: &Path,
    existing: ExistingFile,
) -> Result<(), EuphoriaError> {
    let file_error = |source: io::Error| EuphoriaError::ExportFile {
        path: destination.to_path_buf(),
        source,
    };
    match existing {
        ExistingFile::Replace => std::fs::rename(temporary_path, destination).map_err(file_error),
        ExistingFile::Keep => match std::fs::hard_link(temporary_path, destination) {
            Ok(()) => {
                // The export is in place under its own name. A temporary name
                // left behind does not make it a failure.
                let _ = std::fs::remove_file(temporary_path);
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Err(EuphoriaError::ExportDestinationExists {
                    path: destination.to_path_buf(),
                })
            }
            // File systems such as FAT and exFAT have no hard links.
            Err(_) => install_by_claiming_the_name(temporary_path, destination),
        },
    }
}

/// Installs without replacing on a file system that has no hard links: an
/// empty file is created at `destination`, which fails when the name is
/// taken, and the temporary file is then moved over it.
fn install_by_claiming_the_name(
    temporary_path: &Path,
    destination: &Path,
) -> Result<(), EuphoriaError> {
    let file_error = |source: io::Error| EuphoriaError::ExportFile {
        path: destination.to_path_buf(),
        source,
    };
    match File::options()
        .write(true)
        .create_new(true)
        .open(destination)
    {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(EuphoriaError::ExportDestinationExists {
                path: destination.to_path_buf(),
            });
        }
        Err(error) => return Err(file_error(error)),
    }
    std::fs::rename(temporary_path, destination).map_err(|error| {
        // The empty file was created here, so removing it restores the
        // folder to what it held before the export.
        let _ = std::fs::remove_file(destination);
        file_error(error)
    })
}

/// True when something is at `path`, including a link that leads nowhere.
fn is_present(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn render(library: &Library, options: &ExportOptions) -> Result<String, EuphoriaError> {
    let mut track_ids: HashMap<&Path, usize> = HashMap::with_capacity(library.tracks.len());
    let mut document = String::new();
    document.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n");
    document.push_str("<DJ_PLAYLISTS Version=\"1.0.0\">\n");
    document.push_str("  <PRODUCT");
    push_attribute(&mut document, "Name", "Euphoria");
    push_attribute(&mut document, "Version", env!("CARGO_PKG_VERSION"));
    push_attribute(&mut document, "Company", "The Euphoria contributors");
    document.push_str("/>\n");

    document.push_str("  <COLLECTION");
    push_attribute(&mut document, "Entries", &library.tracks.len().to_string());
    document.push_str(">\n");
    for (index, track) in library.tracks.iter().enumerate() {
        let track_id = index + 1;
        if track_ids.insert(track.path.as_path(), track_id).is_some() {
            return Err(export_error(&track.path, ExportProblem::DuplicatePath));
        }
        push_track(&mut document, track, track_id, options)?;
    }
    document.push_str("  </COLLECTION>\n");

    document.push_str("  <PLAYLISTS>\n");
    document.push_str("    <NODE Type=\"0\" Name=\"ROOT\"");
    push_attribute(&mut document, "Count", &library.playlists.len().to_string());
    document.push_str(">\n");
    for node in &library.playlists {
        push_playlist_node(&mut document, node, 1, &track_ids)?;
    }
    document.push_str("    </NODE>\n");
    document.push_str("  </PLAYLISTS>\n");
    document.push_str("</DJ_PLAYLISTS>\n");
    Ok(document)
}

fn push_track(
    document: &mut String,
    track: &Track,
    track_id: usize,
    options: &ExportOptions,
) -> Result<(), EuphoriaError> {
    let location = location_url(&track.path)
        .ok_or_else(|| export_error(&track.path, ExportProblem::NotAbsolutePath))?;
    let tempo = match track.bpm {
        Some(bpm) => Some(
            tempo_text(bpm, 2)
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

    document.push_str("    <TRACK");
    push_attribute(document, "TrackID", &track_id.to_string());
    if let Some(title) = &track.title {
        push_attribute(document, "Name", title);
    }
    if let Some(artist) = &track.artist {
        push_attribute(document, "Artist", artist);
    }
    let whole_seconds = track.duration.as_secs_f64().round();
    push_attribute(document, "TotalTime", &format!("{whole_seconds:.0}"));
    if let Some(tempo) = &tempo {
        push_attribute(document, "AverageBpm", tempo);
    }
    push_attribute(document, "Location", &location);
    if let Some(key) = track.key {
        push_attribute(document, "Tonality", &tonality(key, options.key_notation));
    }
    if let Some(comment) = &track.comment {
        push_attribute(document, "Comments", comment);
    }

    let beat_grid = tempo.zip(track.first_beat);
    if beat_grid.is_none() && track.cue_points.is_empty() {
        document.push_str("/>\n");
        return Ok(());
    }
    document.push_str(">\n");
    if let Some((tempo, first_beat)) = beat_grid {
        document.push_str("      <TEMPO");
        push_attribute(document, "Inizio", &seconds(first_beat));
        push_attribute(document, "Bpm", &tempo);
        push_attribute(document, "Metro", "4/4");
        push_attribute(document, "Battito", "1");
        document.push_str("/>\n");
    }
    for cue_point in &track.cue_points {
        let number = match cue_point.kind {
            CueKind::Memory => String::from("-1"),
            CueKind::Hot(pad) => pad.to_string(),
        };
        document.push_str("      <POSITION_MARK");
        push_attribute(document, "Name", &cue_point.name);
        push_attribute(document, "Type", "0");
        push_attribute(document, "Start", &seconds(cue_point.time));
        push_attribute(document, "Num", &number);
        document.push_str("/>\n");
    }
    document.push_str("    </TRACK>\n");
    Ok(())
}

fn push_playlist_node(
    document: &mut String,
    node: &PlaylistNode,
    depth: usize,
    track_ids: &HashMap<&Path, usize>,
) -> Result<(), EuphoriaError> {
    // The ROOT node sits two levels into the document.
    let indent = "  ".repeat(depth + 2);
    if let PlaylistNode::Folder(folder) = node
        && depth > MAX_FOLDER_DEPTH
    {
        return Err(EuphoriaError::ExportFolderDepth {
            folder: folder.name.clone(),
            maximum: MAX_FOLDER_DEPTH,
        });
    }
    document.push_str(&indent);
    match node {
        PlaylistNode::Folder(folder) => {
            document.push_str("<NODE Type=\"0\"");
            push_attribute(document, "Name", &folder.name);
            push_attribute(document, "Count", &folder.children.len().to_string());
            if folder.children.is_empty() {
                document.push_str("/>\n");
                return Ok(());
            }
            document.push_str(">\n");
            for child in &folder.children {
                push_playlist_node(document, child, depth + 1, track_ids)?;
            }
        }
        PlaylistNode::Playlist(playlist) => {
            document.push_str("<NODE Type=\"1\"");
            push_attribute(document, "Name", &playlist.name);
            push_attribute(document, "KeyType", "0");
            push_attribute(document, "Entries", &playlist.tracks.len().to_string());
            if playlist.tracks.is_empty() {
                document.push_str("/>\n");
                return Ok(());
            }
            document.push_str(">\n");
            for path in &playlist.tracks {
                let track_id = track_ids
                    .get(path.as_path())
                    .ok_or_else(|| export_error(path, ExportProblem::NotInCollection))?;
                document.push_str(&indent);
                document.push_str("  <TRACK");
                push_attribute(document, "Key", &track_id.to_string());
                document.push_str("/>\n");
            }
        }
    }
    document.push_str(&indent);
    document.push_str("</NODE>\n");
    Ok(())
}

pub(crate) fn export_error(path: &Path, problem: ExportProblem) -> EuphoriaError {
    EuphoriaError::Export {
        path: path.to_path_buf(),
        problem,
    }
}

fn seconds(position: Duration) -> String {
    format!("{:.3}", position.as_secs_f64())
}

/// The tempo with `decimals` decimals, or `None` when it is not a number
/// or is written as zero or less at that precision.
pub(crate) fn tempo_text(bpm: f64, decimals: usize) -> Option<String> {
    if !(bpm.is_finite() && bpm > 0.0) {
        return None;
    }
    let text = format_args!("{bpm:.prec$}", prec = decimals).to_string();
    text.contains(|digit: char| ('1'..='9').contains(&digit))
        .then_some(text)
}

fn tonality(key: Key, notation: KeyNotation) -> String {
    match notation {
        KeyNotation::Camelot => key.camelot(),
        KeyNotation::Musical => {
            let tonic = TONIC_NAMES[usize::from(key.tonic())];
            match key.mode() {
                Mode::Major => tonic.to_string(),
                Mode::Minor => format!("{tonic}m"),
            }
        }
    }
}

/// The `file://localhost/` URL of an absolute path, or `None` when the path
/// starts with neither a slash nor a drive letter.
fn location_url(path: &Path) -> Option<String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let has_drive_letter = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let path_bytes = if has_drive_letter {
        bytes
    } else {
        bytes.strip_prefix(b"/")?
    };

    let mut url = String::from(LOCATION_PREFIX);
    for &byte in path_bytes {
        match byte {
            // A backslash separates folders only in a Windows path. In a Unix
            // file name it is an ordinary character.
            b'\\' if has_drive_letter => url.push('/'),
            b'/' | b':' | b'-' | b'.' | b'_' | b'~' => url.push(char::from(byte)),
            _ if byte.is_ascii_alphanumeric() => url.push(char::from(byte)),
            _ => {
                // Writing to a String cannot fail.
                let _ = write!(url, "%{byte:02X}");
            }
        }
    }
    Some(url)
}

pub(crate) fn push_attribute(document: &mut String, name: &str, value: &str) {
    document.push(' ');
    document.push_str(name);
    document.push_str("=\"");
    for character in value.chars() {
        match character {
            '&' => document.push_str("&amp;"),
            '<' => document.push_str("&lt;"),
            '>' => document.push_str("&gt;"),
            '"' => document.push_str("&quot;"),
            '\'' => document.push_str("&apos;"),
            // Written as references so a parser does not turn them into spaces.
            '\t' => document.push_str("&#9;"),
            '\n' => document.push_str("&#10;"),
            '\r' => document.push_str("&#13;"),
            // XML 1.0 cannot carry these characters, even as references.
            '\0'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}' => {}
            _ => document.push(character),
        }
    }
    document.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "euphoria-export-install-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn file_names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    type Install = fn(&Path, &Path) -> Result<(), EuphoriaError>;

    fn install_keeping(temporary_path: &Path, destination: &Path) -> Result<(), EuphoriaError> {
        install(temporary_path, destination, ExistingFile::Keep)
    }

    const KEEPING_INSTALLS: [(&str, Install); 2] = [
        ("hard-link", install_keeping),
        ("claimed-name", install_by_claiming_the_name),
    ];

    #[test]
    fn a_destination_that_appears_after_the_check_is_not_replaced() {
        for (name, install_without_replacing) in KEEPING_INSTALLS {
            let folder = scratch_folder(&format!("appears-{name}"));
            let destination = folder.join("library.xml");
            assert!(!is_present(&destination).unwrap());
            let (mut temporary_file, temporary_path) = create_temporary_file(&destination).unwrap();
            temporary_file.write_all(b"the export").unwrap();
            drop(temporary_file);
            std::fs::write(&destination, "written by someone else").unwrap();

            let installed = install_without_replacing(&temporary_path, &destination);

            assert!(
                matches!(
                    &installed,
                    Err(EuphoriaError::ExportDestinationExists { path }) if *path == destination
                ),
                "{name}: {installed:?}"
            );
            assert_eq!(
                std::fs::read_to_string(&destination).unwrap(),
                "written by someone else",
                "{name}"
            );
            std::fs::remove_dir_all(&folder).unwrap();
        }
    }

    #[test]
    fn a_free_name_is_taken_and_the_temporary_name_is_gone() {
        for (name, install_without_replacing) in KEEPING_INSTALLS {
            let folder = scratch_folder(&format!("free-{name}"));
            let destination = folder.join("library.xml");
            let (mut temporary_file, temporary_path) = create_temporary_file(&destination).unwrap();
            temporary_file.write_all(b"the export").unwrap();
            drop(temporary_file);

            install_without_replacing(&temporary_path, &destination).unwrap();

            assert_eq!(std::fs::read_to_string(&destination).unwrap(), "the export");
            assert_eq!(file_names(&folder), ["library.xml"], "{name}");
            std::fs::remove_dir_all(&folder).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_destination_is_kept_and_its_target_untouched() {
        for (name, install_without_replacing) in KEEPING_INSTALLS {
            let folder = scratch_folder(&format!("link-{name}"));
            let target = folder.join("precious.flac");
            let destination = folder.join("library.xml");
            std::fs::write(&target, "audio").unwrap();
            let (_, temporary_path) = create_temporary_file(&destination).unwrap();
            std::os::unix::fs::symlink(&target, &destination).unwrap();

            let installed = install_without_replacing(&temporary_path, &destination);

            assert!(
                matches!(
                    installed,
                    Err(EuphoriaError::ExportDestinationExists { .. })
                ),
                "{name}"
            );
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "audio", "{name}");
            assert!(destination.is_symlink(), "{name}");
            std::fs::remove_dir_all(&folder).unwrap();
        }
    }

    #[test]
    fn temporary_files_are_new_hidden_and_differently_named() {
        let folder = scratch_folder("temporary-names");
        let destination = folder.join("library.xml");

        let (_, first_path) = create_temporary_file(&destination).unwrap();
        let (_, second_path) = create_temporary_file(&destination).unwrap();

        assert_ne!(first_path, second_path);
        for path in [&first_path, &second_path] {
            assert_eq!(path.parent(), Some(folder.as_path()));
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                name.starts_with(".library.xml.") && name.ends_with(".tmp"),
                "{name}"
            );
            assert_eq!(std::fs::read(path).unwrap(), b"");
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
