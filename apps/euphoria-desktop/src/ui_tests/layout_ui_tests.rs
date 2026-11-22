// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 21 tests: the sidebar, the resizable edges and the remembered arrangement.

pub(crate) const USUAL_WINDOW: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(1440.0),
    height: px(900.0),
};

/// The smallest window the app allows.
const SMALLEST_WINDOW: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(960.0),
    height: px(560.0),
};

/// Folds the sidebar with `fold`, checks the table took the freed width and
/// the way back is still there, then unfolds it with `unfold`.
fn fold_and_unfold(
    theme: ThemeName,
    fold: impl FnOnce(&mut gpui::Window, &mut gpui::App),
    unfold: impl FnOnce(&mut gpui::Window, &mut gpui::App),
    cx: &mut TestAppContext,
) {
    let (window, view) = open_arranged(theme, Panels::default(), USUAL_WINDOW, cx);
    let sidebar_width = px(220.0);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
        assert_eq!(window.find(track(0)).bounds().left(), sidebar_width);
        let search_left = window.find("search").bounds().left();
        let toggle = window.find("sidebar-toggle").bounds();

        fold(window, cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_none());
        assert!(window.try_find("needs-review").is_none());
        assert_eq!(window.find(track(0)).bounds().left(), px(0.0));
        assert_eq!(window.find("search").bounds().left(), search_left);
        assert_eq!(window.find("sidebar-toggle").bounds(), toggle);
        let inspector = window.find("inspector").bounds();
        match AppTheme::named(theme).layout {
            Layout::InspectorRight => assert_eq!(inspector.size.width, px(320.0)),
            Layout::DeckBelow => assert_eq!(inspector.left(), px(0.0)),
        }
    })
    .unwrap();
    cx.update(|cx| assert!(view.read(cx).panels.sidebar_folded));

    cx.update_window(window, |_, window, cx| {
        unfold(window, cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
        assert_eq!(window.find(track(0)).bounds().left(), sidebar_width);
    })
    .unwrap();
    cx.update(|cx| assert!(!view.read(cx).panels.sidebar_folded));
}

fn use_slide_clock(
    view: &Entity<EuphoriaApp>,
    reduces_motion: bool,
    cx: &mut TestAppContext,
) -> std::rc::Rc<std::cell::Cell<f32>> {
    let origin = std::time::Instant::now();
    let seconds = std::rc::Rc::new(std::cell::Cell::new(0.0_f32));
    let clock = seconds.clone();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            let folded = app.panels.sidebar_folded;
            app.sidebar_slide = crate::sidebar_slide::SidebarSlide::at_rest(folded).with_sources(
                move || origin + std::time::Duration::from_secs_f32(clock.get()),
                move || reduces_motion,
            );
        })
    });
    seconds
}

#[gpui::test]
fn the_sidebar_slides_out_while_the_table_grows_into_the_space(cx: &mut TestAppContext) {
    let (window, view) = open_arranged(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    let seconds = use_slide_clock(&view, false, cx);
    let table_left = |seconds: f32, cx: &mut TestAppContext| {
        let _ = seconds;
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find(track(0)).bounds().left()
        })
        .unwrap()
    };
    assert_eq!(table_left(0.0, cx), px(220.0));

    cx.update_window(window, |_, window, cx| window.click("sidebar-toggle", cx))
        .unwrap();
    seconds.set(crate::sidebar_slide::SLIDE_SECONDS / 2.0);
    let middle = table_left(0.0, cx);
    assert!(middle > px(0.0) && middle < px(220.0), "{middle:?}");
    let clipped = cx
        .update_window(window, |_, window, _| {
            window.find("all-tracks").bounds().left()
        })
        .unwrap();
    assert!(clipped < px(0.0), "the text slides out: {clipped:?}");

    seconds.set(1.0);
    assert_eq!(table_left(1.0, cx), px(0.0));
    cx.update(|cx| assert!(view.read(cx).panels.sidebar_folded));
    assert!(
        cx.update_window(window, |_, window, _| window
            .try_find("all-tracks")
            .is_none())
            .unwrap()
    );

    cx.update_window(window, |_, window, cx| window.click("sidebar-toggle", cx))
        .unwrap();
    seconds.set(1.0 + crate::sidebar_slide::SLIDE_SECONDS * 2.0);
    assert_eq!(table_left(0.0, cx), px(220.0));
}

#[gpui::test]
fn with_motion_reduced_the_sidebar_folds_at_once(cx: &mut TestAppContext) {
    let (window, view) = open_arranged(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    let _seconds = use_slide_clock(&view, true, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find(track(0)).bounds().left(), px(0.0));
    })
    .unwrap();
}

#[gpui::test]
fn a_sidebar_restored_folded_does_not_animate(cx: &mut TestAppContext) {
    let panels = Panels {
        sidebar_folded: true,
        ..Panels::default()
    };
    let (window, view) = open_arranged(ThemeName::Slate, panels, USUAL_WINDOW, cx);
    let _seconds = use_slide_clock(&view, false, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(track(0)).bounds().left(), px(0.0));
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.sidebar_slide.is_moving(app.sidebar_slide.now()));
    });
}

#[gpui::test]
fn the_toolbar_has_no_program_button_beside_write_tags(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-no-program-button");
    let files = scratch.copy_fixtures();
    let (window, _view) = open_taggable_files(&scratch, &files, None, cx);
    assert!(is_shown(window, "write-tags", cx));
    assert!(!is_shown(window, "tag-program", cx));
}

#[gpui::test]
fn the_toolbar_button_folds_the_sidebar_and_brings_it_back(cx: &mut TestAppContext) {
    fold_and_unfold(
        ThemeName::Slate,
        |window, cx| window.click("sidebar-toggle", cx),
        |window, cx| window.click("sidebar-toggle", cx),
        cx,
    );
}

#[gpui::test]
fn the_toolbar_button_folds_the_sidebar_in_the_strip_layout_too(cx: &mut TestAppContext) {
    fold_and_unfold(
        ThemeName::Deck,
        |window, cx| window.click("sidebar-toggle", cx),
        |window, cx| window.click("sidebar-toggle", cx),
        cx,
    );
}

/// The shortcut that folds the sidebar on the platform the tests run on.
const SIDEBAR_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "ctrl-cmd-s"
} else {
    "ctrl-alt-s"
};

#[gpui::test]
fn the_keyboard_shortcut_folds_the_sidebar_and_brings_it_back(cx: &mut TestAppContext) {
    fold_and_unfold(
        ThemeName::Graphite,
        |window, cx| window.press(SIDEBAR_SHORTCUT, cx),
        |window, cx| window.press(SIDEBAR_SHORTCUT, cx),
        cx,
    );
}

#[gpui::test]
fn the_empty_first_run_window_has_no_track_detail_to_resize(cx: &mut TestAppContext) {
    for theme in [ThemeName::Slate, ThemeName::Deck] {
        let (window, _) = open_window_with(
            Vec::new(),
            Box::new(SilentOutput),
            Settings {
                theme,
                ..Settings::default()
            },
            None,
            USUAL_WINDOW,
            cx,
        );
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("all-tracks").is_some(), "{theme:?}");
            assert!(window.try_find("inspector").is_none(), "{theme:?}");
            assert!(window.try_find("panel-edge").is_none(), "{theme:?}");
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_inspector_edge_lands_under_the_pointer_with_the_sidebar_shown_or_folded(
    cx: &mut TestAppContext,
) {
    for sidebar_folded in [false, true] {
        let panels = Panels {
            sidebar_folded,
            ..Panels::default()
        };
        let (window, view) = open_arranged(ThemeName::Slate, panels, USUAL_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            for pointer_x in [1_000.0, 1_100.0, 900.0] {
                drag_edge_to(window, gpui::point(px(pointer_x), px(400.0)), cx);
                let inspector = window.find("inspector").bounds();
                let edge = window.find("panel-edge").bounds();
                assert_eq!(inspector.left(), px(pointer_x), "folded {sidebar_folded}");
                assert_eq!(edge.center().x, px(pointer_x), "folded {sidebar_folded}");
                assert_eq!(window.find(track(0)).bounds().right(), px(pointer_x));
            }
        })
        .unwrap();
        cx.update(|cx| assert_eq!(view.read(cx).panels.inspector_width, 540));
    }
}

#[gpui::test]
fn everything_in_the_strip_fits_at_its_shortest(cx: &mut TestAppContext) {
    let shortest = Panels {
        strip_height: 0,
        ..Panels::default()
    };
    for window_size in [USUAL_WINDOW, SMALLEST_WINDOW] {
        let (window, _) = open_arranged(ThemeName::Deck, shortest, window_size, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let strip = window.find("inspector").bounds();
            assert_eq!(strip.size.height, px(142.0));
            let inside_bottom = strip.bottom() - px(16.0);
            for part in ["waveform", "play-button"] {
                let bounds = window.find(part).bounds();
                assert!(
                    bounds.top() >= strip.top(),
                    "{part}: {bounds:?} in {strip:?}"
                );
                assert!(
                    bounds.bottom() <= inside_bottom,
                    "{part}: {bounds:?} in {strip:?}"
                );
            }
            assert!(window.find("waveform").bounds().size.height >= px(64.0));
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_sidebar_folds_on_the_empty_first_run_window_as_well(cx: &mut TestAppContext) {
    let (window, _) = open_library(0, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_none());
        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
    })
    .unwrap();
}

/// Presses on the middle of the edge of the track detail and releases at
/// `released_at`.
fn drag_edge_to(window: &mut gpui::Window, released_at: Point<gpui::Pixels>, cx: &mut gpui::App) {
    window.render_frame(cx);
    let pressed_at = window.find("panel-edge").bounds().center();
    window.drag(pressed_at, released_at, cx);
    window.render_frame(cx);
}

fn open_with_decks(
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_arranged(ThemeName::Slate, Panels::default(), window_size, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_decks(cx)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    (window, view)
}

fn drag_decks_edge_to(window: &mut gpui::Window, pointer_y: f32, cx: &mut gpui::App) {
    window.render_frame(cx);
    let pressed_at = window.find("decks-edge").bounds().center();
    window.drag(pressed_at, gpui::point(pressed_at.x, px(pointer_y)), cx);
    window.render_frame(cx);
    window.render_frame(cx);
}

fn decks_height(window: &mut gpui::Window) -> gpui::Pixels {
    window.find("decks").bounds().size.height
}

fn column_bounds(view: &Entity<EuphoriaApp>, cx: &mut gpui::App) -> Bounds<gpui::Pixels> {
    view.read(cx)
        .decks_column_bounds
        .get()
        .expect("the column was drawn")
}

#[gpui::test]
fn dragging_the_top_of_the_decks_down_gives_the_table_more_rows(cx: &mut TestAppContext) {
    let (window, view) = open_with_decks(USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        let fitted = decks_height(window);
        let edge_y = window.find("decks-edge").bounds().center().y;

        drag_decks_edge_to(window, edge_y / px(1.0) + 200.0, cx);
        assert!((decks_height(window) - (fitted - px(200.0))).abs() <= px(1.0));
        let edge_moved_to = window.find("decks-edge").bounds().center().y;
        assert!(
            (edge_moved_to - (edge_y + px(200.0))).abs() <= px(1.0),
            "the edge follows the pointer"
        );

        drag_decks_edge_to(window, USUAL_WINDOW.height / px(1.0) - 2.0, cx);
        assert_eq!(decks_height(window), px(crate::panels::SHORTEST_DECKS));

        drag_decks_edge_to(window, 0.0, cx);
        let column = column_bounds(&view, cx);
        let table_height = window.find("decks").bounds().top() - column.top();
        assert!(
            (table_height - px(crate::panels::SHORTEST_TABLE)).abs() <= px(1.0),
            "dragged up, the decks stop where the table keeps its shortest height: {table_height:?}"
        );
        assert!(decks_height(window) <= fitted);
    })
    .unwrap();
    cx.update(|cx| assert!(view.read(cx).panels.decks_height.is_some()));

    cx.update_window(window, |_, window, cx| {
        drag_decks_edge_to(window, USUAL_WINDOW.height / px(1.0) - 2.0, cx);
        window.double_click("decks-edge", cx);
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).panels, Panels::default()));
}

#[gpui::test]
fn dragged_decks_leave_the_table_its_shortest_height_in_the_smallest_window(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_with_decks(SMALLEST_WINDOW, cx);
    cx.update(|cx| view.update(cx, |app, _| app.panels.decks_height = Some(5_000)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let column = column_bounds(&view, cx);
        let decks_top = window.find("decks").bounds().top();
        assert!(decks_top - column.top() >= px(crate::panels::SHORTEST_TABLE));
        assert!(window.try_find(track(0)).is_some(), "a row stays visible");
    })
    .unwrap();
}

#[gpui::test]
fn dragging_the_edge_of_the_inspector_resizes_it_between_its_limits(cx: &mut TestAppContext) {
    let (window, view) = open_arranged(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(320.0));

        drag_edge_to(window, gpui::point(px(900.0), px(400.0)), cx);
        let inspector = window.find("inspector").bounds();
        assert_eq!(inspector.size.width, px(540.0));
        assert_eq!(inspector.left(), px(900.0));
        assert_eq!(window.find(track(0)).bounds().right(), px(900.0));
        let waveform = window.find("waveform").bounds();
        assert_eq!(waveform.size.width, px(540.0 - 33.0));

        drag_edge_to(window, gpui::point(px(1_400.0), px(400.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(288.0));
        assert_eq!(
            window.find("waveform").bounds().size.width,
            px(288.0 - 33.0)
        );

        drag_edge_to(window, gpui::point(px(30.0), px(400.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(640.0));
        let table_width = window.find(track(0)).bounds().size.width;
        assert_eq!(table_width, px(1440.0 - 220.0 - 640.0));
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.panels.inspector_width, 640);
        assert_eq!(app.resizing, None);
    });
}

#[gpui::test]
fn a_double_click_on_the_edge_returns_the_inspector_to_its_usual_width(cx: &mut TestAppContext) {
    let wide = Panels {
        inspector_width: 500,
        ..Panels::default()
    };
    let (window, view) = open_arranged(ThemeName::Slate, wide, USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(500.0));
        window.double_click("panel-edge", cx);
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(320.0));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).panels, Panels::default()));
}

#[gpui::test]
fn dragging_the_edge_of_the_strip_resizes_it_and_the_waveform_grows_with_it(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_arranged(ThemeName::Deck, Panels::default(), USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let usual = window.find("inspector").bounds();
        assert_eq!(usual.size.height, px(180.0));
        assert_eq!(usual.bottom(), px(900.0));
        let usual_waveform_height = window.find("waveform").bounds().size.height;

        drag_edge_to(window, gpui::point(px(700.0), px(560.0)), cx);
        let taller = window.find("inspector").bounds();
        assert_eq!(taller.size.height, px(340.0));
        assert_eq!(taller.top(), px(560.0));
        let taller_waveform_height = window.find("waveform").bounds().size.height;
        assert_eq!(taller_waveform_height - usual_waveform_height, px(160.0));

        drag_edge_to(window, gpui::point(px(700.0), px(890.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.height, px(142.0));

        drag_edge_to(window, gpui::point(px(700.0), px(10.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.height, px(420.0));
        assert!(window.find(track(0)).bounds().bottom() <= px(900.0 - 420.0));

        window.double_click("panel-edge", cx);
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.height, px(180.0));
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.panels, Panels::default());
        assert_eq!(app.resizing, None);
    });
}

#[gpui::test]
fn the_smallest_window_keeps_the_table_whatever_width_was_remembered(cx: &mut TestAppContext) {
    let remembered = Panels {
        inspector_width: 640,
        ..Panels::default()
    };
    let (window, view) = open_arranged(ThemeName::Slate, remembered, SMALLEST_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let inspector = window.find("inspector").bounds();
        let row = window.find(track(0)).bounds();
        assert_eq!(inspector.size.width, px(340.0));
        assert_eq!(inspector.right(), px(960.0));
        assert_eq!(row.size.width, px(400.0));
        assert!(window.find("player-bar").bounds().bottom() <= px(560.0));

        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(560.0));
        assert_eq!(window.find(track(0)).bounds().size.width, px(400.0));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).panels.inspector_width, 640));
}

#[gpui::test]
fn the_smallest_window_keeps_table_rows_whatever_strip_height_was_remembered(
    cx: &mut TestAppContext,
) {
    let remembered = Panels {
        strip_height: 420,
        sidebar_folded: true,
        ..Panels::default()
    };
    let (window, _) = open_arranged(ThemeName::Deck, remembered, SMALLEST_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let inspector = window.find("inspector").bounds();
        let first_row = window.find(track(0)).bounds();
        assert_eq!(inspector.bottom(), px(560.0));
        assert!(inspector.size.height >= px(142.0));
        assert!(
            inspector.top() - first_row.top() >= px(200.0 - 70.0),
            "{:?} of table above a strip of {:?}",
            inspector.top() - first_row.top(),
            inspector.size.height
        );
        assert!(first_row.bottom() <= inspector.top());
    })
    .unwrap();
}

#[gpui::test]
fn the_arrangement_is_written_to_the_settings_file_and_comes_back(cx: &mut TestAppContext) {
    let folder = std::env::temp_dir().join(format!("euphoria-ui-panels-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let settings_path = folder.join("settings.json");
    let (window, _) = open_window_with(
        fixture_tracks(),
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        USUAL_WINDOW,
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        drag_edge_to(window, gpui::point(px(1_000.0), px(400.0)), cx);
        window.click("sidebar-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = Settings::load(&settings_path);
    std::fs::remove_dir_all(&folder).unwrap();
    let saved = saved.unwrap();
    assert_eq!(saved.panels.inspector_width, 440);
    assert!(saved.panels.sidebar_folded);
    assert_eq!(saved.panels.strip_height, 180);
}
