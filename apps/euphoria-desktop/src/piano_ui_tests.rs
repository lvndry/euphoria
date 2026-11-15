// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano of "Check by ear" tells apart the keys outside the scale, the
//! keys of the scale and the keys sounding now, in every theme.

use euphoria_core::{Key, Mode};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px};

use crate::app::EuphoriaApp;
use crate::inspector_sections::SectionKind;
use crate::panels::Panels;
use crate::playback::testing::ListeningOutput;
use crate::settings::Settings;
use crate::tag_settings::NotationChoice;
use crate::theme::{AppTheme, Layout, ThemeName};
use crate::ui_tests::{
    USUAL_WINDOW, click, open_sections, open_window_with, track, tracks_to_review,
};
use crate::views::by_ear::{KeyLook, heard_keys, piano_looks, sounding_fill};
use crate::views::inspector_section::tab_id;

const B_FLAT: usize = 10;
const BLACK_KEYS: [usize; 5] = [1, 3, 6, 8, 10];

fn open_d_minor(
    theme: ThemeName,
    panels: Panels,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let settings = Settings {
        theme,
        panels,
        ..Settings::default()
    };
    let (window, view) = open_window_with(
        tracks_to_review(),
        Box::new(ListeningOutput::default()),
        settings,
        None,
        USUAL_WINDOW,
        cx,
    );
    open_sections(&view, &[SectionKind::ByEar], cx);
    click(window, track(1), cx);
    cx.update(|cx| view.update(cx, |app, cx| app.accept_key(Key::new(2, Mode::Minor), cx)));
    cx.run_until_parked();
    if AppTheme::named(theme).layout == Layout::DeckBelow {
        click(window, tab_id(SectionKind::ByEar), cx);
    }
    (window, view)
}

fn drawn_looks(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> [KeyLook; 12] {
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let (key, _) = heard_keys(app.selected_track().unwrap(), app.key_shift).unwrap();
        assert_eq!(key, Key::new(2, Mode::Minor));
        piano_looks(key, app)
    })
}

fn sounding_keys(looks: &[KeyLook; 12], sounding: gpui::Hsla) -> Vec<usize> {
    (0..12)
        .filter(|&pitch_class| looks[pitch_class].fill == sounding)
        .collect()
}

#[gpui::test]
fn scale_keys_never_look_like_sounding_keys_in_any_theme(cx: &mut TestAppContext) {
    for theme in ThemeName::ALL {
        let (window, view) = open_d_minor(theme, Panels::default(), cx);
        let sounding = cx.update(|cx| sounding_fill(&view.read(cx).theme.palette));

        let resting = drawn_looks(window, &view, cx);
        assert!(
            sounding_keys(&resting, sounding).is_empty(),
            "{theme:?}: nothing plays, so no key is lit"
        );
        assert_ne!(
            resting[B_FLAT].fill, sounding,
            "{theme:?}: Bb is only marked"
        );
        assert!(
            resting[B_FLAT].mark.is_some(),
            "{theme:?}: Bb is in D minor"
        );
        for black in BLACK_KEYS {
            let in_scale = black == B_FLAT;
            assert_eq!(
                resting[black].mark.is_some(),
                in_scale,
                "{theme:?}: {black}"
            );
            if !in_scale {
                assert_ne!(
                    resting[black].fill, resting[B_FLAT].fill,
                    "{theme:?}: a black key outside the scale is plain"
                );
            }
        }

        click(window, ("chord", 0_usize), cx);
        let playing = drawn_looks(window, &view, cx);
        assert_eq!(
            sounding_keys(&playing, sounding),
            [2, 5, 9],
            "{theme:?}: D F A sound"
        );

        cx.executor()
            .advance_clock(std::time::Duration::from_millis(2500));
        cx.run_until_parked();
        let after = drawn_looks(window, &view, cx);
        assert!(
            sounding_keys(&after, sounding).is_empty(),
            "{theme:?}: the keys go dark when the chord ends"
        );
        assert_eq!(after, resting, "{theme:?}");
    }
}

#[gpui::test]
fn the_piano_legend_stays_on_one_line_at_the_narrowest_inspector(cx: &mut TestAppContext) {
    for theme in ThemeName::ALL {
        let panels = Panels {
            inspector_width: 0,
            ..Panels::default()
        };
        let (window, view) = open_d_minor(theme, panels, cx);
        for notation in NotationChoice::ALL {
            cx.update(|cx| {
                view.update(cx, |app, cx| {
                    app.tag_settings.key_notation = notation;
                    cx.notify();
                })
            });
            let height = cx
                .update_window(window, |_, window, cx| {
                    window.render_frame(cx);
                    window.find("piano-legend").bounds().size.height
                })
                .unwrap();
            assert!(height < px(20.0), "{theme:?} {notation:?}: {height:?}");
        }
    }
}
