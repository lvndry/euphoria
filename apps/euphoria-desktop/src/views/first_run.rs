// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The card of three first steps: add music, analyze it, fill in tags. It
//! fills in as the steps happen, and shows over the table until the first
//! analysis and the first write are done.

use gpui::component::ActiveTheme;
use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::first_run::{StepState, Steps};
use crate::format;
use crate::tag_settings::ProgramChoice;
use crate::theme::Palette;
use crate::views::controls::choice_button;
use crate::views::{GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_LARGE};
use crate::views::{SMALL_TEXT_SIZE, primary_button};

/// What the third step promises, in the words of the tag settings.
pub(crate) const TAGS_PROMISE: &str =
    "Missing keys and tempos only. Tags you already have are kept, and every write can be undone.";

/// The line under step two while analysis runs, such as
/// "312 of 575 · about 4 min left".
pub(crate) fn analysis_line(app: &EuphoriaApp) -> Option<String> {
    let progress = app.session.progress();
    let (finished, queued) = progress.counts()?;
    let counts = format!("{} of {}", format::count(finished), format::count(queued));
    Some(match progress.time_left_label() {
        Some(time_left) => format!("{counts} \u{b7} {time_left}"),
        None => counts,
    })
}

/// "Writing for rekordbox" when a DJ program is chosen for the tags.
pub(crate) fn program_line(app: &EuphoriaApp) -> Option<String> {
    match app.tag_settings.program {
        ProgramChoice::Generic => None,
        program => Some(format!("Writing for {}", program.label())),
    }
}

/// The three steps as a card.
pub(crate) fn card(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let mono = cx.theme().mono_font_family.clone();
    let has_music = !app.session.library().tracks().is_empty();
    let first_run = if has_music {
        app.first_run
    } else {
        crate::first_run::FirstRun::default()
    };
    let steps = Steps::of(first_run, has_music);
    let track_count = app.session.library().tracks().len();

    let add_music = step(
        "first-run-add-music",
        1,
        "Add music",
        steps.add_music,
        &palette,
        &mono,
    )
    .child(match steps.add_music {
        StepState::Done => detail(format!("{} added", tracks(track_count)), &palette),
        _ => detail(
            "Drop a folder anywhere in the window, or choose one. MP3, M4A, MP4, FLAC, Ogg Vorbis, WAV and AIFF, searched all the way down.",
            &palette,
        ),
    })
    .when(steps.add_music != StepState::Done, |step| {
        step.child(
            primary_button("Choose folder", &palette)
                .id("choose-folder")
                .mt(px(6.0))
                .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx)))
                .test_support(),
        )
    });

    let analyze = step(
        "first-run-analyze",
        2,
        "Analyze",
        steps.analyze,
        &palette,
        &mono,
    )
    .child(match steps.analyze {
        StepState::Done => detail("Keys and tempos found", &palette).into_any_element(),
        StepState::Now => detail(
            analysis_line(app).unwrap_or_else(|| "Waiting for the files".to_string()),
            &palette,
        )
        .id("first-run-analysis-line")
        .test_support()
        .into_any_element(),
        StepState::Later => {
            detail("Finds the key and tempo of each track", &palette).into_any_element()
        }
    });

    let fill_in_tags = step(
        "first-run-fill-in-tags",
        3,
        "Fill in tags",
        steps.fill_in_tags,
        &palette,
        &mono,
    )
    .child(match steps.fill_in_tags {
        StepState::Done => detail("Tags written", &palette),
        _ => detail(TAGS_PROMISE, &palette),
    })
    .when_some(
        program_line(app).filter(|_| steps.fill_in_tags != StepState::Done),
        |step, line| {
            step.child(
                div()
                    .id("first-run-program")
                    .text_size(SMALL_TEXT_SIZE)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.text)
                    .child(line)
                    .test_support(),
            )
        },
    )
    .when(steps.fill_in_tags == StepState::Now, |step| {
        step.child(
            choice_button("first-run-write-tags", "Write tags", false, &palette)
                .mt(px(6.0))
                .self_start()
                .on_click(cx.listener(|this, _, _, cx| this.open_tag_writer(cx)))
                .test_support(),
        )
    });

    div().w_full().child(
        div()
            .id("first-run")
            .grid()
            .grid_cols(3)
            .gap(GAP_SMALL)
            .px(px(12.0))
            .py(px(10.0))
            .child(add_music.test_support())
            .child(analyze.test_support())
            .child(fill_in_tags.test_support())
            .test_support(),
    )
}

fn tracks(count: usize) -> String {
    if count == 1 {
        "1 track".to_string()
    } else {
        format!("{} tracks", format::count(count))
    }
}

fn step(
    id: &'static str,
    number: usize,
    title: &'static str,
    state: StepState,
    palette: &Palette,
    mono: &SharedString,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .min_w_0()
        .p(px(10.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(if state == StepState::Now {
            palette.accent
        } else {
            palette.line
        })
        .bg(palette.panel)
        .when(state == StepState::Done, |step| step.opacity(0.6))
        .child(
            div()
                .font_family(mono.clone())
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(number.to_string()),
        )
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(title),
        )
}

fn detail(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(text.into())
}
