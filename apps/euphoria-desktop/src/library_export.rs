// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exporting the whole library, or the tracks shown, to a DJ program from
//! the window: pick the program, the scope and the program's options,
//! review what would be written, then write. Rekordbox and Traktor get a
//! file; VirtualDJ and Engine DJ are edited in place, with the checks and
//! the backup the command line has.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use euphoria_core::export::checks;
use euphoria_core::export::engine::{
    EngineOptions, EngineTrack, library_database, preview_library_update, update_library,
};
use euphoria_core::export::program_files;
use euphoria_core::export::rekordbox::{ExistingFile, ExportOptions, Library, write_xml_file};
use euphoria_core::export::rekordbox_collection::{self, CuedTracks};
use euphoria_core::export::traktor::{ModifiedTime, TraktorOptions, write_nml_file};
use euphoria_core::export::traktor_collection;
use euphoria_core::export::virtualdj::{
    VirtualDjOptions, VirtualDjTrack, preview_database_update, update_database_file,
};
use euphoria_core::{
    AssembledLibrary, ExportContents, FileResult, PlaylistLayout, assemble_library_with,
};
use gpui::{Context, PathPromptOptions};

use crate::app::EuphoriaApp;
use crate::playlist_flow::{PlaylistExport, PlaylistFormat, dirs_home, file_name_for};
use crate::tag_settings::{LabelChoice, NotationChoice};

/// The programs a library is exported to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportProgram {
    /// A rekordbox XML file, imported in rekordbox.
    Rekordbox,
    /// A Traktor NML file, imported in Traktor.
    Traktor,
    /// The database of an Engine DJ library, edited in place.
    EngineDj,
    /// The database.xml of VirtualDJ, edited in place.
    VirtualDj,
    /// An M3U8 playlist for any other program.
    M3u8,
}

impl ExportProgram {
    /// Every program, in the order the sheet lists them.
    pub(crate) const ALL: [ExportProgram; 5] = [
        ExportProgram::Rekordbox,
        ExportProgram::Traktor,
        ExportProgram::EngineDj,
        ExportProgram::VirtualDj,
        ExportProgram::M3u8,
    ];

    /// The program's name as people write it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ExportProgram::Rekordbox => "rekordbox",
            ExportProgram::Traktor => "Traktor",
            ExportProgram::EngineDj => "Engine DJ",
            ExportProgram::VirtualDj => "VirtualDJ",
            ExportProgram::M3u8 => "Any program (M3U8)",
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            ExportProgram::Rekordbox => "rekordbox",
            ExportProgram::Traktor => "traktor",
            ExportProgram::EngineDj => "engine",
            ExportProgram::VirtualDj => "virtualdj",
            ExportProgram::M3u8 => "m3u8",
        }
    }

    /// True when the export edits the program's own database.
    pub(crate) fn edits_in_place(self) -> bool {
        matches!(self, ExportProgram::EngineDj | ExportProgram::VirtualDj)
    }

    /// True when the program has a key notation to choose.
    pub(crate) fn has_key_notation(self) -> bool {
        self != ExportProgram::M3u8
    }

    /// True when the export can hold beat grids and cues.
    pub(crate) fn has_beat_grid(self) -> bool {
        self != ExportProgram::M3u8
    }

    /// True when the program has a place for the key and energy label.
    pub(crate) fn has_label(self) -> bool {
        self.edits_in_place()
    }

    /// True when its collection can be read to protect its cues.
    pub(crate) fn has_collection(self) -> bool {
        matches!(self, ExportProgram::Rekordbox | ExportProgram::Traktor)
    }

    /// The places the label can go in this program.
    pub(crate) fn labels(self) -> Vec<LabelChoice> {
        LabelChoice::ALL
            .into_iter()
            .filter(|choice| self == ExportProgram::VirtualDj || *choice != LabelChoice::Grouping)
            .collect()
    }

    fn extension(self) -> &'static str {
        match self {
            ExportProgram::Rekordbox => "xml",
            ExportProgram::Traktor => "nml",
            ExportProgram::M3u8 => "m3u8",
            ExportProgram::EngineDj | ExportProgram::VirtualDj => "",
        }
    }

    /// The default place of the library or database this program edits.
    pub(crate) fn default_target(self) -> Option<PathBuf> {
        match self {
            ExportProgram::EngineDj => program_files::default_engine_library(),
            ExportProgram::VirtualDj => program_files::default_virtualdj_database(),
            _ => None,
        }
    }
}

/// Which tracks are exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportScope {
    /// Every track of the library.
    Library,
    /// The tracks the table shows now.
    Shown,
}

/// What the sheet has been given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportSetup {
    pub(crate) program: ExportProgram,
    pub(crate) scope: ExportScope,
    pub(crate) key_notation: NotationChoice,
    pub(crate) label: LabelChoice,
    pub(crate) beat_grid: bool,
    pub(crate) cues: bool,
    pub(crate) playlists: bool,
    pub(crate) collection: Option<PathBuf>,
    pub(crate) allow_unmatched_cued_tracks: bool,
    pub(crate) output: Option<PathBuf>,
    pub(crate) target: Option<PathBuf>,
}

impl ExportSetup {
    pub(crate) fn new(program: ExportProgram, scope: ExportScope) -> Self {
        Self {
            program,
            scope,
            key_notation: NotationChoice::Camelot,
            label: LabelChoice::CommentStart,
            beat_grid: false,
            cues: false,
            playlists: true,
            collection: None,
            allow_unmatched_cued_tracks: false,
            output: None,
            target: program.default_target(),
        }
    }

    /// Chooses the program, taking its default library or database.
    pub(crate) fn choose_program(&mut self, program: ExportProgram) {
        if self.program == program {
            return;
        }
        self.program = program;
        self.target = program.default_target();
        self.output = None;
        self.collection = None;
        self.cues = false;
        self.allow_unmatched_cued_tracks = false;
        if !program.labels().contains(&self.label) {
            self.label = LabelChoice::CommentStart;
        }
    }

    /// True when cues can be asked for: the beat grid is written, and for a
    /// file export the collection that protects the user's own cues is
    /// given.
    pub(crate) fn can_write_cues(&self) -> bool {
        self.beat_grid && (!self.program.has_collection() || self.collection.is_some())
    }

    /// True when everything the export needs is given.
    pub(crate) fn is_complete(&self) -> bool {
        if self.program.edits_in_place() {
            self.target.is_some()
        } else {
            self.output.is_some()
        }
    }

    /// The name the save dialog suggests.
    pub(crate) fn suggested_file_name(&self) -> String {
        format!(
            "{}.{}",
            file_name_for("Euphoria library"),
            self.program.extension()
        )
    }
}

/// The checks of whether a program is running, which a test replaces.
#[derive(Clone, Copy)]
pub(crate) struct RunningChecks {
    pub(crate) engine: fn() -> Option<bool>,
    pub(crate) virtualdj: fn() -> Option<bool>,
}

impl RunningChecks {
    pub(crate) const REAL: RunningChecks = RunningChecks {
        engine: program_files::engine_is_running,
        virtualdj: program_files::virtualdj_is_running,
    };
}

/// What a review or an export did, for the sheet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ExportReport {
    /// One line that says what happened or would happen.
    pub(crate) headline: String,
    /// The lines beneath it.
    pub(crate) details: Vec<String>,
    /// How many tracks were or would be written.
    pub(crate) track_count: usize,
}

impl ExportReport {
    /// The report as one text for the notice strip.
    pub(crate) fn as_notice(&self) -> String {
        std::iter::once(self.headline.clone())
            .chain(self.details.iter().take(3).cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Everything an export works from, taken from the window so that the work
/// can run off the main thread.
pub(crate) struct LibraryExportJob {
    pub(crate) setup: ExportSetup,
    pub(crate) export: PlaylistExport,
    pub(crate) folders: Vec<PathBuf>,
    pub(crate) running: RunningChecks,
}

fn counted(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{} {noun}", crate::format::count(count))
}

fn listed(paths: &[PathBuf], what: &str) -> Vec<String> {
    const LISTED: usize = 5;
    let mut lines = vec![what.to_string()];
    lines.extend(
        paths
            .iter()
            .take(LISTED)
            .map(|path| format!("  {}", path.display())),
    );
    if paths.len() > LISTED {
        lines.push(format!("  and {} more", paths.len() - LISTED));
    }
    lines
}

impl LibraryExportJob {
    /// Works out what the export does and, unless `dry_run`, does it.
    pub(crate) fn run(self, dry_run: bool) -> Result<ExportReport, String> {
        let setup = self.setup.clone();
        let tracks = self.export.tracks();
        let not_analyzed = tracks
            .iter()
            .filter(|track| track.analysis.is_none())
            .count()
            + self.export.not_in_library();
        let mut report = match setup.program {
            ExportProgram::M3u8 => return self.run_m3u8(dry_run),
            ExportProgram::Rekordbox | ExportProgram::Traktor => {
                self.run_collection_file(&setup, dry_run)?
            }
            ExportProgram::EngineDj => self.run_engine(&setup, dry_run)?,
            ExportProgram::VirtualDj => self.run_virtualdj(&setup, dry_run)?,
        };
        if not_analyzed > 0 {
            report.details.push(format!(
                "{} left out: not analyzed yet or not in the library.",
                counted(not_analyzed, "track was", "tracks were")
            ));
        }
        Ok(report)
    }

    fn results(&self) -> (Vec<FileResult>, HashMap<PathBuf, Vec<euphoria_core::Cue>>) {
        let mut edited = HashMap::new();
        let results = self
            .export
            .tracks()
            .iter()
            .filter_map(|track| {
                let analysis = track.analysis.clone()?;
                if let Some(cues) = &track.edited_cues {
                    edited.insert(track.path.clone(), cues.clone());
                }
                Some(FileResult {
                    path: track.path.clone(),
                    outcome: Ok(analysis),
                })
            })
            .collect();
        (results, edited)
    }

    fn energies(&self) -> HashMap<PathBuf, u8> {
        self.export
            .tracks()
            .iter()
            .filter_map(|track| Some((track.path.clone(), track.analysis.as_ref()?.energy?)))
            .collect()
    }

    fn contents(&self, setup: &ExportSetup) -> ExportContents {
        let (_, edited_cues) = self.results();
        ExportContents {
            beat_grid: setup.beat_grid,
            cues: setup.cues && setup.can_write_cues(),
            edited_cues,
        }
    }

    fn assemble(&self, setup: &ExportSetup, layout: PlaylistLayout<'_>) -> AssembledLibrary {
        let (results, _) = self.results();
        assemble_library_with(results, layout, self.contents(setup))
    }

    fn run_m3u8(self, dry_run: bool) -> Result<ExportReport, String> {
        let setup = self.setup.clone();
        let output = absolute_output(&setup)?;
        let track_count = self.export.tracks().len();
        let audio_files: Vec<PathBuf> = self
            .export
            .tracks()
            .iter()
            .map(|track| track.path.clone())
            .collect();
        checks::check_output_file(&output, ExistingFile::Replace)
            .map_err(|error| error.to_string())?;
        checks::check_output_is_not_an_input(&output, &audio_files, &[])
            .map_err(|error| error.to_string())?;
        if dry_run {
            return Ok(ExportReport {
                headline: format!(
                    "{} would be written to {}.",
                    counted(track_count, "track", "tracks"),
                    output.display()
                ),
                details: Vec::new(),
                track_count,
            });
        }
        let message = self.export.write(&output)?;
        Ok(ExportReport {
            headline: message,
            details: Vec::new(),
            track_count,
        })
    }

    fn run_collection_file(
        &self,
        setup: &ExportSetup,
        dry_run: bool,
    ) -> Result<ExportReport, String> {
        let program = setup.program;
        let name = program.label();
        let output = absolute_output(setup)?;
        checks::check_output_file(&output, ExistingFile::Replace)
            .map_err(|error| error.to_string())?;
        let mut protected: Vec<PathBuf> = Vec::new();
        if program == ExportProgram::Traktor {
            protected.extend(traktor_collection::installed_collections());
        }
        let startup_volume = traktor_collection::startup_volume_name();
        let cued_tracks = match &setup.collection {
            Some(collection) if setup.cues => {
                protected.push(collection.clone());
                let cued = if program == ExportProgram::Rekordbox {
                    rekordbox_collection::tracks_with_cues(collection)
                } else {
                    traktor_collection::tracks_with_cues(collection, &startup_volume)
                }
                .map_err(|error| error.to_string())?;
                if !cued.unmatched.is_empty() && !setup.allow_unmatched_cued_tracks {
                    return Err(unmatched_message(&cued, collection, name));
                }
                cued
            }
            _ => CuedTracks::default(),
        };
        if !protected.is_empty() {
            checks::check_output_is_not_a_collection(&output, &protected, name)
                .map_err(|error| error.to_string())?;
        }
        let audio_files: Vec<PathBuf> = self
            .export
            .tracks()
            .iter()
            .map(|track| track.path.clone())
            .collect();
        checks::check_output_is_not_an_input(&output, &audio_files, &[])
            .map_err(|error| error.to_string())?;

        let loose_playlist_name = output
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Euphoria".to_string());
        let layout = if setup.playlists {
            PlaylistLayout::MirrorFolders {
                folders: &self.folders,
                loose_playlist_name: &loose_playlist_name,
            }
        } else {
            PlaylistLayout::CollectionOnly
        };
        let AssembledLibrary {
            mut library,
            skipped,
            unread_tags,
        } = self.assemble(setup, layout);
        let left_out = checks::leave_out(&mut library, &cued_tracks.paths);
        if library.tracks.is_empty() && left_out > 0 {
            return Err(format!(
                "Every track already has cues in your {name} collection, so nothing was \
                 written. Export again without cues to update their key and tempo."
            ));
        }
        if library.tracks.is_empty() {
            return Err(
                "None of the tracks could be exported, so nothing was written.".to_string(),
            );
        }
        if !dry_run {
            write_library(program, &output, &library, setup, &startup_volume)
                .map_err(|error| format!("The export could not be written: {}", error.chain()))?;
        }
        let track_count = library.tracks.len();
        let mut details = vec![format!(
            "{} in the file.",
            counted(track_count, "track", "tracks")
        )];
        if setup.beat_grid {
            let with_grid = library
                .tracks
                .iter()
                .filter(|track| track.first_beat.is_some())
                .count();
            details.push(format!(
                "{with_grid} of {track_count} have a beat grid; grids rated low confidence are left out."
            ));
        }
        if setup.cues && setup.can_write_cues() {
            let with_cues = library
                .tracks
                .iter()
                .filter(|track| !track.cue_points.is_empty())
                .count();
            details.push(format!("{with_cues} of {track_count} have cues."));
            if left_out > 0 {
                details.push(format!(
                    "{} left out: they have cues of their own in your {name} collection.",
                    counted(left_out, "track was", "tracks were")
                ));
            }
        }
        details.extend(skipped_lines(skipped.len(), unread_tags.len()));
        let headline = if dry_run {
            format!(
                "Nothing is written yet. The file would be {}.",
                output.display()
            )
        } else {
            format!(
                "Exported {} to {} for {name}. {}",
                counted(track_count, "track", "tracks"),
                output.display(),
                import_step(program)
            )
        };
        Ok(ExportReport {
            headline,
            details,
            track_count,
        })
    }

    fn run_engine(&self, setup: &ExportSetup, dry_run: bool) -> Result<ExportReport, String> {
        let library_folder = setup
            .target
            .clone()
            .ok_or_else(|| "Choose your Engine Library folder.".to_string())?;
        let database = library_database(&library_folder);
        if !database.is_file() {
            return Err(format!(
                "{} has no Database2/m.db, so it is not an Engine DJ 2 or later library.",
                library_folder.display()
            ));
        }
        let mut details = Vec::new();
        if !dry_run {
            match (self.running.engine)() {
                Some(true) => {
                    return Err(
                        "Engine DJ is running. Writing its library while it is open can \
                         damage it. Close Engine DJ and export again."
                            .to_string(),
                    );
                }
                Some(false) => {}
                None => details.push(
                    "Could not check whether Engine DJ is running; close it before exporting."
                        .to_string(),
                ),
            }
        }
        preview_library_update(&database, &[], &EngineOptions::default())
            .map_err(|error| error.chain())?;

        let AssembledLibrary {
            library,
            skipped,
            unread_tags,
        } = self.assemble(setup, PlaylistLayout::CollectionOnly);
        if library.tracks.is_empty() {
            return Err(
                "None of the tracks could be exported, so nothing was written.".to_string(),
            );
        }
        let energies = self.energies();
        let tracks: Vec<EngineTrack<'_>> = library
            .tracks
            .iter()
            .map(|track| EngineTrack {
                track,
                energy: energies.get(&track.path).copied(),
            })
            .collect();
        let options = EngineOptions {
            label: setup.label.placement(),
            label_notation: setup.key_notation.key_notation(),
        };
        let backup = program_files::engine_backup_path(&library_folder, ModifiedTime::now());
        let update = if dry_run {
            preview_library_update(&database, &tracks, &options)
        } else {
            update_library(&database, &backup, &tracks, &options)
        }
        .map_err(|error| error.chain())?;

        let schema = update
            .schema
            .map(|schema| format!(" (schema {schema})"))
            .unwrap_or_default();
        let updated = counted(update.updated.len(), "track", "tracks");
        let headline = if update.updated.is_empty() {
            format!(
                "No track is in {}, so nothing is written.",
                database.display()
            )
        } else if dry_run {
            format!(
                "Nothing is written yet. {updated} of {}{schema} would be updated. A copy is saved to {} first.",
                database.display(),
                backup.display()
            )
        } else {
            format!(
                "Updated {updated} in {}{schema}. The library as it was is saved in {}. {}",
                database.display(),
                backup.display(),
                import_step(ExportProgram::EngineDj)
            )
        };
        if !update.not_in_library.is_empty() {
            details.extend(listed(
                &update.not_in_library,
                &format!(
                    "{} not in this library, so not updated. Add them to Engine DJ and let it analyse them, then export again:",
                    counted(update.not_in_library.len(), "track is", "tracks are")
                ),
            ));
        }
        if !update.not_analysed.is_empty() {
            details.push(format!(
                "{} Engine DJ has not analysed got no beat grid or cues.",
                counted(update.not_analysed.len(), "track", "tracks")
            ));
        }
        if setup.cues && !update.kept_own_cues.is_empty() {
            details.push(format!(
                "Kept the hot cues of {} that have hot cues in Engine DJ.",
                counted(update.kept_own_cues.len(), "track", "tracks")
            ));
        }
        if setup.cues && update.cues_left_out > 0 {
            details.push(format!(
                "Left out {} on no pad from 1 to 8: Engine DJ has eight hot cues and no memory cues.",
                counted(update.cues_left_out, "cue", "cues")
            ));
        }
        details.extend(skipped_lines(skipped.len(), unread_tags.len()));
        Ok(ExportReport {
            headline,
            details,
            track_count: update.updated.len(),
        })
    }

    fn run_virtualdj(&self, setup: &ExportSetup, dry_run: bool) -> Result<ExportReport, String> {
        let database = setup
            .target
            .clone()
            .ok_or_else(|| "Choose your VirtualDJ database.xml.".to_string())?;
        if !database.is_file() {
            return Err(format!("{} is not a file.", database.display()));
        }
        let mut details = Vec::new();
        if !dry_run {
            match (self.running.virtualdj)() {
                Some(true) => {
                    return Err(
                        "VirtualDJ is running. It saves its database when it closes, \
                         which would undo the export. Close VirtualDJ and export again."
                            .to_string(),
                    );
                }
                Some(false) => {}
                None => details.push(
                    "Could not check whether VirtualDJ is running; close it before exporting."
                        .to_string(),
                ),
            }
        }
        let AssembledLibrary {
            library,
            skipped,
            unread_tags,
        } = self.assemble(setup, PlaylistLayout::CollectionOnly);
        if library.tracks.is_empty() {
            return Err(
                "None of the tracks could be exported, so nothing was written.".to_string(),
            );
        }
        let energies = self.energies();
        let tracks: Vec<VirtualDjTrack<'_>> = library
            .tracks
            .iter()
            .map(|track| VirtualDjTrack {
                track,
                energy: energies.get(&track.path).copied(),
            })
            .collect();
        let options = VirtualDjOptions {
            label: setup.label.placement(),
            label_notation: setup.key_notation.key_notation(),
        };
        let backup = program_files::virtualdj_backup_path(&database, ModifiedTime::now());
        let merge = if dry_run {
            preview_database_update(&database, &tracks, &options)
        } else {
            update_database_file(&database, &backup, &tracks, &options)
        }
        .map_err(|error| error.chain())?;

        let updated = counted(merge.updated.len(), "song", "songs");
        let headline = if merge.updated.is_empty() {
            format!(
                "No track is in {}, so nothing is written.",
                database.display()
            )
        } else if dry_run {
            format!(
                "Nothing is written yet. {updated} of {} would be updated. A copy is saved to {} first.",
                database.display(),
                backup.display()
            )
        } else {
            format!(
                "Updated {updated} in {}. The database as it was is saved to {}. {}",
                database.display(),
                backup.display(),
                import_step(ExportProgram::VirtualDj)
            )
        };
        if !merge.not_in_database.is_empty() {
            details.extend(listed(
                &merge.not_in_database,
                &format!(
                    "{} not in this database, so not updated. Load them in VirtualDJ once, then export again:",
                    counted(merge.not_in_database.len(), "track is", "tracks are")
                ),
            ));
        }
        if setup.cues && !merge.kept_own_cues.is_empty() {
            details.push(format!(
                "Kept the cues of {} that have cues in VirtualDJ.",
                counted(merge.kept_own_cues.len(), "song", "songs")
            ));
        }
        if setup.cues && merge.cues_without_pad > 0 {
            details.push(format!(
                "Left out {} on no hot cue pad: VirtualDJ has no memory cues.",
                counted(merge.cues_without_pad, "cue", "cues")
            ));
        }
        details.extend(skipped_lines(skipped.len(), unread_tags.len()));
        Ok(ExportReport {
            headline,
            details,
            track_count: merge.updated.len(),
        })
    }
}

fn absolute_output(setup: &ExportSetup) -> Result<PathBuf, String> {
    let output = setup
        .output
        .as_ref()
        .ok_or_else(|| "Choose where to save the file.".to_string())?;
    std::path::absolute(output)
        .map_err(|error| format!("Cannot use {} as the output: {error}", output.display()))
}

fn write_library(
    program: ExportProgram,
    output: &Path,
    library: &Library,
    setup: &ExportSetup,
    startup_volume: &str,
) -> Result<(), euphoria_core::EuphoriaError> {
    if program == ExportProgram::Rekordbox {
        write_xml_file(
            output,
            library,
            &ExportOptions {
                key_notation: setup.key_notation.export_notation(),
            },
            ExistingFile::Replace,
        )
    } else {
        write_nml_file(
            output,
            library,
            &TraktorOptions {
                key_notation: setup.key_notation.key_notation(),
                startup_volume: startup_volume.to_string(),
                modified: ModifiedTime::now(),
            },
            ExistingFile::Replace,
        )
    }
}

fn import_step(program: ExportProgram) -> &'static str {
    match program {
        ExportProgram::Rekordbox => "In rekordbox, open the XML view to import it.",
        ExportProgram::Traktor => "In Traktor, import the file with Import Another Collection.",
        ExportProgram::EngineDj => "Open Engine DJ to see the changes.",
        ExportProgram::VirtualDj => "Open VirtualDJ to see the changes.",
        ExportProgram::M3u8 => "",
    }
}

fn skipped_lines(skipped: usize, unread_tags: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if skipped > 0 {
        lines.push(format!(
            "{} could not be exported.",
            counted(skipped, "track", "tracks")
        ));
    }
    if unread_tags > 0 {
        lines.push(format!(
            "The tags of {} could not be read; they are exported under their file names.",
            counted(unread_tags, "track", "tracks")
        ));
    }
    lines
}

fn unmatched_message(cued: &CuedTracks, collection: &Path, name: &str) -> String {
    let mut message = format!(
        "{} with cues in {} match no file on this computer, so Euphoria cannot tell whether \
         they are among the tracks to export and cannot protect their cues:",
        counted(cued.unmatched.len(), "track", "tracks"),
        collection.display()
    );
    for description in cued.unmatched.iter().take(5) {
        message.push_str("\n  ");
        message.push_str(description);
    }
    if cued.unmatched.len() > 5 {
        message.push_str(&format!("\n  and {} more", cued.unmatched.len() - 5));
    }
    message.push_str(&format!(
        "\nNothing was written. Use the collection of {name} on this computer, or tick \
         \"Export even though these tracks cannot be protected\"."
    ));
    message
}

/// The state of the export sheet.
#[derive(Debug)]
pub(crate) struct LibraryExportPanel {
    pub(crate) setup: ExportSetup,
    pub(crate) stage: ExportStage,
    pub(crate) problem: Option<String>,
}

/// Where the sheet is in its steps.
#[derive(Debug)]
pub(crate) enum ExportStage {
    /// Waiting for the choices.
    Setup,
    /// Checking, or writing when the flag is set.
    Working { writing: bool },
    /// What the export would do, waiting for the go ahead.
    Review(ExportReport),
    /// What the export did.
    Done(ExportReport),
}

impl LibraryExportPanel {
    /// True while the work runs, when the sheet cannot be closed.
    pub(crate) fn is_busy(&self) -> bool {
        matches!(self.stage, ExportStage::Working { .. })
    }
}

impl EuphoriaApp {
    /// File > Export library: opens the sheet.
    pub(crate) fn open_library_export(&mut self, cx: &mut Context<'_, Self>) {
        if self
            .library_export
            .as_ref()
            .is_some_and(LibraryExportPanel::is_busy)
        {
            return;
        }
        if self
            .tag_panel
            .as_ref()
            .is_some_and(crate::tag_flow::TagPanel::is_busy)
        {
            return;
        }
        self.settings_open = false;
        self.tag_panel = None;
        let scope = if self.visible.len() < self.session.library().tracks().len() {
            ExportScope::Shown
        } else {
            ExportScope::Library
        };
        self.library_export = Some(LibraryExportPanel {
            setup: ExportSetup::new(ExportProgram::Rekordbox, scope),
            stage: ExportStage::Setup,
            problem: None,
        });
        cx.notify();
    }

    /// Closes the sheet, unless the export is running.
    pub(crate) fn close_library_export(&mut self, cx: &mut Context<'_, Self>) {
        if self
            .library_export
            .as_ref()
            .is_some_and(LibraryExportPanel::is_busy)
        {
            return;
        }
        self.library_export = None;
        cx.notify();
    }

    /// Changes the choices, going back to the setup when a review was on
    /// show, since it no longer describes them.
    pub(crate) fn change_export_setup(
        &mut self,
        change: impl FnOnce(&mut ExportSetup),
        cx: &mut Context<'_, Self>,
    ) {
        let Some(panel) = &mut self.library_export else {
            return;
        };
        if panel.is_busy() {
            return;
        }
        change(&mut panel.setup);
        if !panel.setup.beat_grid {
            panel.setup.cues = false;
        }
        if !panel.setup.can_write_cues() {
            panel.setup.cues = false;
        }
        panel.stage = ExportStage::Setup;
        panel.problem = None;
        cx.notify();
    }

    /// The tracks the scope covers, as a job for the export.
    fn library_export_job(&self, setup: &ExportSetup) -> LibraryExportJob {
        let library = self.session.library();
        let paths: Vec<PathBuf> = match setup.scope {
            ExportScope::Library => library
                .tracks()
                .iter()
                .map(|track| track.path.clone())
                .collect(),
            ExportScope::Shown => self
                .visible
                .iter()
                .filter_map(|index| library.track(*index))
                .map(|track| track.path.clone())
                .collect(),
        };
        let export = self.export_job("Euphoria library", &paths, PlaylistFormat::M3u8, |_| None);
        LibraryExportJob {
            setup: setup.clone(),
            export,
            folders: library.folders().to_vec(),
            running: self.running_checks,
        }
    }

    /// The Review button: checks everything and shows what would be
    /// written, writing nothing.
    pub(crate) fn review_library_export(&mut self, cx: &mut Context<'_, Self>) {
        self.run_library_export(true, cx);
    }

    /// The Export button of the review: writes.
    pub(crate) fn confirm_library_export(&mut self, cx: &mut Context<'_, Self>) {
        if !matches!(
            self.library_export.as_ref().map(|panel| &panel.stage),
            Some(ExportStage::Review(_))
        ) {
            return;
        }
        self.run_library_export(false, cx);
    }

    fn run_library_export(&mut self, dry_run: bool, cx: &mut Context<'_, Self>) {
        let Some(panel) = &mut self.library_export else {
            return;
        };
        if panel.is_busy() || !panel.setup.is_complete() {
            return;
        }
        let setup = panel.setup.clone();
        panel.stage = ExportStage::Working { writing: !dry_run };
        panel.problem = None;
        let job = self.library_export_job(&setup);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { job.run(dry_run) })
                .await;
            this.update(cx, |this, cx| {
                let Some(panel) = &mut this.library_export else {
                    return;
                };
                match outcome {
                    Ok(report) if dry_run => panel.stage = ExportStage::Review(report),
                    Ok(report) => {
                        this.notice = Some(report.as_notice());
                        panel.stage = ExportStage::Done(report);
                    }
                    Err(problem) => {
                        if !dry_run {
                            this.notice = Some(problem.clone());
                        }
                        panel.stage = ExportStage::Setup;
                        panel.problem = Some(problem);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to save the file, then keeps the choice in the sheet.
    pub(crate) fn choose_export_output(&mut self, cx: &mut Context<'_, Self>) {
        let Some(panel) = &self.library_export else {
            return;
        };
        let suggested = panel.setup.suggested_file_name();
        let directory = dirs_home().unwrap_or_else(|| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = chosen.await {
                this.update(cx, |this, cx| {
                    this.change_export_setup(|setup| setup.output = Some(path), cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Asks for the Engine Library folder, the VirtualDJ database or the
    /// collection file, and keeps the choice in the sheet.
    pub(crate) fn choose_export_source(&mut self, collection: bool, cx: &mut Context<'_, Self>) {
        let Some(panel) = &self.library_export else {
            return;
        };
        let wants_folder = !collection && panel.setup.program == ExportProgram::EngineDj;
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: !wants_folder,
            directories: wants_folder,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.change_export_setup(
                        |setup| {
                            if collection {
                                setup.collection = Some(path);
                            } else {
                                setup.target = Some(path);
                            }
                        },
                        cx,
                    )
                })
                .ok();
            }
        })
        .detach();
    }
}
