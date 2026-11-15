// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The energy journey of a built set: the bars that stand for its tracks,
//! the phases of its curve, and the words that sum the set up.

use std::path::Path;

use euphoria_core::harmony::set::{DEFAULT_ENERGY_TOLERANCE, DEFAULT_MAX_BPM_STEP};
use euphoria_core::{Deviation, SetPlan};

use crate::app::EuphoriaApp;
use crate::set_builder::PlannedSet;

/// The room kept at each side of the chart, in pixels.
pub(crate) const CHART_PAD: f32 = 14.0;

/// The widest a bar gets, in pixels, however few tracks the set has.
pub(crate) const MAX_BAR_WIDTH: f32 = 28.0;

/// The share of its slot a bar fills.
pub(crate) const BAR_SHARE: f32 = 0.62;

/// The shortest a bar is drawn, so the lowest energy still shows.
pub(crate) const MIN_BAR_HEIGHT: f32 = 4.0;

const MIN_PHASE_SHARE: f32 = 0.08;
const MIN_PHASE_RISE: f32 = 2.0;
const PEAK_BAND: f32 = 1.0;

/// How far up the chart `energy` stands, from 0 for 1 to 1 for 10.
pub(crate) fn level_fraction(energy: f32) -> f32 {
    ((energy - 1.0) / 9.0).clamp(0.0, 1.0)
}

/// The height of the bar of `energy` in a chart `chart_height` tall.
pub(crate) fn bar_height(energy: f32, chart_height: f32) -> f32 {
    (level_fraction(energy) * chart_height).max(MIN_BAR_HEIGHT)
}

/// Where the start of the curve and its length lie across a chart
/// `width` wide, in pixels. With a set of `count` tracks the curve runs
/// from the middle of the first bar's slot to the middle of the last.
pub(crate) fn curve_span(width: f32, count: Option<usize>) -> (f32, f32) {
    match count {
        Some(count) if count >= 2 => {
            let slot = (width - 2.0 * CHART_PAD) / count as f32;
            (CHART_PAD + slot / 2.0, width - 2.0 * CHART_PAD - slot)
        }
        _ => (0.0, width),
    }
}

/// How far along the curve a pointer `offset` pixels into a chart `width`
/// wide is, from 0 to 1.
pub(crate) fn curve_fraction_at(offset: f32, width: f32, count: Option<usize>) -> f32 {
    let (start, span) = curve_span(width, count);
    if span <= 0.0 {
        return 0.0;
    }
    ((offset - start) / span).clamp(0.0, 1.0)
}

/// The share of the width of a row inside the chart's margins at which
/// the curve is `fraction` along, for a set of `count` tracks.
pub(crate) fn share_at_curve(fraction: f32, count: usize) -> f32 {
    let count = count.max(2) as f32;
    0.5 / count + fraction * (1.0 - 1.0 / count)
}

/// A stretch of the curve with a name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Phase {
    pub(crate) label: &'static str,
    /// Where it starts and ends along the curve, from 0 to 1.
    pub(crate) from: f32,
    pub(crate) to: f32,
}

/// The warm-up, the peak and the close of a curve, from its shape: the
/// peak is where it stays within a level of its highest point, the warm-up
/// the rise before it and the close the fall after it. A part is named
/// only when the curve is at least two levels lower there and the part
/// is wide enough to carry its name; a flat curve has no phases.
pub(crate) fn phases(curve: &[f32]) -> Vec<Phase> {
    if curve.len() < 2 {
        return Vec::new();
    }
    let highest = curve.iter().copied().fold(f32::MIN, f32::max);
    let lowest = curve.iter().copied().fold(f32::MAX, f32::min);
    if highest - lowest < MIN_PHASE_RISE {
        return Vec::new();
    }
    let last = (curve.len() - 1) as f32;
    let in_peak = |level: f32| level >= highest - PEAK_BAND;
    let first_peak = curve.iter().position(|level| in_peak(*level)).unwrap_or(0);
    let last_peak = curve.iter().rposition(|level| in_peak(*level)).unwrap_or(0);
    let peak_from = first_peak as f32 / last;
    let peak_to = last_peak as f32 / last;
    let mut result = Vec::new();
    let has_warm_up = curve[0] <= highest - MIN_PHASE_RISE && peak_from >= MIN_PHASE_SHARE;
    let has_close =
        curve[curve.len() - 1] <= highest - MIN_PHASE_RISE && 1.0 - peak_to >= MIN_PHASE_SHARE;
    if has_warm_up {
        result.push(Phase {
            label: "Warm-up",
            from: 0.0,
            to: peak_from,
        });
    }
    result.push(Phase {
        label: "Peak",
        from: if has_warm_up { peak_from } else { 0.0 },
        to: if has_close { peak_to } else { 1.0 },
    });
    if has_close {
        result.push(Phase {
            label: "Close",
            from: peak_to,
            to: 1.0,
        });
    }
    result
}

/// One track of the set as a bar of the journey chart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Bar {
    pub(crate) position: usize,
    pub(crate) energy: f32,
    pub(crate) target: f32,
    pub(crate) off_curve: bool,
    pub(crate) is_locked: bool,
    pub(crate) is_playing: bool,
    pub(crate) is_selected: bool,
    pub(crate) title: String,
    pub(crate) tooltip: String,
}

impl EuphoriaApp {
    /// The bars of the built set, one per track in order.
    pub(crate) fn journey_bars(&self) -> Vec<Bar> {
        let Some(planned) = self.set_builder.planned.as_ref() else {
            return Vec::new();
        };
        let library = self.session.library();
        planned
            .plan
            .tracks
            .iter()
            .enumerate()
            .map(|(position, track)| {
                let path: Option<&Path> = planned.paths.get(position).map(|path| path.as_path());
                let title = path
                    .and_then(|path| library.track_by_path(path))
                    .map(|found| found.title().to_string())
                    .or_else(|| path.map(|path| path.display().to_string()))
                    .unwrap_or_default();
                let tooltip = format!(
                    "{title}\n{} · {:.1} BPM · energy {:.0}, wants {:.1}",
                    self.display.key_notation.spell(&track.key),
                    track.bpm,
                    track.energy,
                    track.target_energy
                );
                Bar {
                    position,
                    energy: track.energy,
                    target: track.target_energy,
                    off_curve: is_off_target(&planned.plan, position),
                    is_locked: planned.locked.get(position).copied().unwrap_or(false),
                    is_playing: self.set_playing_position() == Some(position),
                    is_selected: self.set_builder.selected_row == Some(position),
                    title,
                    tooltip,
                }
            })
            .collect()
    }

    /// The seconds each track of the set lasts, zero where unknown.
    pub(crate) fn set_track_seconds(&self, planned: &PlannedSet) -> Vec<f32> {
        let library = self.session.library();
        planned
            .paths
            .iter()
            .map(|path| {
                library
                    .track_by_path(path)
                    .and_then(|track| track.duration_seconds())
                    .unwrap_or(0.0)
            })
            .collect()
    }
}

pub(crate) fn is_off_target(plan: &SetPlan, position: usize) -> bool {
    plan.deviations.iter().any(|deviation| {
        matches!(deviation, Deviation::EnergyOffTarget { position: off, .. } if *off == position)
    })
}

fn is_rule_break(deviation: &Deviation) -> bool {
    matches!(
        deviation,
        Deviation::KeyClash { .. } | Deviation::TempoJump { .. } | Deviation::RepeatedArtist { .. }
    )
}

/// How many rules the set breaks, one for each key clash, tempo jump and
/// repeated artist.
pub(crate) fn rule_break_count(plan: &SetPlan) -> usize {
    plan.deviations
        .iter()
        .filter(|deviation| is_rule_break(deviation))
        .count()
}

/// The rule each break into the track at `position` goes against.
pub(crate) fn rule_breaks_into(plan: &SetPlan, position: usize) -> Vec<String> {
    plan.deviations
        .iter()
        .filter(|deviation| is_rule_break(deviation))
        .filter(|deviation| match **deviation {
            Deviation::KeyClash { position: at, .. }
            | Deviation::TempoJump { position: at, .. }
            | Deviation::RepeatedArtist { position: at } => at == position,
            _ => false,
        })
        .map(|deviation| {
            format!(
                "{} {}",
                deviation_message(deviation),
                deviation_reason(deviation)
            )
        })
        .collect()
}

/// Whether the row at `position` is flagged: off the curve or the end of a
/// mix that breaks a rule.
pub(crate) fn is_flagged(plan: &SetPlan, position: usize) -> bool {
    is_off_target(plan, position) || !rule_breaks_into(plan, position).is_empty()
}

/// The energy cell of a row: the energy, and what the curve asks for.
pub(crate) fn energy_text(energy: f32, target: f32, off_curve: bool) -> String {
    if off_curve {
        format!("{energy:.0} (wants {target:.1})")
    } else {
        format!("{energy:.0} of {target:.1}")
    }
}

/// The length of a set for a chip: "1 h 32", or "48 min" under an hour.
pub(crate) fn set_length_text(seconds: f32) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as u64;
    if minutes >= 60 {
        format!("{} h {:02}", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// How a chip of the summary is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipTone {
    Neutral,
    Warm,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SummaryChip {
    pub(crate) text: String,
    pub(crate) tone: ChipTone,
}

/// The chips that sum the set up: how close its energy follows the curve,
/// how many rules it breaks, and how long it is. A fourth says so when
/// the pool was too small.
pub(crate) fn summary_chips(plan: &SetPlan, track_seconds: f32) -> Vec<SummaryChip> {
    let furthest = plan
        .tracks
        .iter()
        .map(|track| track.energy_error().abs())
        .fold(0.0, f32::max);
    let off_curve = plan
        .deviations
        .iter()
        .any(|deviation| matches!(deviation, Deviation::EnergyOffTarget { .. }));
    let breaks = rule_break_count(plan);
    let rule_text = match breaks {
        0 => "No rule breaks".to_string(),
        1 => "1 rule break".to_string(),
        count => format!("{count} rule breaks"),
    };
    let tracks = plan.tracks.len();
    let mut chips = vec![
        SummaryChip {
            text: format!("Energy within {furthest:.1} of the curve"),
            tone: if off_curve {
                ChipTone::Warm
            } else {
                ChipTone::Neutral
            },
        },
        SummaryChip {
            text: rule_text,
            tone: if breaks > 0 {
                ChipTone::Warm
            } else {
                ChipTone::Neutral
            },
        },
        SummaryChip {
            text: format!(
                "{} · {tracks} {}",
                set_length_text(track_seconds),
                if tracks == 1 { "track" } else { "tracks" }
            ),
            tone: ChipTone::Neutral,
        },
    ];
    if let Some(Deviation::ShortPool {
        requested,
        available,
    }) = plan
        .deviations
        .iter()
        .find(|deviation| matches!(deviation, Deviation::ShortPool { .. }))
    {
        chips.push(SummaryChip {
            text: format!("Pool short: {available} of {requested} tracks"),
            tone: ChipTone::Warm,
        });
    }
    chips
}

/// One line of the details: what falls short, and why it counts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DetailLine {
    pub(crate) message: String,
    pub(crate) reason: String,
}

/// The lines under "Show details": how the set stands on average, then
/// each place it falls short with the reason it does.
pub(crate) fn detail_lines(plan: &SetPlan) -> Vec<DetailLine> {
    let mut lines = vec![DetailLine {
        message: format!(
            "Energy is {:.1} from the curve on average.",
            plan.mean_energy_error()
        ),
        reason: String::new(),
    }];
    lines.push(DetailLine {
        message: if plan.every_transition_is_clean() {
            format!("Every mix is in key and within {DEFAULT_MAX_BPM_STEP:.0} BPM.")
        } else {
            "Some mixes break a rule.".to_string()
        },
        reason: String::new(),
    });
    lines.extend(plan.deviations.iter().map(|deviation| DetailLine {
        message: deviation_message(deviation),
        reason: deviation_reason(deviation),
    }));
    lines
}

/// One sentence for a place the set falls short, positions counted from 1.
pub(crate) fn deviation_message(deviation: &Deviation) -> String {
    match *deviation {
        Deviation::ShortPool {
            requested,
            available,
        } => format!(
            "The pool has {available} usable tracks, so the set has {available} instead of {requested}."
        ),
        Deviation::EnergyOffTarget {
            position,
            target,
            actual,
        } => format!(
            "Track {} is at energy {actual:.0} where the curve asks for {target:.1}.",
            position + 1
        ),
        Deviation::KeyClash { position, relation } => format!(
            "Track {} to {} is {}, not an allowed key move.",
            position,
            position + 1,
            relation.map_or("a key clash", |relation| relation.label())
        ),
        Deviation::TempoJump {
            position,
            bpm_change,
        } => format!(
            "Track {} to {} changes tempo by {bpm_change:+.1} BPM.",
            position,
            position + 1
        ),
        Deviation::RepeatedArtist { position } => format!(
            "Tracks {} and {} are by the same artist.",
            position,
            position + 1
        ),
    }
}

/// Why the deviation counts as one.
pub(crate) fn deviation_reason(deviation: &Deviation) -> String {
    match *deviation {
        Deviation::ShortPool { .. } => {
            "Choose a larger pool or a shorter set to fill the length asked for.".to_string()
        }
        Deviation::EnergyOffTarget { target, actual, .. } => format!(
            "It is {:.1} away, and more than {DEFAULT_ENERGY_TOLERANCE:.0} counts as off the curve.",
            (actual - target).abs()
        ),
        Deviation::KeyClash { .. } => {
            "The set keeps to the same key, a neighbour, the relative key, a diagonal mix or an energy boost.".to_string()
        }
        Deviation::TempoJump { .. } => {
            format!("The set keeps each tempo step within {DEFAULT_MAX_BPM_STEP:.0} BPM.")
        }
        Deviation::RepeatedArtist { .. } => {
            "The set does not play an artist twice in a row.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(levels: &[f32]) -> Vec<f32> {
        levels.to_vec()
    }

    #[test]
    fn a_curve_that_rises_peaks_and_falls_has_three_phases() {
        let result = phases(&curve(&[4.0, 6.0, 8.0, 10.0, 10.0, 7.0, 5.0]));
        let labels: Vec<_> = result.iter().map(|phase| phase.label).collect();
        assert_eq!(labels, ["Warm-up", "Peak", "Close"]);
        assert_eq!(result[0].from, 0.0);
        assert_eq!(result[2].to, 1.0);
        assert_eq!(result[0].to, result[1].from);
        assert_eq!(result[1].to, result[2].from);
    }

    #[test]
    fn a_curve_without_a_fall_has_no_close_and_a_flat_one_has_none() {
        let rising = phases(&curve(&[3.0, 4.0, 5.0, 6.0, 7.0, 8.0]));
        let labels: Vec<_> = rising.iter().map(|phase| phase.label).collect();
        assert_eq!(labels, ["Warm-up", "Peak"]);
        assert!(phases(&curve(&[6.0, 6.5, 6.0, 7.0])).is_empty());
    }

    #[test]
    fn a_bar_is_as_high_as_the_curve_at_its_energy() {
        assert_eq!(bar_height(10.0, 200.0), 200.0);
        assert_eq!(bar_height(5.5, 200.0), 100.0);
        assert_eq!(bar_height(1.0, 200.0), MIN_BAR_HEIGHT);
    }

    #[test]
    fn the_curve_runs_through_the_middle_of_the_first_and_last_slots() {
        let (start, span) = curve_span(528.0, Some(10));
        assert_eq!(start, CHART_PAD + 25.0);
        assert_eq!(start + span, 528.0 - CHART_PAD - 25.0);
        assert_eq!(curve_fraction_at(start, 528.0, Some(10)), 0.0);
        assert_eq!(curve_fraction_at(start + span, 528.0, Some(10)), 1.0);
        assert_eq!(curve_span(500.0, None), (0.0, 500.0));
        let share = share_at_curve(1.0, 10);
        assert!((share - 0.95).abs() < 1e-6);
    }

    #[test]
    fn lengths_read_as_hours_and_minutes() {
        assert_eq!(set_length_text(92.0 * 60.0), "1 h 32");
        assert_eq!(set_length_text(48.0 * 60.0 + 20.0), "48 min");
        assert_eq!(set_length_text(3600.0), "1 h 00");
    }

    #[test]
    fn the_energy_cell_says_what_the_curve_wanted_only_off_the_curve() {
        assert_eq!(energy_text(6.0, 4.9, true), "6 (wants 4.9)");
        assert_eq!(energy_text(6.0, 5.5, false), "6 of 5.5");
    }
}

#[cfg(test)]
mod plan_tests {
    use euphoria_core::{Key, Mode, PlannedTrack, SetPlan, Transition};

    use super::*;

    fn plan_of(errors: &[f32], deviations: Vec<Deviation>) -> SetPlan {
        SetPlan {
            tracks: errors
                .iter()
                .enumerate()
                .map(|(index, error)| PlannedTrack {
                    track: index,
                    position: index as f32 / (errors.len() - 1) as f32,
                    target_energy: 5.0,
                    energy: 5.0 + error,
                    key: Key::new(0, Mode::Minor),
                    bpm: 120.0,
                    pinned: false,
                })
                .collect(),
            transitions: Vec::<Transition>::new(),
            deviations,
            left_out: Vec::new(),
            cost: 0.0,
            lower_bound: 0.0,
        }
    }

    fn every_kind() -> Vec<Deviation> {
        vec![
            Deviation::ShortPool {
                requested: 20,
                available: 14,
            },
            Deviation::EnergyOffTarget {
                position: 4,
                target: 4.9,
                actual: 7.0,
            },
            Deviation::KeyClash {
                position: 2,
                relation: None,
            },
            Deviation::TempoJump {
                position: 3,
                bpm_change: 6.5,
            },
            Deviation::RepeatedArtist { position: 5 },
        ]
    }

    #[test]
    fn the_chips_say_how_close_the_energy_is_how_many_rules_break_and_how_long() {
        let mut errors = vec![0.2; 14];
        errors[3] = 1.9;
        errors[8] = -1.4;
        let deviations = vec![
            Deviation::EnergyOffTarget {
                position: 3,
                target: 5.0,
                actual: 6.9,
            },
            Deviation::KeyClash {
                position: 6,
                relation: None,
            },
            Deviation::TempoJump {
                position: 9,
                bpm_change: 6.0,
            },
        ];
        let chips = summary_chips(&plan_of(&errors, deviations), 92.0 * 60.0);
        let texts: Vec<&str> = chips.iter().map(|chip| chip.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Energy within 1.9 of the curve",
                "2 rule breaks",
                "1 h 32 · 14 tracks"
            ]
        );
        assert_eq!(chips[0].tone, ChipTone::Warm);
        assert_eq!(chips[1].tone, ChipTone::Warm);
        assert_eq!(chips[2].tone, ChipTone::Neutral);
    }

    #[test]
    fn a_clean_set_has_neutral_chips_and_one_rule_break_is_singular() {
        let chips = summary_chips(&plan_of(&[0.5, -0.5], Vec::new()), 600.0);
        assert_eq!(chips[1].text, "No rule breaks");
        assert!(chips.iter().all(|chip| chip.tone == ChipTone::Neutral));
        let one = vec![Deviation::RepeatedArtist { position: 1 }];
        let chips = summary_chips(&plan_of(&[0.0, 0.0], one), 600.0);
        assert_eq!(chips[1].text, "1 rule break");
    }

    #[test]
    fn a_short_pool_gets_its_own_chip() {
        let short = vec![Deviation::ShortPool {
            requested: 20,
            available: 14,
        }];
        let chips = summary_chips(&plan_of(&[0.0; 14], short), 600.0);
        assert_eq!(chips.len(), 4);
        assert_eq!(chips[3].text, "Pool short: 14 of 20 tracks");
    }

    #[test]
    fn the_details_hold_every_line_the_summary_used_to_show_with_a_reason() {
        let plan = plan_of(&[0.0, 0.0, 0.0, 0.0, 2.1, 0.0], every_kind());
        let lines = detail_lines(&plan);
        let former = [
            "The pool has 14 usable tracks, so the set has 14 instead of 20.",
            "Track 5 is at energy 7 where the curve asks for 4.9.",
            "Track 2 to 3 is a key clash, not an allowed key move.",
            "Track 3 to 4 changes tempo by +6.5 BPM.",
            "Tracks 5 and 6 are by the same artist.",
        ];
        for sentence in former {
            let line = lines
                .iter()
                .find(|line| line.message == sentence)
                .unwrap_or_else(|| panic!("missing {sentence}"));
            assert!(!line.reason.is_empty(), "{sentence}");
        }
        assert_eq!(lines.len(), former.len() + 2);
        assert!(lines[0].message.starts_with("Energy is "));
        assert_eq!(lines[1].message, "Some mixes break a rule.");
    }

    #[test]
    fn rows_are_flagged_by_the_deviation_that_reaches_them() {
        let plan = plan_of(&[0.0; 7], every_kind());
        assert!(is_flagged(&plan, 4));
        assert!(is_flagged(&plan, 2));
        assert!(is_flagged(&plan, 3));
        assert!(is_flagged(&plan, 5));
        assert!(!is_flagged(&plan, 0));
        assert!(!is_flagged(&plan, 1));
        assert!(!is_flagged(&plan, 6));
        assert_eq!(rule_breaks_into(&plan, 4), Vec::<String>::new());
        let tempo = rule_breaks_into(&plan, 3);
        assert_eq!(tempo.len(), 1);
        assert!(tempo[0].starts_with("Track 3 to 4 changes tempo by +6.5 BPM."));
        assert_eq!(rule_break_count(&plan), 3);
    }
}
