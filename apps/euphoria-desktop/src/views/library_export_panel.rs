// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sheet that exports the library to a DJ program: the program, the
//! tracks, the program's options and the file or database, then a review of
//! what would be written, then the result.

use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::format;
use crate::library_export::{
    ExportProgram, ExportReport, ExportScope, ExportSetup, ExportStage, LibraryExportPanel,
};
use crate::tag_settings::NotationChoice;
use crate::theme::Palette;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, modal, primary_button, secondary_button, section_label};

pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<AnyElement> {
    let panel = app.library_export.as_ref()?;
    let palette = app.theme.palette;
    let (backdrop, card) = modal("library-export", px(640.0), &palette);
    let card = card.id("library-export-card");
    let contents = match &panel.stage {
        ExportStage::Setup => setup_contents(app, panel, &palette, cx),
        ExportStage::Working { writing } => vec![
            title("Export library"),
            quiet(
                if *writing {
                    "Writing. Nothing else in this window changes your files meanwhile."
                } else {
                    "Checking. Nothing is written yet."
                },
                &palette,
            ),
        ],
        ExportStage::Review(report) => review_contents(panel, report, &palette, cx),
        ExportStage::Done(report) => {
            let mut contents = vec![title("Export finished")];
            contents.extend(report_lines(report, &palette));
            contents.push(buttons(vec![
                primary_button("Done", &palette)
                    .id("export-done")
                    .on_click(cx.listener(|this, _, _, cx| this.close_library_export(cx)))
                    .test_support()
                    .into_any_element(),
            ]));
            contents
        }
    };
    Some(
        backdrop
            .child(card.children(contents).test_support())
            .test_support()
            .into_any_element(),
    )
}

fn setup_contents(
    app: &EuphoriaApp,
    panel: &LibraryExportPanel,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let setup = &panel.setup;
    let program = setup.program;
    let total = app.session.library().tracks().len();
    let shown = app.visible.len();
    let mut contents = vec![title("Export library")];

    contents.push(section_label("Program", palette).into_any_element());
    contents.push(row(ExportProgram::ALL
        .into_iter()
        .map(|choice| {
            choice_button(
                format!("export-program-{}", choice.id()),
                choice.label(),
                choice == program,
                palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_export_setup(|setup| setup.choose_program(choice), cx)
            }))
            .test_support()
            .into_any_element()
        })
        .collect()));

    contents.push(section_label("Tracks", palette).into_any_element());
    contents.push(row(vec![
        choice_button(
            "export-scope-library",
            format!("Whole library ({})", format::count(total)),
            setup.scope == ExportScope::Library,
            palette,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_export_setup(|setup| setup.scope = ExportScope::Library, cx)
        }))
        .test_support()
        .into_any_element(),
        choice_button(
            "export-scope-shown",
            format!("Tracks shown ({})", format::count(shown)),
            setup.scope == ExportScope::Shown,
            palette,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_export_setup(|setup| setup.scope = ExportScope::Shown, cx)
        }))
        .test_support()
        .into_any_element(),
    ]));

    if program.has_key_notation() {
        contents.push(section_label("Key notation", palette).into_any_element());
        contents.push(row(NotationChoice::ALL
            .into_iter()
            .map(|notation| {
                choice_button(
                    format!("export-notation-{notation:?}"),
                    notation.label(),
                    setup.key_notation == notation,
                    palette,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_export_setup(|setup| setup.key_notation = notation, cx)
                }))
                .test_support()
                .into_any_element()
            })
            .collect()));
    }
    if program.has_label() {
        contents.push(section_label("Key and energy label", palette).into_any_element());
        contents.push(row(program
            .labels()
            .into_iter()
            .map(|label| {
                choice_button(
                    format!("export-label-{label:?}"),
                    label.label(),
                    setup.label == label,
                    palette,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_export_setup(|setup| setup.label = label, cx)
                }))
                .test_support()
                .into_any_element()
            })
            .collect()));
    }
    if program.has_beat_grid() {
        let mut toggles = vec![
            toggle(
                "export-beat-grid",
                "Beat grid",
                setup.beat_grid,
                palette,
                cx,
                |setup| setup.beat_grid = !setup.beat_grid,
            ),
            if setup.can_write_cues() {
                toggle("export-cues", "Cues", setup.cues, palette, cx, |setup| {
                    setup.cues = !setup.cues
                })
            } else {
                choice_button("export-cues", "Cues", false, palette)
                    .opacity(0.5)
                    .test_support()
                    .into_any_element()
            },
        ];
        if matches!(program, ExportProgram::Rekordbox | ExportProgram::Traktor) {
            toggles.push(toggle(
                "export-playlists",
                "A playlist per folder",
                setup.playlists,
                palette,
                cx,
                |setup| setup.playlists = !setup.playlists,
            ));
        }
        contents.push(section_label("Contents", palette).into_any_element());
        contents.push(row(toggles));
        if !setup.can_write_cues() {
            contents.push(quiet(
                if program.has_collection() {
                    "Cues need the beat grid and your collection file, so that no track loses its own cues."
                } else {
                    "Cues need the beat grid."
                },
                palette,
            ));
        }
    }
    if program.has_collection() && setup.beat_grid {
        contents.push(path_row(
            "Your collection file",
            setup.collection.as_deref(),
            "export-choose-collection",
            cx.listener(|this, _, _, cx| this.choose_export_source(true, cx)),
            palette,
        ));
        if setup.collection.is_some() {
            contents.push(row(vec![toggle(
                "export-allow-unmatched",
                "Export even though cued tracks that match no file cannot be protected",
                setup.allow_unmatched_cued_tracks,
                palette,
                cx,
                |setup| setup.allow_unmatched_cued_tracks = !setup.allow_unmatched_cued_tracks,
            )]));
        }
    }
    if program.edits_in_place() {
        contents.push(path_row(
            if program == ExportProgram::EngineDj {
                "Engine Library folder"
            } else {
                "VirtualDJ database.xml"
            },
            setup.target.as_deref(),
            "export-choose-target",
            cx.listener(|this, _, _, cx| this.choose_export_source(false, cx)),
            palette,
        ));
        contents.push(quiet(
            "This edits your database in place. A copy is saved first, the export is refused while the program runs, and nothing is written before you review it.",
            palette,
        ));
    } else {
        contents.push(path_row(
            "File to write",
            setup.output.as_deref(),
            "export-choose-output",
            cx.listener(|this, _, _, cx| this.choose_export_output(cx)),
            palette,
        ));
    }
    if let Some(problem) = &panel.problem {
        contents.push(
            div()
                .id("export-problem")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.accent)
                .child(problem.clone())
                .test_support()
                .into_any_element(),
        );
    }
    let review: AnyElement = if setup.is_complete() {
        primary_button("Review", palette)
            .id("export-review")
            .on_click(cx.listener(|this, _, _, cx| this.review_library_export(cx)))
            .test_support()
            .into_any_element()
    } else {
        primary_button("Review", palette)
            .id("export-review")
            .opacity(0.5)
            .test_support()
            .into_any_element()
    };
    contents.push(buttons(vec![
        secondary_button("Cancel", palette)
            .id("export-cancel")
            .on_click(cx.listener(|this, _, _, cx| this.close_library_export(cx)))
            .test_support()
            .into_any_element(),
        review,
    ]));
    contents
}

fn review_contents(
    panel: &LibraryExportPanel,
    report: &ExportReport,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let mut contents = vec![title("Review the export")];
    contents.extend(report_lines(report, palette));
    let label = if panel.setup.program.edits_in_place() {
        format!("Edit {} database", panel.setup.program.label())
    } else {
        "Export".to_string()
    };
    let can_write = report.track_count > 0 || !panel.setup.program.edits_in_place();
    let confirm: AnyElement = if can_write {
        primary_button(label, palette)
            .id("export-confirm")
            .on_click(cx.listener(|this, _, _, cx| this.confirm_library_export(cx)))
            .test_support()
            .into_any_element()
    } else {
        primary_button(label, palette)
            .id("export-confirm")
            .opacity(0.5)
            .test_support()
            .into_any_element()
    };
    contents.push(buttons(vec![
        secondary_button("Back", palette)
            .id("export-back")
            .on_click(cx.listener(|this, _, _, cx| this.change_export_setup(|_| {}, cx)))
            .test_support()
            .into_any_element(),
        confirm,
    ]));
    contents
}

fn report_lines(report: &ExportReport, palette: &Palette) -> Vec<AnyElement> {
    let mut lines = vec![
        div()
            .id("export-headline")
            .text_size(SMALL_TEXT_SIZE)
            .child(report.headline.clone())
            .test_support()
            .into_any_element(),
    ];
    lines.extend(report.details.iter().map(|line| quiet(line, palette)));
    lines
}

fn toggle(
    id: &'static str,
    label: &'static str,
    chosen: bool,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
    change: fn(&mut ExportSetup),
) -> AnyElement {
    choice_button(id, label, chosen, palette)
        .on_click(cx.listener(move |this, _, _, cx| this.change_export_setup(change, cx)))
        .test_support()
        .into_any_element()
}

fn path_row(
    label: &str,
    path: Option<&std::path::Path>,
    id: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    palette: &Palette,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(format!(
                    "{label}: {}",
                    path.map(|path| path.display().to_string())
                        .unwrap_or_else(|| "not chosen".to_string())
                )),
        )
        .child(
            secondary_button("Choose\u{2026}", palette)
                .id(id)
                .on_click(on_click)
                .test_support(),
        )
        .into_any_element()
}

fn row(children: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(6.0))
        .children(children)
        .into_any_element()
}

fn title(text: &str) -> AnyElement {
    div()
        .text_size(px(15.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.to_string())
        .into_any_element()
}

fn quiet(text: &str, palette: &Palette) -> AnyElement {
    div()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(text.to_string())
        .into_any_element()
}

fn buttons(buttons: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .justify_end()
        .gap(px(8.0))
        .pt(px(4.0))
        .children(buttons)
        .into_any_element()
}
