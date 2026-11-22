// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filter bar of the track list as the window runs it: typing a range,
//! choosing qualities, and clearing.

use gpui::component::input::InputState;
use gpui::{Context, Entity, Window};

use crate::app::EuphoriaApp;
use crate::format;
use crate::track_filters::{QualityChoice, SetStage, TypedTrackFilters};

impl EuphoriaApp {
    /// Reads the four range fields into the filters and narrows the list.
    pub(crate) fn apply_track_filter_inputs(&mut self, cx: &mut Context<'_, Self>) {
        let typed = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let typed = TypedTrackFilters {
            bpm_from: typed(&self.track_filter_inputs.bpm_from),
            bpm_to: typed(&self.track_filter_inputs.bpm_to),
            energy_from: typed(&self.track_filter_inputs.energy_from),
            energy_to: typed(&self.track_filter_inputs.energy_to),
        };
        let filters = self.track_filters.with_typed(&typed);
        self.set_track_filters(filters, cx);
    }

    /// Adds `choice` to the qualities shown, or takes it out.
    pub(crate) fn toggle_track_quality(
        &mut self,
        choice: QualityChoice,
        cx: &mut Context<'_, Self>,
    ) {
        let mut filters = self.track_filters.clone();
        filters.toggle_quality(choice);
        self.set_track_filters(filters, cx);
    }

    /// Adds `stage` to the set stages shown, or takes it out.
    pub(crate) fn toggle_track_stage(&mut self, stage: SetStage, cx: &mut Context<'_, Self>) {
        let mut filters = self.track_filters.clone();
        filters.toggle_stage(stage);
        self.set_track_filters(filters, cx);
    }

    /// Turns matching half and double the tempo range on or off.
    pub(crate) fn toggle_half_or_double(&mut self, cx: &mut Context<'_, Self>) {
        let mut filters = self.track_filters.clone();
        filters.half_or_double = !filters.half_or_double;
        self.set_track_filters(filters, cx);
    }

    /// Takes every filter off and empties the fields.
    pub(crate) fn clear_track_filters(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let cleared = crate::track_filters::TrackFilters {
            half_or_double: self.track_filters.half_or_double,
            ..Default::default()
        };
        self.set_track_filters(cleared, cx);
        self.show_track_filters(window, cx);
    }

    /// Opens the filter bar, or closes it when no filter is set.
    pub(crate) fn toggle_filter_bar(&mut self, cx: &mut Context<'_, Self>) {
        if self.track_filters.is_active() {
            return;
        }
        self.is_filter_bar_open = !self.is_filter_bar_open;
        cx.notify();
    }

    fn set_track_filters(
        &mut self,
        filters: crate::track_filters::TrackFilters,
        cx: &mut Context<'_, Self>,
    ) {
        if filters == self.track_filters {
            return;
        }
        self.track_filters = filters;
        self.refresh_visible();
        self.save_settings(cx);
        cx.notify();
    }

    /// Fills the range fields with the filters, as when the app opens.
    pub(crate) fn show_track_filters(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let filters = self.track_filters.clone();
        let fill = |input: &Entity<InputState>,
                    text: Option<String>,
                    window: &mut Window,
                    cx: &mut Context<'_, Self>| {
            input.update(cx, |input, cx| {
                input.set_value(text.unwrap_or_default(), window, cx);
            });
        };
        let inputs = &self.track_filter_inputs;
        fill(
            &inputs.bpm_from,
            filters.bpm_min.map(format::bpm),
            window,
            cx,
        );
        fill(&inputs.bpm_to, filters.bpm_max.map(format::bpm), window, cx);
        fill(
            &inputs.energy_from,
            filters.energy_min.map(|energy| energy.to_string()),
            window,
            cx,
        );
        fill(
            &inputs.energy_to,
            filters.energy_max.map(|energy| energy.to_string()),
            window,
            cx,
        );
    }
}
