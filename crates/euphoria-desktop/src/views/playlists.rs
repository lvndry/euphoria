// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playlists in the window: their section of the sidebar, the menus of a
//! playlist and of the table's rows, and the line over a playlist's table
//! that shows it as a set at a glance.

use gpui::component::input::Input;
use gpui::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, FontWeight, Hsla, PathBuilder, Render, SharedString,
    TestSupportExt, WeakEntity, Window, canvas, div, point, px,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::playlist_flow::{DraggedPlaylist, DraggedTracks, PlaylistFormat};
use crate::playlists::{LABELS, Playlist, PlaylistId, SetShape};
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, section_label};

/// Height of a playlist's row in the sidebar.
const ROW_HEIGHT: f32 = 28.0;

/// The tallest the list of playlists grows before it scrolls.
const LIST_MAXIMUM_HEIGHT: f32 = 220.0;

/// How many keys the key path shows before it says how many more there are.
const KEY_PATH_LIMIT: usize = 24;

/// What follows the pointer while something is dragged.
pub(crate) struct DragLabel {
    pub(crate) text: SharedString,
    pub(crate) background: Hsla,
    pub(crate) foreground: Hsla,
}

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(self.background)
            .text_color(self.foreground)
            .text_size(SMALL_TEXT_SIZE)
            .font_weight(FontWeight::MEDIUM)
            .child(self.text.clone())
    }
}

/// The element id of a playlist's row in the sidebar.
pub(crate) fn playlist_row_id(id: PlaylistId) -> (&'static str, usize) {
    ("playlist", id.number())
}

/// The Playlists section of the sidebar: a heading with a button that makes
/// a new playlist, and a row for each playlist.
pub(crate) fn sidebar_section(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let hover_background = palette.raised;
    let playlists = app.session.playlists().all();
    let library = app.session.library();
    let rows: Vec<AnyElement> = playlists
        .iter()
        .map(|playlist| {
            let in_library = playlist
                .tracks
                .iter()
                .filter(|path| library.index_of(path).is_some())
                .count();
            playlist_row(app, playlist, in_library, cx)
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(8.0))
                .pt(px(12.0))
                .pb(px(4.0))
                .child(section_label("Playlists", &palette))
                .child(
                    div()
                        .id("new-playlist")
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(18.0))
                        .rounded(px(4.0))
                        .text_color(palette.text_tertiary)
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover_background))
                        .child(Icon::new(IconName::Plus).size(px(12.0)))
                        .tooltip(|window, cx| {
                            Tooltip::new("New playlist. Drag tracks onto a playlist to add them.")
                                .build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create_playlist(window, cx);
                        }))
                        .test_support(),
                ),
        )
        .when(rows.is_empty(), |section| {
            section.child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(palette.text_tertiary)
                    .child("No playlists yet"),
            )
        })
        .child(
            div()
                .id("playlist-list")
                .flex()
                .flex_col()
                .gap(px(1.0))
                .max_h(px(LIST_MAXIMUM_HEIGHT))
                .overflow_y_scroll()
                .children(rows)
                .child(
                    // Dropping a playlist below the last one moves it to the
                    // end.
                    div()
                        .id("playlist-list-end")
                        .h(px(6.0))
                        .flex_shrink_0()
                        .drag_over::<DraggedPlaylist>(move |style, _, _, _| {
                            style.border_t_2().border_color(palette.accent)
                        })
                        .on_drop(cx.listener(|this, dragged: &DraggedPlaylist, _, cx| {
                            this.drop_playlist(dragged.0, None, cx);
                        })),
                ),
        )
}

fn playlist_row(
    app: &EuphoriaApp,
    playlist: &Playlist,
    in_library: usize,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let id = playlist.id;
    let is_current = app.playlist == Some(id) && !app.set_builder.is_open;
    let hover_background = palette.raised;
    let name: SharedString = playlist.name.clone().into();
    let drag_name = name.clone();
    let is_renaming = app.renaming_playlist == Some(id);
    let app_handle = cx.entity().downgrade();

    let label = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .child(match &playlist.label {
            Some(emoji) => format!("{emoji} {name}"),
            None => name.to_string(),
        });
    div()
        .id(playlist_row_id(id))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.0))
        .h(px(ROW_HEIGHT))
        .px(px(8.0))
        .rounded(px(6.0))
        .map(|row| {
            if is_current {
                row.bg(palette.raised)
                    .text_color(palette.text)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                row.text_color(palette.text_secondary)
                    .hover(move |style| style.bg(hover_background.opacity(0.6)))
            }
        })
        .cursor_pointer()
        .map(|row| {
            if is_renaming {
                row.child(
                    div()
                        .flex_1()
                        .child(Input::new(&app.playlist_name_input).id("playlist-name")),
                )
            } else {
                row.child(label).child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(palette.text_tertiary)
                        .child(format::count(in_library)),
                )
            }
        })
        .on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    this.start_renaming_playlist(id, window, cx);
                } else {
                    this.show_playlist(id, cx);
                }
            }),
        )
        .on_drag(DraggedPlaylist(id), move |_, _, _, cx| {
            let text = drag_name.clone();
            cx.new(|_| DragLabel {
                text,
                background: palette.accent,
                foreground: palette.accent_ink,
            })
        })
        .drag_over::<DraggedTracks>(move |style, _, _, _| style.bg(palette.accent.opacity(0.25)))
        .drag_over::<DraggedPlaylist>(move |style, _, _, _| {
            style.border_t_2().border_color(palette.accent)
        })
        .on_drop(cx.listener(move |this, dragged: &DraggedTracks, _, cx| {
            this.drop_tracks_on_playlist(id, *dragged, cx);
        }))
        .on_drop(cx.listener(move |this, dragged: &DraggedPlaylist, _, cx| {
            this.drop_playlist(dragged.0, Some(id), cx);
        }))
        .test_support()
        .context_menu(move |menu, window, cx| playlist_menu(menu, id, &app_handle, window, cx))
        .into_any_element()
}

/// Runs `action` on the app from a menu item.
fn on_app(
    app: &WeakEntity<EuphoriaApp>,
    action: impl Fn(&mut EuphoriaApp, &mut Window, &mut Context<'_, EuphoriaApp>) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let app = app.clone();
    move |_, window, cx| {
        app.update(cx, |this, cx| action(this, window, cx)).ok();
    }
}

/// The menu of a playlist: rename, label, export and delete.
fn playlist_menu(
    menu: PopupMenu,
    id: PlaylistId,
    app: &WeakEntity<EuphoriaApp>,
    window: &mut Window,
    cx: &mut Context<'_, PopupMenu>,
) -> PopupMenu {
    let label_app = app.clone();
    menu.item(
        PopupMenuItem::new("Rename").on_click(on_app(app, move |this, window, cx| {
            this.start_renaming_playlist(id, window, cx);
        })),
    )
    .submenu("Label", window, cx, move |menu, _, _| {
        let with_labels = LABELS.iter().fold(menu, |menu, label| {
            menu.item(
                PopupMenuItem::new(*label).on_click(on_app(&label_app, move |this, _, cx| {
                    this.set_playlist_label(id, Some(label), cx)
                })),
            )
        });
        with_labels
            .separator()
            .item(PopupMenuItem::new("No label").on_click(on_app(
                &label_app,
                move |this, _, cx| {
                    this.set_playlist_label(id, None, cx);
                },
            )))
    })
    .separator()
    .item(
        PopupMenuItem::new("Export for rekordbox\u{2026}")
            .on_click(on_app(app, move |this, _, cx| {
                this.export_playlist(id, PlaylistFormat::Rekordbox, cx)
            })),
    )
    .item(
        PopupMenuItem::new("Export as M3U8\u{2026}").on_click(on_app(app, move |this, _, cx| {
            this.export_playlist(id, PlaylistFormat::M3u8, cx);
        })),
    )
    .separator()
    .item(
        PopupMenuItem::new("Delete playlist").on_click(on_app(app, move |this, _, cx| {
            this.delete_playlist(id, cx);
        })),
    )
}

/// The menu of the table's rows: add the selection to a playlist, make a
/// playlist of it, or take it out of the playlist on show.
pub(crate) fn track_menu(
    menu: PopupMenu,
    app: &WeakEntity<EuphoriaApp>,
    playlists: &[(PlaylistId, String)],
    shown_playlist: Option<PlaylistId>,
    window: &mut Window,
    cx: &mut Context<'_, PopupMenu>,
) -> PopupMenu {
    let targets: Vec<(PlaylistId, String)> = playlists
        .iter()
        .filter(|(id, _)| Some(*id) != shown_playlist)
        .cloned()
        .collect();
    let submenu_app = app.clone();
    let menu = if targets.is_empty() {
        menu
    } else {
        menu.submenu("Add to playlist", window, cx, move |menu, _, _| {
            targets.iter().fold(menu, |menu, (id, name)| {
                let id = *id;
                menu.item(
                    PopupMenuItem::new(name.clone())
                        .on_click(on_app(&submenu_app, move |this, _, cx| {
                            this.add_selection_to_playlist(id, cx)
                        })),
                )
            })
        })
    };
    let menu = menu.item(
        PopupMenuItem::new("New playlist from selection")
            .on_click(on_app(app, |this, window, cx| {
                this.create_playlist_from_selection(window, cx)
            })),
    );
    if shown_playlist.is_some() {
        menu.separator()
            .item(PopupMenuItem::new("Remove from playlist").on_click(on_app(
                app,
                |this, _, cx| {
                    this.remove_selection_from_playlist(cx);
                },
            )))
    } else {
        menu
    }
}

/// The line over a playlist's table: its keys in order with the steps out
/// of key marked, its tempo range and its energy as a small line, and the
/// export buttons.
pub(crate) fn set_summary(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    let id = app.playlist?;
    let playlist = app.session.playlists().get(id)?;
    let library = app.session.library();
    let shape = SetShape::of(
        playlist
            .tracks
            .iter()
            .filter_map(|path| {
                library
                    .index_of(path)
                    .and_then(|index| library.track(index))
            })
            .map(|track| (track.key(), track.bpm(), track.energy())),
    );
    let theme = &app.theme;
    let palette = theme.palette;
    let notation = app.display.key_notation;

    let shown_keys = shape.keys.len().min(KEY_PATH_LIMIT);
    let key_path = div()
        .id("set-key-path")
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(2.0))
        .min_w_0()
        .children(shape.keys.iter().take(KEY_PATH_LIMIT).enumerate().flat_map(
            |(position, (key, is_smooth))| {
                let step = (position > 0).then(|| {
                    div()
                        .text_size(LABEL_TEXT_SIZE)
                        .text_color(if *is_smooth {
                            palette.text_tertiary
                        } else {
                            palette.danger
                        })
                        .child(if *is_smooth { "\u{203a}" } else { "\u{00d7}" })
                        .into_any_element()
                });
                step.into_iter()
                    .chain(std::iter::once(
                        key_chip(key, theme, notation).into_any_element(),
                    ))
            },
        ))
        .when(shape.keys.len() > shown_keys, |path| {
            path.child(
                div()
                    .text_size(LABEL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(format!("+{}", shape.keys.len() - shown_keys)),
            )
        })
        .tooltip({
            let clashes = shape.clashes();
            move |window, cx| {
                Tooltip::new(match clashes {
                    0 => "The keys in play order. Every step is a compatible move.".to_string(),
                    1 => "The keys in play order. The step marked \u{00d7} is not a compatible move."
                        .to_string(),
                    count => format!(
                        "The keys in play order. The {count} steps marked \u{00d7} are not compatible moves."
                    ),
                })
                .build(window, cx)
            }
        })
        .test_support();

    let tempo = shape.tempo_range.map(|(slowest, fastest)| {
        div()
            .id("set-tempo-range")
            .flex_shrink_0()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(if (fastest - slowest).abs() < 0.05 {
                format!("{} BPM", format::bpm(slowest))
            } else {
                format!(
                    "{}\u{2013}{} BPM",
                    format::bpm(slowest),
                    format::bpm(fastest)
                )
            })
            .test_support()
    });

    let energy = (shape.energies.len() >= 2).then(|| energy_line(&shape.energies, &palette));

    let export_button = |element_id: &'static str, label: &'static str, format: PlaylistFormat| {
        let hover_background = palette.raised;
        div()
            .id(element_id)
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(5.0))
            .border_1()
            .border_color(palette.line)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.export_playlist(id, format, cx)))
            .test_support()
    };

    Some(
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .min_w_0()
            .child(key_path)
            .children(tempo)
            .children(energy)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child("Export")
                    .child(export_button(
                        "export-rekordbox",
                        "rekordbox",
                        PlaylistFormat::Rekordbox,
                    ))
                    .child(export_button("export-m3u8", "M3U8", PlaylistFormat::M3u8)),
            ),
    )
}

/// The energy of each track in play order as a small line, from 1 at the
/// bottom to 10 at the top.
fn energy_line(energies: &[u8], palette: &Palette) -> AnyElement {
    let points: Vec<f32> = energies
        .iter()
        .map(|energy| (f32::from(*energy).clamp(1.0, 10.0) - 1.0) / 9.0)
        .collect();
    let colour = palette.accent;
    let summary: SharedString = format!(
        "Energy in play order: {}",
        energies
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
    .into();
    div()
        .id("set-energy")
        .flex_shrink_0()
        .w(px(72.0))
        .h(px(18.0))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    let step = bounds.size.width / (points.len() - 1) as f32;
                    let mut path = PathBuilder::stroke(px(1.5));
                    for (index, height) in points.iter().enumerate() {
                        let at = point(
                            bounds.origin.x + step * index as f32,
                            bounds.origin.y + bounds.size.height * (1.0 - height),
                        );
                        if index == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, colour);
                    }
                },
            )
            .size_full(),
        )
        .tooltip(move |window, cx| Tooltip::new(summary.clone()).build(window, cx))
        .test_support()
        .into_any_element()
}
