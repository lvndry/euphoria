// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes key, tempo, beat grid, cues and the energy label into the
//! `<Song>` entries of an existing VirtualDJ `database.xml`.
//!
//! The database is changed in place, edit by edit: every byte outside the
//! attributes and elements written here is kept as it was, including
//! elements and attributes this module does not know, other songs, the
//! layout and the line endings. A song is matched by its `FilePath`.
//! Songs the database does not hold are not added: VirtualDJ adds a file
//! itself when it first sees it.
//!
//! The format follows VirtualDJ's own description of the database
//! (<https://www.virtualdj.com/wiki/VDJ_database.html>), the example of a
//! database VirtualDJ wrote on the Mixxx wiki
//! (<https://github.com/mixxxdj/mixxx/wiki/Virtual-Dj-Cue-Storage-Format>),
//! and the reader and writer of vdj-data
//! (<https://github.com/epistemex/vdj-data>, `src/vdj.poi.js`,
//! `src/vdj.song.js`):
//!
//! - `Bpm` of `<Scan>` and `<Tags>` is the time between two beats in
//!   seconds, written with six decimals.
//! - `Key` is a musical name with sharps, such as `Am`, `A#` or `F#m`.
//!   VirtualDJ shows it in the notation its `keyDisplay` option asks for.
//! - The beat grid is anchored by `<Poi Pos="…" Type="beatgrid" />`, in
//!   seconds. VirtualDJ 2026 keeps the anchor in `Phase` of `<Scan>` as
//!   well (<https://github.com/DeedjayNo1/beatgrid_fix>), so both are
//!   written.
//! - A hot cue is `<Poi Name="…" Pos="…" Num="1" />`, numbered from 1,
//!   with no `Type`, as VirtualDJ writes it.
//! - `<Comment>` holds the comment as text.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};

use super::rekordbox::{
    CueKind, ExistingFile, Track, escape_attribute_value, export_error, is_xml_character,
    tempo_text, write_file,
};
use crate::error::{EuphoriaError, ExportProblem};
use crate::key::{Key, Mode};
use crate::tags::KeyNotation;
use crate::tags::LabelPlacement;
use crate::tags::label::{label_text, with_label};

const ROOT_ELEMENT: &str = "VirtualDJ_Database";
const BYTE_ORDER_MARK: char = '\u{feff}';
const DEFAULT_CHILD_INDENT: &str = "  ";
const SECONDS_PER_MINUTE: f64 = 60.0;

const SHARP_TONIC_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Choices that change what a VirtualDJ export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VirtualDjOptions {
    /// Where the key and energy label, such as "8A - Energy 7", goes: the
    /// start or the end of `<Comment>`, the start of `Grouping` in
    /// `<Tags>`, or nowhere. The text already there is kept, and a label an
    /// earlier export left is replaced.
    pub label: LabelPlacement,
    /// How the key is spelled in the label. The `Key` attributes are
    /// always musical names, which VirtualDJ converts for display.
    pub label_notation: KeyNotation,
}

/// One track to write, with the energy rating that goes into its label.
#[derive(Debug, Clone, Copy)]
pub struct VirtualDjTrack<'track> {
    /// The track, as [`super::assemble_library_with`] builds it. Its
    /// `comment` is not used: the label is made from `key` and `energy`.
    pub track: &'track Track,
    /// The energy rating, 1 to 10.
    pub energy: Option<u8>,
}

/// What a merge changed, and the new text of the database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatabaseMerge {
    /// The whole database with the changes made.
    pub document: String,
    /// The tracks whose song was updated, in the order given.
    pub updated: Vec<PathBuf>,
    /// The tracks that match no song of the database, in the order given.
    pub not_in_database: Vec<PathBuf>,
    /// The tracks whose song has cues of its own, which were kept: no cue
    /// was written to them.
    pub kept_own_cues: Vec<PathBuf>,
    /// The number of cues left out because they are on no hot cue pad.
    /// VirtualDJ has no memory cues.
    pub cues_without_pad: usize,
}

/// The `Key` text VirtualDJ uses for `key`, such as `Am`, `A#` or `F#m`.
pub fn virtualdj_key(key: Key) -> String {
    let tonic = SHARP_TONIC_NAMES[usize::from(key.tonic() % 12)];
    match key.mode() {
        Mode::Major => tonic.to_string(),
        Mode::Minor => format!("{tonic}m"),
    }
}

/// The `Bpm` text VirtualDJ uses for a tempo: the seconds between two
/// beats, with six decimals, or `None` for a tempo that is not a positive
/// number.
pub fn seconds_per_beat_text(bpm: f64) -> Option<String> {
    tempo_text(bpm, 6)?;
    Some(format!("{:.6}", SECONDS_PER_MINUTE / bpm))
}

/// The tempo a `Bpm` text of VirtualDJ stands for, or `None` when it is not
/// a positive number.
pub fn bpm_from_seconds_per_beat(text: &str) -> Option<f64> {
    let seconds = text.trim().parse::<f64>().ok()?;
    (seconds.is_finite() && seconds > 0.0).then(|| SECONDS_PER_MINUTE / seconds)
}

/// Merges `tracks` into the text of a VirtualDJ database.
///
/// A song matches a track when its `FilePath` is the track's path, or, on
/// a Windows path with a drive letter, the same path in another case.
/// Failing that, `resolve` is asked for the real location of a `FilePath`
/// whose file name is the name of one of the tracks, such as
/// [`std::fs::canonicalize`], so that another spelling of the same file
/// matches too.
///
/// For each matched song, `Key` and `Bpm` of `<Tags>` and `<Scan>` are set
/// when the track has them. A track with a first beat and a tempo gets a
/// beat grid: `Phase` of `<Scan>`, and one `beatgrid` point in place of the
/// ones the song had. Hot cues are written as numbered points when the song
/// has no cue of its own. The label goes where `options` says.
///
/// `database_path` is only used in error messages.
///
/// # Errors
///
/// Returns [`EuphoriaError::VirtualDjDatabase`] when the text is not XML
/// or its root is not `VirtualDJ_Database`, and the errors of the rekordbox
/// export for a tempo that cannot be written or text XML cannot carry.
/// Nothing is changed then.
pub fn merge_database(
    database: &str,
    database_path: &Path,
    tracks: &[VirtualDjTrack<'_>],
    options: &VirtualDjOptions,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> Result<DatabaseMerge, EuphoriaError> {
    let refuse = |reason: String| EuphoriaError::VirtualDjDatabase {
        path: database_path.to_path_buf(),
        reason,
    };
    let (byte_order_mark, text) = match database.strip_prefix(BYTE_ORDER_MARK) {
        Some(rest) => (true, rest),
        None => (false, database),
    };
    let document = Document::parse(text)
        .map_err(|error| refuse(format!("it is not well-formed XML ({error})")))?;
    let root = document.root_element();
    if root.tag_name().name() != ROOT_ELEMENT {
        return Err(refuse(format!(
            "its root element is <{}>, not <{ROOT_ELEMENT}>",
            root.tag_name().name()
        )));
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };

    let mut by_path: HashMap<String, usize> = HashMap::with_capacity(tracks.len());
    let mut by_file_name: HashMap<String, Vec<usize>> = HashMap::with_capacity(tracks.len());
    for (index, entry) in tracks.iter().enumerate() {
        let path_text = comparable_path(&entry.track.path.to_string_lossy());
        if by_path.insert(path_text, index).is_some() {
            return Err(export_error(
                &entry.track.path,
                ExportProblem::DuplicatePath,
            ));
        }
        if let Some(file_name) = entry.track.path.file_name() {
            by_file_name
                .entry(file_name.to_string_lossy().to_lowercase())
                .or_default()
                .push(index);
        }
    }

    let mut matched = vec![false; tracks.len()];
    let mut kept_own_cues = vec![false; tracks.len()];
    let mut cues_without_pad = 0;
    let mut edits = Vec::new();
    for song in root
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Song")
    {
        let Some(file_path) = song.attribute("FilePath") else {
            continue;
        };
        let Some(index) = match_track(file_path, tracks, &by_path, &by_file_name, &resolve) else {
            continue;
        };
        let entry = tracks[index];
        let outcome = song_edits(text, song, entry, options, newline, &mut edits)?;
        matched[index] = true;
        kept_own_cues[index] |= outcome.kept_own_cues;
        cues_without_pad += outcome.cues_without_pad;
    }

    let mut merged = String::with_capacity(database.len() + edits.len() * 32);
    if byte_order_mark {
        merged.push(BYTE_ORDER_MARK);
    }
    merged.push_str(&apply_edits(text, edits));
    Document::parse(&merged).map_err(|error| {
        refuse(format!(
            "the updated database would not be well-formed XML ({error})"
        ))
    })?;

    let pick = |flags: &[bool]| -> Vec<PathBuf> {
        tracks
            .iter()
            .zip(flags)
            .filter(|(_, flag)| **flag)
            .map(|(entry, _)| entry.track.path.clone())
            .collect()
    };
    let unmatched: Vec<bool> = matched.iter().map(|flag| !flag).collect();
    Ok(DatabaseMerge {
        document: merged,
        updated: pick(&matched),
        not_in_database: pick(&unmatched),
        kept_own_cues: pick(&kept_own_cues),
        cues_without_pad,
    })
}

/// Merges `tracks` into the VirtualDJ database at `database`, after saving
/// a copy of it at `backup`.
///
/// The backup is a new file, never one that exists. The database is read
/// again just before it is replaced, and the export stops when it changed
/// in the meantime, as it does when VirtualDJ saves it. The new database
/// is written to a temporary file next to it and moved into place, so the
/// database holds either the old or the new text.
///
/// # Errors
///
/// The errors of [`merge_database`]. Returns
/// [`EuphoriaError::VirtualDjDatabase`] when the database is not UTF-8 or
/// changed while the export ran, and [`EuphoriaError::ExportFile`] when it
/// or the backup cannot be read or written.
pub fn update_database_file(
    database: &Path,
    backup: &Path,
    tracks: &[VirtualDjTrack<'_>],
    options: &VirtualDjOptions,
) -> Result<DatabaseMerge, EuphoriaError> {
    let original = read_database(database)?;
    let merge = merge_database(&original, database, tracks, options, |file_path| {
        std::fs::canonicalize(file_path).ok()
    })?;
    if merge.updated.is_empty() {
        return Ok(merge);
    }
    write_file(backup, original.as_bytes(), ExistingFile::Keep)?;
    if read_database(database)? != original {
        return Err(EuphoriaError::VirtualDjDatabase {
            path: database.to_path_buf(),
            reason: "it changed while the export ran; close VirtualDJ and export again".to_string(),
        });
    }
    write_file(database, merge.document.as_bytes(), ExistingFile::Replace)?;
    Ok(merge)
}

/// Reads a VirtualDJ database and merges `tracks` into it, writing nothing.
///
/// # Errors
///
/// As [`update_database_file`], without the errors of writing.
pub fn preview_database_update(
    database: &Path,
    tracks: &[VirtualDjTrack<'_>],
    options: &VirtualDjOptions,
) -> Result<DatabaseMerge, EuphoriaError> {
    let original = read_database(database)?;
    merge_database(&original, database, tracks, options, |file_path| {
        std::fs::canonicalize(file_path).ok()
    })
}

fn read_database(database: &Path) -> Result<String, EuphoriaError> {
    let bytes = std::fs::read(database).map_err(|source| EuphoriaError::ExportFile {
        path: database.to_path_buf(),
        source,
    })?;
    String::from_utf8(bytes).map_err(|_| EuphoriaError::VirtualDjDatabase {
        path: database.to_path_buf(),
        reason: "it is not UTF-8 text".to_string(),
    })
}

/// A path as it is compared: without the `\\?\` prefix Windows gives a
/// resolved path, and in lower case when it starts with a drive letter,
/// since Windows paths are not case sensitive.
fn comparable_path(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = path.as_bytes();
    let has_drive_letter = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if has_drive_letter {
        path.replace('/', "\\").to_lowercase()
    } else {
        path.to_string()
    }
}

fn match_track(
    file_path: &str,
    tracks: &[VirtualDjTrack<'_>],
    by_path: &HashMap<String, usize>,
    by_file_name: &HashMap<String, Vec<usize>>,
    resolve: &impl Fn(&str) -> Option<PathBuf>,
) -> Option<usize> {
    if let Some(index) = by_path.get(&comparable_path(file_path)) {
        return Some(*index);
    }
    let file_name = file_path.rsplit(['/', '\\']).next()?.to_lowercase();
    let candidates = by_file_name.get(&file_name)?;
    let resolved = resolve(file_path)?;
    candidates
        .iter()
        .copied()
        .find(|index| tracks[*index].track.path == resolved)
}

/// A change to the text: `range` is replaced with `text`. An empty range
/// inserts.
#[derive(Debug)]
struct Edit {
    range: Range<usize>,
    text: String,
}

/// `text` with `edits` made. Edits at the same place are made in the order
/// they were pushed.
fn apply_edits(text: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for edit in edits {
        debug_assert!(edit.range.start >= cursor, "edits overlap");
        let start = edit.range.start.max(cursor);
        result.push_str(&text[cursor..start]);
        result.push_str(&edit.text);
        cursor = cursor.max(edit.range.end);
    }
    result.push_str(&text[cursor..]);
    result
}

struct SongOutcome {
    kept_own_cues: bool,
    cues_without_pad: usize,
}

/// Where a new child element of a song goes.
struct Layout<'text> {
    text: &'text str,
    newline: &'text str,
    child_indent: String,
}

impl Layout<'_> {
    fn after(&self, node: Node<'_, '_>, element: &str) -> Edit {
        let at = node.range().end;
        Edit {
            range: at..at,
            text: format!("{}{}{element}", self.newline, self.child_indent),
        }
    }

    fn before(&self, node: Node<'_, '_>, element: &str) -> Edit {
        let at = node.range().start;
        Edit {
            range: at..at,
            text: format!("{element}{}{}", self.newline, self.child_indent),
        }
    }

    /// Removes `node` with the white space that starts its line.
    fn remove(&self, node: Node<'_, '_>) -> Edit {
        let range = node.range();
        let before = &self.text[..range.start];
        let line_start = before
            .rfind('\n')
            .map(|newline| {
                if before[..newline].ends_with('\r') {
                    newline - 1
                } else {
                    newline
                }
            })
            .filter(|start| before[*start..].trim().is_empty());
        let start = line_start.unwrap_or(range.start);
        Edit {
            range: start..range.end,
            text: String::new(),
        }
    }
}

fn song_edits(
    text: &str,
    song: Node<'_, '_>,
    entry: VirtualDjTrack<'_>,
    options: &VirtualDjOptions,
    newline: &str,
    edits: &mut Vec<Edit>,
) -> Result<SongOutcome, EuphoriaError> {
    let track = entry.track;
    let children: Vec<Node<'_, '_>> = song.children().filter(Node::is_element).collect();
    let child = |name: &str| {
        children
            .iter()
            .copied()
            .find(|node| node.has_tag_name(name))
    };
    let layout = Layout {
        text,
        newline,
        child_indent: children
            .first()
            .map(|first| indent_of(text, first.range().start))
            .unwrap_or_else(|| DEFAULT_CHILD_INDENT.to_string()),
    };
    let new_children_at = if children.is_empty() {
        Some(empty_song_insertion_point(text, song))
    } else {
        None
    };
    let mut first_children = String::new();
    let mut add_child = |edits: &mut Vec<Edit>, after: Option<Node<'_, '_>>, element: String| match (
        after,
        children.first(),
    ) {
        (Some(after), _) => edits.push(layout.after(after, &element)),
        (None, Some(first)) => edits.push(layout.before(*first, &element)),
        (None, None) => {
            first_children.push_str(newline);
            first_children.push_str(&layout.child_indent);
            first_children.push_str(&element);
        }
    };

    let key = track.key.map(virtualdj_key);
    let seconds_per_beat = match track.bpm {
        Some(bpm) => Some(
            seconds_per_beat_text(bpm)
                .ok_or_else(|| export_error(&track.path, ExportProblem::InvalidTempo))?,
        ),
        None => None,
    };
    let grid_anchor = track
        .first_beat
        .filter(|_| seconds_per_beat.is_some())
        .map(|first_beat| format!("{:.6}", first_beat.as_secs_f64()));
    let label = label_text(track.key, entry.energy, options.label_notation);

    let mut tag_values: Vec<(&str, String)> = Vec::new();
    if let Some(seconds_per_beat) = &seconds_per_beat {
        tag_values.push(("Bpm", seconds_per_beat.clone()));
    }
    if let Some(key) = &key {
        tag_values.push(("Key", key.clone()));
    }
    if let (Some(label), LabelPlacement::Grouping) = (&label, options.label) {
        let existing = child("Tags").and_then(|tags| tags.attribute("Grouping"));
        tag_values.push(("Grouping", with_label(label, existing, false)));
    }
    let tags = child("Tags");
    match tags {
        Some(tags) => set_attributes(text, tags, &tag_values, edits)?,
        None if !tag_values.is_empty() => {
            add_child(edits, None, new_element("Tags", &tag_values)?);
        }
        None => {}
    }

    let comment = child("Comment");
    let comment_label = match options.label {
        LabelPlacement::CommentStart => label.as_ref().map(|label| (label, false)),
        LabelPlacement::CommentEnd => label.as_ref().map(|label| (label, true)),
        LabelPlacement::Grouping | LabelPlacement::None => None,
    };
    if let Some((label, at_end)) = comment_label {
        match comment {
            Some(comment) => {
                if let Some(edit) = comment_edit(text, comment, label, at_end)? {
                    edits.push(edit);
                }
            }
            None => {
                let after = child("Infos").or(tags);
                add_child(
                    edits,
                    after,
                    format!("<Comment>{}</Comment>", escape_text(label)?),
                );
            }
        }
    }

    let mut scan_values: Vec<(&str, String)> = Vec::new();
    if let Some(seconds_per_beat) = &seconds_per_beat {
        scan_values.push(("Bpm", seconds_per_beat.clone()));
    }
    if let Some(key) = &key {
        scan_values.push(("Key", key.clone()));
    }
    if let Some(anchor) = &grid_anchor {
        scan_values.push(("Phase", anchor.clone()));
    }
    let scan = child("Scan");
    if let Some(scan) = scan {
        set_attributes(text, scan, &scan_values, edits)?;
    }

    let points: Vec<Node<'_, '_>> = children
        .iter()
        .copied()
        .filter(|node| node.has_tag_name("Poi"))
        .collect();
    let after_metadata = scan.or(comment).or(child("Infos")).or(tags);
    if let Some(anchor) = &grid_anchor {
        let grid_point = new_element(
            "Poi",
            &[("Pos", anchor.clone()), ("Type", "beatgrid".to_string())],
        )?;
        let grids: Vec<Node<'_, '_>> = points
            .iter()
            .copied()
            .filter(|point| point.attribute("Type") == Some("beatgrid"))
            .collect();
        match grids.split_first() {
            Some((first, rest)) => {
                edits.push(Edit {
                    range: first.range(),
                    text: grid_point,
                });
                for other in rest {
                    edits.push(layout.remove(*other));
                }
            }
            None => match points.first() {
                Some(first) => edits.push(layout.before(*first, &grid_point)),
                None => add_child(edits, after_metadata, grid_point),
            },
        }
    }

    let hot_cues: Vec<(u8, &str, f64)> = track
        .cue_points
        .iter()
        .filter_map(|cue_point| match cue_point.kind {
            CueKind::Hot(pad) => Some((pad, cue_point.name.as_str(), cue_point.time.as_secs_f64())),
            CueKind::Memory => None,
        })
        .collect();
    let memory_only = track
        .cue_points
        .iter()
        .filter(|cue_point| {
            cue_point.kind == CueKind::Memory
                && !track.cue_points.iter().any(|other| {
                    matches!(other.kind, CueKind::Hot(_))
                        && other.time == cue_point.time
                        && other.name == cue_point.name
                })
        })
        .count();
    let has_own_cues = points
        .iter()
        .any(|point| matches!(point.attribute("Type"), None | Some("cue")));
    let mut outcome = SongOutcome {
        kept_own_cues: false,
        cues_without_pad: 0,
    };
    if !track.cue_points.is_empty() {
        if has_own_cues {
            outcome.kept_own_cues = true;
        } else {
            outcome.cues_without_pad = memory_only;
            let mut cue_elements = Vec::with_capacity(hot_cues.len());
            for (pad, name, seconds) in hot_cues {
                cue_elements.push(new_element(
                    "Poi",
                    &[
                        ("Name", name.to_string()),
                        ("Pos", format!("{seconds:.6}")),
                        ("Num", (u16::from(pad) + 1).to_string()),
                    ],
                )?);
            }
            let after = points.last().copied().or(after_metadata);
            for element in cue_elements {
                add_child(edits, after, element);
            }
        }
    }

    if let Some(at) = new_children_at
        && !first_children.is_empty()
    {
        let song_indent = indent_of(text, song.range().start);
        let text = if at.is_empty() {
            format!("{first_children}{newline}{song_indent}")
        } else {
            format!(">{first_children}{newline}{song_indent}</Song>")
        };
        edits.push(Edit { range: at, text });
    }
    Ok(outcome)
}

/// Where the children of a song with none go: the end of its start tag
/// for `<Song …></Song>`, as an empty range, or the `/>` that ends
/// `<Song …/>`, as the range to replace.
fn empty_song_insertion_point(text: &str, song: Node<'_, '_>) -> Range<usize> {
    let range = song.range();
    let element = &text[range.clone()];
    if element.ends_with("/>") {
        return range.end - 2..range.end;
    }
    let closing = range.start + element.rfind("</").unwrap_or(element.len());
    closing..closing
}

/// The white space between the start of the line and `position`, or the
/// default indent when something else is there.
fn indent_of(text: &str, position: usize) -> String {
    let before = &text[..position];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let indent = &before[line_start..];
    if indent
        .chars()
        .all(|character| character == ' ' || character == '\t')
    {
        indent.to_string()
    } else {
        DEFAULT_CHILD_INDENT.to_string()
    }
}

/// Sets each attribute of `values` on `element`: an attribute it has gets
/// the new value in place, the others are added after its last attribute.
fn set_attributes(
    text: &str,
    element: Node<'_, '_>,
    values: &[(&str, String)],
    edits: &mut Vec<Edit>,
) -> Result<(), EuphoriaError> {
    let mut added = String::new();
    for (name, value) in values {
        let escaped = escape_attribute_value(value)?;
        match element
            .attributes()
            .find(|attribute| attribute.name() == *name && attribute.namespace().is_none())
        {
            Some(attribute) => {
                if attribute.value() != value {
                    edits.push(Edit {
                        range: attribute.range_value(),
                        text: escaped,
                    });
                }
            }
            None => {
                added.push(' ');
                added.push_str(name);
                added.push_str("=\"");
                added.push_str(&escaped);
                added.push('"');
            }
        }
    }
    if !added.is_empty() {
        let at = element
            .attributes()
            .map(|attribute| attribute.range().end)
            .max()
            .unwrap_or_else(|| {
                let start = element.range().start + 1;
                start
                    + text[start..]
                        .find(|character: char| {
                            character.is_whitespace() || character == '/' || character == '>'
                        })
                        .unwrap_or(0)
            });
        edits.push(Edit {
            range: at..at,
            text: added,
        });
    }
    Ok(())
}

/// The edit that puts `label` into an existing `<Comment>`, or `None` when
/// it already holds it. A comment with child elements is left alone.
fn comment_edit(
    text: &str,
    comment: Node<'_, '_>,
    label: &str,
    at_end: bool,
) -> Result<Option<Edit>, EuphoriaError> {
    if comment.children().any(|node| !node.is_text()) {
        return Ok(None);
    }
    let existing: String = comment.children().filter_map(|node| node.text()).collect();
    let existing = (!existing.is_empty()).then_some(existing);
    let new = with_label(label, existing.as_deref(), at_end);
    if existing.as_deref() == Some(new.as_str()) {
        return Ok(None);
    }
    let escaped = escape_text(&new)?;
    let range = comment.range();
    let element = &text[range.clone()];
    if let (Some(first), Some(last)) = (comment.first_child(), comment.last_child()) {
        return Ok(Some(Edit {
            range: first.range().start..last.range().end,
            text: escaped,
        }));
    }
    if element.ends_with("/>") {
        let start_tag = element[..element.len() - 2].trim_end();
        return Ok(Some(Edit {
            range,
            text: format!("{start_tag}>{escaped}</Comment>"),
        }));
    }
    let closing = element.rfind("</").unwrap_or(element.len());
    let at = range.start + closing;
    Ok(Some(Edit {
        range: at..at,
        text: escaped,
    }))
}

fn new_element(name: &str, values: &[(&str, String)]) -> Result<String, EuphoriaError> {
    let mut element = format!("<{name}");
    for (attribute, value) in values {
        element.push(' ');
        element.push_str(attribute);
        element.push_str("=\"");
        element.push_str(&escape_attribute_value(value)?);
        element.push('"');
    }
    element.push_str(" />");
    Ok(element)
}

fn escape_text(value: &str) -> Result<String, EuphoriaError> {
    if !value.chars().all(is_xml_character) {
        return Err(EuphoriaError::ExportTextNotXml);
    }
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\r' => escaped.push_str("&#13;"),
            _ => escaped.push(character),
        }
    }
    Ok(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_use_sharps_and_a_trailing_m_for_minor() {
        assert_eq!(virtualdj_key(Key::new(9, Mode::Minor)), "Am");
        assert_eq!(virtualdj_key(Key::new(10, Mode::Major)), "A#");
        assert_eq!(virtualdj_key(Key::new(6, Mode::Minor)), "F#m");
        assert_eq!(virtualdj_key(Key::new(1, Mode::Major)), "C#");
    }

    #[test]
    fn tempo_is_written_as_seconds_per_beat() {
        assert_eq!(seconds_per_beat_text(128.0).as_deref(), Some("0.468750"));
        assert_eq!(seconds_per_beat_text(77.0).as_deref(), Some("0.779221"));
        assert_eq!(seconds_per_beat_text(0.0), None);
        assert_eq!(seconds_per_beat_text(f64::NAN), None);
        let bpm = bpm_from_seconds_per_beat("0.468750").unwrap();
        assert!((bpm - 128.0).abs() < 1e-9);
        assert_eq!(bpm_from_seconds_per_beat("0"), None);
    }

    #[test]
    fn windows_paths_compare_without_case_or_the_verbatim_prefix() {
        assert_eq!(
            comparable_path(r"\\?\C:\Music\A.mp3"),
            comparable_path(r"c:\music\a.mp3")
        );
        assert_ne!(
            comparable_path("/Music/A.mp3"),
            comparable_path("/music/a.mp3")
        );
    }

    const DATABASE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<VirtualDJ_Database Version=\"2026\">\r\n <Song FilePath=\"/music/a.mp3\" FileSize=\"1\" Mystery=\"kept\">\r\n  <Tags Author=\"A\" Title=\"T\" Flag=\"1\" />\r\n  <Infos SongLength=\"300.0\" PlayCount=\"7\" />\r\n  <Comment>my note</Comment>\r\n  <Scan Version=\"801\" Bpm=\"0.500000\" Key=\"C\" Flag=\"32768\" />\r\n  <Poi Pos=\"0.100000\" Type=\"beatgrid\" />\r\n  <Poi Pos=\"9.000000\" Type=\"beatgrid\" Bpm=\"125\" />\r\n  <Poi Pos=\"10.000000\" Type=\"automix\" Point=\"fadeStart\" />\r\n  <Unknown Thing=\"1\"><Inner/></Unknown>\r\n </Song>\r\n <Song FilePath=\"/music/other.mp3\" FileSize=\"2\">\r\n  <Tags Title=\"Other\" />\r\n </Song>\r\n</VirtualDJ_Database>\r\n";

    fn track() -> Track {
        Track {
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(128.0),
            first_beat: Some(std::time::Duration::from_millis(250)),
            cue_points: vec![
                super::super::rekordbox::CuePoint {
                    name: "Drop".to_string(),
                    time: std::time::Duration::from_secs(30),
                    kind: CueKind::Memory,
                },
                super::super::rekordbox::CuePoint {
                    name: "Drop".to_string(),
                    time: std::time::Duration::from_secs(30),
                    kind: CueKind::Hot(1),
                },
            ],
            ..Track::new("/music/a.mp3", std::time::Duration::from_secs(300))
        }
    }

    #[test]
    fn a_song_is_updated_in_place_and_everything_else_is_kept() {
        let track = track();
        let merge = merge_database(
            DATABASE,
            Path::new("database.xml"),
            &[VirtualDjTrack {
                track: &track,
                energy: Some(7),
            }],
            &VirtualDjOptions::default(),
            |_| None,
        )
        .unwrap();
        assert_eq!(merge.updated, vec![PathBuf::from("/music/a.mp3")]);
        let expected = DATABASE
            .replace("Flag=\"1\" />", "Flag=\"1\" Bpm=\"0.468750\" Key=\"Am\" />")
            .replace("<Comment>my note</Comment>", "<Comment>8A - Energy 7 - my note</Comment>")
            .replace("Bpm=\"0.500000\" Key=\"C\" Flag=\"32768\" />", "Bpm=\"0.468750\" Key=\"Am\" Flag=\"32768\" Phase=\"0.250000\" />")
            .replace("<Poi Pos=\"0.100000\" Type=\"beatgrid\" />\r\n  <Poi Pos=\"9.000000\" Type=\"beatgrid\" Bpm=\"125\" />", "<Poi Pos=\"0.250000\" Type=\"beatgrid\" />")
            .replace("<Poi Pos=\"10.000000\" Type=\"automix\" Point=\"fadeStart\" />", "<Poi Pos=\"10.000000\" Type=\"automix\" Point=\"fadeStart\" />\r\n  <Poi Name=\"Drop\" Pos=\"30.000000\" Num=\"2\" />");
        assert_eq!(merge.document, expected);
    }

    #[test]
    fn edits_at_one_place_keep_their_order() {
        let edits = vec![
            Edit {
                range: 1..1,
                text: "x".to_string(),
            },
            Edit {
                range: 1..1,
                text: "y".to_string(),
            },
            Edit {
                range: 2..3,
                text: "Z".to_string(),
            },
        ];
        assert_eq!(apply_edits("abcd", edits), "axybZd");
    }
}
