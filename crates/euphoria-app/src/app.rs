// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's root view: it owns the library session and what the user has
//! chosen to see, and lays the screen out from the views.

use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::component::input::{InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    Context, Div, Entity, ExternalPaths, FocusHandle, KeyDownEvent, MouseButton, MouseMoveEvent,
    PathPromptOptions, Pixels, ScrollStrategy, Subscription, Task, UniformListScrollHandle, Window,
    canvas, div, px, relative,
};

use crate::folder_tree::{FolderColumn, FolderTree, TreeKey, TreeMove};
use crate::library::{LibraryFilter, SortColumn, SortOrder, Track};
use crate::mixes::{MixSuggestion, suggestions};
use crate::panels::{PanelEdge, Panels};
use crate::peaks::{PEAK_COUNT, compute_band_peaks};
use crate::playback::{AudioOutput, PlaybackProblem, TrackSource, Voice, VoiceState};
use crate::player::{Player, PlayerRequest, StartAttempt, fraction_along};
use crate::settings::{FolderTreeMemory, Settings};
use crate::state::{Scan, Session, scan};
use crate::theme::{AppTheme, Layout, ThemeName};
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
    /// The folders the user added and the subfolders holding their tracks.
    pub(crate) folder_tree: FolderTree,
    /// Indexes into the folder tree of the folders the sidebar lists.
    pub(crate) folder_rows: Vec<usize>,
    /// The unfolded folders of the tree.
    pub(crate) expanded_folders: HashSet<PathBuf>,
    /// The Folder column of every track, relative to the folder on show.
    pub(crate) folder_column: FolderColumn,
    pub(crate) folder_scroll: UniformListScrollHandle,
    pub(crate) tree_focus: FocusHandle,
    pub(crate) sort_order: Option<SortOrder>,
    /// Indexes into the library of the rows the table shows, in order.
    pub(crate) visible: Vec<usize>,
    /// Index into the library of the selected track.
    pub(crate) selected: Option<usize>,
    /// Tracks that mix with the selected one, smoothest first.
    pub(crate) mixes: Vec<MixSuggestion>,
    pub(crate) table_scroll: UniformListScrollHandle,
    pub(crate) table_focus: FocusHandle,
    /// A problem to tell the user about, shown until dismissed.
    pub(crate) notice: Option<String>,
    /// Folders being searched for audio files right now.
    pub(crate) scans_in_progress: usize,
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
    output: Box<dyn AudioOutput>,
    voice: Option<ActiveVoice>,
    /// Opens the file of the track about to play.
    start_task: Option<Task<()>>,
    is_following_playhead: bool,
    query: String,
    settings_path: Option<PathBuf>,
    waveforms: WaveformCache,
    waveform_task: Option<Task<()>>,
    /// The file the waveform task is decoding.
    waveform_task_path: Option<PathBuf>,
    is_collecting: bool,
    /// The library revision the folder tree was built from.
    folder_tree_revision: Option<u64>,
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
            folder_tree: FolderTree::default(),
            folder_rows: Vec::new(),
            expanded_folders,
            folder_column: FolderColumn::default(),
            folder_scroll: UniformListScrollHandle::new(),
            tree_focus: cx.focus_handle(),
            sort_order: theme.opens_sorted_by_key.then_some(SortOrder {
                column: SortColumn::Key,
                descending: false,
            }),
            visible: Vec::new(),
            selected: None,
            mixes: Vec::new(),
            table_scroll: UniformListScrollHandle::new(),
            table_focus: cx.focus_handle(),
            notice,
            scans_in_progress: 0,
            toolbar_drag_armed: false,
            panels: settings.panels,
            resizing: None,
            detail_area_bounds: Rc::new(Cell::new(None)),
            player: Player::default(),
            waveform_bounds: Rc::new(Cell::new(None)),
            progress_line_bounds: Rc::new(Cell::new(None)),
            window_focus,
            output,
            voice: None,
            start_task: None,
            is_following_playhead: false,
            query: String::new(),
            settings_path,
            waveforms: WaveformCache::new(WAVEFORM_CAPACITY),
            waveform_task: None,
            waveform_task_path: None,
            is_collecting: false,
            folder_tree_revision: None,
            folder_column_basis: None,
            _subscriptions: vec![search_changed, quitting],
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
        self.session.add(found);
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
    fn report(&mut self, problem: String) {
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
        if self.folder_tree_revision != Some(library.revision()) {
            self.folder_tree = FolderTree::build(library.folders(), library.tracks());
            self.folder_tree_revision = Some(library.revision());
            self.folder_rows = self.folder_tree.visible_rows(&self.expanded_folders);
        }
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
        self.visible = library.visible_tracks(
            self.filter,
            self.folder.as_deref(),
            &self.query,
            self.sort_order,
            &self.folder_column,
        );
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
        self.filter = LibraryFilter::AllTracks;
        self.folder = None;
        self.refresh_visible();
        self.remember_folder_tree();
        cx.notify();
    }

    /// Shows only the tracks that need review, within the folder on show.
    pub(crate) fn show_tracks_needing_review(&mut self, cx: &mut Context<'_, Self>) {
        self.filter = LibraryFilter::NeedsReview;
        self.refresh_visible();
        cx.notify();
    }

    /// Shows every track in the folder at `path` and the folders inside it.
    pub(crate) fn show_folder(&mut self, path: PathBuf, cx: &mut Context<'_, Self>) {
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
        if self.selected == Some(index) {
            return;
        }
        self.selected = Some(index);
        self.refresh_mixes();
        self.load_selected_waveform(cx);
        self.point_player_at_selection(cx);
        cx.notify();
    }

    /// Gives the player the selected track. While a track is playing, the
    /// newly selected one plays in its place.
    fn point_player_at_selection(&mut self, cx: &mut Context<'_, Self>) {
        let selected = self
            .selected_track()
            .map(|track| (track.path.clone(), track.duration_seconds()));
        let request = match &selected {
            Some((path, duration_seconds)) => self.player.select(Some(path), *duration_seconds),
            None => self.player.select(None, None),
        };
        self.carry_out(request, cx);
    }

    /// The play and pause button, or the space bar.
    pub(crate) fn toggle_playback(&mut self, cx: &mut Context<'_, Self>) {
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
    fn carry_out(&mut self, request: Option<PlayerRequest>, cx: &mut Context<'_, Self>) {
        match request {
            None => {}
            Some(PlayerRequest::Start {
                path,
                from_seconds,
                attempt,
            }) => {
                self.voice = None;
                self.start_task = Some(cx.spawn(async move |this, cx| {
                    let opened = cx
                        .background_executor()
                        .spawn(async move { TrackSource::open(&path, from_seconds) })
                        .await;
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

    /// Asks the core which tracks mix with the selected one.
    fn refresh_mixes(&mut self) {
        self.mixes = self
            .selected
            .map(|selected| suggestions(self.session.library(), selected, MIX_LIMIT))
            .unwrap_or_default();
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
        let settings = Settings {
            folders: self.session.library().folders().to_vec(),
            theme: self.theme.name,
            panels: self.panels,
            folder_tree: FolderTreeMemory {
                expanded: Some(expanded),
                selected: self.folder.clone(),
            },
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
        let path = self
            .selected_track()
            .filter(|track| track.analysis().is_some())
            .map(|track| track.path.clone());
        let Some(path) = path else {
            self.waveform_task = None;
            return;
        };
        if self.waveform_task_path.as_ref() == Some(&path) && self.waveform_task.is_some() {
            return;
        }
        let is_decoded = self.waveforms.touch(&path)
            && !matches!(self.waveforms.get(&path), Some(Waveform::Reading));
        if is_decoded {
            self.waveform_task = None;
            return;
        }
        self.waveform_task_path = Some(path.clone());
        self.waveform_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WAVEFORM_DELAY).await;
            let marked = this.update(cx, |this, cx| {
                this.waveforms.insert(path.clone(), Waveform::Reading);
                cx.notify();
            });
            if marked.is_err() {
                return;
            }
            let decoded_path = path.clone();
            let waveform = cx
                .background_executor()
                .spawn(async move {
                    match euphoria_core::decode_mono(&decoded_path) {
                        Ok(audio) => Waveform::Ready(
                            compute_band_peaks(
                                audio.samples(),
                                audio.sample_rate().get(),
                                PEAK_COUNT,
                            )
                            .into(),
                        ),
                        Err(error) => Waveform::Unreadable(error.chain()),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.waveforms.insert(path, waveform);
                this.waveform_task_path = None;
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Render for EuphoriaApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let palette = self.theme.palette;
        let shows_sidebar = !self.panels.sidebar_folded;
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
                .child(views::track_table::render(self, cx))
                .child(views::inspector::render(self, window, cx))
                .child(views::panel_edge::render(self, window, cx));
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .when(shows_sidebar, |body| {
                    body.child(views::sidebar::render(self, cx))
                })
                .child(library_and_detail)
        };

        let has_player_bar = !self.is_empty() && self.theme.layout == Layout::InspectorRight;

        div()
            .id("euphoria")
            .track_focus(&self.window_focus)
            .key_context(views::LIBRARY_KEY_CONTEXT)
            .on_key_down(cx.listener(|this, event, window, cx| {
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
                    this.resize_to(event.position, window, cx);
                } else {
                    this.finish_resizing(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_resizing(cx)),
            )
            .child(views::toolbar::render(self, cx))
            .child(body)
            .when(has_player_bar, |window| {
                window.child(views::player::bar(self, cx))
            })
            .children(views::notice::render(self, cx))
    }
}
