// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that draw sidebar rows at the sidebar's width, in a headless
//! window, and check that their counts show in full.

use gpui::prelude::*;
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, Bounds, Context, IntoElement, Point, Render, TestAppContext, TestSupportExt,
    Window, WindowBounds, WindowOptions, div, px, size,
};

use crate::app::text_width;
use crate::panels::SIDEBAR_WIDTH;
use crate::theme::{AppTheme, ThemeName};
use crate::views::TEXT_SIZE;
use crate::views::sidebar::{RowCount, count_badge, row};

const LONG_LABEL: &str = "Tracks the analysis was not sure about at all";

struct SidebarRows {
    theme: AppTheme,
}

impl Render for SidebarRows {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        let palette = self.theme.palette;
        div()
            .flex()
            .flex_col()
            .w(px(SIDEBAR_WIDTH))
            .px(px(8.0))
            .bg(palette.sidebar)
            .text_size(TEXT_SIZE)
            .child(
                row(
                    LONG_LABEL,
                    Some(count_badge("exact-count", RowCount::Settled(628), &palette)),
                    false,
                    &palette,
                )
                .id("exact-row")
                .test_support(),
            )
            .child(
                row(
                    LONG_LABEL,
                    Some(count_badge(
                        "compact-count",
                        RowCount::Settling(2_200),
                        &palette,
                    )),
                    true,
                    &palette,
                )
                .id("compact-row")
                .test_support(),
            )
    }
}

fn open_rows(theme: ThemeName, cx: &mut TestAppContext) -> AnyWindowHandle {
    cx.update(|cx| {
        gpui::init(cx);
        let (window, _) = gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(SIDEBAR_WIDTH), px(200.0)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| SidebarRows {
                    theme: AppTheme::named(theme),
                })
            },
        )
        .unwrap();
        window
    })
}

#[gpui::test]
fn sidebar_counts_show_in_full_beside_a_long_label_in_every_theme(cx: &mut TestAppContext) {
    for theme in ThemeName::ALL {
        let window = open_rows(theme, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            for (row_id, count_id, text) in [
                ("exact-row", "exact-count", "628"),
                ("compact-row", "compact-count", "2.2k"),
            ] {
                let row = window.find(row_id).bounds();
                let count = window.find(count_id);
                assert!(count.visible(), "{theme:?} {text}");
                let bounds = count.bounds();
                let full_width = text_width(window, text, TEXT_SIZE);
                assert!(
                    bounds.size.width / px(1.0) >= full_width - 0.5,
                    "{theme:?}: {text} needs {full_width}px and got {:?}",
                    bounds.size.width
                );
                assert!(
                    bounds.right() <= row.right(),
                    "{theme:?}: {text} runs past its row"
                );
                assert!(
                    row.right() <= px(SIDEBAR_WIDTH),
                    "{theme:?}: the row stays inside the sidebar"
                );
            }
        })
        .unwrap();
    }
}
