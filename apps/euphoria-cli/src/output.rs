// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Formats analysis results as JSON and as the plain table read by scripts,
//! writes the summaries after a run, and writes every error, warning and
//! hint in one shape: a coloured label, the file, the reason, and what to
//! do next.

use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use euphoria_core::quality::{QualityReport, QualityVerdict};
use euphoria_core::tags::{
    GridOutcome, SeratoReport, SeratoSkip, WritePlan, describe_serato_value, serato_field_name,
};
use euphoria_core::{
    BeatGrid, Confidence, Cue, EuphoriaError, FileResult, Key, KeySection, SUPPORTED_EXTENSIONS,
    SkippedTrack, TempoRange, UnreadTags,
};
use serde::Serialize;

use crate::batch::{UserEdits, UserEditsByFile};
use crate::export::Program;
use crate::style::Theme;
use crate::tags::Failure;
use crate::terminal::Printable;
use crate::view::format_duration;

/// Writes a labeled message, wrapping only for the stream's terminal.
fn write_message(
    writer: &mut impl Write,
    theme: &Theme,
    label: &str,
    message: impl Display,
) -> io::Result<()> {
    let width = theme
        .is_terminal()
        .then(crate::view::standard_error_width)
        .flatten();
    write_message_at_width(writer, label, message, width)
}

fn write_message_at_width(
    writer: &mut impl Write,
    label: &str,
    message: impl Display,
    width: Option<usize>,
) -> io::Result<()> {
    let line = format!("{label} {message}\n");
    match width {
        Some(width) => writer.write_all(crate::view::wrap_lines(&line, width).as_bytes()),
        None => writer.write_all(line.as_bytes()),
    }
}

/// Writes `error: message`.
pub fn write_error(
    writer: &mut impl Write,
    theme: &Theme,
    message: impl Display,
) -> io::Result<()> {
    write_message(writer, theme, &theme.error_label(), message)
}

/// Writes `warning: message`.
pub fn write_warning(
    writer: &mut impl Write,
    theme: &Theme,
    message: impl Display,
) -> io::Result<()> {
    write_message(writer, theme, &theme.warning_label(), message)
}

/// Writes `hint: message`, what to do next.
pub fn write_hint(writer: &mut impl Write, theme: &Theme, message: impl Display) -> io::Result<()> {
    write_message(writer, theme, &theme.hint_label(), message)
}

/// Writes `note: message`, something worth knowing that needs no action.
pub fn write_note(writer: &mut impl Write, theme: &Theme, message: impl Display) -> io::Result<()> {
    write_message(writer, theme, &theme.note_label(), message)
}

/// Writes `error: file: reason` for a file something went wrong with.
fn write_file_error(
    writer: &mut impl Write,
    theme: &Theme,
    path: &Path,
    reason: &EuphoriaError,
) -> io::Result<()> {
    write_error(
        writer,
        theme,
        format_args!(
            "{}: {}",
            Printable(path.display()),
            Printable(reason.chain())
        ),
    )
}

/// Writes that no audio file was found and which files are looked for.
pub fn write_no_audio_files(writer: &mut impl Write, theme: &Theme) -> io::Result<()> {
    write_warning(writer, theme, "no audio files found.")?;
    write_hint(
        writer,
        theme,
        format_args!(
            "Euphoria reads files ending in .{}. Check the path, or give a folder that holds them.",
            SUPPORTED_EXTENSIONS.join(", .")
        ),
    )
}

#[derive(Serialize)]
struct JsonTrack<'result> {
    path: &'result Path,
    duration_seconds: Option<f32>,
    key: Option<JsonKey>,
    runner_up_key: Option<JsonKey>,
    bpm: Option<f32>,
    bpm_confidence: Option<&'static str>,
    bpm_alternative: Option<f32>,
    energy: Option<u8>,
    tuning_cents: Option<f32>,
    key_timeline: Vec<JsonKeySection>,
    beat_grid: Option<JsonBeatGrid>,
    cues: Vec<JsonCue>,
    quality: Option<JsonQuality<'result>>,
    needs_review: Option<bool>,
    /// The values the user set by hand, which replace the detected ones:
    /// any of "key", "bpm" and "cues".
    set_by_user: Vec<&'static str>,
    error: Option<String>,
}

#[derive(Serialize)]
struct JsonCue {
    seconds: f64,
    name: String,
    /// The hot cue pad, "A" to "H", or `None` for a memory cue only.
    hot_cue: Option<String>,
}

impl From<Cue> for JsonCue {
    fn from(cue: Cue) -> Self {
        Self {
            seconds: round_to_thousandths(cue.seconds),
            name: cue.name,
            hot_cue: cue.hot_cue.map(|hot_cue| hot_cue.to_string()),
        }
    }
}

#[derive(Serialize)]
struct JsonBeatGrid {
    first_downbeat_seconds: f64,
    bpm: f64,
    confidence: &'static str,
}

impl From<BeatGrid> for JsonBeatGrid {
    fn from(grid: BeatGrid) -> Self {
        Self {
            first_downbeat_seconds: round_to_thousandths(grid.first_downbeat_seconds),
            bpm: round_to_thousandths(grid.bpm),
            confidence: grid.confidence().as_str(),
        }
    }
}

#[derive(Serialize)]
struct JsonQuality<'result> {
    #[serde(flatten)]
    report: &'result QualityReport,
    reason: String,
}

impl<'result> From<&'result QualityReport> for JsonQuality<'result> {
    fn from(report: &'result QualityReport) -> Self {
        Self {
            report,
            reason: report.reason(),
        }
    }
}

#[derive(Serialize)]
struct JsonKeySection {
    start_seconds: f32,
    end_seconds: f32,
    key: JsonKey,
}

impl From<&KeySection> for JsonKeySection {
    fn from(section: &KeySection) -> Self {
        Self {
            start_seconds: round_to_tenths(section.start_seconds),
            end_seconds: round_to_tenths(section.end_seconds),
            key: section.key.into(),
        }
    }
}

#[derive(Serialize)]
struct JsonKey {
    camelot: String,
    name: String,
    open_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<&'static str>,
}

impl JsonKey {
    fn rated(key: Key, confidence: Option<Confidence>) -> Self {
        Self {
            confidence: confidence.map(Confidence::as_str),
            ..Self::from(key)
        }
    }
}

impl From<Key> for JsonKey {
    fn from(key: Key) -> Self {
        Self {
            camelot: key.camelot(),
            name: key.to_string(),
            open_key: key.open_key(),
            confidence: None,
        }
    }
}

/// Writes one JSON array with an object per file, including failed files with
/// their error. Tempos are taken to be reported in `range`, which decides
/// the half or double tempo offered as an alternative.
pub fn write_json(
    writer: &mut impl Write,
    results: &[FileResult],
    range: TempoRange,
    user_edits: &UserEditsByFile,
) -> io::Result<()> {
    let no_edits = UserEdits::default();
    let tracks: Vec<JsonTrack<'_>> = results
        .iter()
        .map(|result| match &result.outcome {
            Ok(analysis) => {
                let edits = user_edits
                    .get(&result.path)
                    .and_then(|looked_up| looked_up.as_ref().ok())
                    .unwrap_or(&no_edits);
                JsonTrack {
                    path: &result.path,
                    duration_seconds: Some(round_to_tenths(analysis.duration_seconds)),
                    key: analysis
                        .key
                        .map(|key| JsonKey::rated(key, analysis.key_confidence())),
                    runner_up_key: analysis.key_runner_up.map(JsonKey::from),
                    bpm: analysis.bpm.map(round_to_tenths),
                    bpm_confidence: analysis.tempo_confidence().map(Confidence::as_str),
                    bpm_alternative: analysis
                        .bpm
                        .and_then(|bpm| range.alternative(bpm))
                        .map(round_to_tenths),
                    energy: analysis.energy,
                    tuning_cents: analysis.tuning_cents.map(round_to_tenths),
                    key_timeline: analysis
                        .key_timeline
                        .iter()
                        .map(JsonKeySection::from)
                        .collect(),
                    beat_grid: analysis.beat_grid.map(JsonBeatGrid::from),
                    cues: edits
                        .track_cues(analysis)
                        .cues
                        .into_iter()
                        .map(JsonCue::from)
                        .collect(),
                    quality: analysis.quality.as_ref().map(JsonQuality::from),
                    needs_review: Some(analysis.needs_review()),
                    set_by_user: set_by_user(edits),
                    error: None,
                }
            }
            Err(error) => JsonTrack {
                path: &result.path,
                duration_seconds: None,
                key: None,
                runner_up_key: None,
                bpm: None,
                bpm_confidence: None,
                bpm_alternative: None,
                energy: None,
                tuning_cents: None,
                key_timeline: Vec::new(),
                beat_grid: None,
                cues: Vec::new(),
                quality: None,
                needs_review: None,
                set_by_user: Vec::new(),
                error: Some(error.chain()),
            },
        })
        .collect();
    serde_json::to_writer_pretty(&mut *writer, &tracks)?;
    writeln!(writer)
}

/// The values the user set in `edits`, as JSON names them.
fn set_by_user(edits: &UserEdits) -> Vec<&'static str> {
    let mut names = Vec::new();
    if edits.correction.key.is_some() {
        names.push("key");
    }
    if edits.correction.bpm.is_some() {
        names.push("bpm");
    }
    if edits.cues.is_some() {
        names.push("cues");
    }
    names
}

/// Writes the table that goes into pipes and files, with the same columns
/// whatever the width, so that scripts can read it. The Conf column holds
/// the confidence of the key and of the tempo, each as H, M or L for high,
/// medium or low, or U for a value the user set. A terminal gets the table
/// of [`crate::view`] instead.
pub fn write_plain_table(
    writer: &mut impl Write,
    results: &[FileResult],
    user_edits: &UserEditsByFile,
) -> io::Result<()> {
    let header = [
        "Key", "", "BPM", "Energy", "Conf", "Quality", "Time", "File",
    ];
    let rows: Vec<[String; 8]> = results
        .iter()
        .filter_map(|result| {
            let analysis = result.outcome.as_ref().ok()?;
            let correction = user_edits
                .get(&result.path)
                .and_then(|looked_up| looked_up.as_ref().ok())
                .map(|edits| edits.correction);
            let key_letter = match correction.and_then(|correction| correction.key) {
                Some(_) => "U",
                None => confidence_letter(analysis.key_confidence()),
            };
            let tempo_letter = match correction.and_then(|correction| correction.bpm) {
                Some(_) => "U",
                None => confidence_letter(analysis.tempo_confidence()),
            };
            Some([
                analysis
                    .key
                    .map(|key| key.camelot())
                    .unwrap_or_else(|| "-".to_string()),
                analysis.key.map(|key| key.to_string()).unwrap_or_default(),
                analysis
                    .bpm
                    .map(|bpm| format!("{bpm:.1}"))
                    .unwrap_or_else(|| "-".to_string()),
                analysis
                    .energy
                    .map(|energy| energy.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                format!("{key_letter}/{tempo_letter}"),
                analysis
                    .quality
                    .as_ref()
                    .map(|quality| plain_verdict(quality.verdict).to_string())
                    .unwrap_or_else(|| "-".to_string()),
                format_duration(analysis.duration_seconds),
                Printable(result.path.display()).to_string(),
            ])
        })
        .collect();

    let mut widths = header.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    let header_cells = header.map(str::to_string);
    for row in std::iter::once(&header_cells).chain(&rows) {
        let [
            camelot,
            key_name,
            bpm,
            energy,
            confidence,
            quality,
            duration,
            file,
        ] = row;
        writeln!(
            writer,
            "{camelot:<camelot_width$}  {key_name:<key_name_width$}  {bpm:>bpm_width$}  {energy:>energy_width$}  {confidence:<confidence_width$}  {quality:<quality_width$}  {duration:>duration_width$}  {file}",
            camelot_width = widths[0],
            key_name_width = widths[1],
            bpm_width = widths[2],
            energy_width = widths[3],
            confidence_width = widths[4],
            quality_width = widths[5],
            duration_width = widths[6],
        )?;
    }
    Ok(())
}

/// The verdict as the plain table has always written it.
fn plain_verdict(verdict: QualityVerdict) -> &'static str {
    match verdict {
        QualityVerdict::Lossless => "Lossless",
        QualityVerdict::Good => "Good",
        QualityVerdict::Acceptable => "Acceptable",
        QualityVerdict::WeakOnABigSystem => "Weak",
        QualityVerdict::LikelyReencoded => "Re-encoded?",
    }
}

fn confidence_letter(confidence: Option<Confidence>) -> &'static str {
    match confidence {
        Some(Confidence::High) => "H",
        Some(Confidence::Medium) => "M",
        Some(Confidence::Low) => "L",
        None => "-",
    }
}

/// The verdicts from best to worst.
const VERDICT_ORDER: [QualityVerdict; 5] = [
    QualityVerdict::Lossless,
    QualityVerdict::Good,
    QualityVerdict::Acceptable,
    QualityVerdict::WeakOnABigSystem,
    QualityVerdict::LikelyReencoded,
];

/// What the listing after an analysis was narrowed to, if anything.
#[derive(Debug, Clone, Default)]
pub struct Narrowing {
    /// Only tracks rated low confidence are listed.
    pub needs_review: bool,
    /// Only tracks with one of these verdicts are listed, when not empty.
    pub verdicts: Vec<QualityVerdict>,
    /// How many tracks the listing shows.
    pub shown_count: usize,
}

impl Narrowing {
    fn is_narrowed(&self) -> bool {
        self.needs_review || !self.verdicts.is_empty()
    }
}

/// Writes, in plain sentences, what the numbers of a run count: how many of
/// the tracks are listed when the listing was narrowed, how many have a key
/// or tempo worth checking by ear, how many have each sound quality, and
/// how many files failed. Ends with where to look next.
pub fn write_analysis_summary(
    writer: &mut impl Write,
    theme: &Theme,
    results: &[FileResult],
    narrowing: &Narrowing,
) -> io::Result<()> {
    let analyses: Vec<_> = results
        .iter()
        .filter_map(|result| result.outcome.as_ref().ok())
        .collect();
    let failure_count = results.len() - analyses.len();
    let mut hints: Vec<String> = Vec::new();

    if narrowing.is_narrowed() {
        let mut reasons = Vec::new();
        if narrowing.needs_review {
            reasons.push("a key or tempo rated low confidence".to_string());
        }
        if !narrowing.verdicts.is_empty() {
            let labels: Vec<String> = narrowing
                .verdicts
                .iter()
                .map(|verdict| verdict.label().to_lowercase())
                .collect();
            reasons.push(format!("sound quality {}", or_list(&labels)));
        }
        writeln!(
            writer,
            "Showing {} of {}, those with {}.",
            theme.bold(narrowing.shown_count),
            counted(analyses.len(), "track", "tracks"),
            reasons.join(" and ")
        )?;
    }

    if !analyses.is_empty() {
        let review_count = analyses
            .iter()
            .filter(|analysis| analysis.needs_review())
            .count();
        if review_count == 0 {
            writeln!(writer, "Key and tempo: no track is rated low confidence.")?;
        } else {
            writeln!(
                writer,
                "Key and tempo: {} of {} rated low confidence ({}), worth checking by ear.",
                theme.bold(review_count),
                counted(analyses.len(), "track", "tracks"),
                theme.caution("●○○")
            )?;
            if !narrowing.needs_review {
                hints.push("--needs-review lists only the tracks to check by ear.".to_string());
            }
        }
    }

    let verdicts: Vec<QualityVerdict> = analyses
        .iter()
        .filter_map(|analysis| Some(analysis.quality.as_ref()?.verdict))
        .collect();
    let parts: Vec<String> = VERDICT_ORDER
        .iter()
        .filter_map(|verdict| {
            let count = verdicts.iter().filter(|found| *found == verdict).count();
            (count > 0).then_some((count, *verdict))
        })
        .enumerate()
        .map(|(index, (count, verdict))| {
            let noun = if index > 0 {
                ""
            } else if count == 1 {
                " track"
            } else {
                " tracks"
            };
            format!(
                "{}{noun} {}",
                theme.bold(count),
                verdict.label().to_lowercase()
            )
        })
        .collect();
    if !parts.is_empty() {
        writeln!(writer, "Sound quality: {}.", parts.join(", "))?;
        let worth_a_look: Vec<&str> = [
            (QualityVerdict::WeakOnABigSystem, "weak"),
            (QualityVerdict::LikelyReencoded, "reencoded"),
        ]
        .iter()
        .filter(|(verdict, _)| verdicts.contains(verdict))
        .map(|(_, name)| *name)
        .collect();
        if !worth_a_look.is_empty() && narrowing.verdicts.is_empty() {
            hints.push(format!(
                "--quality {} lists the tracks worth replacing, and --detail says why.",
                worth_a_look.join(",")
            ));
        }
    }

    if failure_count > 0 {
        writeln!(
            writer,
            "{} could not be analyzed; the errors above say why.",
            counted_files(failure_count)
        )?;
    }
    for hint in hints {
        write_hint(writer, theme, hint)?;
    }
    Ok(())
}

/// `first, second or third`.
fn or_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// Writes one `error:` line per file that could not be analyzed, with why.
pub fn write_failures(
    writer: &mut impl Write,
    theme: &Theme,
    results: &[FileResult],
) -> io::Result<()> {
    for result in results {
        if let Err(error) = &result.outcome {
            write_file_error(writer, theme, &result.path, error)?;
        }
    }
    Ok(())
}

/// Writes how many of the tracks were served from the cache.
pub fn write_cache_summary(
    writer: &mut impl Write,
    cached_count: usize,
    total: usize,
) -> io::Result<()> {
    let noun = if total == 1 { "track" } else { "tracks" };
    writeln!(
        writer,
        "{cached_count} of {total} {noun} came from the cache."
    )
}

/// Writes that the cache could not be used and why, that the files are
/// analyzed regardless, and how to make the warning go away.
pub fn write_cache_warning(
    writer: &mut impl Write,
    theme: &Theme,
    error: &EuphoriaError,
) -> io::Result<()> {
    write_warning(
        writer,
        theme,
        format_args!("analyzing without the cache: {}", Printable(error.chain())),
    )?;
    write_hint(
        writer,
        theme,
        "Run `euphoria cache clear` to reset the cache, or pass --no-cache to skip it.",
    )
}

/// Warns that the cache could not be told about rewritten files, which
/// are then analyzed again on the next run.
pub fn write_cache_record_warning(
    writer: &mut impl Write,
    theme: &Theme,
    error: &EuphoriaError,
) -> io::Result<()> {
    write_warning(
        writer,
        theme,
        format_args!(
            "the cache could not be updated for the changed files: {}",
            Printable(error.chain())
        ),
    )?;
    write_note(
        writer,
        theme,
        "Their tags are as reported. They will be analyzed again the next time they are read.",
    )
}

/// What an export wrote.
pub struct ExportSummary<'export> {
    /// Tracks in the exported file.
    pub track_count: usize,
    /// Playlists in the exported file, or `None` when playlists were not
    /// asked for.
    pub playlist_count: Option<usize>,
    /// How many of the tracks were served from the cache.
    pub cached_count: usize,
    /// The file that was written.
    pub output_file: &'export Path,
}

/// Writes one line saying how much was exported and where.
pub fn write_export_summary(
    writer: &mut impl Write,
    theme: &Theme,
    summary: &ExportSummary<'_>,
) -> io::Result<()> {
    write!(
        writer,
        "Exported {}",
        theme.bold(counted(summary.track_count, "track", "tracks"))
    )?;
    if let Some(playlist_count) = summary.playlist_count {
        write!(
            writer,
            " in {}",
            counted(playlist_count, "playlist", "playlists")
        )?;
    }
    write!(
        writer,
        " to {}",
        theme.bold(Printable(summary.output_file.display()))
    )?;
    if summary.cached_count > 0 {
        write!(writer, " ({} from the cache)", summary.cached_count)?;
    }
    writeln!(writer)
}

/// Writes how many exported tracks got a beat grid, and why the others did
/// not.
pub fn write_beat_grid_summary(
    writer: &mut impl Write,
    with_grid: usize,
    track_count: usize,
    program: Program,
) -> io::Result<()> {
    write!(
        writer,
        "Beat grids for {with_grid} of {}",
        counted(track_count, "track", "tracks")
    )?;
    if with_grid < track_count {
        write!(
            writer,
            "; the others have no clear beat or a grid rated low confidence, so {} analyzes them",
            program.name()
        )?;
    }
    writeln!(writer, ".")
}

/// Writes how many exported tracks got cues, and why the others did not.
pub fn write_cue_summary(
    writer: &mut impl Write,
    with_cues: usize,
    track_count: usize,
) -> io::Result<()> {
    write!(
        writer,
        "Cues for {with_cues} of {}",
        counted(track_count, "track", "tracks")
    )?;
    if with_cues < track_count {
        write!(
            writer,
            "; tracks without a beat grid in the file get none, unless you edited their cues in the app"
        )?;
    }
    writeln!(writer, ".")
}

/// Writes which cued tracks of the collection could not be matched to a
/// file here, when the user chose to export anyway.
pub fn write_unmatched_cued_tracks(
    writer: &mut impl Write,
    unmatched: &[String],
    program: Program,
) -> io::Result<()> {
    writeln!(
        writer,
        "Not protected, as --allow-unmatched-cued-tracks asked: {} with cues in your {} collection {} no file on this computer:",
        counted(unmatched.len(), "track", "tracks"),
        program.name(),
        if unmatched.len() == 1 {
            "matches"
        } else {
            "match"
        },
    )?;
    for description in unmatched {
        writeln!(writer, "  {}", Printable(description))?;
    }
    Ok(())
}

/// Writes how many tracks were left out of the export because they have
/// cues in rekordbox, and how to update them.
pub fn write_left_out_for_cues(
    writer: &mut impl Write,
    left_out: usize,
    program: Program,
) -> io::Result<()> {
    writeln!(
        writer,
        "Left out {} that already {} cues in your {} collection, so importing this file cannot change them. Export again without --cues to update {} key and tempo.",
        counted(left_out, "track", "tracks"),
        if left_out == 1 { "has" } else { "have" },
        program.name(),
        if left_out == 1 { "its" } else { "their" },
    )
}

/// The steps to bring an exported file into rekordbox.
pub const REKORDBOX_IMPORT_STEPS: [&str; 3] = [
    "Preferences > View > Layout: tick rekordbox xml.",
    "Preferences > Advanced > Database: choose this file.",
    "Refresh rekordbox xml in the tree, then import its tracks or playlists.",
];

/// The steps to bring an exported file into Traktor.
pub const TRAKTOR_IMPORT_STEPS: [&str; 3] = [
    "Back up collection.nml in Documents/Native Instruments/Traktor <version>.",
    "In the browser tree, right click Track Collection > Import Another Collection.",
    "Choose this file. Lock the beat grid or untick BPM before analyzing these tracks.",
];

/// What to do in Engine DJ after its library was updated.
pub const ENGINE_IMPORT_STEPS: [&str; 3] = [
    "Start Engine DJ. The key, tempo and comment show in the collection.",
    "Sync the library to your drives as usual; a drive's own library is not changed by this export.",
    "To undo, close Engine DJ and put the backup in place of Database2/m.db.",
];

/// What to do in VirtualDJ after its database was updated.
pub const VIRTUALDJ_IMPORT_STEPS: [&str; 3] = [
    "Start VirtualDJ. The key, tempo and comment show in the browser.",
    "VirtualDJ shows keys in the notation of its keyDisplay option.",
    "To undo, close VirtualDJ and put the backup in place of database.xml.",
];

/// Writes how to bring an exported file into the program, and where the
/// full guide is.
pub fn write_import_steps(
    writer: &mut impl Write,
    theme: &Theme,
    program: Program,
) -> io::Result<()> {
    let (steps, guide): (&[&str], &str) = match program {
        Program::Rekordbox => (&REKORDBOX_IMPORT_STEPS, "export-to-rekordbox.md"),
        Program::Traktor => (&TRAKTOR_IMPORT_STEPS, "export-to-traktor.md"),
        Program::Engine => (&ENGINE_IMPORT_STEPS, "export-to-engine-dj.md"),
        Program::VirtualDj => (&VIRTUALDJ_IMPORT_STEPS, "export-to-virtualdj.md"),
    };
    match program {
        Program::Engine | Program::VirtualDj => writeln!(writer, "In {}:", program.name())?,
        Program::Rekordbox | Program::Traktor => {
            writeln!(writer, "To import it in {}:", program.name())?;
        }
    }
    for (number, step) in steps.iter().enumerate() {
        writeln!(writer, "  {}. {step}", number + 1)?;
    }
    writeln!(
        writer,
        "{}",
        theme.dim(format!(
            "Full guide: https://github.com/lvndry/euphoria/blob/main/docs/guides/{guide}"
        ))
    )
}

/// Writes one `error:` line per file an export left out, with why.
pub fn write_skipped_tracks(
    writer: &mut impl Write,
    theme: &Theme,
    skipped: &[SkippedTrack],
) -> io::Result<()> {
    for skipped_track in skipped {
        write_file_error(writer, theme, &skipped_track.path, &skipped_track.reason)?;
    }
    Ok(())
}

/// Writes how many files an export left out, after their errors.
pub fn write_skipped_summary(writer: &mut impl Write, skipped_count: usize) -> io::Result<()> {
    writeln!(
        writer,
        "Left {} out of the export; the errors above say why.",
        counted_files(skipped_count)
    )
}

/// Writes one `warning:` line per exported track that is named after its
/// file because its tags could not be read, with why.
pub fn write_unread_tags(
    writer: &mut impl Write,
    theme: &Theme,
    unread_tags: &[UnreadTags],
) -> io::Result<()> {
    for track in unread_tags {
        write_warning(
            writer,
            theme,
            format_args!(
                "{}: named after its file, as its title could not be read: {}",
                Printable(track.path.display()),
                Printable(track.reason.chain())
            ),
        )?;
    }
    Ok(())
}

/// Shown where a tag field does not exist.
const ABSENT_FIELD: &str = "(none)";

/// Writes, for each file a tag write would change, its path and one line
/// per field with the value it has and the value it would get.
pub fn write_tag_preview(
    writer: &mut impl Write,
    theme: &Theme,
    plans: &[WritePlan],
) -> io::Result<()> {
    for plan in plans {
        writeln!(writer, "{}", theme.bold(Printable(plan.path().display())))?;
        for change in plan.changes() {
            let shown = |value: Option<&str>| -> String {
                match value {
                    None => ABSENT_FIELD.to_string(),
                    Some(value) => describe_serato_value(&change.field, value)
                        .unwrap_or_else(|| value.to_string()),
                }
            };
            writeln!(
                writer,
                "  {} {} {} {}",
                theme.dim(format_args!(
                    "{}:",
                    serato_field_name(&change.field).unwrap_or(&change.field)
                )),
                theme.dim(Printable(shown(change.current.as_deref()))),
                theme.dim("->"),
                theme.good(Printable(shown(change.new.as_deref())))
            )?;
        }
    }
    Ok(())
}

/// Writes a `warning:` line for each file whose Serato cues or grid could
/// not all go where Euphoria wanted, because cues or a grid of yours were
/// there, and for each file that got no Serato data, then one line of
/// totals.
pub fn write_serato_reports(
    writer: &mut impl Write,
    theme: &Theme,
    reports: &[(PathBuf, SeratoReport)],
) -> io::Result<()> {
    let mut placed = 0;
    let mut kept = 0;
    for (path, report) in reports {
        placed += report.placed.len();
        kept += report.kept;
        let mut problems = Vec::new();
        match &report.skipped {
            Some(SeratoSkip::Wav) => {
                problems.push("no Serato cues or grid: Serato reads none from WAV tags".to_string())
            }
            Some(SeratoSkip::Ogg) => problems.push(
                "no Serato cues or grid: Serato's layout in Ogg files is not known well enough to write"
                    .to_string(),
            ),
            Some(SeratoSkip::OldId3) => problems
                .push("no Serato cues or grid: its ID3v2.2 tag cannot hold them".to_string()),
            Some(SeratoSkip::Unreadable { field, reason }) => problems.push(format!(
                "no Serato cues or grid: its {} could not be read ({reason}), so it is left as it is",
                serato_field_name(field).unwrap_or(field)
            )),
            None => {}
        }
        for cue in &report.placed {
            if cue.pad != cue.wanted_pad {
                problems.push(format!(
                    "{} went to pad {}, since pad {} holds a cue of yours",
                    if cue.name.is_empty() {
                        "a cue"
                    } else {
                        &cue.name
                    },
                    cue.pad + 1,
                    cue.wanted_pad + 1
                ));
            }
        }
        if !report.no_free_pad.is_empty() {
            problems.push(format!(
                "{} left out: every pad holds a cue of yours",
                report.no_free_pad.join(", ")
            ));
        }
        if let GridOutcome::KeptYours { position, bpm } = report.grid {
            problems.push(format!(
                "your Serato beat grid ({bpm:.2} BPM from {position:.3} s) is kept, not replaced"
            ));
        }
        for problem in problems {
            write_warning(
                writer,
                theme,
                format_args!("{}: {}", Printable(path.display()), Printable(&problem)),
            )?;
        }
    }
    writeln!(
        writer,
        "Serato: {} to add, {} of yours kept.",
        counted(placed, "cue point", "cue points"),
        counted(kept, "cue point", "cue points"),
    )
}

/// Writes one `error:` line per file that was left out of a tag write or an
/// undo, with the reason.
pub fn write_tag_failures(
    writer: &mut impl Write,
    theme: &Theme,
    failures: &[Failure],
) -> io::Result<()> {
    for failure in failures {
        write_file_error(writer, theme, &failure.path, &failure.error)?;
    }
    Ok(())
}

/// Writes one line per file that was changed without the log of the write
/// being able to note it.
pub fn write_unrecorded_tag_writes(
    writer: &mut impl Write,
    theme: &Theme,
    unrecorded: &[Failure],
) -> io::Result<()> {
    for failure in unrecorded {
        write_warning(
            writer,
            theme,
            format_args!(
                "{}: the file was changed, but the record of it could not be saved: {}. \
                 Its backup was saved before the change, so `euphoria tags undo` still restores it.",
                Printable(failure.path.display()),
                Printable(failure.error.chain())
            ),
        )?;
    }
    Ok(())
}

/// Writes how many files a tag write would change, how many already hold
/// the values, and how many cannot be written.
pub fn write_tag_plan_summary(
    writer: &mut impl Write,
    theme: &Theme,
    change_count: usize,
    unchanged_count: usize,
    failure_count: usize,
) -> io::Result<()> {
    write!(
        writer,
        "{} to change",
        theme.bold(counted_files(change_count))
    )?;
    if unchanged_count > 0 {
        write!(writer, ", {unchanged_count} already up to date")?;
    }
    if failure_count > 0 {
        write!(writer, ", {failure_count} left out because of errors")?;
    }
    writeln!(writer, ".")
}

/// Writes how a tag write went and how to take it back.
pub fn write_tag_write_summary(
    writer: &mut impl Write,
    theme: &Theme,
    written_count: usize,
    failure_count: usize,
) -> io::Result<()> {
    let completed = if failure_count == 0 {
        "Wrote the tags of"
    } else {
        "Completed tag writes for"
    };
    write!(
        writer,
        "{completed} {}",
        theme.bold(counted_files(written_count))
    )?;
    if failure_count > 0 {
        write!(
            writer,
            "; {} reported errors and their tags may have changed",
            counted_files(failure_count)
        )?;
    }
    writeln!(writer, ".")?;
    if written_count > 0 {
        write_hint(
            writer,
            theme,
            "To put the old tags back, run `euphoria tags undo`.",
        )?;
    }
    Ok(())
}

/// Writes the path of each file an undo restored.
pub fn write_restored_files(writer: &mut impl Write, restored: &[PathBuf]) -> io::Result<()> {
    for path in restored {
        writeln!(writer, "{}", Printable(path.display()))?;
    }
    Ok(())
}

/// Writes how an undo went.
pub fn write_tag_undo_summary(
    writer: &mut impl Write,
    theme: &Theme,
    restored_count: usize,
    untouched_count: usize,
    failure_count: usize,
) -> io::Result<()> {
    write!(
        writer,
        "Restored the tags of {}",
        theme.bold(counted_files(restored_count))
    )?;
    if untouched_count > 0 {
        write!(
            writer,
            ", {untouched_count} already had their original tags"
        )?;
    }
    if failure_count > 0 {
        write!(
            writer,
            ", {failure_count} could not be restored and can be tried again with the same command"
        )?;
    }
    writeln!(writer, ".")
}

pub fn counted_files(count: usize) -> String {
    counted(count, "file", "files")
}

pub fn counted(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

/// A time or tempo to a thousandth, which is a millisecond for times.
fn round_to_thousandths(value: f64) -> f64 {
    let scaled = value * 1_000.0;
    if scaled.is_finite() {
        scaled.round() / 1_000.0
    } else {
        value
    }
}

fn round_to_tenths(value: f32) -> f32 {
    let scaled = value * 10.0;
    if scaled.is_finite() {
        scaled.round() / 10.0
    } else {
        // Scaling can overflow a finite value whose spacing already
        // exceeds a tenth, so there is no fractional digit to round.
        value
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use euphoria_core::{Correction, Mode, SectionCue, TrackAnalysis};

    use super::*;

    fn sample_quality() -> QualityReport {
        QualityReport {
            codec: "aac".to_string(),
            lossless: false,
            bitrate_kbps: Some(128),
            sample_rate: 44_100,
            bit_depth: None,
            channels: Some(2),
            cutoff_hz: Some(16_123.0),
            clipped_share: 0.0,
            integrated_loudness_lufs: Some(-9.25),
            true_peak_dbtp: Some(0.8),
            verdict: QualityVerdict::WeakOnABigSystem,
        }
    }

    fn sample_results() -> Vec<FileResult> {
        vec![
            FileResult {
                path: PathBuf::from("music/low tide.flac"),
                outcome: Ok(TrackAnalysis {
                    duration_seconds: 408.4,
                    key: Some(Key::new(9, Mode::Minor)),
                    bpm: Some(170.0),
                    energy: Some(7),
                    tuning_cents: Some(-31.26),
                    key_runner_up: Some(Key::new(0, Mode::Major)),
                    key_score_margin: Some(0.02),
                    tempo_estimate: Some(85.0),
                    tempo_score_margin: Some(0.05),
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: vec![
                        KeySection {
                            start_seconds: 0.0,
                            end_seconds: 250.04,
                            key: Key::new(9, Mode::Minor),
                        },
                        KeySection {
                            start_seconds: 250.04,
                            end_seconds: 408.4,
                            key: Key::new(4, Mode::Minor),
                        },
                    ],
                    quality: Some(sample_quality()),
                }),
            },
            FileResult {
                path: PathBuf::from("music/intro.wav"),
                outcome: Ok(TrackAnalysis {
                    duration_seconds: 59.6,
                    key: None,
                    bpm: None,
                    energy: None,
                    tuning_cents: None,
                    key_runner_up: None,
                    key_score_margin: None,
                    tempo_estimate: None,
                    tempo_score_margin: None,
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: Vec::new(),
                    quality: None,
                }),
            },
            FileResult {
                path: PathBuf::from("music/broken.mp3"),
                outcome: Err(EuphoriaError::UnknownKeyText("example".to_string())),
            },
        ]
    }

    fn render(write: impl Fn(&mut Vec<u8>, &[FileResult]) -> io::Result<()>) -> String {
        let mut buffer = Vec::new();
        write(&mut buffer, &sample_results()).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    fn write_json_in_default_range(writer: &mut Vec<u8>, results: &[FileResult]) -> io::Result<()> {
        write_json(
            writer,
            results,
            TempoRange::DEFAULT,
            &UserEditsByFile::new(),
        )
    }

    fn summary(results: &[FileResult], narrowing: &Narrowing) -> String {
        let mut buffer = Vec::new();
        write_analysis_summary(&mut buffer, &Theme::PLAIN, results, narrowing).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn direct_messages_wrap_without_splitting_words_or_changing_piped_bytes() {
        let message = "nothing was written because standard input is not a terminal; run again with --yes or --dry-run.";
        let mut terminal = Vec::new();
        write_message_at_width(
            &mut terminal,
            &Theme::COLOURED.error_label(),
            message,
            Some(40),
        )
        .unwrap();
        let terminal = String::from_utf8(terminal).unwrap();
        let plain = console::strip_ansi_codes(&terminal);
        assert!(plain.lines().count() > 1);
        assert!(
            plain
                .lines()
                .all(|line| console::measure_text_width(line) <= 40)
        );
        assert_eq!(
            plain.split_whitespace().collect::<Vec<_>>(),
            format!("error: {message}")
                .split_whitespace()
                .collect::<Vec<_>>()
        );
        let mut piped = Vec::new();
        write_message_at_width(&mut piped, "error:", message, None).unwrap();
        assert_eq!(
            String::from_utf8(piped).unwrap(),
            format!("error: {message}\n")
        );
    }

    #[test]
    fn undo_counts_files_that_already_have_their_original_tags() {
        let mut output = Vec::new();
        write_tag_undo_summary(&mut output, &Theme::PLAIN, 0, 2, 0).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Restored the tags of 0 files, 2 already had their original tags.\n"
        );
    }

    #[test]
    fn tag_write_errors_do_not_promise_that_the_files_are_unchanged() {
        let mut output = Vec::new();
        write_tag_write_summary(&mut output, &Theme::PLAIN, 0, 1).unwrap();
        let report = String::from_utf8(output).unwrap();
        assert_eq!(
            report,
            "Completed tag writes for 0 files; 1 file reported errors and their tags may have changed.\n"
        );
        assert!(!report.contains("were not changed"));
    }

    #[test]
    fn rounding_large_finite_values_keeps_them_finite() {
        for value in [f32::MAX, -f32::MAX] {
            assert_eq!(round_to_tenths(value), value);
        }
        assert_eq!(round_to_tenths(124.04), 124.0);
        assert_eq!(round_to_tenths(124.06), 124.1);
    }

    #[test]
    fn the_plain_table_keeps_its_columns_for_scripts() {
        let expected = "\
Key             BPM  Energy  Conf  Quality  Time  File
8A   A minor  170.0       7  H/L   Weak     6:48  music/low tide.flac
-                 -       -  -/-   -        1:00  music/intro.wav
";
        assert_eq!(
            render(|writer, results| write_plain_table(writer, results, &UserEditsByFile::new())),
            expected
        );
    }

    #[test]
    fn values_the_user_set_are_marked_in_the_table_and_the_json() {
        let results = sample_results();
        let mut user_edits = UserEditsByFile::new();
        user_edits.insert(
            results[0].path.clone(),
            Ok(UserEdits {
                correction: Correction::of_bpm(85.0).unwrap(),
                cues: None,
            }),
        );
        let mut table = Vec::new();
        write_plain_table(&mut table, &results, &user_edits).unwrap();
        let table = String::from_utf8(table).unwrap();
        assert!(table.contains("H/U"), "{table}");

        let mut json = Vec::new();
        write_json(&mut json, &results, TempoRange::DEFAULT, &user_edits).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(json[0]["set_by_user"], serde_json::json!(["bpm"]));
        assert_eq!(json[1]["set_by_user"], serde_json::json!([]));
    }

    #[test]
    fn the_json_lists_the_users_cues_in_place_of_the_automatic_ones() {
        let mut results = sample_results();
        if let Ok(analysis) = &mut results[1].outcome {
            analysis.cues = vec![SectionCue {
                seconds: 0.5,
                section: euphoria_core::CueSection::Start,
            }];
        }
        let json_of = |user_edits: &UserEditsByFile| {
            let mut json = Vec::new();
            write_json(&mut json, &results, TempoRange::DEFAULT, user_edits).unwrap();
            serde_json::from_slice::<serde_json::Value>(&json).unwrap()
        };
        assert_eq!(
            json_of(&UserEditsByFile::new())[1]["cues"],
            serde_json::json!([{"seconds": 0.5, "name": "Start", "hot_cue": "A"}])
        );

        let mut user_edits = UserEditsByFile::new();
        user_edits.insert(
            results[1].path.clone(),
            Ok(UserEdits {
                correction: Correction::default(),
                cues: Some(vec![
                    Cue {
                        seconds: 40.0,
                        name: "Vocal".to_string(),
                        hot_cue: None,
                    },
                    Cue {
                        seconds: 8.25,
                        name: "Intro".to_string(),
                        hot_cue: euphoria_core::HotCue::from_letter('D'),
                    },
                ]),
            }),
        );
        let json = json_of(&user_edits);
        assert_eq!(
            json[1]["cues"],
            serde_json::json!([
                {"seconds": 8.25, "name": "Intro", "hot_cue": "D"},
                {"seconds": 40.0, "name": "Vocal", "hot_cue": null},
            ])
        );
        assert_eq!(json[1]["set_by_user"], serde_json::json!(["cues"]));
    }

    #[test]
    fn the_summary_says_what_every_number_counts_and_where_to_look_next() {
        let mut results = sample_results();
        let mut good = sample_quality();
        good.verdict = QualityVerdict::Good;
        if let Ok(analysis) = &mut results[1].outcome {
            analysis.quality = Some(good);
        }
        let expected = "\
Key and tempo: 1 of 2 tracks rated low confidence (●○○), worth checking by ear.
Sound quality: 1 track good, 1 weak on a big system.
1 file could not be analyzed; the errors above say why.
hint: --needs-review lists only the tracks to check by ear.
hint: --quality weak lists the tracks worth replacing, and --detail says why.
";
        assert_eq!(summary(&results, &Narrowing::default()), expected);
    }

    #[test]
    fn a_narrowed_listing_says_what_it_shows_and_drops_its_own_hint() {
        let narrowing = Narrowing {
            needs_review: true,
            verdicts: vec![QualityVerdict::WeakOnABigSystem],
            shown_count: 1,
        };
        let shown = summary(&sample_results(), &narrowing);
        assert!(
            shown.starts_with(
                "Showing 1 of 2 tracks, those with a key or tempo rated low confidence and sound quality weak on a big system.\n"
            ),
            "{shown}"
        );
        assert!(!shown.contains("hint:"), "{shown}");
    }

    #[test]
    fn a_batch_without_trouble_says_so() {
        let results = &sample_results()[1..2];
        assert_eq!(
            summary(results, &Narrowing::default()),
            "Key and tempo: no track is rated low confidence.\n"
        );
        assert_eq!(summary(&[], &Narrowing::default()), "");
    }

    #[test]
    fn json_includes_every_file_and_the_error_of_failed_ones() {
        let parsed: serde_json::Value =
            serde_json::from_str(&render(write_json_in_default_range)).unwrap();
        assert_eq!(parsed[0]["key"]["camelot"], "8A");
        assert_eq!(parsed[0]["key"]["name"], "A minor");
        assert_eq!(parsed[0]["key"]["open_key"], "1m");
        assert_eq!(parsed[0]["key"]["confidence"], "high");
        assert_eq!(
            parsed[0]["runner_up_key"],
            serde_json::json!({ "camelot": "8B", "name": "C major", "open_key": "1d" })
        );
        assert_eq!(parsed[0]["bpm"], 170.0);
        assert_eq!(parsed[0]["bpm_confidence"], "low");
        assert_eq!(parsed[0]["bpm_alternative"], 85.0);
        assert_eq!(parsed[0]["needs_review"], true);
        assert!(parsed[1]["runner_up_key"].is_null());
        assert!(parsed[1]["bpm_confidence"].is_null());
        assert!(parsed[1]["bpm_alternative"].is_null());
        assert_eq!(parsed[1]["needs_review"], false);
        assert!(parsed[2]["needs_review"].is_null());
        assert_eq!(parsed[0]["duration_seconds"], 408.4);
        assert_eq!(parsed[0]["energy"], 7);
        assert_eq!(parsed[0]["tuning_cents"], -31.3);
        assert_eq!(
            parsed[0]["key_timeline"],
            serde_json::json!([
                {
                    "start_seconds": 0.0,
                    "end_seconds": 250.0,
                    "key": { "camelot": "8A", "name": "A minor", "open_key": "1m" }
                },
                {
                    "start_seconds": 250.0,
                    "end_seconds": 408.4,
                    "key": { "camelot": "9A", "name": "E minor", "open_key": "2m" }
                }
            ])
        );
        assert_eq!(parsed[0]["quality"]["codec"], "aac");
        assert_eq!(parsed[0]["quality"]["bitrate_kbps"], 128);
        assert_eq!(parsed[0]["quality"]["cutoff_hz"], 16_123.0);
        assert_eq!(parsed[0]["quality"]["verdict"], "weak-on-a-big-system");
        assert!(
            parsed[0]["quality"]["reason"]
                .as_str()
                .unwrap()
                .starts_with("128 kbit/s. ")
        );
        assert!(parsed[1]["quality"].is_null());
        assert!(parsed[2]["quality"].is_null());
        assert!(parsed[1]["tuning_cents"].is_null());
        assert_eq!(parsed[1]["key_timeline"], serde_json::json!([]));
        assert_eq!(parsed[2]["key_timeline"], serde_json::json!([]));
        assert!(parsed[1]["key"].is_null());
        assert!(parsed[1]["energy"].is_null());
        assert!(parsed[2]["energy"].is_null());
        assert!(parsed[2]["error"].as_str().unwrap().contains("example"));
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn cache_summary_counts_cached_tracks() {
        let mut buffer = Vec::new();
        write_cache_summary(&mut buffer, 5, 7).unwrap();
        write_cache_summary(&mut buffer, 1, 1).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "5 of 7 tracks came from the cache.\n1 of 1 track came from the cache.\n"
        );
    }

    #[test]
    fn cache_warning_gives_the_cause_and_the_two_ways_out() {
        let mut buffer = Vec::new();
        let error = EuphoriaError::UnknownKeyText("example".to_string());
        write_cache_warning(&mut buffer, &Theme::PLAIN, &error).unwrap();
        let warning = String::from_utf8(buffer).unwrap();
        assert!(warning.starts_with("warning: analyzing without the cache: "));
        assert!(warning.contains("example"));
        assert!(warning.contains("\nhint: Run `euphoria cache clear`"));
        assert!(warning.contains("--no-cache"));
    }

    #[test]
    fn labels_are_coloured_only_when_asked() {
        let mut plain = Vec::new();
        write_error(&mut plain, &Theme::PLAIN, "boom").unwrap();
        assert_eq!(plain, b"error: boom\n");
        let mut coloured = Vec::new();
        write_error(&mut coloured, &Theme::COLOURED, "boom").unwrap();
        assert_eq!(coloured, b"\x1b[1;31merror:\x1b[0m boom\n");
    }

    fn export_summary(summary: &ExportSummary<'_>) -> String {
        let mut buffer = Vec::new();
        write_export_summary(&mut buffer, &Theme::PLAIN, summary).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn export_summary_counts_tracks_and_playlists_on_one_line() {
        let output_file = PathBuf::from("/Users/dj/library.xml");
        let mut summary = ExportSummary {
            track_count: 340,
            playlist_count: Some(12),
            cached_count: 0,
            output_file: &output_file,
        };
        assert_eq!(
            export_summary(&summary),
            "Exported 340 tracks in 12 playlists to /Users/dj/library.xml\n"
        );

        summary.track_count = 1;
        summary.playlist_count = Some(1);
        summary.cached_count = 1;
        assert_eq!(
            export_summary(&summary),
            "Exported 1 track in 1 playlist to /Users/dj/library.xml (1 from the cache)\n"
        );

        summary.playlist_count = None;
        summary.cached_count = 0;
        assert_eq!(
            export_summary(&summary),
            "Exported 1 track to /Users/dj/library.xml\n"
        );
    }

    #[test]
    fn import_steps_are_numbered_and_end_with_the_guide() {
        let mut buffer = Vec::new();
        write_import_steps(&mut buffer, &Theme::PLAIN, Program::Rekordbox).unwrap();
        let steps = String::from_utf8(buffer).unwrap();
        let lines: Vec<&str> = steps.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "To import it in rekordbox:");
        assert!(lines[1].starts_with("  1. Preferences"), "{steps}");
        assert!(lines[3].starts_with("  3. "), "{steps}");
        assert!(
            lines[4].ends_with("docs/guides/export-to-rekordbox.md"),
            "{steps}"
        );
    }

    #[test]
    fn skipped_tracks_are_errors_with_the_reason() {
        let skipped = [SkippedTrack {
            path: PathBuf::from("music/broken.mp3"),
            reason: EuphoriaError::UnknownKeyText("example".to_string()),
        }];
        let mut buffer = Vec::new();
        write_skipped_tracks(&mut buffer, &Theme::PLAIN, &skipped).unwrap();
        write_skipped_summary(&mut buffer, skipped.len()).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "error: music/broken.mp3: unrecognised key text: \"example\"\n\
             Left 1 file out of the export; the errors above say why.\n"
        );
    }

    #[test]
    fn unread_tags_are_warnings_with_the_reason() {
        let unread = |name: &str| UnreadTags {
            path: PathBuf::from(name),
            reason: EuphoriaError::UnknownKeyText("example".to_string()),
        };
        let mut buffer = Vec::new();
        write_unread_tags(
            &mut buffer,
            &Theme::PLAIN,
            &[unread("music/a.mp3"), unread("music/b.mp3")],
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "warning: music/a.mp3: named after its file, as its title could not be read: unrecognised key text: \"example\"\n\
             warning: music/b.mp3: named after its file, as its title could not be read: unrecognised key text: \"example\"\n"
        );
    }

    #[test]
    fn each_failed_file_is_an_error_line() {
        let mut buffer = Vec::new();
        write_failures(&mut buffer, &Theme::PLAIN, &sample_results()).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "error: music/broken.mp3: unrecognised key text: \"example\"\n"
        );
    }
}
