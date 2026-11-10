// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's root view: it owns the library session and what the user has
//! chosen to see, and lays the screen out from the views.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::component::input::{InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    Context, Entity, ExternalPaths, FocusHandle, KeyDownEvent, PathPromptOptions, ScrollStrategy,
    Subscription, Task, UniformListScrollHandle, Window, div, relative,
};

use crate::library::{LibraryFilter, SortColumn, SortOrder, Track};
use crate::peaks::{PEAK_COUNT, compute_peaks};
use crate::settings::Settings;
use crate::state::{Scan, Session, scan};
use crate::theme::{AppTheme, Layout, ThemeName};
use crate::views;

/// How often finished files are moved from the workers into the table.
const COLLECT_INTERVAL: Duration = Duration::from_millis(80);

/// How long a track must stay selected before its audio is decoded for the
/// waveform, so that moving quickly through the table decodes nothing.
const WAVEFORM_DELAY: Duration = Duration::from_millis(180);

/// How long quitting waits for the files in the middle of analysis.
const QUIT_PATIENCE: Duration = Duration::from_secs(20);

/// The waveform of one track.
#[derive(Debug, Clone)]
pub(crate) enum Waveform {
    /// The audio is being decoded.
    Reading,
    /// Peaks from the start of the track to its end, each from 0.0 to 1.0.
    Ready(Arc<[f32]>),
    /// The audio could not be decoded; the text says why.
    Unreadable(String),
}

/// The root view of the window.
pub(crate) struct EuphoriaApp {
    pub(crate) session: Session,
    pub(crate) theme: AppTheme,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) filter: LibraryFilter,
    pub(crate) sort_order: Option<SortOrder>,
    /// Indexes into the library of the rows the table shows, in order.
    pub(crate) visible: Vec<usize>,
    /// Index into the library of the selected track.
    pub(crate) selected: Option<usize>,
    pub(crate) table_scroll: UniformListScrollHandle,
    pub(crate) table_focus: FocusHandle,
    /// A problem to tell the user about, shown until dismissed.
    pub(crate) notice: Option<String>,
    /// Folders being searched for audio files right now.
    pub(crate) scans_in_progress: usize,
    /// True between a press on the toolbar and the drag or release after it.
    pub(crate) toolbar_drag_armed: bool,
    query: String,
    settings_path: Option<PathBuf>,
    waveforms: HashMap<PathBuf, Waveform>,
    waveform_task: Option<Task<()>>,
    is_collecting: bool,
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
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
        let quitting = cx.on_app_quit(|this: &mut Self, _| {
            this.session.stop(QUIT_PATIENCE);
            async {}
        });

        let theme = AppTheme::named(settings.theme);
        theme.apply_to_components(cx);
        let mut this = Self {
            session,
            theme,
            search_input,
            filter: LibraryFilter::AllTracks,
            sort_order: theme.opens_sorted_by_key.then_some(SortOrder {
                column: SortColumn::Key,
                descending: false,
            }),
            visible: Vec::new(),
            selected: None,
            table_scroll: UniformListScrollHandle::new(),
            table_focus: cx.focus_handle(),
            notice,
            scans_in_progress: 0,
            toolbar_drag_armed: false,
            query: String::new(),
            settings_path,
            waveforms: HashMap::new(),
            waveform_task: None,
            is_collecting: false,
            _subscriptions: vec![search_changed, quitting],
        };
        if !settings.folders.is_empty() {
            this.add_paths(settings.folders, cx);
        }
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
                Ok(paths) => this.add_paths(paths, cx),
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
    /// tracks as pending rows and queues them for analysis.
    pub(crate) fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        self.scans_in_progress += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { scan(&paths) })
                .await;
            this.update(cx, |this, cx| this.finish_scan(&found, cx))
                .ok();
        })
        .detach();
    }

    fn finish_scan(&mut self, found: &Scan, cx: &mut Context<'_, Self>) {
        self.scans_in_progress = self.scans_in_progress.saturating_sub(1);
        let folder_count_before = self.session.library().folders().len();
        self.session.add(found);

        let mut problems: Vec<String> = found
            .problems
            .iter()
            .map(|problem| format!("Could not add a folder: {problem}"))
            .collect();
        if found.problems.is_empty() && found.files.is_empty() {
            problems.push("No audio files found there.".to_string());
        }
        if self.session.library().folders().len() != folder_count_before {
            problems.extend(self.save_settings());
        }
        if !problems.is_empty() {
            self.notice = Some(problems.join(" "));
        }

        self.refresh_visible();
        self.start_collecting(cx);
        cx.notify();
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
                        this.load_selected_waveform(cx);
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
    /// the filter or the sort order changed.
    pub(crate) fn refresh_visible(&mut self) {
        self.visible =
            self.session
                .library()
                .visible_tracks(&self.filter, &self.query, self.sort_order);
    }

    /// Shows one part of the library.
    pub(crate) fn set_filter(&mut self, filter: LibraryFilter, cx: &mut Context<'_, Self>) {
        self.filter = filter;
        self.refresh_visible();
        cx.notify();
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
        self.load_selected_waveform(cx);
        cx.notify();
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
        crate::install_menus(name, cx);
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

    /// Writes the folders and theme to disk. Returns what to tell the user
    /// when that fails.
    fn save_settings(&self) -> Option<String> {
        let settings_path = self.settings_path.as_ref()?;
        let settings = Settings {
            folders: self.session.library().folders().to_vec(),
            theme: self.theme.name,
        };
        settings
            .save(settings_path)
            .err()
            .map(|error| format!("Could not save settings: {error}."))
    }

    /// Decodes the selected track in the background and keeps its peaks,
    /// unless they are already known. Failed tracks are not decoded again.
    fn load_selected_waveform(&mut self, cx: &mut Context<'_, Self>) {
        let Some(track) = self.selected_track() else {
            self.waveform_task = None;
            return;
        };
        if track.analysis().is_none() || self.waveforms.contains_key(&track.path) {
            self.waveform_task = None;
            return;
        }
        let path = track.path.clone();
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
                        Ok(audio) => {
                            Waveform::Ready(compute_peaks(audio.samples(), PEAK_COUNT).into())
                        }
                        Err(error) => Waveform::Unreadable(error.chain()),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.waveforms.insert(path, waveform);
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Render for EuphoriaApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let palette = self.theme.palette;
        let body = if self.is_empty() {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(views::sidebar::render(self, cx))
                .child(views::empty_state::render(self, cx))
        } else {
            let library_and_detail = match self.theme.layout {
                Layout::InspectorRight => div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(views::track_table::render(self, cx))
                    .child(views::inspector::render(self, cx)),
                Layout::DeckBelow => div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(views::track_table::render(self, cx))
                    .child(views::inspector::render(self, cx)),
            };
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(views::sidebar::render(self, cx))
                .child(library_and_detail)
        };

        div()
            .id("euphoria")
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.text)
            .text_size(views::TEXT_SIZE)
            .line_height(relative(1.4))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(palette.selection))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| {
                this.add_paths(dropped.paths().to_vec(), cx);
            }))
            .child(views::toolbar::render(self, cx))
            .child(body)
            .children(views::notice::render(self, cx))
    }
}
