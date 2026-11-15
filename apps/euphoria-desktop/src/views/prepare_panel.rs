// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sheet that prepares the selected files for a club system: the
//! options, a table with what each file gets, and a progress bar while the
//! copies are written.

use euphoria_core::prepare::{LossyOutput, TrackPlan};
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, TestSupportExt, div, px, relative};

use crate::app::EuphoriaApp;
use crate::prepare_files::{
    CEILINGS_DB, PreparePanel, PrepareStage, PreviewRow, TARGETS_LUFS, destination_text, files,
};
use crate::surface_motion::Modal;
use crate::theme::Palette;
use crate::views::controls::{segment, segmented, toggle_row};
use crate::views::{
    GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_SMALL, SMALL_TEXT_SIZE,
    TITLE_TEXT_SIZE, dialog_primary_button, modal, secondary_button, section_label, tabular_digits,
};

const NAME_MINIMUM_WIDTH: f32 = 160.0;
const LOUDNESS_WIDTH: f32 = 130.0;
const GAIN_WIDTH: f32 = 64.0;
const LIMITING_WIDTH: f32 = 72.0;
const TUNING_WIDTH: f32 = 72.0;
const NOTE_WIDTH: f32 = 120.0;
const TABLE_HEIGHT: f32 = 240.0;

pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<AnyElement> {
    let panel = app.prepare_panel.as_ref()?;
    let palette = app.theme.palette;
    let (backdrop, card) = modal(
        "prepare-files",
        px(780.0),
        &palette,
        app.modal_fades.value(Modal::Prepare),
    );
    let card = card.id("prepare-files-card");
    let contents = match panel.stage {
        PrepareStage::Setup => setup_contents(app, panel, &palette, cx),
        PrepareStage::Writing => writing_contents(panel, &palette),
    };
    Some(
        backdrop
            .child(card.children(contents).test_support())
            .test_support()
            .into_any_element(),
    )
}

fn title(text: String) -> AnyElement {
    div()
        .text_size(TITLE_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
        .into_any_element()
}

fn quiet(text: impl Into<gpui::SharedString>, palette: &Palette) -> gpui::Div {
    div()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(text.into())
}

fn choices(children: Vec<AnyElement>, palette: &Palette) -> AnyElement {
    div()
        .flex()
        .child(segmented(children, palette))
        .into_any_element()
}

fn labelled(label: &str, control: AnyElement, palette: &Palette) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .child(section_label(label.to_string(), palette))
        .child(control)
        .into_any_element()
}

fn number_label(value: f32) -> String {
    let text = format!("{value:.1}");
    text.replace('-', "\u{2212}")
}

fn setup_contents(
    app: &EuphoriaApp,
    panel: &PreparePanel,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let mut contents = vec![
        title(format!("Prepare {}", files(panel.tracks.len()))),
        quiet(
            "Writes new 24 bit copies at one loudness, under a true-peak ceiling, with the tags and cover art. The originals are never changed.",
            palette,
        )
        .into_any_element(),
    ];

    let targets = TARGETS_LUFS
        .into_iter()
        .enumerate()
        .map(|(index, target)| {
            segment(
                format!("prepare-loudness-{target}"),
                format!("{} LUFS", number_label(target).trim_end_matches(".0")),
                panel.settings.target_lufs == target,
                index == 0,
                palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_prepare_options(|panel| panel.settings.target_lufs = target, cx)
            }))
            .test_support()
            .into_any_element()
        })
        .collect();
    let ceilings = CEILINGS_DB
        .into_iter()
        .enumerate()
        .map(|(index, ceiling)| {
            segment(
                format!("prepare-ceiling-{ceiling}"),
                format!("{} dBTP", number_label(ceiling)),
                panel.settings.true_peak_ceiling_db == ceiling,
                index == 0,
                palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_prepare_options(
                    |panel| panel.settings.true_peak_ceiling_db = ceiling,
                    cx,
                )
            }))
            .test_support()
            .into_any_element()
        })
        .collect();
    let lossy = [(LossyOutput::Flac, "FLAC"), (LossyOutput::Wav, "WAV")]
        .into_iter()
        .enumerate()
        .map(|(index, (choice, label))| {
            segment(
                format!("prepare-lossy-{}", label.to_lowercase()),
                label,
                panel.lossy == choice,
                index == 0,
                palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_prepare_options(|panel| panel.lossy = choice, cx)
            }))
            .test_support()
            .into_any_element()
        })
        .collect();
    contents.push(
        div()
            .flex()
            .flex_wrap()
            .gap(GAP_MEDIUM)
            .child(labelled("Loudness", choices(targets, palette), palette))
            .child(labelled(
                "True-peak ceiling",
                choices(ceilings, palette),
                palette,
            ))
            .child(labelled(
                "MP3 and AAC become",
                choices(lossy, palette),
                palette,
            ))
            .into_any_element(),
    );
    contents.push(
        toggle_row(
            "prepare-tune",
            "Correct tuning",
            "Tracks measured off 440 Hz are shifted back to concert pitch. The tempo stays the same.",
            panel.settings.correct_tuning,
            &app.switches,
            palette,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.change_prepare_options(
                |panel| panel.settings.correct_tuning = !panel.settings.correct_tuning,
                cx,
            )
        }))
        .test_support()
        .into_any_element(),
    );
    contents.push(
        div()
            .flex()
            .items_center()
            .gap(GAP_MEDIUM)
            .child(
                div()
                    .id("prepare-destination")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(format!(
                        "Copies go to: {}",
                        destination_text(panel.output_folder.as_deref())
                    ))
                    .test_support(),
            )
            .when(panel.output_folder.is_some(), |row| {
                row.child(
                    secondary_button("Next to originals", palette)
                        .id("prepare-next-to-originals")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.change_prepare_options(|panel| panel.output_folder = None, cx)
                        }))
                        .test_support(),
                )
            })
            .child(
                secondary_button("Choose folder\u{2026}", palette)
                    .id("prepare-choose-folder")
                    .on_click(cx.listener(|this, _, _, cx| this.choose_prepare_folder(cx)))
                    .test_support(),
            )
            .into_any_element(),
    );

    contents.push(section_label("What each file gets", palette).into_any_element());
    contents.push(table(panel, palette));
    contents.push(summary_line(panel, palette));
    if let Some(problem) = &panel.problem {
        contents.push(
            div()
                .id("prepare-problem")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.danger)
                .child(problem.clone())
                .test_support()
                .into_any_element(),
        );
    }
    let can_write = panel.files_to_write() > 0 && !panel.is_measuring();
    let confirm = dialog_primary_button(
        if panel.files_to_write() > 0 {
            format!("Prepare {}", files(panel.files_to_write()))
        } else {
            "Prepare".to_string()
        },
        palette,
    )
    .id("prepare-confirm")
    .when(can_write, |button| {
        button.on_click(cx.listener(|this, _, _, cx| this.confirm_prepare_files(cx)))
    })
    .when(!can_write, |button| button.opacity(0.5))
    .test_support()
    .into_any_element();
    contents.push(
        div()
            .flex()
            .justify_end()
            .gap(GAP_SMALL)
            .pt(px(4.0))
            .child(
                secondary_button("Cancel", palette)
                    .id("prepare-cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.close_prepare_files(cx)))
                    .test_support(),
            )
            .child(confirm)
            .into_any_element(),
    );
    contents
}

fn summary_line(panel: &PreparePanel, palette: &Palette) -> AnyElement {
    let text = if panel.is_measuring() {
        format!(
            "Measuring: {} of {} done.",
            crate::format::count(panel.measured_count()),
            crate::format::count(panel.tracks.len())
        )
    } else {
        let capped = panel
            .rows
            .iter()
            .filter(|row| row.plan().is_some_and(|plan| plan.gain_capped))
            .count();
        let mut text = format!("{} will be written.", files(panel.files_to_write()));
        if capped > 0 {
            text.push_str(&format!(
                " {capped} would need more than {:.0} dB of limiting, so they get less gain.",
                panel.settings.maximum_limiting_db
            ));
        }
        text
    };
    quiet(text, palette)
        .id("prepare-summary")
        .test_support()
        .into_any_element()
}

fn table(panel: &PreparePanel, palette: &Palette) -> AnyElement {
    let header = row_cells(
        div().text_color(palette.text_tertiary),
        "File".to_string(),
        "Loudness, LUFS".to_string(),
        "Gain".to_string(),
        "Limiting".to_string(),
        "Tuning".to_string(),
        "Note".to_string(),
    )
    .text_size(LABEL_TEXT_SIZE)
    .font_weight(FontWeight::SEMIBOLD)
    .pb(GAP_TIGHT)
    .border_b_1()
    .border_color(palette.line);
    div()
        .id("prepare-table")
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .id("prepare-rows")
                .flex()
                .flex_col()
                .max_h(px(TABLE_HEIGHT))
                .overflow_y_scroll()
                .children(
                    panel
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(index, row)| preview_row(index, row, palette)),
                ),
        )
        .test_support()
        .into_any_element()
}

fn row_cells(
    base: gpui::Div,
    name: String,
    loudness: String,
    gain: String,
    limiting: String,
    tuning: String,
    note: String,
) -> gpui::Div {
    let cell = |width: f32, text: String| {
        div()
            .flex_shrink_0()
            .w(px(width))
            .truncate()
            .font_features(tabular_digits())
            .child(text)
    };
    base.flex()
        .items_center()
        .gap(GAP_SMALL)
        .py(GAP_TIGHT)
        .child(
            div()
                .flex_1()
                .min_w(px(NAME_MINIMUM_WIDTH))
                .truncate()
                .child(name),
        )
        .child(cell(LOUDNESS_WIDTH, loudness))
        .child(cell(GAIN_WIDTH, gain))
        .child(cell(LIMITING_WIDTH, limiting))
        .child(cell(TUNING_WIDTH, tuning))
        .child(cell(NOTE_WIDTH, note))
}

fn preview_row(index: usize, row: &PreviewRow, palette: &Palette) -> AnyElement {
    let dash = "\u{2013}".to_string();
    let (loudness, gain, limiting, tuning, note, note_colour) = match &row.outcome {
        None => (
            dash.clone(),
            dash.clone(),
            dash.clone(),
            dash.clone(),
            "Measuring\u{2026}".to_string(),
            palette.text_tertiary,
        ),
        Some(Err(reason)) => (
            dash.clone(),
            dash.clone(),
            dash.clone(),
            dash.clone(),
            reason.clone(),
            palette.danger,
        ),
        Some(Ok(plan)) => plan_cells(plan, palette),
    };
    row_cells(
        div().text_size(SMALL_TEXT_SIZE),
        row.file_name.clone(),
        loudness,
        gain,
        limiting,
        tuning,
        note,
    )
    .id(("prepare-row", index))
    .text_color(palette.text)
    .child(div().w(px(0.0)).h(px(0.0)).text_color(note_colour))
    .test_support()
    .into_any_element()
}

fn plan_cells(
    plan: &TrackPlan,
    palette: &Palette,
) -> (String, String, String, String, String, gpui::Hsla) {
    let dash = "\u{2013}".to_string();
    if let Some(reason) = plan.skipped {
        return (
            dash.clone(),
            dash.clone(),
            dash.clone(),
            dash,
            format!("Skipped: {}", reason_word(reason)),
            palette.text_tertiary,
        );
    }
    let loudness = match (plan.measured_lufs, plan.predicted_lufs) {
        (Some(before), Some(after)) => {
            format!("{} \u{2192} {}", number_label(before), number_label(after))
        }
        _ => dash.clone(),
    };
    let tuning = plan
        .tuning_correction_cents
        .map(|cents| format!("{:+.0} cents", cents).replace('-', "\u{2212}"))
        .unwrap_or_else(|| dash.clone());
    let mut notes = Vec::new();
    if plan.gain_capped {
        notes.push("Gain capped".to_string());
    }
    if plan.clipping.runs > 0 {
        notes.push(format!("Clipped \u{d7}{}", plan.clipping.runs));
    }
    let colour = if notes.is_empty() {
        palette.text_tertiary
    } else {
        palette.warning
    };
    (
        loudness,
        format!("{:+.1} dB", plan.gain_db).replace('-', "\u{2212}"),
        format!("{:.1} dB", plan.limiting_db),
        tuning,
        notes.join(", "),
        colour,
    )
}

fn reason_word(reason: euphoria_core::prepare::SkipReason) -> &'static str {
    match reason {
        euphoria_core::prepare::SkipReason::Silent => "silent",
        euphoria_core::prepare::SkipReason::TooShort => "too short",
    }
}

fn writing_contents(panel: &PreparePanel, palette: &Palette) -> Vec<AnyElement> {
    let total = panel.files_to_write().max(1);
    let (done, current) = panel.progress.position();
    let fraction = ((done as f32 + current) / total as f32).clamp(0.0, 1.0);
    vec![
        title(format!("Preparing {}", files(panel.files_to_write()))),
        quiet(
            format!(
                "{} of {} done. Nothing else in this window changes your files meanwhile.",
                crate::format::count(done.min(total)),
                crate::format::count(total)
            ),
            palette,
        )
        .id("prepare-writing")
        .test_support()
        .into_any_element(),
        div()
            .id("prepare-progress")
            .h(px(6.0))
            .w_full()
            .rounded(RADIUS_SMALL)
            .bg(palette.raised)
            .child(
                div()
                    .h_full()
                    .w(relative(fraction))
                    .rounded(RADIUS_SMALL)
                    .bg(palette.accent),
            )
            .test_support()
            .into_any_element(),
    ]
}
