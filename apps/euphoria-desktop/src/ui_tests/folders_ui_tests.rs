// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 12 tests: the folder tree of the sidebar.

const DJ_TRACKS: &str = "/music/DJ tracks";

/// Index of the Folder column among the table's column headers.
const FOLDER_COLUMN: usize = 12;

/// A made up DJ folder: two tracks in afro_house/warmup, one that failed in
/// afro_house/peak, one in deep and one directly in the folder.
fn dj_tracks() -> Vec<DemoTrack> {
    [
        ("afro_house/warmup/one.mp3", true),
        ("afro_house/warmup/two.mp3", true),
        ("afro_house/peak/three.mp3", false),
        ("deep/four.mp3", true),
        ("loose.mp3", true),
    ]
    .into_iter()
    .map(|(relative, is_analyzed)| DemoTrack {
        path: Path::new(DJ_TRACKS).join(relative),
        outcome: if is_analyzed {
            Ok(TrackAnalysis {
                duration_seconds: 300.0,
                key: None,
                bpm: Some(124.0),
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
            })
        } else {
            Err("cannot decode".to_string())
        },
    })
    .collect()
}

/// The made up DJ folder added as one folder, opened with `settings`.
fn open_dj_folder(
    settings: Settings,
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_folders(&[DJ_TRACKS], dj_tracks(), settings, settings_path, cx)
}

/// Made up `tracks` in the made up `folders`, as if the folders had been
/// added, opened with `settings`.
fn open_folders(
    folders: &[&str],
    tracks: Vec<DemoTrack>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let folders: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
    let settings = Settings {
        folder_tree: FolderTreeMemory {
            expanded: settings
                .folder_tree
                .expanded
                .or_else(|| Some(folders.clone())),
            selected: settings.folder_tree.selected,
        },
        ..settings
    };
    cx.update(|cx| {
        gpui::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: USUAL_WINDOW,
            })),
            ..Default::default()
        };
        let (window, view) = gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                for folder in folders {
                    session.add_demo_folder(folder);
                }
                session.add_demo_tracks(tracks);
                EuphoriaApp::new(
                    session,
                    settings,
                    settings_path,
                    vec![],
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    })
}

/// The index in the folder tree of the folder at `relative` inside the DJ
/// folder, or of the DJ folder itself for "".
fn folder_index(view: &Entity<EuphoriaApp>, relative: &str, cx: &mut TestAppContext) -> usize {
    let path = if relative.is_empty() {
        PathBuf::from(DJ_TRACKS)
    } else {
        Path::new(DJ_TRACKS).join(relative)
    };
    cx.update(|cx| view.read(cx).folder_tree.index_of(&path).unwrap())
}

/// The names of the listed folders, indented two spaces a level.
fn listed_folders(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let app = view.read(cx);
        app.folder_rows
            .iter()
            .map(|index| {
                let node = app.folder_tree.node(*index).unwrap();
                format!("{}{}", "  ".repeat(node.depth), node.name)
            })
            .collect()
    })
}

fn shown_titles(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let app = view.read(cx);
        app.visible
            .iter()
            .map(|index| app.session.library().tracks()[*index].file_name.clone())
            .collect()
    })
}

fn shown_folder(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        view.read(cx)
            .folder
            .as_ref()
            .map(|folder| folder.to_string_lossy().into_owned())
    })
}

fn folder_text(view: &Entity<EuphoriaApp>, file_name: &str, cx: &mut TestAppContext) -> String {
    cx.update(|cx| {
        let app = view.read(cx);
        let index = app
            .session
            .library()
            .tracks()
            .iter()
            .position(|track| track.file_name == file_name)
            .unwrap();
        app.folder_column.label(index).unwrap().text.clone()
    })
}

fn joined(names: &[&str]) -> String {
    names.join(std::path::MAIN_SEPARATOR_STR)
}

#[gpui::test]
fn the_triangle_unfolds_a_folder_and_folds_it_again(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    assert_eq!(
        listed_folders(&view, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
    let afro_house = folder_index(&view, "afro_house", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(disclosure_id(afro_house), cx);
    })
    .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        [
            "DJ tracks",
            "  afro_house",
            "    peak",
            "    warmup",
            "  deep"
        ]
    );
    assert_eq!(shown_folder(&view, cx), None);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(disclosure_id(afro_house), cx);
    })
    .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
}

#[gpui::test]
fn choosing_a_folder_shows_the_tracks_under_it_and_all_tracks_shows_everything(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let afro_house = folder_index(&view, "afro_house", cx);
    assert_eq!(shown_titles(&view, cx).len(), 5);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(afro_house), cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx), ["one.mp3", "two.mp3", "three.mp3"]);
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house")
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("needs-review", cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx), ["three.mp3"]);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(afro_house), cx);
        window.render_frame(cx);
        window.click("search", cx);
        window.input("two", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(shown_titles(&view, cx), ["two.mp3"]);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("all-tracks", cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx), ["two.mp3"]);
    assert_eq!(shown_folder(&view, cx), None);
}

#[gpui::test]
fn sorting_inside_a_folder_groups_the_tracks_by_subfolder(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
        window.render_frame(cx);
        window.click(("column", FOLDER_COLUMN), cx);
    })
    .unwrap();
    assert_eq!(
        shown_titles(&view, cx),
        ["loose.mp3", "three.mp3", "one.mp3", "two.mp3", "four.mp3"]
    );
}

#[gpui::test]
fn the_folder_column_is_relative_to_the_folder_on_show(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    assert_eq!(
        folder_text(&view, "one.mp3", cx),
        joined(&["afro_house", "warmup"])
    );
    assert_eq!(folder_text(&view, "loose.mp3", cx), "");

    let afro_house = folder_index(&view, "afro_house", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(afro_house), cx);
        window.render_frame(cx);
        assert!(window.try_find(("folder-cell", 0_usize)).is_some());
    })
    .unwrap();
    assert_eq!(folder_text(&view, "one.mp3", cx), "warmup");
    assert_eq!(folder_text(&view, "three.mp3", cx), "peak");

    cx.update_window(window, |_, window, cx| {
        window.click(disclosure_id(afro_house), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let warmup = folder_index(&view, "afro_house/warmup", cx);
    cx.update_window(window, |_, window, cx| {
        window.click(folder_row_id(warmup), cx);
        window.render_frame(cx);
        assert!(window.try_find(("folder-cell", 0_usize)).is_none());
    })
    .unwrap();
    assert_eq!(folder_text(&view, "one.mp3", cx), "");
    assert_eq!(shown_titles(&view, cx), ["one.mp3", "two.mp3"]);
}

#[gpui::test]
fn with_two_folders_added_all_tracks_names_the_folder_first(cx: &mut TestAppContext) {
    let mut tracks = dj_tracks();
    tracks.push(DemoTrack {
        path: PathBuf::from("/promos/label/new.mp3"),
        outcome: Err("cannot decode".to_string()),
    });
    let (_, view) = open_folders(
        &[DJ_TRACKS, "/promos"],
        tracks,
        Settings::default(),
        None,
        cx,
    );
    assert_eq!(
        folder_text(&view, "one.mp3", cx),
        joined(&["DJ tracks", "afro_house", "warmup"])
    );
    assert_eq!(
        folder_text(&view, "new.mp3", cx),
        joined(&["promos", "label"])
    );
    assert_eq!(folder_text(&view, "loose.mp3", cx), "DJ tracks");
}

#[gpui::test]
fn the_arrow_keys_move_through_the_folders_and_fold_and_unfold_them(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
        window.press("down", cx);
    })
    .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house")
    );

    cx.update_window(window, |_, window, cx| window.press("right", cx))
        .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        [
            "DJ tracks",
            "  afro_house",
            "    peak",
            "    warmup",
            "  deep"
        ]
    );
    cx.update_window(window, |_, window, cx| window.press("right", cx))
        .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house/peak")
    );
    assert_eq!(shown_titles(&view, cx), ["three.mp3"]);

    cx.update_window(window, |_, window, cx| {
        window.press("down", cx);
        window.press("left", cx);
    })
    .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house")
    );
    cx.update_window(window, |_, window, cx| window.press("left", cx))
        .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
    cx.update_window(window, |_, window, cx| window.press("end", cx))
        .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/deep")
    );
    cx.update_window(window, |_, window, cx| {
        window.press("up", cx);
        window.press("up", cx);
    })
    .unwrap();
    assert_eq!(shown_folder(&view, cx).as_deref(), Some("/music/DJ tracks"));
    assert_eq!(shown_titles(&view, cx).len(), 5);
}

#[gpui::test]
fn the_tree_and_the_folder_on_show_come_back_and_gone_folders_are_tolerated(
    cx: &mut TestAppContext,
) {
    let folder = std::env::temp_dir().join(format!("euphoria-ui-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let settings_path = folder.join("settings.json");
    let (window, view) = open_dj_folder(Settings::default(), Some(settings_path.clone()), cx);
    let afro_house = folder_index(&view, "afro_house", cx);
    let deep = folder_index(&view, "deep", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(disclosure_id(afro_house), cx);
        window.render_frame(cx);
        window.click(folder_row_id(deep), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = Settings::load(&settings_path);
    std::fs::remove_dir_all(&folder).unwrap();
    let saved = saved.unwrap();
    assert_eq!(
        saved.folder_tree.selected,
        Some(Path::new(DJ_TRACKS).join("deep"))
    );

    let (_, reopened) = open_dj_folder(saved.clone(), None, cx);
    assert_eq!(
        listed_folders(&reopened, cx),
        [
            "DJ tracks",
            "  afro_house",
            "    peak",
            "    warmup",
            "  deep"
        ]
    );
    assert_eq!(shown_titles(&reopened, cx), ["four.mp3"]);

    let gone = Settings {
        folder_tree: FolderTreeMemory {
            expanded: Some(vec![PathBuf::from("/not/here"), PathBuf::from(DJ_TRACKS)]),
            selected: Some(PathBuf::from("/not/here/either")),
        },
        ..saved
    };
    let (window, after_loss) = open_dj_folder(gone, None, cx);
    cx.run_until_parked();
    assert_eq!(
        listed_folders(&after_loss, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
    assert_eq!(shown_folder(&after_loss, cx), None);
    assert_eq!(shown_titles(&after_loss, cx).len(), 5);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
    })
    .unwrap();
}

#[gpui::test]
fn a_tree_of_hundreds_of_folders_scrolls_and_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
    let tracks: Vec<DemoTrack> = (0..3_000)
        .map(|number| DemoTrack {
            path: PathBuf::from(format!(
                "/music/DJ tracks/label {:03}/release {}/track {number}.mp3",
                number % 300,
                number % 3
            )),
            outcome: Err("cannot decode".to_string()),
        })
        .collect();
    let (window, view) = open_folders(&[DJ_TRACKS], tracks, Settings::default(), None, cx);
    assert_eq!(listed_folders(&view, cx).len(), 301);
    let last = cx.update(|cx| *view.read(cx).folder_rows.last().unwrap());
    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let tree = window.find("folder-tree").bounds();
        assert!(tree.bottom() <= px(900.0), "{tree:?}");
        assert!(window.try_find(folder_row_id(last)).is_none());
        window.click(folder_row_id(root), cx);
        window.press("end", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find(folder_row_id(last)).is_some());
    })
    .unwrap();
}

#[gpui::test]
fn all_tracks_counts_the_whole_library_while_a_folder_is_on_show(cx: &mut TestAppContext) {
    let mut tracks = dj_tracks();
    tracks.push(DemoTrack {
        path: PathBuf::from("/promos/label/new.mp3"),
        outcome: Err("cannot decode".to_string()),
    });
    let (window, view) = open_folders(
        &[DJ_TRACKS, "/promos"],
        tracks,
        Settings::default(),
        None,
        cx,
    );
    let counts = |view: &Entity<EuphoriaApp>, cx: &mut TestAppContext| {
        cx.update(|cx| {
            let counts = view.read(cx).sidebar_counts();
            (counts.total, counts.needs_review)
        })
    };
    assert_eq!(counts(&view, cx), (6, 2));

    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx).len(), 5);
    assert_eq!(counts(&view, cx), (6, 1));

    let deep = folder_index(&view, "deep", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(deep), cx);
    })
    .unwrap();
    assert_eq!(counts(&view, cx), (6, 0));
}

/// How wide `text` is in the table's text size.
fn text_width(window: &mut gpui::Window, text: &str) -> gpui::Pixels {
    let run = window.text_style().to_run(text.len());
    window
        .text_system()
        .shape_line(
            text.to_string().into(),
            crate::views::TEXT_SIZE,
            &[run],
            None,
        )
        .width
}

#[gpui::test]
fn two_level_folder_paths_show_in_full_at_the_usual_window_size(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let root = folder_index(&view, "", cx);
    let one = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .position(|track| track.file_name == "one.mp3")
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
        window.render_frame(cx);
        let path = joined(&["afro_house", "warmup"]);
        let needed = text_width(window, &path);
        let folder_cell = window.find(("folder-cell", one)).bounds();
        let title = window.find(("column", 1_usize)).bounds();
        assert!(
            folder_cell.size.width >= needed,
            "{folder_cell:?} for {needed:?}"
        );
        assert!(title.size.width >= px(160.0), "{title:?}");
    })
    .unwrap();
}

#[gpui::test]
fn the_smallest_window_leaves_columns_out_before_squeezing_the_title(cx: &mut TestAppContext) {
    for sidebar_folded in [false, true] {
        let panels = Panels {
            sidebar_folded,
            ..Panels::default()
        };
        let (window, _) = open_arranged(ThemeName::Slate, panels, SMALLEST_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let title = window.find(("column", 1_usize)).bounds();
            let energy = window.find(("column", ENERGY_COLUMN)).bounds();
            let row = window.find(track(0)).bounds();
            assert!(
                title.size.width >= px(150.0),
                "folded {sidebar_folded}: {title:?}"
            );
            assert!(energy.right() <= row.right(), "folded {sidebar_folded}");
            assert!(window.try_find(("column", BPM_COLUMN)).is_some());
            assert!(window.try_find(("column", FORMAT_COLUMN)).is_none());
        })
        .unwrap();
    }
}
