// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that open the real root view in a headless window and use it with
//! clicks and keys, as a person would.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use euphoria_core::TrackAnalysis;
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Entity, Modifiers, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::EuphoriaApp;
use crate::columns::ColumnId;
use crate::demo::{self, DemoTrack};
use crate::library::{SortColumn, SortOrder};
use crate::panels::Panels;
use crate::playback::testing::{ListeningOutput, NoOutput, SilentOutput};
use crate::playback::{AudioOutput, PlaybackProblem};
use crate::player::{PlayerRequest, PlayerStatus};
use crate::settings::{FolderTreeMemory, Settings};
use crate::state::Session;
use crate::tag_flow::TagPanel;
use crate::tag_settings::{NotationChoice, ProgramChoice, TagSettings};
use crate::tag_writing;
use crate::tag_writing::testing::{self as tag_testing, ScratchFolder};
use crate::theme::{AppTheme, Layout, ThemeName};
use crate::track_overview::{EnergyCurve, EnergyStep};
use crate::views::sidebar::{disclosure_id, folder_row_id};
use crate::views::tag_panel::ProblemList;
use crate::views::track_table::{edge_id, header_id};
use crate::waveforms::Waveform;

/// Index of the BPM column among the table's column headers.
const BPM_COLUMN: usize = 5;

/// Index of the Energy column among the table's column headers.
const ENERGY_COLUMN: usize = 6;

/// A library of made up tracks whose files do not exist, with an audio
/// output that makes no sound.
pub(crate) fn open_library(
    track_count: usize,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(demo::tracks(track_count), Box::new(SilentOutput), cx)
}

/// A library of the one second audio files the core is tested with, each
/// already analyzed, with an audio output that makes no sound.
pub(crate) fn open_fixture_library(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(fixture_tracks(), Box::new(SilentOutput), cx)
}

pub(crate) fn fixture_tracks() -> Vec<DemoTrack> {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
    [
        "sine-440hz-mono.flac",
        "sine-440hz-mono.mp3",
        "sine-440hz-stereo.wav",
    ]
    .into_iter()
    .map(|name| DemoTrack {
        path: fixtures.join(name),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 1.0,
            key: None,
            bpm: None,
            energy: None,
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            loudness_range_lu: None,
            pitch_class_profile: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            tempo_octave_evidence: None,
            beat_grid: None,
            cues: Vec::new(),
            key_timeline: Vec::new(),
            quality: None,
        }),
    })
    .collect()
}

pub(crate) fn open_window_on(
    tracks: Vec<DemoTrack>,
    output: Box<dyn AudioOutput>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let usual_size = size(px(1440.0), px(900.0));
    open_window_with(tracks, output, Settings::default(), None, usual_size, cx)
}

/// The fixture library in a window of `window_size`, opened with `settings`
/// as if they had been remembered, and saving to `settings_path` when given.
pub(crate) fn open_window_with(
    tracks: Vec<DemoTrack>,
    output: Box<dyn AudioOutput>,
    mut settings: Settings,
    settings_path: Option<PathBuf>,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    // The first run card has tests of its own; the others are about
    // someone who is past it.
    if settings.folders.is_empty() {
        settings
            .first_run
            .get_or_insert(crate::first_run::FirstRun::already_done());
    }
    cx.update(|cx| {
        gpui::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: window_size,
            })),
            ..Default::default()
        };
        let (window, view) = gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session.add_demo_tracks(tracks);
                EuphoriaApp::new(session, settings, settings_path, vec![], output, window, cx)
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    })
}

/// The fixture library opened in `theme` with a remembered arrangement.
fn open_arranged(
    theme: ThemeName,
    panels: Panels,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let settings = Settings {
        theme,
        panels,
        ..Settings::default()
    };
    open_window_with(
        fixture_tracks(),
        Box::new(SilentOutput),
        settings,
        None,
        window_size,
        cx,
    )
}

pub(crate) fn track(index: usize) -> (&'static str, usize) {
    ("track", index)
}

include!("table_ui_tests.rs");
include!("playback_ui_tests.rs");
include!("layout_ui_tests.rs");
include!("folders_ui_tests.rs");
include!("tag_writer_ui_tests.rs");
include!("track_overview_ui_tests.rs");
include!("needs_review_ui_tests.rs");
include!("playlists_ui_tests.rs");
include!("tag_editor_ui_tests.rs");
include!("by_ear_ui_tests.rs");
include!("decks_ui_tests.rs");
include!("cues_ui_tests.rs");
include!("stems_ui_tests.rs");
include!("inspector_ui_tests.rs");
include!("review_settlement_ui_tests.rs");
