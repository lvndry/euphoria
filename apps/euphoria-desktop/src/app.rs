// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's root view: it owns the library session and what the user has
//! chosen to see, and lays the screen out from the views.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::component::input::{InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, ExternalPaths, FocusHandle, KeyDownEvent, Modifiers, MouseButton,
    MouseMoveEvent, PathPromptOptions, Pixels, ScrollStrategy, Subscription, Task,
    UniformListScrollHandle, Window, canvas, div, px, relative,
};

use crate::columns::{ColumnId, ColumnWidths, TableLayout, fitted_width, widest_text};
use crate::display_settings::{DisplaySettings, TempoPreset, TempoRangeSetting};
use crate::folder_tree::{FolderColumn, FolderTree, TreeKey, TreeMove};
use crate::library::{LibraryCounts, LibraryFilter, SortColumn, SortOrder, Track};
use crate::mixes::{MixSuggestion, suggestions};
use crate::panels::{PanelEdge, Panels};
use crate::peaks::{PEAK_COUNT, compute_band_peaks};
use crate::playback::{AudioOutput, PlaybackProblem, TrackSource, Voice, VoiceState};
use crate::player::{Player, PlayerRequest, StartAttempt, fraction_along};
use crate::playlists::PlaylistId;
use crate::settings::{FolderTreeMemory, Settings};
use crate::state::{Scan, Session, scan};
use crate::tag_flow::TagPanel;
use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice, TagSettings};
use crate::theme::{AppTheme, Layout, ThemeName};
use crate::track_overview::energy_curve_from;
use crate::views;
use crate::views::waveform::SeenBounds;
use crate::waveforms::{WAVEFORM_CAPACITY, Waveform, WaveformCache};

/// How often finished files are moved from the workers into the table.
const COLLECT_INTERVAL: Duration = Duration::from_millis(80);

/// How long a track must stay selected before its audio is decoded for the
/// waveform, so that moving quickly through the table decodes nothing.
const WAVEFORM_DELAY: Duration = Duration::from_millis(180);

/// How long quitting gives the files in the middle of analysis to finish.
/// The wait is off the main thread, and the app quits when it is over whether
/// or not they finished: an abandoned file is analyzed again next time.
const QUIT_PATIENCE: Duration = Duration::from_millis(150);

/// How many tracks the "Mixes well with" list shows.
const MIX_LIMIT: usize = 8;

/// How often the playhead and the position are brought up to date while a
/// track plays.
const PLAYHEAD_INTERVAL: Duration = Duration::from_millis(33);

/// How often the app looks at a separation running on its thread.
const STEM_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The sound of the track that is playing or paused.
struct ActiveVoice {
    /// Dropping this stops the sound.
    voice: Box<dyn Voice>,
    state: Arc<VoiceState>,
}

/// The root view of the window.
pub(crate) struct EuphoriaApp {
    pub(crate) session: Session,
    pub(crate) theme: AppTheme,
    pub(crate) search_input: Entity<InputState>,
    /// All tracks, or only those needing review, within the folder on show.
    pub(crate) filter: LibraryFilter,
    /// The folder whose tracks the table shows, or `None` for every folder.
    pub(crate) folder: Option<PathBuf>,
    /// The playlist whose tracks the table shows, in its order, in place of
    /// the library.
    pub(crate) playlist: Option<PlaylistId>,
    /// The place in the playlist on show of each track in it, from 1.
    pub(crate) playlist_positions: HashMap<usize, usize>,
    /// True while the table shows the starred tracks, in the order they
    /// were starred, in place of the library.
    pub(crate) showing_favorites: bool,
    /// The marked transition, from one track into another, whose note is
    /// being typed.
    pub(crate) editing_transition_note: Option<(PathBuf, PathBuf)>,
    /// The field the note of a transition is typed in.
    pub(crate) transition_note_input: Entity<InputState>,
    /// The playlist whose name is being typed.
    pub(crate) renaming_playlist: Option<PlaylistId>,
    /// The field the name of a playlist is typed in.
    pub(crate) playlist_name_input: Entity<InputState>,
    /// The folders the user added and the subfolders holding their tracks.
    pub(crate) folder_tree: FolderTree,
    /// Indexes into the folder tree of the folders the sidebar lists.
    pub(crate) folder_rows: Vec<usize>,
    /// The unfolded folders of the tree.
    pub(crate) expanded_folders: HashSet<PathBuf>,
    /// The Folder column of every track, relative to the folder on show.
    pub(crate) folder_column: FolderColumn,
    /// The widest rendered folder path among the rows shown, cached until
    /// those paths or their font change.
    folder_path_widths: views::track_table::FolderPathWidths,
    /// Which columns the table shows and how wide its Folder column is,
    /// worked out each frame from the room the table has.
    pub(crate) table_layout: TableLayout,
    /// The widths the user gave columns, by dragging or fitting them.
    pub(crate) column_widths: ColumnWidths,
    /// The column whose header edge is being dragged.
    pub(crate) column_resize: Option<ColumnResize>,
    /// Where the column headers were last drawn.
    pub(crate) table_header_bounds: SeenBounds,
    /// Library indexes of the tracks selected in the table besides the one
    /// the inspector shows. Empty when only that one is selected.
    pub(crate) marked: HashSet<usize>,
    /// How tags are written.
    pub(crate) tag_settings: TagSettings,
    /// How keys and tempos are shown.
    pub(crate) display: DisplaySettings,
    /// The custom tempo range's slowest and fastest tempos, as typed.
    pub(crate) tempo_from_input: Entity<InputState>,
    pub(crate) tempo_to_input: Entity<InputState>,
    /// True when the settings show the custom tempo range, chosen or
    /// remembered, rather than a preset.
    pub(crate) is_custom_tempo_range: bool,
    /// Why the custom tempo range typed is not used, when it is not.
    pub(crate) tempo_range_problem: Option<String>,
    /// The steps of a tag write or undo, while one is on screen.
    pub(crate) tag_panel: Option<TagPanel>,
    /// True while the settings panel is open.
    pub(crate) settings_open: bool,
    /// Where the backups of tag writes are kept, shared with the command
    /// line so that either can undo the other's write.
    pub(crate) write_log_directory: Option<PathBuf>,
    /// Counts tag tasks, so a result for a panel already closed is dropped.
    pub(crate) tag_task_generation: u64,
    pub(crate) folder_scroll: UniformListScrollHandle,
    pub(crate) tree_focus: FocusHandle,
    pub(crate) sort_order: Option<SortOrder>,
    /// Indexes into the library of the rows the table shows, in order.
    pub(crate) visible: Vec<usize>,
    /// Index into the library of the selected track.
    pub(crate) selected: Option<usize>,
    /// Tracks that mix with the selected one, smoothest first, in its key
    /// shifted by `key_shift`.
    pub(crate) mixes: Vec<MixSuggestion>,
    /// Semitones the selected track is heard and suggested in, away from its
    /// own key. Back to 0 when another track is selected.
    pub(crate) key_shift: i8,
    pub(crate) table_scroll: UniformListScrollHandle,
    pub(crate) table_focus: FocusHandle,
    /// A problem to tell the user about, shown until dismissed.
    pub(crate) notice: Option<String>,
    /// Folders being searched for audio files right now.
    pub(crate) scans_in_progress: usize,
    /// Remembered folders that are not there now, such as one on a drive
    /// that is not connected. They stay remembered, to be looked for again
    /// at the next launch.
    pub(crate) unavailable_folders: Vec<PathBuf>,
    /// True between a press on the toolbar and the drag or release after it.
    pub(crate) toolbar_drag_armed: bool,
    /// Whether the sidebar is folded and how large the track detail is.
    pub(crate) panels: Panels,
    /// The edge being dragged, between the press on it and the release.
    pub(crate) resizing: Option<PanelEdge>,
    /// Where the table and the track detail were last drawn, together.
    pub(crate) detail_area_bounds: SeenBounds,
    /// What the player is doing with the selected track.
    pub(crate) player: Player,
    /// Where the waveform was last drawn, for turning a click into a
    /// position in the track.
    pub(crate) waveform_bounds: SeenBounds,
    /// Where the progress line of the player bar was last drawn.
    pub(crate) progress_line_bounds: SeenBounds,
    /// Holds the keyboard when no field or table has it, so that the space
    /// bar still reaches the player.
    pub(crate) window_focus: FocusHandle,
    /// Where tracks and reference sounds are played.
    pub(crate) output: Box<dyn AudioOutput>,
    /// The reference sounds and the correction editor of the track detail.
    pub(crate) by_ear: crate::key_by_ear::ByEar,
    /// Separating tracks into stems, and what to hear of the separated ones.
    pub(crate) stems: crate::stems::StemSession,
    /// The two decks, their mixer and the tracks to play next.
    pub(crate) decks: crate::decks::Decks,
    /// The curve, pool and set of the set builder page.
    pub(crate) set_builder: crate::set_builder::SetBuilder,
    /// The tempo the user types to correct the selected track's.
    pub(crate) bpm_input: Entity<InputState>,
    /// Moving, adding, renaming and deleting the selected track's cues.
    pub(crate) cue_editor: crate::cue_editing::CueEditor,
    /// The field the name of a cue is typed in.
    pub(crate) cue_name_input: Entity<InputState>,
    voice: Option<ActiveVoice>,
    /// Opens the file of the track about to play.
    start_task: Option<Task<()>>,
    is_following_playhead: bool,
    pub(crate) query: String,
    settings_path: Option<PathBuf>,
    waveforms: WaveformCache,
    waveform_task: Option<Task<()>>,
    /// The file the waveform task is decoding.
    waveform_task_path: Option<PathBuf>,
    is_collecting: bool,
    /// The library's structure revision the folder tree was built from,
    /// and the revision its failure counts were last brought up to date at.
    folder_tree_revisions: Option<(u64, u64)>,
    /// What the Folder column was worked out for: how many tracks and
    /// folders the library had, and the folder on show.
    folder_column_basis: Option<(usize, usize, Option<PathBuf>)>,
    _subscriptions: Vec<Subscription>,
}

impl EuphoriaApp {
    /// Builds the view, applies the remembered theme and starts analyzing
    /// the remembered folders.
    pub(crate) fn new(
        session: Session,
        settings: Settings,
        settings_path: Option<PathBuf>,
        notice: Option<String>,
        output: Box<dyn AudioOutput>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let window_focus = cx.focus_handle();
        window_focus.focus(window, cx);
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search file name, key or BPM"));
        let search_changed = cx.subscribe(
            &search_input,
            |this: &mut Self, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.query = input.read(cx).value().to_string();
                    this.refresh_visible();
                    cx.notify();
                }
            },
        );
        let quitting = cx.on_app_quit(|this: &mut Self, cx| {
            let stop_signal = this.session.stop_signal();
            stop_signal.skip_queued_files();
            cx.background_executor().spawn(async move {
                stop_signal.wait_for_files_in_progress(QUIT_PATIENCE);
            })
        });

        let display = settings.display;
        let typed_range = |text: u16, window: &mut Window, cx: &mut Context<'_, Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(text.to_string()))
        };
        let tempo_from_input = typed_range(display.tempo_range.slowest, window, cx);
        let tempo_to_input = typed_range(display.tempo_range.fastest, window, cx);
        let range_typed = |this: &mut Self,
                           _: Entity<InputState>,
                           event: &InputEvent,
                           cx: &mut Context<'_, Self>| {
            if matches!(event, InputEvent::Change) {
                this.apply_typed_tempo_range(cx);
            }
        };
        let playlist_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Playlist name"));
        let playlist_named = cx.subscribe(
            &playlist_name_input,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.finish_renaming_playlist(cx);
                }
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        let transition_note_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("How this transition goes, where to mix in")
        });
        let transition_noted = cx.subscribe(
            &transition_note_input,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.finish_editing_transition_note(cx);
                }
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        let bpm_input = cx.new(|cx| InputState::new(window, cx).placeholder("BPM"));
        let bpm_entered = cx.subscribe(&bpm_input, |this: &mut Self, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.accept_typed_bpm(cx);
            }
        });
        let cue_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Cue name"));
        let cue_named = cx.subscribe(
            &cue_name_input,
            |this: &mut Self, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    this.finish_renaming_cue(cx);
                }
                InputEvent::Change | InputEvent::Focus => {}
            },
        );
        let tempo_from_changed = cx.subscribe(&tempo_from_input, range_typed);
        let tempo_to_changed = cx.subscribe(&tempo_to_input, range_typed);
        let mut session = session;
        session.set_tempo_range(display.tempo_range.range_or_default());

        let theme = AppTheme::named(settings.theme);
        theme.apply_to_components(cx);
        let expanded_folders = settings
            .folder_tree
            .expanded
            .clone()
            .unwrap_or_else(|| settings.folders.clone())
            .into_iter()
            .collect();
        let mut this = Self {
            session,
            theme,
            search_input,
            filter: LibraryFilter::AllTracks,
            folder: settings.folder_tree.selected.clone(),
            playlist: None,
            showing_favorites: false,
            editing_transition_note: None,
            transition_note_input,
            playlist_positions: HashMap::new(),
            renaming_playlist: None,
            playlist_name_input,
            folder_tree: FolderTree::default(),
            folder_rows: Vec::new(),
            expanded_folders,
            folder_column: FolderColumn::default(),
            folder_path_widths: views::track_table::FolderPathWidths::default(),
            table_layout: TableLayout::default(),
            column_widths: settings.column_widths.clone(),
            column_resize: None,
            table_header_bounds: Rc::new(Cell::new(None)),
            marked: HashSet::new(),
            tag_settings: settings.tags,
            display,
            tempo_from_input,
            tempo_to_input,
            is_custom_tempo_range: display.tempo_range.preset().is_none(),
            tempo_range_problem: None,
            tag_panel: None,
            settings_open: false,
            write_log_directory: if cfg!(test) {
                None
            } else {
                euphoria_core::tags::default_write_log_directory().ok()
            },
            tag_task_generation: 0,
            folder_scroll: UniformListScrollHandle::new(),
            tree_focus: cx.focus_handle(),
            sort_order: theme.opens_sorted_by_key.then_some(SortOrder {
                column: SortColumn::Key,
                descending: false,
            }),
            visible: Vec::new(),
            selected: None,
            mixes: Vec::new(),
            key_shift: 0,
            table_scroll: UniformListScrollHandle::new(),
            table_focus: cx.focus_handle(),
            notice,
            scans_in_progress: 0,
            unavailable_folders: Vec::new(),
            toolbar_drag_armed: false,
            panels: settings.panels,
            resizing: None,
            detail_area_bounds: Rc::new(Cell::new(None)),
            player: Player::default(),
            waveform_bounds: Rc::new(Cell::new(None)),
            progress_line_bounds: Rc::new(Cell::new(None)),
            window_focus,
            output,
            by_ear: crate::key_by_ear::ByEar::default(),
            stems: crate::stems::StemSession::new(crate::stems::model_separation),
            decks: crate::decks::Decks::default(),
            set_builder: crate::set_builder::SetBuilder::default(),
            bpm_input,
            cue_editor: crate::cue_editing::CueEditor::default(),
            cue_name_input,
            voice: None,
            start_task: None,
            is_following_playhead: false,
            query: String::new(),
            settings_path,
            waveforms: WaveformCache::new(WAVEFORM_CAPACITY),
            waveform_task: None,
            waveform_task_path: None,
            is_collecting: false,
            folder_tree_revisions: None,
            folder_column_basis: None,
            _subscriptions: vec![
                playlist_named,
                transition_noted,
                bpm_entered,
                cue_named,
                search_changed,
                quitting,
                tempo_from_changed,
                tempo_to_changed,
            ],
        };
        if !settings.folders.is_empty() {
            this.add_paths(settings.folders, false, cx);
        }
        this.refresh_visible();
        if this.scans_in_progress == 0 {
            this.forget_folder_if_gone();
        }
        this.select_first_track_if_none(cx);
        this
    }

    /// The selected track, when there is one.
    pub(crate) fn selected_track(&self) -> Option<&Track> {
        self.selected
            .and_then(|index| self.session.library().track(index))
    }

    /// The waveform of the selected track, once it has been asked for.
    pub(crate) fn selected_waveform(&self) -> Option<&Waveform> {
        self.selected_track()
            .and_then(|track| self.waveforms.get(&track.path))
    }

    /// Keeps `waveform` for the selected track, as if it had been decoded.
    #[cfg(test)]
    pub(crate) fn show_waveform(&mut self, waveform: Waveform) {
        if let Some(path) = self.selected_track().map(|track| track.path.clone()) {
            self.waveforms.insert(path, waveform);
        }
    }

    /// True when there is nothing to show yet and nothing on its way.
    pub(crate) fn is_empty(&self) -> bool {
        self.session.library().tracks().is_empty() && self.scans_in_progress == 0
    }

    /// Opens the native folder picker and adds what the user chooses.
    pub(crate) fn choose_folder(&mut self, cx: &mut Context<'_, Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Choose folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let outcome = match chosen.await {
                Ok(Ok(Some(paths))) => Ok(paths),
                Ok(Ok(None)) | Err(_) => Ok(Vec::new()),
                Ok(Err(error)) => Err(format!("Could not open the folder picker: {error:#}")),
            };
            this.update(cx, |this, cx| match outcome {
                Ok(paths) if paths.is_empty() => {}
                Ok(paths) => this.add_paths(paths, true, cx),
                Err(problem) => {
                    this.notice = Some(problem);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Searches files and folders for audio in the background, then adds the
    /// tracks: those the cache knows filled in, the rest as pending rows
    /// queued for analysis. With `unfold_new_folders`, folders the library
    /// did not have yet open unfolded in the sidebar.
    pub(crate) fn add_paths(
        &mut self,
        paths: Vec<PathBuf>,
        unfold_new_folders: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.scans_in_progress += 1;
        cx.notify();
        let cache = self.session.cache();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { scan(&paths, cache.as_deref()) })
                .await;
            this.update(cx, |this, cx| {
                this.finish_scan(found, unfold_new_folders, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_scan(
        &mut self,
        mut found: Scan,
        unfold_new_folders: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.scans_in_progress = self.scans_in_progress.saturating_sub(1);
        let folder_count_before = self.session.library().folders().len();
        if unfold_new_folders {
            let library = self.session.library();
            let new_folders: Vec<PathBuf> = found
                .folders
                .iter()
                .filter(|folder| !library.folders().contains(folder))
                .cloned()
                .collect();
            self.expanded_folders.extend(new_folders);
        }

        let mut problems: Vec<String> = std::mem::take(&mut found.problems)
            .into_iter()
            .map(|problem| format!("Could not add a folder: {problem}"))
            .collect();
        if problems.is_empty() && found.files.is_empty() {
            problems.push("No audio files found there.".to_string());
        }
        for folder in std::mem::take(&mut found.unavailable_folders) {
            if !self.unavailable_folders.contains(&folder) {
                self.unavailable_folders.push(folder);
            }
        }
        let found_files = found.files.clone();
        self.session.add(found);
        let library_folders = self.session.library().folders();
        self.unavailable_folders
            .retain(|folder| !library_folders.contains(folder));
        self.read_track_names(found_files, cx);
        self.refresh_visible();
        if self.scans_in_progress == 0 {
            self.forget_folder_if_gone();
        }
        if self.session.library().folders().len() != folder_count_before {
            problems.extend(self.save_settings());
        }
        if !problems.is_empty() {
            self.report(problems.join(" "));
        }

        self.refresh_visible();
        self.refresh_mixes();
        self.select_first_track_if_none(cx);
        self.start_collecting(cx);
        cx.notify();
    }

    /// Shows a problem in the notice strip, after any that is already there
    /// so that an earlier one is not lost.
    pub(crate) fn report(&mut self, problem: String) {
        self.notice = Some(match self.notice.take() {
            Some(earlier) => format!("{earlier} {problem}"),
            None => problem,
        });
    }

    /// Selects the first row when nothing is selected, so the inspector has
    /// a track to show as soon as the library has one.
    fn select_first_track_if_none(&mut self, cx: &mut Context<'_, Self>) {
        if self.selected.is_none()
            && let Some(first) = self.visible.first().copied()
        {
            self.select(first, cx);
        }
    }

    /// Moves finished files into the table a few times a second for as long
    /// as analysis is running.
    fn start_collecting(&mut self, cx: &mut Context<'_, Self>) {
        if self.is_collecting || !self.session.progress().is_active() {
            return;
        }
        self.is_collecting = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(COLLECT_INTERVAL).await;
                let still_analyzing = this.update(cx, |this, cx| {
                    if this.session.collect_finished() > 0 {
                        this.refresh_visible();
                        this.refresh_mixes();
                        this.load_selected_waveform(cx);
                        this.point_player_at_selection(cx);
                        if let Some(problem) = this.session.take_cache_problem() {
                            this.report(format!(
                                "Results are not being saved to the cache: {problem}."
                            ));
                        }
                        cx.notify();
                    }
                    let is_active = this.session.progress().is_active();
                    this.is_collecting = is_active;
                    is_active
                });
                if !matches!(still_analyzing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Recomputes which rows the table shows after the library, the search,
    /// the filter, the folder on show or the sort order changed, and the
    /// folder tree and the Folder column when they depend on what changed.
    pub(crate) fn refresh_visible(&mut self) {
        let library = self.session.library();
        let revisions = (library.structure_revision(), library.revision());
        match self.folder_tree_revisions {
            Some(built) if built == revisions => {}
            Some((structure, _)) if structure == revisions.0 => {
                self.folder_tree.recount_reviews(library.tracks());
            }
            _ => {
                self.folder_tree = FolderTree::build(library.folders(), library.tracks());
                self.folder_rows = self.folder_tree.visible_rows(&self.expanded_folders);
            }
        }
        self.folder_tree_revisions = Some(revisions);
        let basis = (
            library.tracks().len(),
            library.folders().len(),
            self.folder.clone(),
        );
        if self.folder_column_basis.as_ref() != Some(&basis) {
            self.folder_column =
                FolderColumn::new(library.tracks(), library.folders(), self.folder.as_deref());
            self.folder_column_basis = Some(basis);
        }
        let playlist = self
            .playlist
            .and_then(|id| self.session.playlists().get(id));
        if self.playlist.is_some() && playlist.is_none() {
            self.playlist = None;
        }
        self.playlist_positions = playlist
            .map(|playlist| {
                // Numbered before the tracks that are not in the library are
                // left out, so each keeps its place in the saved order.
                playlist
                    .tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(position, path)| {
                        library.index_of(path).map(|index| (index, position + 1))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if playlist.is_some() {
            self.showing_favorites = false;
        }
        let favorites = self
            .showing_favorites
            .then(|| self.session.favorites().tracks());
        self.visible = match playlist
            .map(|playlist| playlist.tracks.as_slice())
            .or(favorites)
        {
            Some(paths) => {
                library.playlist_rows(paths, &self.query, self.sort_order, &self.folder_column)
            }
            None => library.visible_tracks(
                self.filter,
                self.folder.as_deref(),
                &self.query,
                self.sort_order,
                &self.folder_column,
            ),
        };
        let visible: HashSet<usize> = self.visible.iter().copied().collect();
        self.marked.retain(|index| visible.contains(index));
        self.folder_path_widths.set_paths(
            self.visible
                .iter()
                .filter_map(|index| self.folder_column.label(*index))
                .map(|label| label.text.as_str()),
        );
        self.refresh_deck_metadata();
        self.refresh_suggestions();
    }

    /// Works out which columns fit the table in the window as it is now,
    /// and how wide to draw the Folder column: wide enough for the longest
    /// path shown when there is room.
    fn lay_out_table(&mut self, window: &Window) {
        let mut table_width = window.viewport_size().width / px(1.0) - self.panels.sidebar_width();
        if self.theme.layout == Layout::InspectorRight {
            table_width -= self.inspector_width(window) / px(1.0);
        }
        let style = window.text_style();
        let longest_path = self.folder_path_widths.widest(style.font(), |path| {
            let text = path.to_string().into();
            let run = style.to_run(path.len());
            window
                .text_system()
                .shape_line(text, views::TEXT_SIZE, &[run], None)
                .width
                / px(1.0)
        });
        self.table_layout = TableLayout::new(table_width, &self.column_widths, longest_path);
    }

    /// Reads the title and artist of `paths` from their tags, a batch at a
    /// time off the main thread, and shows them as they come. A file whose
    /// tags cannot be read keeps its file name as its title.
    pub(crate) fn read_track_names(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        if paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            for batch in paths.chunks(NAME_BATCH) {
                let batch = batch.to_vec();
                let names = cx
                    .background_executor()
                    .spawn(async move {
                        batch
                            .into_iter()
                            .filter_map(|path| {
                                let name = euphoria_core::read_track_name(&path).ok()?;
                                Some((path, name))
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;
                let applied = this.update(cx, |this, cx| {
                    this.session.set_track_names(names);
                    this.refresh_visible();
                    cx.notify();
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// True when the track at `index` is selected in the table.
    pub(crate) fn is_marked(&self, index: usize) -> bool {
        if self.marked.is_empty() {
            self.selected == Some(index)
        } else {
            self.marked.contains(&index)
        }
    }

    /// A click on the row of the track at `index`: on its own it selects
    /// that track; with Command (Control elsewhere) it adds the track to the
    /// selection or takes it out; with Shift it selects every row from the
    /// selected one to it.
    pub(crate) fn click_track(
        &mut self,
        index: usize,
        modifiers: Modifiers,
        cx: &mut Context<'_, Self>,
    ) {
        if modifiers.shift
            && let Some(anchor) = self.selected
            && let (Some(from), Some(to)) = (
                self.visible.iter().position(|row| *row == anchor),
                self.visible.iter().position(|row| *row == index),
            )
        {
            let (first, last) = (from.min(to), from.max(to));
            self.marked = self.visible[first..=last].iter().copied().collect();
            cx.notify();
            return;
        }
        if modifiers.secondary(cx) {
            let mut marked = if self.marked.is_empty() {
                self.selected.into_iter().collect()
            } else {
                std::mem::take(&mut self.marked)
            };
            let was_marked = !marked.insert(index);
            if was_marked && marked.len() > 1 {
                marked.remove(&index);
                let next = self
                    .visible
                    .iter()
                    .copied()
                    .find(|row| marked.contains(row));
                if let Some(next) = next.filter(|_| self.selected == Some(index)) {
                    self.select(next, cx);
                }
            } else {
                self.select(index, cx);
            }
            self.marked = marked;
            cx.notify();
            return;
        }
        self.select(index, cx);
    }

    /// Selects every track the table shows.
    pub(crate) fn select_all_shown(&mut self, cx: &mut Context<'_, Self>) {
        self.marked = self.visible.iter().copied().collect();
        if self
            .selected
            .is_none_or(|selected| !self.marked.contains(&selected))
            && let Some(first) = self.visible.first().copied()
        {
            let marked = std::mem::take(&mut self.marked);
            self.select(first, cx);
            self.marked = marked;
        }
        cx.notify();
    }

    /// A press on the edge after `column` starts resizing it. The columns
    /// before it keep their widths while it moves, as in a macOS table.
    pub(crate) fn start_column_resize(
        &mut self,
        column: ColumnId,
        pointer_x: Pixels,
        cx: &mut Context<'_, Self>,
    ) {
        let (Some(left), Some(width), Some(header)) = (
            self.table_layout.left_of(column),
            self.table_layout
                .columns()
                .iter()
                .find(|(shown, _)| *shown == column)
                .map(|(_, width)| *width),
            self.table_header_bounds.get(),
        ) else {
            return;
        };
        let grabbed_at = (pointer_x - header.left()) / px(1.0) - left - width;
        for (before, width) in self.table_layout.columns().to_vec() {
            if before == column {
                break;
            }
            if self.column_widths.get(before).is_none() {
                self.column_widths.set(before, width);
            }
        }
        self.column_resize = Some(ColumnResize {
            column,
            left: left + grabbed_at,
        });
        cx.stop_propagation();
    }

    /// The pointer moved to `pointer_x` while a column edge is held.
    fn resize_column_to(&mut self, pointer_x: Pixels, cx: &mut Context<'_, Self>) {
        let (Some(resize), Some(header)) = (self.column_resize, self.table_header_bounds.get())
        else {
            return;
        };
        let width = (pointer_x - header.left()) / px(1.0) - resize.left;
        self.column_widths.set(resize.column, width);
        cx.notify();
    }

    /// A double click on the edge after `column` fits it to its widest
    /// contents among the rows shown, measured as they are drawn.
    pub(crate) fn fit_column(
        &mut self,
        column: ColumnId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.column_resize = None;
        let contents = self.column_texts(column);
        let content_width =
            widest_text(contents, |text| text_width(window, text, views::TEXT_SIZE)) + FIT_SLACK;
        let header_width =
            text_width(window, column.label(), views::LABEL_TEXT_SIZE) + SORT_ARROW_WIDTH;
        self.column_widths
            .set(column, fitted_width(column, content_width, header_width));
        self.remember_column_widths(cx);
    }

    /// The distinct texts `column` shows among the rows on show. Every one
    /// is kept, since in a proportional font a text with fewer characters
    /// can be the wider, but a text is copied only the first time it is
    /// met, and the row number only for the last row, which has the most
    /// digits.
    pub(crate) fn column_texts(&self, column: ColumnId) -> HashSet<String> {
        let library = self.session.library();
        let mut texts: HashSet<String> = HashSet::new();
        for (row, index) in self.visible.iter().enumerate() {
            let Some(track) = library.track(*index) else {
                continue;
            };
            let text: Option<Cow<'_, str>> = match column {
                ColumnId::Number => (row + 1 == self.visible.len())
                    .then(|| Cow::Owned(crate::format::count(row + 1))),
                ColumnId::Title => Some(Cow::Borrowed(track.title())),
                ColumnId::Artist => track.artist.as_deref().map(Cow::Borrowed),
                ColumnId::Key => track
                    .key()
                    .map(|key| Cow::Owned(self.display.key_notation.spell(&key))),
                ColumnId::MusicalKey => track.key().map(|key| Cow::Owned(key.to_string())),
                ColumnId::Bpm => track.bpm().map(|bpm| Cow::Owned(crate::format::bpm(bpm))),
                ColumnId::Energy => track.energy().map(|_| Cow::Borrowed(WIDEST_ENERGY_TEXT)),
                ColumnId::Quality => track
                    .quality()
                    .map(|report| Cow::Borrowed(crate::format::quality_verdict(report.verdict))),
                ColumnId::Time => track
                    .duration_seconds()
                    .map(|seconds| Cow::Owned(crate::format::duration(seconds))),
                ColumnId::Format => Some(Cow::Borrowed(track.format.as_str())),
                ColumnId::Folder => self
                    .folder_column
                    .label(*index)
                    .map(|label| Cow::Borrowed(label.text.as_str())),
            };
            if let Some(text) = text
                && !texts.contains(text.as_ref())
            {
                texts.insert(text.into_owned());
            }
        }
        texts
    }

    /// Reset column widths: every column back to its usual width.
    pub(crate) fn reset_column_widths(&mut self, cx: &mut Context<'_, Self>) {
        self.column_widths.reset();
        self.remember_column_widths(cx);
    }

    fn remember_column_widths(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(problem) = self.save_settings() {
            self.report(problem);
        }
        cx.notify();
    }

    /// Escape: closes the settings or the tag panel, unless files are being
    /// changed. Returns true when a panel was open.
    pub(crate) fn close_panels(&mut self, cx: &mut Context<'_, Self>) -> bool {
        if self.settings_open {
            self.close_settings(cx);
            return true;
        }
        if self.tag_panel.is_some() {
            self.close_tag_panel(cx);
            return true;
        }
        false
    }

    /// Chooses how keys are shown in the table and the track detail.
    pub(crate) fn set_key_display(&mut self, notation: NotationChoice, cx: &mut Context<'_, Self>) {
        self.display.key_notation = notation;
        self.remember_tag_settings(cx);
    }

    /// Shows tempos in the range of `preset`.
    pub(crate) fn set_tempo_preset(
        &mut self,
        preset: TempoPreset,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.is_custom_tempo_range = false;
        self.tempo_range_problem = None;
        self.use_tempo_range(preset.setting(), cx);
        self.show_tempo_range_in_inputs(window, cx);
    }

    /// Shows the fields of a custom tempo range, starting from the range in
    /// use, and uses what is typed in them.
    pub(crate) fn choose_custom_tempo_range(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.is_custom_tempo_range = true;
        self.tempo_range_problem = None;
        self.show_tempo_range_in_inputs(window, cx);
        cx.notify();
    }

    /// Writes the range in use into both custom fields, so that editing one
    /// of them starts from the range on show. Setting a field's text this
    /// way sends no change, so nothing is applied half updated.
    fn show_tempo_range_in_inputs(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let range = self.display.tempo_range;
        self.tempo_from_input.update(cx, |input, cx| {
            input.set_value(range.slowest.to_string(), window, cx);
        });
        self.tempo_to_input.update(cx, |input, cx| {
            input.set_value(range.fastest.to_string(), window, cx);
        });
    }

    /// Uses the custom range typed, or says why it cannot be used.
    fn apply_typed_tempo_range(&mut self, cx: &mut Context<'_, Self>) {
        let typed = |input: &Entity<InputState>, cx: &mut Context<'_, Self>| {
            input.read(cx).value().trim().parse::<u16>().ok()
        };
        let (Some(slowest), Some(fastest)) = (
            typed(&self.tempo_from_input, cx),
            typed(&self.tempo_to_input, cx),
        ) else {
            self.tempo_range_problem = Some("Type both tempos as whole numbers.".to_string());
            cx.notify();
            return;
        };
        let setting = TempoRangeSetting { slowest, fastest };
        match setting.range() {
            Ok(_) => {
                self.tempo_range_problem = None;
                self.use_tempo_range(setting, cx);
            }
            Err(reason) => {
                self.tempo_range_problem = Some(reason);
                cx.notify();
            }
        }
    }

    /// Shows every tempo in `setting`'s range and remembers it. Nothing is
    /// analyzed again.
    fn use_tempo_range(&mut self, setting: TempoRangeSetting, cx: &mut Context<'_, Self>) {
        let Ok(range) = setting.range() else {
            return;
        };
        self.display.tempo_range = setting;
        self.session.set_tempo_range(range);
        self.refresh_visible();
        self.refresh_mixes();
        self.remember_tag_settings(cx);
    }

    /// Opens the settings panel.
    pub(crate) fn open_settings(&mut self, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        self.tag_task_generation += 1;
        self.tag_panel = None;
        self.settings_open = true;
        cx.notify();
    }

    /// Closes the settings panel.
    pub(crate) fn close_settings(&mut self, cx: &mut Context<'_, Self>) {
        self.settings_open = false;
        cx.notify();
    }

    /// Chooses the DJ program tags are written for.
    pub(crate) fn set_tag_program(&mut self, program: ProgramChoice, cx: &mut Context<'_, Self>) {
        self.tag_settings.choose_program(program);
        self.remember_tag_settings(cx);
    }

    /// Chooses how keys are spelled in tags.
    pub(crate) fn set_key_notation(
        &mut self,
        notation: NotationChoice,
        cx: &mut Context<'_, Self>,
    ) {
        self.tag_settings.key_notation = notation;
        self.remember_tag_settings(cx);
    }

    /// Chooses where the key and energy label goes.
    pub(crate) fn set_label_place(&mut self, label: LabelChoice, cx: &mut Context<'_, Self>) {
        self.tag_settings.label = label;
        self.remember_tag_settings(cx);
    }

    /// Ticks or unticks writing the tempo.
    pub(crate) fn toggle_write_tempo(&mut self, cx: &mut Context<'_, Self>) {
        self.tag_settings.write_tempo = !self.tag_settings.write_tempo;
        self.remember_tag_settings(cx);
    }

    fn remember_tag_settings(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(problem) = self.save_settings() {
            self.report(problem);
        }
        cx.notify();
    }

    /// The counts beside All tracks and Needs review. All tracks always
    /// counts the whole library; Needs review counts within the folder on
    /// show, since that is what it shows.
    pub(crate) fn sidebar_counts(&self) -> LibraryCounts {
        let counts = self.session.library().counts();
        let failed_in_folder = self
            .folder
            .as_ref()
            .and_then(|folder| self.folder_tree.index_of(folder))
            .and_then(|index| self.folder_tree.node(index))
            .map(|node| node.review_count);
        LibraryCounts {
            total: counts.total,
            needs_review: failed_in_folder.unwrap_or(counts.needs_review),
            quality_to_check: self
                .session
                .library()
                .quality_to_check_in(self.folder.as_deref()),
            missing: counts.missing,
        }
    }

    /// A remembered folder that is no longer in the library is not shown.
    fn forget_folder_if_gone(&mut self) {
        let is_gone = self
            .folder
            .as_ref()
            .is_some_and(|folder| self.folder_tree.index_of(folder).is_none());
        if is_gone {
            self.folder = None;
            self.refresh_visible();
        }
    }

    /// Shows every track of every folder.
    pub(crate) fn show_all_tracks(&mut self, cx: &mut Context<'_, Self>) {
        self.close_set_builder();
        self.playlist = None;
        self.showing_favorites = false;
        self.filter = LibraryFilter::AllTracks;
        self.folder = None;
        self.refresh_visible();
        self.remember_folder_tree();
        cx.notify();
    }

    /// Shows only the tracks that need review, within the folder on show.
    pub(crate) fn show_tracks_needing_review(&mut self, cx: &mut Context<'_, Self>) {
        self.close_set_builder();
        self.playlist = None;
        self.showing_favorites = false;
        self.filter = LibraryFilter::NeedsReview;
        self.refresh_visible();
        cx.notify();
    }

    /// Shows only the tracks whose quality is weak on a big system or likely
    /// re-encoded, within the folder on show.
    pub(crate) fn show_tracks_with_quality_to_check(&mut self, cx: &mut Context<'_, Self>) {
        self.close_set_builder();
        self.playlist = None;
        self.showing_favorites = false;
        self.filter = LibraryFilter::QualityToCheck;
        self.refresh_visible();
        cx.notify();
    }

    /// Shows every track in the folder at `path` and the folders inside it.
    pub(crate) fn show_folder(&mut self, path: PathBuf, cx: &mut Context<'_, Self>) {
        self.close_set_builder();
        self.playlist = None;
        self.showing_favorites = false;
        self.filter = LibraryFilter::AllTracks;
        self.folder = Some(path);
        self.refresh_visible();
        self.reveal_folder_row();
        self.remember_folder_tree();
        cx.notify();
    }

    /// Unfolds the folder at `path`, or folds it.
    pub(crate) fn toggle_folder(&mut self, path: PathBuf, cx: &mut Context<'_, Self>) {
        if !self.expanded_folders.remove(&path) {
            self.expanded_folders.insert(path);
        }
        self.folder_rows = self.folder_tree.visible_rows(&self.expanded_folders);
        self.remember_folder_tree();
        cx.notify();
    }

    /// Handles the keys that move through the folder tree.
    pub(crate) fn tree_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<'_, Self>) {
        let key = match event.keystroke.key.as_str() {
            "up" => TreeKey::Up,
            "down" => TreeKey::Down,
            "left" => TreeKey::Left,
            "right" => TreeKey::Right,
            "home" => TreeKey::First,
            "end" => TreeKey::Last,
            _ => return,
        };
        cx.stop_propagation();
        let current = self
            .folder
            .as_ref()
            .and_then(|folder| self.folder_tree.index_of(folder));
        let chosen =
            self.folder_tree
                .navigate(&self.folder_rows, &self.expanded_folders, current, key);
        let path_at =
            |tree: &FolderTree, index: usize| tree.node(index).map(|node| node.path.clone());
        match chosen {
            None => {}
            Some(TreeMove::Select(index)) => {
                if let Some(path) = path_at(&self.folder_tree, index) {
                    self.show_folder(path, cx);
                }
            }
            Some(TreeMove::Expand(index) | TreeMove::Collapse(index)) => {
                if let Some(path) = path_at(&self.folder_tree, index) {
                    self.toggle_folder(path, cx);
                }
            }
        }
    }

    /// Scrolls the sidebar to the folder on show.
    fn reveal_folder_row(&mut self) {
        let row = self
            .folder
            .as_ref()
            .and_then(|folder| self.folder_tree.index_of(folder))
            .and_then(|index| self.folder_rows.iter().position(|row| *row == index));
        if let Some(row) = row {
            self.folder_scroll
                .scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    fn remember_folder_tree(&mut self) {
        if let Some(problem) = self.save_settings() {
            self.report(problem);
        }
    }

    /// Sorts by `column`, or reverses the order when it is already the
    /// sorted column.
    pub(crate) fn sort_by(&mut self, column: SortColumn, cx: &mut Context<'_, Self>) {
        self.sort_order = Some(SortOrder::after_click(self.sort_order, column));
        self.refresh_visible();
        cx.notify();
    }

    /// Selects the track at `index` in the library.
    pub(crate) fn select(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if !self.marked.is_empty() {
            self.marked.clear();
            cx.notify();
        }
        if self.selected == Some(index) {
            return;
        }
        // Blurring the name field commits the edit. Do it while its
        // track is still selected: the blur event may arrive after this
        // selection handler and otherwise lose the pending name.
        self.finish_renaming_cue(cx);
        // A typed tempo belongs to the track for which the editor opened.
        self.by_ear.is_editing = false;
        self.by_ear.bpm_problem = None;
        self.selected = Some(index);
        self.key_shift = 0;
        self.cue_editor.forget_selection();
        self.refresh_mixes();
        self.load_selected_waveform(cx);
        self.point_player_at_selection(cx);
        cx.notify();
    }

    /// Gives the player the selected track. While a track is playing, the
    /// newly selected one plays in its place.
    fn point_player_at_selection(&mut self, cx: &mut Context<'_, Self>) {
        self.stop_drone_of_other_tracks();
        let selected = self
            .selected_track()
            .map(|track| (track.path.clone(), track.duration_seconds()));
        let request = match &selected {
            Some((path, duration_seconds)) => self.player.select(Some(path), *duration_seconds),
            None => self.player.select(None, None),
        };
        self.carry_out(request, cx);
    }

    /// Pauses the player when it plays, as when the decks take the sound.
    pub(crate) fn stop_player(&mut self, cx: &mut Context<'_, Self>) {
        if self.player.offers_pause() {
            self.toggle_playback(cx);
        }
    }

    /// The play and pause button, or the space bar.
    pub(crate) fn toggle_playback(&mut self, cx: &mut Context<'_, Self>) {
        if self.decks.is_open {
            self.notice = Some("Close the decks to use the track player.".to_string());
            cx.notify();
            return;
        }
        let request = self.player.toggle();
        self.carry_out(request, cx);
        cx.notify();
    }

    /// A press or a drag at `pointer_x` on a strip that stands for the whole
    /// track, last drawn at `seen_bounds`: playing moves to that point.
    pub(crate) fn seek_to_pointer(
        &mut self,
        pointer_x: Pixels,
        seen_bounds: &SeenBounds,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(bounds) = seen_bounds.get() else {
            return;
        };
        let fraction = fraction_along(
            pointer_x / px(1.0),
            bounds.left() / px(1.0),
            bounds.size.width / px(1.0),
        );
        let request = self.player.seek_to_fraction(fraction);
        self.carry_out(request, cx);
        cx.notify();
    }

    /// Passes what the player asked for on to the audio output.
    pub(crate) fn carry_out(&mut self, request: Option<PlayerRequest>, cx: &mut Context<'_, Self>) {
        match request {
            None => {}
            Some(PlayerRequest::Start {
                path,
                from_seconds,
                with_stems,
                attempt,
            }) => {
                self.voice = None;
                let semitones = self.key_shift;
                let opening = if with_stems {
                    self.open_stems(&path, from_seconds)
                } else {
                    TrackSource::open_shifted(&path, from_seconds, semitones)
                };
                self.start_task = Some(cx.spawn(async move |this, cx| {
                    let opened = cx.background_executor().spawn(async move { opening }).await;
                    this.update(cx, |this, cx| this.finish_starting(attempt, opened, cx))
                        .ok();
                }));
            }
            Some(PlayerRequest::Pause) => {
                if let Some(active) = &self.voice {
                    active.voice.pause();
                }
            }
            Some(PlayerRequest::Resume) => {
                if let Some(active) = &self.voice {
                    active.voice.resume();
                }
                self.follow_playhead(cx);
            }
            Some(PlayerRequest::SeekTo(seconds)) => {
                if let Some(active) = &self.voice {
                    active.state.seek_to(seconds);
                }
            }
            Some(PlayerRequest::Stop) => {
                self.start_task = None;
                self.voice = None;
            }
        }
    }

    /// The file of the track to play is open, or could not be opened.
    pub(crate) fn finish_starting(
        &mut self,
        attempt: StartAttempt,
        opened: Result<TrackSource, PlaybackProblem>,
        cx: &mut Context<'_, Self>,
    ) {
        match opened {
            Err(problem) => {
                self.player
                    .failed_to_start(attempt, problem.message().to_string());
            }
            Ok(source) => {
                let state = source.state();
                if self.player.started(attempt, source.duration_seconds()) {
                    // The control may have changed while the file opened.
                    state.set_shift(self.key_shift);
                    match self.output.play(source) {
                        Ok(voice) => {
                            self.voice = Some(ActiveVoice { voice, state });
                            self.follow_playhead(cx);
                        }
                        Err(problem) => self.player.interrupted(problem.message().to_string()),
                    }
                }
            }
        }
        cx.notify();
    }

    /// Moves the playhead and the position along a few dozen times a second
    /// for as long as a track is playing.
    fn follow_playhead(&mut self, cx: &mut Context<'_, Self>) {
        if self.is_following_playhead {
            return;
        }
        self.is_following_playhead = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PLAYHEAD_INTERVAL).await;
                let still_playing = this.update(cx, |this, cx| {
                    this.read_playhead(cx);
                    this.is_following_playhead = this.player.is_playing();
                    this.is_following_playhead
                });
                if !matches!(still_playing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The stems of the track at `path`, mixed as the player hears them,
    /// to be played in place of the track.
    fn open_stems(&self, path: &Path, from_seconds: f32) -> Result<TrackSource, PlaybackProblem> {
        let files = self
            .stems
            .ready_files(path)
            .ok_or(PlaybackProblem::FileMissing)?;
        TrackSource::open_stems(&files, self.stems.mix(), from_seconds)
    }

    /// The track detail's stem switch changed what is heard: the sound
    /// follows, by restarting the track when it sounds.
    pub(crate) fn set_stem_mode(
        &mut self,
        mode: crate::stems::StemMode,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.stems.set_mode(mode) {
            return;
        }
        if self.player.offers_pause() {
            let request = self.player.toggle();
            self.carry_out(request, cx);
        }
        cx.notify();
    }

    /// The player's track is separated, or it is not: the player starts the
    /// track again when that changes while it sounds, so that it sounds the
    /// way the player hears it.
    pub(crate) fn stems_changed(&mut self, cx: &mut Context<'_, Self>) {
        let Some(track) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        let ready = self.stems.ready_files(&track).is_some();
        let was = self.player.offers_stems();
        self.player.stems_ready(ready);
        if ready != was && self.player.offers_pause() {
            let request = self.player.toggle();
            self.carry_out(request, cx);
        }
        self.report_stem_problem(&track);
        cx.notify();
    }

    /// Asks the stems of the selected track to be separated, once the app
    /// has not asked for them yet.
    pub(crate) fn request_stems(&mut self, cx: &mut Context<'_, Self>) {
        let Some(track) = self.selected_track() else {
            return;
        };
        let path = track.path.clone();
        if self.stems.start(&path) {
            cx.spawn(async move |this, cx| {
                loop {
                    let finished = this
                        .update(cx, |this, _cx| this.stems.poll())
                        .ok()
                        .flatten();
                    if let Some(path) = finished {
                        this.update(cx, |this, cx| {
                            if this.player.track() == Some(path.as_path()) {
                                this.stems_changed(cx);
                            }
                            this.decks.stem_library.recheck(&path);
                            if this.decks.is_open
                                && this.decks.mode == crate::decks::DeckMode::Mashup
                            {
                                this.scan_for_stems(cx);
                            }
                        })
                        .ok();
                        break;
                    }
                    cx.background_executor().timer(STEM_POLL_INTERVAL).await;
                }
            })
            .detach();
            cx.notify();
        }
    }

    /// Tells the user when the stems of the track could not be separated.
    fn report_stem_problem(&mut self, path: &Path) {
        if let Some(crate::stems::StemStatus::Failed(problem)) = self.stems.status(path) {
            self.report(problem.clone());
        }
        if let Some(phase) = self
            .stems
            .progress()
            .filter(|_| self.player.track() == Some(path))
        {
            self.report(format!("Separating the stems: {}", phase.description()));
        }
    }

    /// Brings the player up to date with what the sound has done: how far it
    /// has got, whether it reached the end, or what stopped it.
    pub(crate) fn read_playhead(&mut self, cx: &mut Context<'_, Self>) {
        if !self.player.is_playing() {
            return;
        }
        let Some(active) = &self.voice else {
            return;
        };
        if let Some(problem) = active.state.problem() {
            self.player.interrupted(problem.message().to_string());
            self.voice = None;
        } else if active.state.is_finished() {
            self.player.finished();
            self.voice = None;
        } else {
            self.player.advance_to(active.state.position_seconds());
        }
        cx.notify();
    }

    /// Selects the track at `index` in the library and scrolls the table to
    /// it when it is among the rows shown.
    pub(crate) fn select_and_reveal(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if let Some(row) = self.visible.iter().position(|visible| *visible == index) {
            self.table_scroll
                .scroll_to_item(row, ScrollStrategy::Center);
        }
        self.select(index, cx);
    }

    /// Asks the core which tracks mix with the selected one, in its key
    /// shifted by `key_shift`.
    pub(crate) fn refresh_mixes(&mut self) {
        self.mixes = self
            .selected
            .map(|selected| {
                suggestions(
                    self.session.library(),
                    self.session.favorites(),
                    selected,
                    self.key_shift,
                    MIX_LIMIT,
                )
            })
            .unwrap_or_default();
    }

    /// Gives the playing track the key shift chosen for it.
    pub(crate) fn apply_key_shift_to_playback(&self) {
        if let Some(active) = &self.voice {
            active.state.set_shift(self.key_shift);
        }
    }

    /// Moves the selection up or down the visible rows.
    fn move_selection(&mut self, rows: isize, cx: &mut Context<'_, Self>) {
        if self.visible.is_empty() {
            return;
        }
        let current_row = self
            .selected
            .and_then(|selected| self.visible.iter().position(|index| *index == selected));
        let last_row = self.visible.len() - 1;
        let target_row = match current_row {
            Some(row) => row.saturating_add_signed(rows).min(last_row),
            None if rows > 0 => 0,
            None => last_row,
        };
        self.table_scroll
            .scroll_to_item(target_row, ScrollStrategy::Nearest);
        self.select(self.visible[target_row], cx);
    }

    /// Handles the keys that move through the table.
    pub(crate) fn table_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<'_, Self>) {
        let rows = match event.keystroke.key.as_str() {
            "down" => 1,
            "up" => -1,
            "pagedown" => 20,
            "pageup" => -20,
            "home" => isize::MIN,
            "end" => isize::MAX,
            _ => return,
        };
        self.move_selection(rows, cx);
        cx.stop_propagation();
    }

    /// Switches to one of the built in themes and remembers the choice.
    pub(crate) fn set_theme(&mut self, name: ThemeName, cx: &mut Context<'_, Self>) {
        if self.theme.name == name {
            return;
        }
        self.theme = AppTheme::named(name);
        self.theme.apply_to_components(cx);
        if self.theme.opens_sorted_by_key {
            self.sort_order = Some(SortOrder {
                column: SortColumn::Key,
                descending: false,
            });
            self.refresh_visible();
        }
        if let Some(problem) = self.save_settings() {
            self.notice = Some(problem);
        }
        crate::install_menus(name, self.panels.sidebar_folded, cx);
        cx.refresh_windows();
        cx.notify();
    }

    /// Puts the keyboard in the search field.
    pub(crate) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    /// Hides the notice.
    pub(crate) fn dismiss_notice(&mut self, cx: &mut Context<'_, Self>) {
        self.notice = None;
        cx.notify();
    }

    /// Folds the sidebar away, or brings it back, and remembers the choice.
    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<'_, Self>) {
        self.panels.toggle_sidebar();
        self.remember_panels(cx);
        crate::install_menus(self.theme.name, self.panels.sidebar_folded, cx);
    }

    /// A press on the edge of the track detail starts resizing it.
    pub(crate) fn start_resizing(&mut self, edge: PanelEdge, cx: &mut Context<'_, Self>) {
        self.resizing = Some(edge);
        cx.stop_propagation();
    }

    /// The pointer moved to `position`. While an edge is held, the track
    /// detail follows it.
    fn resize_to(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.column_resize.is_some() {
            self.resize_column_to(position.x, cx);
            return;
        }
        match self.resizing {
            None => return,
            Some(PanelEdge::InspectorLeft) => {
                self.panels.drag_inspector_edge_to(
                    position.x / px(1.0),
                    window.viewport_size().width / px(1.0),
                );
            }
            Some(PanelEdge::StripTop) => {
                let Some(area) = self.detail_area_bounds.get() else {
                    return;
                };
                self.panels.drag_strip_edge_to(
                    position.y / px(1.0),
                    area.bottom() / px(1.0),
                    area.size.height / px(1.0),
                );
            }
        }
        cx.notify();
    }

    /// The button was released: the size the edge was dragged to is kept.
    fn finish_resizing(&mut self, cx: &mut Context<'_, Self>) {
        if self.resizing.take().is_some() {
            self.remember_panels(cx);
        }
        if self.column_resize.take().is_some() {
            self.remember_column_widths(cx);
        }
    }

    /// A double click on an edge returns that size to its default.
    pub(crate) fn reset_size(&mut self, edge: PanelEdge, cx: &mut Context<'_, Self>) {
        match edge {
            PanelEdge::InspectorLeft => self.panels.reset_inspector_width(),
            PanelEdge::StripTop => self.panels.reset_strip_height(),
        }
        self.resizing = None;
        self.remember_panels(cx);
    }

    fn remember_panels(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(problem) = self.save_settings() {
            self.report(problem);
        }
        cx.notify();
    }

    /// The width to draw the inspector at in a window of the current size.
    pub(crate) fn inspector_width(&self, window: &Window) -> Pixels {
        px(self
            .panels
            .inspector_width_in(window.viewport_size().width / px(1.0)))
    }

    /// The height to draw the strip under the table at. Until the area it
    /// shares with the table has been drawn once, the window's height
    /// stands in for that area's.
    pub(crate) fn strip_height(&self, window: &Window) -> Pixels {
        let available_height = match self.detail_area_bounds.get() {
            Some(area) => area.size.height,
            None => window.viewport_size().height,
        };
        px(self.panels.strip_height_in(available_height / px(1.0)))
    }

    /// Writes the folders, the theme and the arrangement to disk. Returns what to tell the user
    /// when that fails.
    fn save_settings(&self) -> Option<String> {
        let settings_path = self.settings_path.as_ref()?;
        let mut expanded: Vec<PathBuf> = self.expanded_folders.iter().cloned().collect();
        expanded.sort();
        let mut folders = self.session.library().folders().to_vec();
        folders.extend(self.unavailable_folders.iter().cloned());
        let settings = Settings {
            folders,
            theme: self.theme.name,
            panels: self.panels,
            folder_tree: FolderTreeMemory {
                expanded: Some(expanded),
                selected: self.folder.clone(),
            },
            column_widths: self.column_widths.clone(),
            tags: self.tag_settings,
            display: self.display,
        };
        settings
            .save(settings_path)
            .err()
            .map(|error| format!("Could not save settings: {error}."))
    }

    /// Decodes the selected track in the background and keeps its peaks,
    /// unless they are already kept or already being decoded. Tracks without
    /// an analysis are not decoded.
    fn load_selected_waveform(&mut self, cx: &mut Context<'_, Self>) {
        let selected = self
            .selected_track()
            .filter(|track| track.analysis().is_some())
            .map(|track| (track.path.clone(), track.bpm()));
        let Some((path, tempo)) = selected else {
            self.waveform_task = None;
            self.waveform_task_path = None;
            return;
        };
        if self.waveform_task_path.as_ref() == Some(&path) && self.waveform_task.is_some() {
            return;
        }
        // A waveform whose energy curve was never finished, because another
        // track was selected first, is decoded again to finish it.
        let is_decoded = self.waveforms.touch(&path)
            && matches!(
                self.waveforms.get(&path),
                Some(Waveform::Ready(_, Some(_))) | Some(Waveform::Unreadable(_))
            );
        if is_decoded {
            self.waveform_task = None;
            self.waveform_task_path = None;
            return;
        }
        self.waveform_task_path = Some(path.clone());
        let decode = self.decode_waveform(path.clone(), tempo, cx);
        self.waveform_task = Some(cx.spawn(async move |this, cx| {
            decode.await;
            this.update(cx, |this, _| {
                if this.waveform_task_path.as_ref() == Some(&path) {
                    this.waveform_task_path = None;
                }
            })
            .ok();
        }));
    }

    /// The waveform of the track at `path`, when it was read.
    pub(crate) fn waveform_of(&self, path: &Path) -> Option<&Waveform> {
        self.waveforms.get(path)
    }

    /// Loads the waveform of the track on a deck, when it is not loaded
    /// already.
    pub(crate) fn load_deck_waveform(
        &mut self,
        path: &Path,
        tempo: Option<f32>,
        cx: &mut Context<'_, Self>,
    ) -> Option<Task<()>> {
        let is_decoded = self.waveforms.touch(path)
            && matches!(
                self.waveforms.get(path),
                Some(Waveform::Ready(_, Some(_))) | Some(Waveform::Unreadable(_))
            );
        if is_decoded {
            None
        } else {
            Some(self.decode_waveform(path.to_path_buf(), tempo, cx))
        }
    }

    /// Decodes the waveform and the energy curve of the track at `path`
    /// off the main thread, and keeps them.
    fn decode_waveform(
        &mut self,
        path: PathBuf,
        tempo: Option<f32>,
        cx: &mut Context<'_, Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WAVEFORM_DELAY).await;
            let marked = this.update(cx, |this, cx| {
                if !matches!(this.waveforms.get(&path), Some(Waveform::Ready(..))) {
                    this.waveforms.insert(path.clone(), Waveform::Reading);
                }
                cx.notify();
            });
            if marked.is_err() {
                return;
            }
            let decoded_path = path.clone();
            let (waveform, audio) = cx
                .background_executor()
                .spawn(async move {
                    match euphoria_core::decode_mono(&decoded_path) {
                        Ok(audio) => {
                            let columns = compute_band_peaks(
                                audio.samples(),
                                audio.sample_rate().get(),
                                PEAK_COUNT,
                            );
                            (Waveform::Ready(columns.into(), None), Some(audio))
                        }
                        Err(error) => (Waveform::Unreadable(error.chain()), None),
                    }
                })
                .await;
            let shown = this.update(cx, |this, cx| {
                this.waveforms.insert(path.clone(), waveform);
                cx.notify();
            });
            // The waveform shows first; the energy curve, from the same
            // decoded audio, joins it when ready.
            let Some(audio) = audio.filter(|_| shown.is_ok()) else {
                return;
            };
            let curve = cx
                .background_executor()
                .spawn(async move {
                    let duration_seconds =
                        audio.samples().len() as f32 / audio.sample_rate().get() as f32;
                    energy_curve_from(euphoria_core::energy_curve(&audio, tempo), duration_seconds)
                })
                .await;
            this.update(cx, |this, cx| {
                this.waveforms.set_energy(&path, curve);
                cx.notify();
            })
            .ok();
        })
    }
}

impl Render for EuphoriaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let palette = self.theme.palette;
        let shows_sidebar = !self.panels.sidebar_folded;
        self.lay_out_table(window);
        let body = if self.is_empty() {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .when(shows_sidebar, |body| {
                    body.child(views::sidebar::render(self, cx))
                })
                .child(views::empty_state::render(self, cx))
        } else {
            let seen_bounds = self.detail_area_bounds.clone();
            let library_and_detail = div()
                .relative()
                .flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .when(self.theme.layout == Layout::DeckBelow, Div::flex_col)
                .child(
                    canvas(
                        move |bounds, _, _| seen_bounds.set(Some(bounds)),
                        |_, _, _, _| (),
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(views::track_table::render(self, cx))
                        .when(self.decks.is_open, |column| {
                            column.child(views::decks::render(self, cx))
                        }),
                )
                .child(views::inspector::render(self, window, cx))
                .child(views::panel_edge::render(self, window, cx));
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .when(shows_sidebar, |body| {
                    body.child(views::sidebar::render(self, cx))
                })
                .map(|body| {
                    if self.set_builder.is_open {
                        body.child(views::set_builder::render(self, cx))
                    } else {
                        body.child(library_and_detail)
                    }
                })
        };

        let has_player_bar = !self.is_empty() && self.theme.layout == Layout::InspectorRight;

        div()
            .id("euphoria")
            .track_focus(&self.window_focus)
            .key_context(views::LIBRARY_KEY_CONTEXT)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.close_panels(cx) {
                    return;
                }
                // With the keyboard on the window itself, the arrow keys
                // move through the table as they do when the table has it.
                if this.window_focus.is_focused(window) {
                    this.table_key_down(event, cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.text)
            .text_size(views::TEXT_SIZE)
            .line_height(relative(1.4))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(palette.selection))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| {
                this.add_paths(dropped.paths().to_vec(), true, cx);
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    if !this.drag_cue_to(event.position.x, event.modifiers, cx) {
                        this.resize_to(event.position, window, cx);
                    }
                } else {
                    this.finish_resizing(cx);
                    this.release_cue(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.finish_resizing(cx);
                    this.release_cue(cx);
                }),
            )
            .child(views::toolbar::render(self, cx))
            .child(body)
            .when(has_player_bar, |window| {
                window.child(views::player::bar(self, cx))
            })
            .children(views::notice::render(self, cx))
            .children(views::tag_panel::render(self, cx))
            .when(self.settings_open, |window| {
                window.child(views::settings_panel::render(self, cx))
            })
    }
}

/// How many files have their names read in one go.
const NAME_BATCH: usize = 256;

/// Room added to measured contents when a column is fitted, for the
/// rounding of text layout.
const FIT_SLACK: f32 = 4.0;

/// Text as wide as an energy rating and its bar, which the Energy column is
/// fitted to.
const WIDEST_ENERGY_TEXT: &str = "10 10 10 10 ";

/// Room for the sort arrow beside a column's header.
const SORT_ARROW_WIDTH: f32 = 16.0;

/// The column whose header edge is held, and where it starts from the
/// table's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ColumnResize {
    /// The column being resized.
    pub(crate) column: ColumnId,
    /// Where it starts, from the left edge of the table, moved by how far
    /// right of its edge it was grabbed, so it keeps its width until the
    /// pointer moves.
    pub(crate) left: f32,
}

/// How wide `text` is drawn at `size` in the window's font.
pub(crate) fn text_width(window: &Window, text: &str, size: Pixels) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let run = window.text_style().to_run(text.len());
    window
        .text_system()
        .shape_line(text.to_string().into(), size, &[run], None)
        .width
        / px(1.0)
}

#[cfg(test)]
mod waveform_task_tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn a_deck_decode_keeps_the_selected_tracks_pending_waveform_task(cx: &mut TestAppContext) {
        let (_, view) = crate::ui_tests::open_fixture_library(cx);
        let (next_index, next_path) = cx.update(|cx| {
            view.update(cx, |app, cx| {
                let selected = app.selected.unwrap();
                let others: Vec<usize> = (0..app.session.library().tracks().len())
                    .filter(|index| *index != selected)
                    .collect();
                let deck_track = app.session.library().track(others[0]).unwrap();
                app.load_deck_waveform(&deck_track.path.clone(), deck_track.bpm(), cx)
                    .unwrap()
                    .detach();
                (
                    others[1],
                    app.session.library().track(others[1]).unwrap().path.clone(),
                )
            })
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(90));
        cx.run_until_parked();
        cx.update(|cx| view.update(cx, |app, cx| app.select(next_index, cx)));
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update(|cx| {
            let app = view.read(cx);
            assert_eq!(
                app.waveform_task_path.as_ref(),
                Some(&next_path),
                "the earlier deck decode must not relinquish the selected track's pending task"
            );
        });
        cx.executor().advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(matches!(
                view.read(cx).selected_waveform(),
                Some(Waveform::Ready(_, Some(_)))
            ))
        });
    }

    #[gpui::test]
    fn refreshing_after_the_waveform_arrives_keeps_the_pending_energy_task(
        cx: &mut TestAppContext,
    ) {
        let (_, view) = crate::ui_tests::open_fixture_library(cx);
        let refreshed = Rc::new(Cell::new(false));
        let observed = refreshed.clone();
        let _subscription = cx.update(|cx| {
            cx.observe(&view, move |view, cx| {
                view.update(cx, |app, cx| {
                    if matches!(app.selected_waveform(), Some(Waveform::Ready(_, None)))
                        && !observed.replace(true)
                    {
                        let task_path = app.waveform_task_path.clone();
                        app.load_selected_waveform(cx);
                        assert!(task_path.is_some(), "the pending curve still owns its path");
                        assert_eq!(app.waveform_task_path, task_path);
                    }
                });
            })
        });
        for _ in 0..20 {
            cx.executor().advance_clock(Duration::from_millis(200));
            cx.run_until_parked();
        }
        assert!(
            refreshed.get(),
            "refreshed between waveform and curve completion"
        );
        cx.update(|cx| {
            assert!(matches!(
                view.read(cx).selected_waveform(),
                Some(Waveform::Ready(_, Some(_)))
            ));
        });
    }
}
