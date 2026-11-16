// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The results as a person reads them on a terminal: a table that fits the
//! width of the window, and a detailed view of one track.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use euphoria_core::quality::{QualityReport, QualityVerdict};
use euphoria_core::rekordbox_grid::GridSource;
use euphoria_core::{
    Confidence, Correction, FileResult, Key, ReviewReason, TagKey, TagKeyAgreement, TempoRange,
    TrackAnalysis, TrackCues, note_names, notes_in_key,
};

use crate::grid_import::GridInfo;
use crate::style::Theme;
use crate::tag_key::TagKeys;
use crate::terminal::Printable;

/// The width used when the output is not a terminal.
pub const DEFAULT_WIDTH: usize = 100;

/// The width of the terminal standard output goes to, or
/// [`DEFAULT_WIDTH`].
pub fn terminal_width() -> usize {
    console::Term::stdout()
        .size_checked()
        .map_or(DEFAULT_WIDTH, |(_, columns)| usize::from(columns))
}

/// The width of the terminal standard error goes to, when it is one.
pub fn standard_error_width() -> Option<usize> {
    console::Term::stderr()
        .size_checked()
        .map(|(_, columns)| usize::from(columns))
}

/// `text` with every line longer than `width` broken between words, the
/// lines it continues on indented by two spaces, so that a terminal never
/// breaks a word in two.
pub fn wrap_lines(text: &str, width: usize) -> String {
    let mut wrapped = String::new();
    for line in text.lines() {
        let mut current = String::new();
        let mut has_word = false;
        for word in line.split(' ') {
            if has_word
                && !current.trim().is_empty()
                && width_of(&current) + 1 + width_of(word) > width
            {
                wrapped.push_str(&current);
                wrapped.push('\n');
                current = "  ".to_string();
                has_word = false;
            }
            if has_word {
                current.push(' ');
            }
            current.push_str(word);
            has_word = true;
        }
        wrapped.push_str(&current);
        wrapped.push('\n');
    }
    wrapped
}

/// How a key is named.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum KeyNotationChoice {
    /// Camelot codes such as 8A.
    #[default]
    Camelot,
    /// Musical names such as Am.
    Musical,
    /// Open Key codes such as 1m, as Traktor writes them.
    OpenKey,
}

impl KeyNotationChoice {
    /// The key in this notation, as short as it goes.
    pub(crate) fn short(self, key: Key) -> String {
        match self {
            KeyNotationChoice::Camelot => key.camelot(),
            KeyNotationChoice::Musical => key.short_name(),
            KeyNotationChoice::OpenKey => key.open_key(),
        }
    }

    /// The key in another notation, shown next to it in a quieter style.
    pub fn companion(self, key: Key) -> String {
        match self {
            KeyNotationChoice::Camelot | KeyNotationChoice::OpenKey => key.to_string(),
            KeyNotationChoice::Musical => key.camelot(),
        }
    }
}

/// What the views need besides the results.
#[derive(Debug, Clone, Copy)]
pub struct ViewOptions {
    pub notation: KeyNotationChoice,
    pub tempo_range: TempoRange,
    /// Whether the detailed view shows the notes the key analysis heard.
    pub notes: bool,
    /// Columns of text the view may use.
    pub width: usize,
}

/// Three dots, as many filled as the level is high: a DJ reads it at a
/// glance, and the detailed view names the level next to the same dots.
fn confidence_meter(theme: &Theme, confidence: Option<Confidence>) -> String {
    let filled = match confidence {
        Some(Confidence::High) => 3,
        Some(Confidence::Medium) => 2,
        Some(Confidence::Low) => 1,
        None => return "   ".to_string(),
    };
    let dots = format!(
        "{}{}",
        "●".repeat(filled),
        theme.dim("○".repeat(3 - filled))
    );
    if confidence == Some(Confidence::Low) {
        theme.caution(dots)
    } else {
        dots
    }
}

const ENERGY_BAR_CELLS: usize = 10;

/// A bar of ten cells, one per point of energy.
fn energy_bar(theme: &Theme, energy: u8) -> String {
    let filled = usize::from(energy.min(10));
    format!(
        "{}{}",
        "━".repeat(filled),
        theme.dim("─".repeat(ENERGY_BAR_CELLS - filled))
    )
}

/// Writes each cue on a line of its own: its pad, where it is, its name and
/// the energy of the section it starts.
fn write_cues(writer: &mut impl Write, theme: &Theme, cues: &TrackCues) -> io::Result<()> {
    let name_width = cues
        .cues
        .iter()
        .map(|cue| width_of(&cue.name))
        .max()
        .unwrap_or(0);
    for (index, cue) in cues.cues.iter().enumerate() {
        let pad = cue
            .hot_cue
            .map_or_else(|| "-".to_string(), |hot_cue| hot_cue.to_string());
        let name = Printable(&cue.name).to_string();
        let padding = " ".repeat(name_width - width_of(&name));
        let mut text = format!(
            "{pad}  {:>5}  {name}{padding}",
            format_duration(cue.seconds as f32)
        );
        if let Some(energy) = cue.energy {
            text.push_str(&format!("  energy {}", theme.bold(energy)));
        }
        let label = if index == 0 { "Cues" } else { "" };
        write_line(writer, theme, label, text.trim_end())?;
    }
    if cues.is_edited() {
        write_line(writer, theme, "", &theme.user("set by you"))?;
    } else if cues.cues.iter().all(|cue| cue.energy.is_none()) {
        write_line(
            writer,
            theme,
            "",
            &theme.dim("energy not measured yet, analyze again with --reanalyze"),
        )?;
    }
    Ok(())
}

/// The sound quality in a word, for the table.
pub fn quality_word(verdict: QualityVerdict) -> &'static str {
    match verdict {
        QualityVerdict::Lossless => "lossless",
        QualityVerdict::LimitedBandwidth => "limited bw",
        QualityVerdict::Good => "good",
        QualityVerdict::Acceptable => "acceptable",
        QualityVerdict::WeakOnABigSystem => "weak",
        QualityVerdict::LikelyReencoded => "re-encoded?",
    }
}

/// `text` coloured by how good `verdict` is.
fn quality_coloured(theme: &Theme, verdict: QualityVerdict, text: &str) -> String {
    match verdict {
        QualityVerdict::Lossless | QualityVerdict::Good => theme.good(text),
        QualityVerdict::LimitedBandwidth => text.to_string(),
        QualityVerdict::Acceptable => text.to_string(),
        QualityVerdict::WeakOnABigSystem => theme.caution(text),
        QualityVerdict::LikelyReencoded => theme.bad(text),
    }
}

pub fn format_duration(seconds: f32) -> String {
    let whole_seconds = seconds.round() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

/// Width on the terminal of text that may hold styles.
pub fn width_of(text: &str) -> usize {
    console::measure_text_width(text)
}

/// One cell: its text with styles, laid out by the width of the text
/// without them.
#[derive(Debug, Clone)]
struct Cell {
    styled: String,
    width: usize,
}

impl Cell {
    fn new(styled: String) -> Self {
        let width = width_of(&styled);
        Self { styled, width }
    }

    fn empty() -> Self {
        Self::new(String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
}

/// The columns of the table. The order of the variants is the order on
/// screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Column {
    Key,
    KeyCompanion,
    TagKey,
    Bpm,
    Energy,
    EnergyBar,
    Quality,
    Time,
    Review,
}

impl Column {
    const ALL: [Column; 9] = [
        Column::Key,
        Column::KeyCompanion,
        Column::TagKey,
        Column::Bpm,
        Column::Energy,
        Column::EnergyBar,
        Column::Quality,
        Column::Time,
        Column::Review,
    ];

    /// The order columns are left out in when the window is narrow, the
    /// most decorative first. The key, the tempo and the file always stay.
    const DROP_ORDER: [&'static [Column]; 7] = [
        &[Column::EnergyBar],
        &[Column::KeyCompanion],
        &[Column::Time],
        &[Column::TagKey],
        &[Column::Review],
        &[Column::Quality],
        &[Column::Energy],
    ];

    fn heading(self) -> &'static str {
        match self {
            Column::Key => "Key",
            Column::Bpm => "BPM",
            Column::Energy => "Energy",
            Column::Quality => "Quality",
            Column::Time => "Time",
            Column::TagKey => "Tags",
            Column::Review => "Review",
            Column::KeyCompanion | Column::EnergyBar => "",
        }
    }

    fn align(self) -> Align {
        match self {
            Column::Key | Column::Bpm | Column::Energy | Column::Time => Align::Right,
            _ => Align::Left,
        }
    }
}

const GAP: &str = "  ";
/// The file column keeps at least this many columns, or its longest name
/// when that is shorter, before other columns are dropped to make room.
const SHORTEST_FILE_COLUMN: usize = 24;

/// Everything shown for one track, in every column.
struct Row {
    cells: Vec<(Column, Cell)>,
    folder: String,
    name: String,
}

/// What the Tags column shows: nothing without a key field or a detected
/// key, "=" when they agree, "?" for text that is not a key, and otherwise
/// the key from the tags with how it differs, marked when it needs a look.
fn tag_key_cell(
    theme: &Theme,
    tag_keys: &TagKeys,
    path: &Path,
    analysis: &TrackAnalysis,
    options: &ViewOptions,
) -> String {
    let (Some(tag_key), Some(agreement)) = (tag_keys.get(path), tag_keys.agreement(path, analysis))
    else {
        return String::new();
    };
    match (agreement, tag_key.key) {
        (TagKeyAgreement::Differs(difference), Some(tagged)) => {
            let text = format!("{} {}", options.notation.short(tagged), difference.as_str());
            if tag_keys.conflict(path, analysis).is_some() {
                theme.caution(text)
            } else {
                theme.dim(text)
            }
        }
        (TagKeyAgreement::Same, _) => theme.dim("="),
        _ => theme.dim("?"),
    }
}

/// The cells a row has besides the analysis: the tags and the review.
struct Notes {
    tag_key: String,
    review: String,
}

fn row(
    theme: &Theme,
    analysis: &TrackAnalysis,
    options: &ViewOptions,
    notes: Notes,
    folder: String,
    name: String,
) -> Row {
    let Notes { tag_key, review } = notes;
    let key_text = analysis.key.map(|key| options.notation.short(key));
    let key = match (analysis.key, &key_text) {
        (Some(key), Some(text)) => theme.key(key, text),
        _ => theme.dim("-"),
    };
    let bpm = analysis
        .bpm
        .map_or_else(|| theme.dim("-"), |bpm| format!("{bpm:.1}"));
    let quality = analysis.quality.as_ref().map_or_else(
        || theme.dim("-"),
        |quality| quality_coloured(theme, quality.verdict, quality_word(quality.verdict)),
    );
    let cells = Column::ALL
        .iter()
        .map(|column| {
            let styled = match column {
                Column::Key => key.clone(),
                Column::KeyCompanion => analysis
                    .key
                    .map(|key| theme.dim(options.notation.companion(key)))
                    .unwrap_or_default(),
                Column::TagKey => tag_key.clone(),
                Column::Review => review.clone(),
                Column::Bpm => bpm.clone(),
                Column::Energy => analysis
                    .energy
                    .map_or_else(|| theme.dim("-"), |energy| energy.to_string()),
                Column::EnergyBar => analysis
                    .energy
                    .map(|energy| energy_bar(theme, energy))
                    .unwrap_or_default(),
                Column::Quality => quality.clone(),
                Column::Time => theme.dim(format_duration(analysis.duration_seconds)),
            };
            (*column, Cell::new(styled))
        })
        .collect();
    Row {
        cells,
        folder,
        name,
    }
}

/// A reason to review a track in a few words for the table.
fn review_word(reason: &ReviewReason) -> &'static str {
    match reason {
        ReviewReason::Failed(_) => "failed",
        ReviewReason::KeyUnsure => "key",
        ReviewReason::TempoUnsure => "tempo",
        ReviewReason::KeyConflict { .. } => "tags",
    }
}

/// A reason to review a track in words for the detailed view.
fn review_sentence(
    reason: &ReviewReason,
    analysis: &TrackAnalysis,
    options: &ViewOptions,
) -> String {
    match reason {
        ReviewReason::Failed(message) => format!("could not be analyzed: {message}"),
        ReviewReason::KeyUnsure => match analysis.key_runner_up {
            Some(runner_up) => format!(
                "key may be its relative or a fifth away (runner up {}  {})",
                options.notation.short(runner_up),
                options.notation.companion(runner_up)
            ),
            None => "key may be its relative or a fifth away".to_string(),
        },
        ReviewReason::TempoUnsure => {
            match analysis
                .bpm
                .and_then(|bpm| Some((bpm, options.tempo_range.alternative(bpm)?)))
            {
                Some((bpm, alternative)) => {
                    let counted = if alternative < bpm { "half" } else { "double" };
                    format!("BPM may be {alternative:.1}, counted at {counted} time")
                }
                None => "BPM may be half or double".to_string(),
            }
        }
        ReviewReason::KeyConflict { tag_key } => {
            format!("the tags say {}", options.notation.short(*tag_key))
        }
    }
}

/// What the Review column shows: the reasons in a word each, "checked" for
/// a track the user already settled, and nothing otherwise.
fn review_cell(theme: &Theme, reasons: &[ReviewReason], reviewed: bool) -> String {
    if !reasons.is_empty() {
        let words: Vec<&str> = reasons.iter().map(review_word).collect();
        theme.caution(words.join("+"))
    } else if reviewed {
        theme.dim("checked")
    } else {
        String::new()
    }
}

/// The folder all `paths` are in, so the table can leave it out of every
/// name.
fn common_folder(paths: &[&Path]) -> PathBuf {
    let mut folders = paths
        .iter()
        .map(|path| path.parent().unwrap_or(Path::new("")));
    let Some(first) = folders.next() else {
        return PathBuf::new();
    };
    let mut common: Vec<_> = first.components().collect();
    for folder in folders {
        let shared = common
            .iter()
            .zip(folder.components())
            .take_while(|(kept, other)| **kept == *other)
            .count();
        common.truncate(shared);
    }
    common.iter().collect()
}

/// The folder of `path` below `common`, with a trailing separator, and its
/// file name, both made safe to print.
fn folder_and_name(path: &Path, common: &Path) -> (String, String) {
    let relative = path.strip_prefix(common).unwrap_or(path);
    let name = relative
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = relative
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .map(|folder| format!("{}{}", folder.display(), std::path::MAIN_SEPARATOR))
        .unwrap_or_default();
    (Printable(folder).to_string(), Printable(name).to_string())
}

/// `text` cut to `width` columns, with an ellipsis in the middle so that
/// both the start and the end, where the extension is, stay readable.
pub fn shorten_middle(text: &str, width: usize) -> String {
    let characters: Vec<char> = text.chars().collect();
    if width_of(text) <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let kept = width - 1;
    let end_count = kept.div_ceil(2);
    let start_count = kept - end_count;
    let mut start = String::new();
    for character in &characters {
        if width_of(&format!("{start}{character}")) > start_count {
            break;
        }
        start.push(*character);
    }
    let mut end: Vec<char> = Vec::new();
    for character in characters.iter().rev() {
        let candidate: String = std::iter::once(*character)
            .chain(end.iter().rev().copied())
            .collect();
        if width_of(&candidate) > end_count {
            break;
        }
        end.push(*character);
    }
    end.reverse();
    format!("{start}…{}", end.into_iter().collect::<String>())
}

/// The file cell in at most `width` columns: the name in full when it fits,
/// the folders below the common one in a quieter style before it, and the
/// folders shortened first.
fn file_cell(theme: &Theme, folder: &str, name: &str, width: usize) -> String {
    let name_width = width_of(name);
    if name_width >= width {
        return shorten_middle(name, width);
    }
    let folder_room = width - name_width;
    let folder = if width_of(folder) <= folder_room {
        folder.to_string()
    } else if folder_room >= 2 {
        let tail: Vec<char> = folder.chars().collect();
        let mut kept = String::new();
        for character in tail.iter().rev() {
            if width_of(&kept) + width_of(&character.to_string()) + 1 > folder_room {
                break;
            }
            kept.insert(0, *character);
        }
        format!("…{kept}")
    } else {
        String::new()
    };
    format!("{}{name}", theme.dim(folder))
}

fn pad(cell: &Cell, width: usize, align: Align) -> String {
    let padding = " ".repeat(width.saturating_sub(cell.width));
    match align {
        Align::Left => format!("{}{padding}", cell.styled),
        Align::Right => format!("{padding}{}", cell.styled),
    }
}

/// Writes the analyzed files as a table that fits in `options.width`
/// columns: the key in its notation and colour, how sure it is, how the key
/// in the file's tags compares when the file has one, the tempo and how
/// sure it is, the energy, the sound quality in a word, the length
/// and the file. Files that failed are left out; they are reported on
/// their own.
pub fn write_table(
    writer: &mut impl Write,
    theme: &Theme,
    results: &[FileResult],
    options: &ViewOptions,
    tag_keys: &TagKeys,
) -> io::Result<()> {
    let analyzed: Vec<(&Path, &TrackAnalysis)> = results
        .iter()
        .filter_map(|result| Some((result.path.as_path(), result.outcome.as_ref().ok()?)))
        .collect();
    if analyzed.is_empty() {
        return Ok(());
    }
    let paths: Vec<&Path> = analyzed.iter().map(|(path, _)| *path).collect();
    let common = common_folder(&paths);
    let rows: Vec<Row> = analyzed
        .iter()
        .map(|(path, analysis)| {
            let (folder, name) = folder_and_name(path, &common);
            let tag_key = tag_key_cell(theme, tag_keys, path, analysis, options);
            let review = review_cell(
                theme,
                &tag_keys.review_reasons(path, analysis),
                tag_keys.reviewed(path),
            );
            let notes = Notes { tag_key, review };
            row(theme, analysis, options, notes, folder, name)
        })
        .collect();

    let column_width = |column: Column, rows: &[Row]| {
        rows.iter()
            .filter_map(|row| row.cells.iter().find(|(kept, _)| *kept == column))
            .map(|(_, cell)| cell.width)
            .chain(std::iter::once(column.heading().len()))
            .max()
            .unwrap_or(0)
    };
    let longest_file = rows
        .iter()
        .map(|row| width_of(&row.folder) + width_of(&row.name))
        .max()
        .unwrap_or(0)
        .max("File".len());

    let mut shown: Vec<Column> = Column::ALL
        .iter()
        .copied()
        .filter(|column| column_width(*column, &rows) > 0)
        .filter(|column| {
            !matches!(column, Column::TagKey | Column::Review)
                || rows.iter().any(|row| {
                    row.cells
                        .iter()
                        .any(|(kept, cell)| kept == column && cell.width > 0)
                })
        })
        .collect();
    let mut dropped_steps = 0;
    let fixed_width = |shown: &[Column]| -> usize {
        shown
            .iter()
            .map(|column| column_width(*column, &rows) + GAP.len())
            .sum::<usize>()
    };
    while dropped_steps < Column::DROP_ORDER.len()
        && fixed_width(&shown) + longest_file.min(SHORTEST_FILE_COLUMN) > options.width
    {
        let step = Column::DROP_ORDER[dropped_steps];
        shown.retain(|column| !step.contains(column));
        dropped_steps += 1;
    }
    let widths: Vec<(Column, usize)> = shown
        .iter()
        .map(|column| (*column, column_width(*column, &rows)))
        .collect();
    let file_width = options
        .width
        .saturating_sub(widths.iter().map(|(_, width)| width + GAP.len()).sum())
        .clamp(1, longest_file);

    let mut heading = String::new();
    for (column, width) in &widths {
        heading.push_str(&pad(
            &Cell::new(column.heading().to_string()),
            *width,
            column.align(),
        ));
        heading.push_str(GAP);
    }
    heading.push_str("File");
    writeln!(writer, "{}", theme.dim(heading))?;

    for row in &rows {
        let mut line = String::new();
        for (column, width) in &widths {
            let cell = row
                .cells
                .iter()
                .find(|(kept, _)| kept == column)
                .map(|(_, cell)| cell.clone())
                .unwrap_or_else(Cell::empty);
            line.push_str(&pad(&cell, *width, column.align()));
            line.push_str(GAP);
        }
        line.push_str(&file_cell(theme, &row.folder, &row.name, file_width));
        writeln!(writer, "{}", line.trim_end())?;
    }
    Ok(())
}

/// Labels of the detailed view are padded to this width.
const LABEL_WIDTH: usize = 15;

/// Writes `text` wrapped to the width of the view, with every line after
/// the first indented under the values.
fn write_wrapped(
    writer: &mut impl Write,
    theme: &Theme,
    label: &str,
    text: &str,
    width: usize,
) -> io::Result<()> {
    let indent = 2 + LABEL_WIDTH;
    let room = width.saturating_sub(indent).max(30);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && width_of(&line) + 1 + width_of(word) > room {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    for (index, text) in lines.iter().enumerate() {
        let shown_label = if index == 0 { label } else { "" };
        write_line(writer, theme, shown_label, text)?;
    }
    Ok(())
}

fn write_line(writer: &mut impl Write, theme: &Theme, label: &str, value: &str) -> io::Result<()> {
    let padded = format!("{label:<LABEL_WIDTH$}");
    writeln!(writer, "  {}{value}", theme.dim(padded))
}

fn confidence_words(theme: &Theme, confidence: Confidence) -> String {
    let words = match confidence {
        Confidence::High => "high confidence".to_string(),
        Confidence::Medium => "medium confidence".to_string(),
        Confidence::Low => theme.caution("low confidence, worth checking by ear"),
    };
    format!("{} {words}", confidence_meter(theme, Some(confidence)))
}

/// The audio format in a few words, such as "mp3, 320 kbit/s, 44.1 kHz,
/// stereo".
fn format_words(quality: &QualityReport) -> String {
    let mut parts = vec![quality.codec.clone()];
    if let Some(bitrate) = quality.bitrate_kbps {
        parts.push(format!("{bitrate} kbit/s"));
    }
    if let Some(bit_depth) = quality.bit_depth {
        parts.push(format!("{bit_depth} bit"));
    }
    parts.push(format!("{:.1} kHz", quality.sample_rate as f32 / 1000.0));
    match quality.channels {
        Some(1) => parts.push("mono".to_string()),
        Some(2) => parts.push("stereo".to_string()),
        Some(count) => parts.push(format!("{count} channels")),
        None => {}
    }
    parts.push(match quality.cutoff_hz {
        Some(cutoff) => format!("sound stops at {:.1} kHz", cutoff / 1000.0),
        None => "sound reaches the top of the spectrum".to_string(),
    });
    parts.join(", ")
}

fn signed(value: f32) -> String {
    if value >= 0.0 {
        format!("+{value:.1}")
    } else {
        format!("{value:.1}")
    }
}

/// Writes what the key field of the tags holds and how it compares with
/// the key shown above it.
fn write_tag_key(
    writer: &mut impl Write,
    theme: &Theme,
    tag_key: &TagKey,
    detected: Option<Key>,
    options: &ViewOptions,
    key_set: bool,
) -> io::Result<()> {
    let Some(tagged) = tag_key.key else {
        let text = format!("\"{}\", not a key Euphoria reads", Printable(&tag_key.text));
        return write_line(writer, theme, "Key in tags", &theme.dim(text));
    };
    let spelled = format!(
        "{}  {}",
        theme.key(tagged, options.notation.short(tagged)),
        options.notation.companion(tagged)
    );
    match detected.map(|detected| tag_key.compare(detected)) {
        Some(TagKeyAgreement::Differs(difference)) => {
            write_line(writer, theme, "Key in tags", &spelled)?;
            let compared = if key_set {
                "from the key you set"
            } else {
                "from the detected key"
            };
            write_line(
                writer,
                theme,
                "",
                &theme.caution(format!("{}, differs {compared}", difference.label())),
            )?;
            if !key_set {
                write_wrapped(writer, theme, "", difference.advice(), options.width)?;
            }
            Ok(())
        }
        Some(TagKeyAgreement::Same) => write_line(
            writer,
            theme,
            "Key in tags",
            &format!("{spelled}  {}", theme.dim("the same key")),
        ),
        Some(TagKeyAgreement::Unreadable) | None => {
            write_line(writer, theme, "Key in tags", &spelled)
        }
    }
}

const NOTE_LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Columns per note in the row of notes: room for a two letter name and a
/// space.
const NOTE_COLUMN_WIDTH: usize = 3;

/// The bar of one note: eight heights, the lowest for a note not heard.
fn note_bar(level: f32) -> char {
    let steps = (NOTE_LEVELS.len() - 1) as f32;
    let index = (level.clamp(0.0, 1.0) * steps).round() as usize;
    NOTE_LEVELS[index]
}

/// Writes the 12 notes the key analysis heard as a row of bars over their
/// names, C first, with a dot under each note of the key.
fn write_notes(
    writer: &mut impl Write,
    theme: &Theme,
    analysis: &TrackAnalysis,
    options: &ViewOptions,
) -> io::Result<()> {
    let Some(profile) = analysis.pitch_class_profile else {
        let reason = if analysis.key.is_some() {
            "not measured for this track yet, analyze it again with --reanalyze"
        } else {
            "none, no key was found"
        };
        return write_line(writer, theme, "Notes", &theme.dim(reason));
    };
    let key = analysis.key;
    let in_key = key.map(notes_in_key).unwrap_or([false; 12]);
    let names = note_names(key);
    let mut bars = Vec::new();
    let mut labels = Vec::new();
    let mut marks = Vec::new();
    for (pitch_class, level) in profile.iter().enumerate() {
        let bar = note_bar(*level).to_string();
        let name = names[pitch_class];
        let padding = " ".repeat(NOTE_COLUMN_WIDTH - name.len());
        match (in_key[pitch_class], key) {
            (true, Some(key)) => {
                bars.push(theme.key(key, bar));
                labels.push(format!("{}{padding}", theme.bold(name)));
                marks.push(theme.key(key, "•"));
            }
            _ => {
                bars.push(theme.dim(bar));
                labels.push(format!("{}{padding}", theme.dim(name)));
                marks.push(" ".to_string());
            }
        }
    }
    let gap = " ".repeat(NOTE_COLUMN_WIDTH - 1);
    write_line(writer, theme, "Notes", &bars.join(&gap))?;
    write_line(writer, theme, "", labels.concat().trim_end())?;
    if let Some(key) = key {
        let caption = theme.dim(format!("• in the key, {}", options.notation.short(key)));
        let marked = marks.join(&gap);
        write_line(
            writer,
            theme,
            "",
            &format!("{}   {caption}", marked.trim_end()),
        )?;
    }
    Ok(())
}

/// What was read about a file besides its analysis, for the detailed view.
#[derive(Debug, Clone, Default)]
pub struct FileFacts<'facts> {
    /// The key field of the file's tags.
    pub tag_key: Option<&'facts TagKey>,
    /// Where the beat grid came from, when a collection was given.
    pub grid_info: Option<&'facts GridInfo>,
    /// Why the track is worth checking by ear.
    pub review: Vec<ReviewReason>,
    /// True when the user set or confirmed the key or the tempo.
    pub reviewed: bool,
    /// Where the track tends to sit in a set, among the tracks analyzed
    /// together.
    pub stage: Option<euphoria_core::SetStage>,
    /// The track's cues: the user's when set, and otherwise those analysis
    /// placed.
    pub cues: Option<TrackCues>,
}

/// Writes everything known about one track, a label per line: the key with
/// its runner up and confidence, the tempo with its alternative and
/// confidence, the energy, the sound quality with its reason and format,
/// the loudness, the tuning, the key changes and the length.
pub fn write_detail(
    writer: &mut impl Write,
    theme: &Theme,
    path: &Path,
    analysis: &TrackAnalysis,
    options: &ViewOptions,
    correction: Option<&Correction>,
    facts: FileFacts<'_>,
) -> io::Result<()> {
    let FileFacts {
        tag_key,
        grid_info,
        review,
        reviewed,
        stage,
        cues,
    } = facts;
    let key_set = correction.is_some_and(|correction| correction.key.is_some());
    let bpm_set = correction.is_some_and(|correction| correction.bpm.is_some());
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    writeln!(writer, "{}", theme.bold(Printable(&name)))?;
    if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        writeln!(writer, "{}", theme.dim(Printable(folder.display())))?;
    }
    writeln!(writer)?;

    let width = options.width;
    match analysis.key {
        Some(key) => {
            let short = options.notation.short(key);
            write_line(
                writer,
                theme,
                "Key",
                &format!(
                    "{}  {}",
                    theme.key(key, &short),
                    options.notation.companion(key)
                ),
            )?;
            if key_set {
                write_line(writer, theme, "", &theme.user("set by you"))?;
            }
            if let Some(runner_up) = analysis.key_runner_up {
                write_line(
                    writer,
                    theme,
                    "",
                    &theme.dim(format!(
                        "runner up {}  {}",
                        options.notation.short(runner_up),
                        options.notation.companion(runner_up)
                    )),
                )?;
            }
        }
        None => write_line(writer, theme, "Key", &theme.dim("none found"))?,
    }
    if options.notes {
        write_notes(writer, theme, analysis, options)?;
    }
    if let Some(tag_key) = tag_key {
        write_tag_key(writer, theme, tag_key, analysis.key, options, key_set)?;
    }

    match analysis.bpm {
        Some(bpm) => {
            write_line(
                writer,
                theme,
                "Tempo",
                &format!("{} BPM", theme.bold(format!("{bpm:.1}"))),
            )?;
            if bpm_set {
                write_line(writer, theme, "", &theme.user("set by you"))?;
            }
            if let Some(alternative) = options.tempo_range.alternative(bpm) {
                let counted = if alternative < bpm { "half" } else { "double" };
                write_line(
                    writer,
                    theme,
                    "",
                    &theme.dim(format!("or {alternative:.1} BPM counted at {counted} time")),
                )?;
            }
        }
        None => write_line(writer, theme, "Tempo", &theme.dim("none found"))?,
    }

    if !review.is_empty() {
        let words: Vec<String> = review
            .iter()
            .map(|reason| review_sentence(reason, analysis, options))
            .collect();
        write_wrapped(
            writer,
            theme,
            "Review",
            &theme.caution(words.join(", ")),
            width,
        )?;
    } else if reviewed {
        write_line(
            writer,
            theme,
            "Review",
            &theme.user("checked by you, no longer in doubt"),
        )?;
    }

    if let Some(grid) = analysis.beat_grid {
        write_line(
            writer,
            theme,
            "Beat grid",
            &format!(
                "first downbeat at {:.3} s, {:.2} BPM",
                grid.first_downbeat_seconds, grid.bpm
            ),
        )?;
        match grid_info.and_then(|info| info.source) {
            Some(GridSource::Rekordbox) => write_line(
                writer,
                theme,
                "",
                &theme.dim("from your rekordbox collection"),
            )?,
            Some(GridSource::User) => write_line(writer, theme, "", &theme.user("set by you"))?,
            _ => {
                write_line(
                    writer,
                    theme,
                    "",
                    &confidence_words(theme, grid.confidence()),
                )?;
                if grid_info.is_some() {
                    write_line(writer, theme, "", &theme.dim("detected by Euphoria"))?;
                }
            }
        }
    } else if let Some(GridSource::User) = grid_info.and_then(|info| info.source) {
        write_line(
            writer,
            theme,
            "Beat grid",
            &theme.dim("none, you set the tempo, so no grid was fitted to a different one"),
        )?;
    }
    if let Some(reason) = grid_info.and_then(|info| info.collection.skipped_because()) {
        write_wrapped(
            writer,
            theme,
            "",
            &theme.dim(format!("rekordbox grid not used: {reason}")),
            width,
        )?;
    }

    match analysis.energy {
        Some(energy) => write_line(
            writer,
            theme,
            "Energy",
            &format!(
                "{} of 10  {}",
                theme.bold(energy),
                energy_bar(theme, energy)
            ),
        )?,
        None => write_line(writer, theme, "Energy", &theme.dim("nothing to rate"))?,
    }
    if let Some(stage) = stage {
        write_line(
            writer,
            theme,
            "Stage",
            &format!(
                "{}  {}",
                theme.bold(stage.label()),
                theme.dim("from tempo and loudness among these tracks; not the energy")
            ),
        )?;
    }

    if let Some(cues) = cues.filter(|cues| !cues.cues.is_empty()) {
        write_cues(writer, theme, &cues)?;
    }

    if let Some(quality) = &analysis.quality {
        write_line(
            writer,
            theme,
            "Sound quality",
            &quality_coloured(theme, quality.verdict, quality.verdict.label()),
        )?;
        write_wrapped(writer, theme, "", &quality.reason(), width)?;
        write_wrapped(writer, theme, "", &theme.dim(format_words(quality)), width)?;
        for note in quality.notes() {
            write_wrapped(writer, theme, "", &note, width)?;
        }
        if let Some(loudness) = quality.integrated_loudness_lufs {
            let mut text = format!("{loudness:.1} LUFS");
            if let Some(peak) = quality.true_peak_dbtp {
                text.push_str(&format!(", true peak {} dBTP", signed(peak)));
            }
            write_line(writer, theme, "Loudness", &text)?;
        }
    }

    if let Some(cents) = analysis.tuning_cents {
        let text = if cents.abs() < 5.0 {
            "at concert pitch, A = 440 Hz".to_string()
        } else {
            format!("{} cents from A = 440 Hz", signed(cents))
        };
        write_line(writer, theme, "Tuning", &text)?;
    }

    if analysis.key_timeline.len() > 1 {
        let changes: Vec<String> = analysis
            .key_timeline
            .iter()
            .map(|section| {
                format!(
                    "{} {}",
                    theme.dim(format_duration(section.start_seconds)),
                    theme.key(section.key, options.notation.short(section.key))
                )
            })
            .collect();
        write_wrapped(writer, theme, "Key changes", &changes.join(", "), width)?;
    }

    write_line(
        writer,
        theme,
        "Length",
        &format_duration(analysis.duration_seconds),
    )
}

#[cfg(test)]
mod tests {
    use euphoria_core::quality::QualityReport;
    use euphoria_core::{EuphoriaError, KeySection, Mode};

    use super::*;

    fn quality(verdict: QualityVerdict) -> QualityReport {
        QualityReport {
            codec: "mp3".to_string(),
            lossless: false,
            bitrate_kbps: Some(96),
            sample_rate: 44_100,
            bit_depth: None,
            channels: Some(2),
            cutoff_hz: Some(15_374.7),
            clipped_share: 0.0,
            integrated_loudness_lufs: Some(-12.36),
            true_peak_dbtp: Some(1.04),
            damage: Default::default(),
            cancelled_share: 0.0,
            verdict,
        }
    }

    fn analysis(key_margin: f32, tempo_margin: f32) -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 408.4,
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(124.9),
            energy: Some(7),
            tuning_cents: Some(-31.26),
            key_runner_up: Some(Key::new(0, Mode::Major)),
            key_score_margin: Some(key_margin),
            key_source: None,
            loudness_range_lu: None,
            pitch_class_profile: None,
            tempo_estimate: Some(124.9),
            tempo_score_margin: Some(tempo_margin),
            beat_grid: None,
            cues: Vec::new(),
            key_timeline: vec![
                KeySection {
                    start_seconds: 0.0,
                    end_seconds: 250.0,
                    key: Key::new(9, Mode::Minor),
                },
                KeySection {
                    start_seconds: 250.0,
                    end_seconds: 408.4,
                    key: Key::new(4, Mode::Minor),
                },
            ],
            quality: Some(quality(QualityVerdict::WeakOnABigSystem)),
        }
    }

    fn results() -> Vec<FileResult> {
        vec![
            FileResult {
                path: PathBuf::from("music/house/low tide.flac"),
                outcome: Ok(analysis(0.02, 0.5)),
            },
            FileResult {
                path: PathBuf::from(
                    "music/techno/a rather long name of a track (extended mix).mp3",
                ),
                outcome: Ok(analysis(0.001, 0.2)),
            },
            FileResult {
                path: PathBuf::from("music/broken.mp3"),
                outcome: Err(EuphoriaError::UnknownKeyText("example".to_string())),
            },
        ]
    }

    fn options(width: usize) -> ViewOptions {
        ViewOptions {
            notation: KeyNotationChoice::Camelot,
            tempo_range: TempoRange::DEFAULT,
            notes: false,
            width,
        }
    }

    fn table(width: usize) -> String {
        let mut buffer = Vec::new();
        write_table(
            &mut buffer,
            &Theme::PLAIN,
            &results(),
            &options(width),
            &TagKeys::default(),
        )
        .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn a_wide_table_shows_every_column_without_the_common_folder() {
        let expected = "\
Key             BPM  Energy              Quality  Time  Review  File
 8A  A minor  124.9       7  ━━━━━━━───  weak     6:48          house/low tide.flac
 8A  A minor  124.9       7  ━━━━━━━───  weak     6:48  key     techno/a rather long name of a track (extended mix).mp3
";
        assert_eq!(table(200), expected);
    }

    #[test]
    fn the_tempo_reason_names_the_half_or_double_to_try() {
        let options = options(200);
        let mut doubtful = analysis(0.02, 0.01);
        doubtful.bpm = Some(174.0);
        assert_eq!(
            review_sentence(&ReviewReason::TempoUnsure, &doubtful, &options),
            "BPM may be 87.0, counted at half time"
        );
        doubtful.bpm = Some(95.0);
        assert_eq!(
            review_sentence(&ReviewReason::TempoUnsure, &doubtful, &options),
            "BPM may be 190.0, counted at double time"
        );
    }

    #[test]
    fn the_review_column_gives_each_reason_a_word_and_marks_checked_tracks() {
        let theme = Theme::PLAIN;
        assert_eq!(review_cell(&theme, &[], false), "");
        assert_eq!(review_cell(&theme, &[], true), "checked");
        let key = Key::new(0, Mode::Major);
        let reasons = [
            ReviewReason::KeyUnsure,
            ReviewReason::TempoUnsure,
            ReviewReason::KeyConflict { tag_key: key },
        ];
        assert_eq!(review_cell(&theme, &reasons, true), "key+tempo+tags");
        assert_eq!(
            review_cell(&theme, &[ReviewReason::Failed("x".into())], false),
            "failed"
        );
    }

    #[test]
    fn a_narrow_table_drops_the_decoration_first() {
        let narrow = table(60);
        let heading = narrow.lines().next().unwrap();
        assert!(!heading.contains("Time"), "{narrow}");
        assert!(!narrow.contains('━'), "{narrow}");
        for line in narrow.lines() {
            assert!(width_of(line) <= 60, "{line:?} is wider than 60");
        }
        assert!(narrow.contains('…'), "{narrow}");
        assert!(narrow.contains("…tended mix).mp3"), "{narrow}");
    }

    #[test]
    fn the_narrowest_table_keeps_the_key_the_tempo_and_the_file() {
        let narrowest = table(30);
        let heading = narrowest.lines().next().unwrap();
        assert!(heading.starts_with("Key"), "{narrowest}");
        assert!(heading.contains("BPM"), "{narrowest}");
        assert!(heading.ends_with("File"), "{narrowest}");
        assert!(!heading.contains("Energy"), "{narrowest}");
    }

    #[test]
    fn the_table_uses_the_chosen_notation() {
        let mut buffer = Vec::new();
        let mut musical = options(200);
        musical.notation = KeyNotationChoice::Musical;
        write_table(
            &mut buffer,
            &Theme::PLAIN,
            &results(),
            &musical,
            &TagKeys::default(),
        )
        .unwrap();
        let shown = String::from_utf8(buffer).unwrap();
        assert!(shown.contains("Am  8A"), "{shown}");
    }

    #[test]
    fn colour_never_changes_the_layout() {
        let mut buffer = Vec::new();
        write_table(
            &mut buffer,
            &Theme::COLOURED,
            &results(),
            &options(200),
            &TagKeys::default(),
        )
        .unwrap();
        let coloured = String::from_utf8(buffer).unwrap();
        assert!(coloured.contains('\u{1b}'));
        let stripped: Vec<String> = coloured
            .lines()
            .map(|line| console::strip_ansi_codes(line).into_owned())
            .collect();
        assert_eq!(stripped.join("\n") + "\n", table(200));
    }

    #[test]
    fn long_lines_break_between_words_and_short_ones_stay() {
        let text = "hint: --quality weak lists the tracks\nshort\n\n  kept  as is\n";
        assert_eq!(
            wrap_lines(text, 20),
            "hint: --quality weak\n  lists the tracks\nshort\n\n  kept  as is\n"
        );
        let coloured = format!("{} one two three", Theme::COLOURED.bold("bold"));
        assert_eq!(
            console::strip_ansi_codes(&wrap_lines(&coloured, 13)),
            "bold one two\n  three\n"
        );
    }

    #[test]
    fn names_are_shortened_in_the_middle_keeping_the_extension() {
        assert_eq!(shorten_middle("short.mp3", 20), "short.mp3");
        let shortened = shorten_middle("a rather long name of a track.mp3", 15);
        assert_eq!(width_of(&shortened), 15);
        assert!(shortened.starts_with("a rath"), "{shortened}");
        assert!(shortened.ends_with("ack.mp3"), "{shortened}");
    }

    #[test]
    fn file_names_cannot_act_on_the_terminal() {
        let results = vec![FileResult {
            path: PathBuf::from("music/evil\r\u{1b}[2K.mp3"),
            outcome: Ok(analysis(0.02, 0.5)),
        }];
        let mut buffer = Vec::new();
        write_table(
            &mut buffer,
            &Theme::PLAIN,
            &results,
            &options(200),
            &TagKeys::default(),
        )
        .unwrap();
        let shown = String::from_utf8(buffer).unwrap();
        assert!(shown.contains("evil\\r\\u{1b}[2K.mp3"), "{shown:?}");
    }

    #[test]
    fn the_detail_lists_each_cue_with_the_energy_of_its_section() {
        let show = |cues: TrackCues| {
            let mut buffer = Vec::new();
            write_detail(
                &mut buffer,
                &Theme::PLAIN,
                Path::new("track.mp3"),
                &analysis(0.5, 0.5),
                &options(100),
                None,
                FileFacts {
                    cues: Some(cues),
                    ..FileFacts::default()
                },
            )
            .unwrap();
            String::from_utf8(buffer).unwrap()
        };
        let section = |seconds, section, energy| euphoria_core::SectionCue {
            seconds,
            section,
            energy,
        };
        let automatic = TrackCues::automatic(&[
            section(0.5, euphoria_core::CueSection::Start, Some(3)),
            section(62.0, euphoria_core::CueSection::Breakdown, Some(2)),
            section(93.0, euphoria_core::CueSection::Drop, Some(9)),
        ]);
        let shown = show(automatic);
        assert!(
            shown.contains(
                "  Cues           A   0:01  Start      energy 3\n                 B   1:02  Breakdown  energy 2\n                 C   1:33  Drop       energy 9\n"
            ),
            "{shown}"
        );

        let unmeasured =
            TrackCues::automatic(&[section(0.5, euphoria_core::CueSection::Start, None)]);
        assert!(
            show(unmeasured).contains("energy not measured yet, analyze again with --reanalyze")
        );

        let edited = TrackCues::edited(vec![euphoria_core::Cue {
            seconds: 12.0,
            name: "Vocal".to_string(),
            hot_cue: None,
            energy: None,
        }]);
        let shown = show(edited);
        assert!(
            shown.contains("  Cues           -   0:12  Vocal\n"),
            "{shown}"
        );
        assert!(shown.contains("set by you"), "{shown}");
        assert!(!show(TrackCues::edited(Vec::new())).contains("Cues"));
    }

    #[test]
    fn the_detail_shows_the_stage_apart_from_the_energy_and_only_when_there_is_one() {
        let show = |stage| {
            let mut buffer = Vec::new();
            write_detail(
                &mut buffer,
                &Theme::PLAIN,
                Path::new("music/low tide.flac"),
                &analysis(0.001, 0.5),
                &options(100),
                None,
                FileFacts {
                    stage,
                    ..FileFacts::default()
                },
            )
            .unwrap();
            String::from_utf8(buffer).unwrap()
        };
        let with_stage = show(Some(euphoria_core::SetStage::Peak));
        assert!(
            with_stage.contains("  Stage          Peak  from tempo and loudness among these tracks; not the energy\n"),
            "{with_stage}"
        );
        assert!(!show(None).contains("Stage"));
    }

    fn detail_with_notes(analysis: &TrackAnalysis, notes: bool) -> String {
        let mut buffer = Vec::new();
        write_detail(
            &mut buffer,
            &Theme::PLAIN,
            Path::new("music/low tide.flac"),
            analysis,
            &ViewOptions {
                notes,
                ..options(100)
            },
            None,
            FileFacts::default(),
        )
        .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn the_notes_are_bars_over_their_names_with_the_key_marked() {
        let mut track = analysis(0.001, 0.5);
        track.pitch_class_profile =
            Some([0.9, 0.0, 0.5, 0.1, 0.8, 0.4, 0.05, 0.6, 0.1, 1.0, 0.15, 0.3]);

        let shown = detail_with_notes(&track, true);

        let expected = "  Notes          ▇  ▁  ▅  ▂  ▇  ▄  ▁  ▅  ▂  █  ▂  ▃\n\
                        \x20                C  C# D  Eb E  F  F# G  Ab A  Bb B\n\
                        \x20                •     •     •  •     •     •     •   • in the key, 8A\n";
        assert!(shown.contains(expected), "{shown}");
        assert!(!detail_with_notes(&track, false).contains("Notes"));
    }

    #[test]
    fn notes_not_yet_measured_ask_for_a_new_analysis() {
        let shown = detail_with_notes(&analysis(0.001, 0.5), true);
        assert!(
            shown.contains(
                "  Notes          not measured for this track yet, analyze it again with --reanalyze\n"
            ),
            "{shown}"
        );
    }

    #[test]
    fn the_detail_names_everything_known_about_the_track() {
        let mut buffer = Vec::new();
        let track = analysis(0.001, 0.5);
        write_detail(
            &mut buffer,
            &Theme::PLAIN,
            Path::new("music/low tide.flac"),
            &track,
            &options(100),
            None,
            FileFacts::default(),
        )
        .unwrap();
        let detail = String::from_utf8(buffer).unwrap();
        let expected = "\
low tide.flac
music

  Key            8A  A minor
                 runner up 8B  C major
  Tempo          124.9 BPM
  Energy         7 of 10  ━━━━━━━───
  Sound quality  Weak on a big system
                 96 kbit/s. Fine for practice and small rooms; on a club system the top end and
                 detail suffer.
                 mp3, 96 kbit/s, 44.1 kHz, stereo, sound stops at 15.4 kHz
  Loudness       -12.4 LUFS, true peak +1.0 dBTP
  Tuning         -31.3 cents from A = 440 Hz
  Key changes    0:00 8A, 4:10 9A
  Length         6:48
";
        assert_eq!(detail, expected);
    }

    #[test]
    fn values_the_user_set_are_marked_and_the_beat_grid_is_shown() {
        let path = PathBuf::from("music/low tide.flac");
        let correction = Correction::of_bpm(126.0).unwrap();
        let track = correction.apply(TrackAnalysis {
            beat_grid: Some(euphoria_core::BeatGrid {
                first_downbeat_seconds: 0.4126,
                bpm: 124.9,
                salience: 100.0,
                drift_seconds: None,
            }),
            ..analysis(0.02, 0.5)
        });
        let mut buffer = Vec::new();
        write_detail(
            &mut buffer,
            &Theme::PLAIN,
            &path,
            &track,
            &options(100),
            Some(&correction),
            FileFacts::default(),
        )
        .unwrap();
        let detail = String::from_utf8(buffer).unwrap();
        assert!(
            detail.contains("  Tempo          126.0 BPM\n                 set by you\n"),
            "{detail}"
        );
        assert!(
            !detail.contains("Beat grid"),
            "a set tempo drops the grid: {detail}"
        );

        let mut buffer = Vec::new();
        let with_grid = TrackAnalysis {
            beat_grid: Some(euphoria_core::BeatGrid {
                first_downbeat_seconds: 0.4126,
                bpm: 124.9,
                salience: 100.0,
                drift_seconds: None,
            }),
            ..analysis(0.02, 0.5)
        };
        write_detail(
            &mut buffer,
            &Theme::PLAIN,
            &path,
            &with_grid,
            &options(100),
            None,
            FileFacts::default(),
        )
        .unwrap();
        let detail = String::from_utf8(buffer).unwrap();
        assert!(
            detail.contains("  Beat grid      first downbeat at 0.413 s, 124.90 BPM\n"),
            "{detail}"
        );
    }

    #[test]
    fn the_detail_offers_the_half_or_double_tempo() {
        let mut buffer = Vec::new();
        let mut track = analysis(0.02, 0.5);
        track.bpm = Some(170.0);
        track.tempo_estimate = Some(170.0);
        write_detail(
            &mut buffer,
            &Theme::PLAIN,
            Path::new("dnb.flac"),
            &track,
            &options(100),
            None,
            FileFacts::default(),
        )
        .unwrap();
        let detail = String::from_utf8(buffer).unwrap();
        assert!(
            detail.contains("or 85.0 BPM counted at half time"),
            "{detail}"
        );
        assert!(detail.starts_with("dnb.flac\n\n"), "{detail}");
    }
    fn tag_key_results() -> Vec<FileResult> {
        ["agrees.mp3", "fifth.mp3", "garbage.mp3", "untagged.mp3"]
            .iter()
            .map(|name| FileResult {
                path: PathBuf::from(format!("music/{name}")),
                outcome: Ok(analysis(0.5, 0.5)),
            })
            .collect()
    }

    #[test]
    fn the_tags_column_marks_agreement_conflicts_and_unreadable_fields() {
        let results = tag_key_results();
        let tag_keys = TagKeys::from_texts(&[
            (Path::new("music/agrees.mp3"), "Am"),
            (Path::new("music/fifth.mp3"), "9A 124"),
            (Path::new("music/garbage.mp3"), "banger"),
        ]);
        let mut buffer = Vec::new();
        write_table(
            &mut buffer,
            &Theme::PLAIN,
            &results,
            &options(140),
            &tag_keys,
        )
        .unwrap();
        let shown = String::from_utf8(buffer).unwrap();
        let line_of = |name: &str| {
            shown
                .lines()
                .find(|line| line.ends_with(name))
                .unwrap_or_else(|| panic!("{name} in {shown}"))
                .to_string()
        };
        assert!(shown.lines().next().unwrap().contains("Tags"), "{shown}");
        assert!(line_of("agrees.mp3").contains(" = "), "{shown}");
        assert!(line_of("fifth.mp3").contains("9A fifth"), "{shown}");
        assert!(line_of("garbage.mp3").contains(" ? "), "{shown}");
        assert!(!line_of("untagged.mp3").contains('?'), "{shown}");
    }

    #[test]
    fn the_tags_column_is_left_out_when_no_file_has_a_key_field() {
        let mut buffer = Vec::new();
        write_table(
            &mut buffer,
            &Theme::PLAIN,
            &tag_key_results(),
            &options(140),
            &TagKeys::default(),
        )
        .unwrap();
        let shown = String::from_utf8(buffer).unwrap();
        assert!(!shown.contains("Tags"), "{shown}");
    }

    fn detail_with_tag_key(text: &str, correction: Option<&Correction>) -> String {
        let mut buffer = Vec::new();
        let tag_key = TagKey::from_text(text).unwrap();
        write_detail(
            &mut buffer,
            &Theme::PLAIN,
            Path::new("music/low tide.flac"),
            &analysis(0.5, 0.5),
            &options(100),
            correction,
            FileFacts {
                tag_key: Some(&tag_key),
                ..FileFacts::default()
            },
        )
        .unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn the_detail_names_the_difference_and_which_reading_to_trust() {
        let detail = detail_with_tag_key("8B", None);
        assert!(detail.contains("Key in tags    8B  C major"), "{detail}");
        assert!(
            detail.contains("relative key, differs from the detected key"),
            "{detail}"
        );
        assert!(
            detail.contains("Listen for whether the track sounds minor"),
            "{detail}"
        );
    }

    #[test]
    fn the_detail_says_when_the_tags_agree_or_cannot_be_read() {
        let agrees = detail_with_tag_key("1m", None);
        assert!(
            agrees.contains("Key in tags    8A  A minor  the same key"),
            "{agrees}"
        );
        let garbage = detail_with_tag_key("banger", None);
        assert!(
            garbage.contains("Key in tags    \"banger\", not a key Euphoria reads"),
            "{garbage}"
        );
    }

    #[test]
    fn a_key_the_user_set_is_compared_without_advice() {
        let correction = Correction {
            key: Some(Key::new(9, Mode::Minor)),
            ..Correction::default()
        };
        let detail = detail_with_tag_key("8B", Some(&correction));
        assert!(detail.contains("differs from the key you set"), "{detail}");
        assert!(!detail.contains("Listen for"), "{detail}");
    }
}
