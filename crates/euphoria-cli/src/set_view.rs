// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A planned set as a person reads it: a table of the tracks with each
//! transition, the planned and actual curves side by side, and where the
//! set falls short; or the same as JSON.

use std::io::{self, Write};
use std::path::PathBuf;

use euphoria_core::harmony::set::LOWEST_ENERGY;
use euphoria_core::{Deviation, FileResult, MissingFeature, SetOptions, SetPlan, Transition};
use serde::Serialize;

use crate::output;
use crate::set::PoolTrack;
use crate::style::Theme;
use crate::terminal::Printable;
use crate::view::{KeyNotationChoice, shorten_middle, width_of};

/// The blocks of the curve chart, from the lowest energy to the highest.
const LEVEL_BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

const ENERGY_BAR_CELLS: usize = 10;

const GAP: &str = "  ";

/// What the views of a set need.
pub struct SetView<'set> {
    pub pool: &'set [PoolTrack],
    pub plan: &'set SetPlan,
    pub options: &'set SetOptions,
    pub notation: KeyNotationChoice,
    /// Columns of text the table may use.
    pub width: usize,
}

impl SetView<'_> {
    fn track(&self, position: usize) -> &PoolTrack {
        &self.pool[self.plan.tracks[position].track]
    }

    fn is_off_target(&self, position: usize) -> bool {
        self.plan.deviations.iter().any(|deviation| {
            matches!(deviation, Deviation::EnergyOffTarget { position: off, .. } if *off == position)
        })
    }

    fn total_seconds(&self) -> f32 {
        (0..self.plan.tracks.len())
            .map(|position| self.track(position).analysis.duration_seconds)
            .filter(|seconds| seconds.is_finite())
            .sum()
    }
}

/// The block that draws `energy` in the curve chart.
fn level_block(energy: f32) -> char {
    let share = ((energy - LOWEST_ENERGY) / 9.0).clamp(0.0, 1.0);
    LEVEL_BLOCKS[(share * (LEVEL_BLOCKS.len() - 1) as f32).round() as usize]
}

fn signed(value: f32) -> String {
    if value >= 0.0 {
        format!("+{value:.1}")
    } else {
        format!("{value:.1}")
    }
}

/// The key move as the table shows it, coloured when it breaks a rule.
fn move_words(theme: &Theme, transition: &Transition) -> (String, usize) {
    let words = match transition.relation {
        Some(relation) => relation.label(),
        None => "Key clash",
    };
    let styled = if transition.key_allowed {
        words.to_string()
    } else {
        theme.bad(words)
    };
    (styled, width_of(words))
}

/// One row of the table: each cell styled, with its width unstyled.
struct Row {
    cells: Vec<(String, usize)>,
    track: String,
    pinned: bool,
}

const HEADINGS: [&str; 7] = ["#", "Key", "BPM", "Step", "Move", "Energy", "Target"];
const RIGHT_ALIGNED: [bool; 7] = [true, true, true, true, false, false, true];

/// Writes the set as a table, one track per line with the transition into
/// it, then the planned and actual curves.
pub fn write_table(writer: &mut impl Write, theme: &Theme, view: &SetView<'_>) -> io::Result<()> {
    let plan = view.plan;
    let rows: Vec<Row> = plan
        .tracks
        .iter()
        .enumerate()
        .map(|(position, planned)| {
            let track = view.track(position);
            let key_text = view.notation.short(planned.key);
            let key = (theme.key(planned.key, &key_text), width_of(&key_text));
            let bpm_text = format!("{:.1}", planned.bpm);
            let (step, movement) = match position
                .checked_sub(1)
                .map(|index| &plan.transitions[index])
            {
                Some(transition) => {
                    let step_text = signed(transition.bpm_change);
                    let step_width = width_of(&step_text);
                    let step = if transition.tempo_allowed {
                        step_text
                    } else {
                        theme.bad(step_text)
                    };
                    ((step, step_width), move_words(theme, transition))
                }
                None => ((String::new(), 0), (String::new(), 0)),
            };
            let energy_text = format!("{:>2}", planned.energy.round() as i32);
            let filled = (planned.energy.round().clamp(0.0, 10.0)) as usize;
            let bar = format!(
                "{}{}",
                "━".repeat(filled),
                theme.dim("─".repeat(ENERGY_BAR_CELLS - filled))
            );
            let energy_number = if view.is_off_target(position) {
                theme.caution(&energy_text)
            } else {
                energy_text.clone()
            };
            let energy = (
                format!("{energy_number} {bar}"),
                width_of(&energy_text) + 1 + ENERGY_BAR_CELLS,
            );
            let target_text = format!("{:.1}", planned.target_energy);
            let number = (position + 1).to_string();
            Row {
                cells: vec![
                    (theme.dim(&number), width_of(&number)),
                    key,
                    (bpm_text.clone(), width_of(&bpm_text)),
                    step,
                    movement,
                    energy,
                    (theme.dim(&target_text), width_of(&target_text)),
                ],
                track: Printable(track.display_name()).to_string(),
                pinned: planned.pinned,
            }
        })
        .collect();

    let widths: Vec<usize> = HEADINGS
        .iter()
        .enumerate()
        .map(|(column, heading)| {
            rows.iter()
                .map(|row| row.cells[column].1)
                .chain(std::iter::once(heading.len()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let fixed: usize = widths.iter().map(|width| width + GAP.len()).sum();
    let track_width = view.width.saturating_sub(fixed).max(12);

    let mut heading = String::new();
    for (column, title) in HEADINGS.iter().enumerate() {
        heading.push_str(&pad(
            title,
            title.len(),
            widths[column],
            RIGHT_ALIGNED[column],
        ));
        heading.push_str(GAP);
    }
    heading.push_str("Track");
    writeln!(writer, "{}", theme.dim(heading))?;
    for row in &rows {
        let mut line = String::new();
        for (column, (styled, width)) in row.cells.iter().enumerate() {
            line.push_str(&pad(styled, *width, widths[column], RIGHT_ALIGNED[column]));
            line.push_str(GAP);
        }
        let pin_note = if row.pinned { " (pinned)" } else { "" };
        let name = shorten_middle(
            &row.track,
            track_width.saturating_sub(pin_note.len()).max(4),
        );
        line.push_str(&name);
        line.push_str(&theme.dim(pin_note));
        writeln!(writer, "{}", line.trim_end())?;
    }

    writeln!(writer)?;
    let planned: String = plan
        .tracks
        .iter()
        .map(|track| level_block(track.target_energy))
        .collect();
    let actual: String = plan
        .tracks
        .iter()
        .enumerate()
        .map(|(position, track)| {
            let block = level_block(track.energy).to_string();
            if view.is_off_target(position) {
                theme.caution(block)
            } else {
                block
            }
        })
        .collect();
    writeln!(writer, "{}  {}", theme.dim("Curve "), theme.dim(planned))?;
    writeln!(writer, "{}  {actual}", theme.dim("Set   "))?;
    Ok(())
}

fn pad(styled: &str, width: usize, column_width: usize, right: bool) -> String {
    let padding = " ".repeat(column_width.saturating_sub(width));
    if right {
        format!("{padding}{styled}")
    } else {
        format!("{styled}{padding}")
    }
}

/// The length of a set: `1:52:10`, or `48:30` under an hour.
fn set_length(seconds: f32) -> String {
    let whole_seconds = seconds.max(0.0).round() as u64;
    let hours = whole_seconds / 3600;
    let minutes = whole_seconds % 3600 / 60;
    let rest = whole_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

/// Positions as people count them, from 1.
fn counted_position(position: usize) -> usize {
    position + 1
}

/// Writes how long the set is, how close it follows the curve, whether
/// every mix keeps the rules, and each place it falls short.
pub fn write_summary(writer: &mut impl Write, theme: &Theme, view: &SetView<'_>) -> io::Result<()> {
    let plan = view.plan;
    let options = view.options;
    let rules = if plan.every_transition_is_clean() {
        format!(
            "every mix in key and within {} BPM",
            trimmed(options.max_bpm_step)
        )
    } else {
        "some mixes break a rule, listed below".to_string()
    };
    writeln!(
        writer,
        "{}, {}. Energy within {:.1} of the curve on average; {rules}.",
        output::counted(plan.tracks.len(), "track", "tracks"),
        set_length(view.total_seconds()),
        plan.mean_energy_error(),
    )?;

    for message in deviation_messages(view) {
        output::write_warning(writer, theme, message)?;
    }
    if !plan.left_out.is_empty() {
        let count = |missing: MissingFeature| {
            plan.left_out
                .iter()
                .filter(|left| left.missing == missing)
                .count()
        };
        let reasons: Vec<String> = [
            (MissingFeature::Key, "a key"),
            (MissingFeature::Tempo, "a tempo"),
            (MissingFeature::Energy, "an energy"),
        ]
        .into_iter()
        .filter(|(missing, _)| count(*missing) > 0)
        .map(|(missing, what)| format!("{} without {what}", count(missing)))
        .collect();
        output::write_note(
            writer,
            theme,
            format_args!(
                "{} left out of the pool: {}.",
                output::counted(plan.left_out.len(), "track", "tracks"),
                reasons.join(", ")
            ),
        )?;
    }
    if !plan.deviations.is_empty() {
        output::write_hint(
            writer,
            theme,
            "a larger pool, more --moves or a larger --max-bpm-step lets the set follow the \
             curve more closely.",
        )?;
    }
    Ok(())
}

fn trimmed(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

/// One sentence per place the set falls short, with tracks off the curve
/// in a row told together.
fn deviation_messages(view: &SetView<'_>) -> Vec<String> {
    let plan = view.plan;
    let mut messages = Vec::new();
    let mut energy_run: Vec<(usize, f32, f32)> = Vec::new();
    let flush = |run: &mut Vec<(usize, f32, f32)>, messages: &mut Vec<String>| {
        if run.is_empty() {
            return;
        }
        let below = run[0].2 < run[0].1;
        let largest = run
            .iter()
            .map(|(_, target, actual)| (target - actual).abs())
            .fold(0.0, f32::max);
        let first = counted_position(run[0].0);
        let last = counted_position(run[run.len() - 1].0);
        let tracks = if first == last {
            format!("track {first} is")
        } else {
            format!("tracks {first} to {last} are")
        };
        messages.push(format!(
            "{tracks} {} the curve by up to {largest:.1}: the pool has nothing closer that mixes in.",
            if below { "below" } else { "above" }
        ));
        run.clear();
    };
    for deviation in &plan.deviations {
        if let Deviation::EnergyOffTarget {
            position,
            target,
            actual,
        } = *deviation
        {
            let continues = energy_run
                .last()
                .is_some_and(|&(last, last_target, last_actual)| {
                    last + 1 == position && (last_actual < last_target) == (actual < target)
                });
            if !continues {
                flush(&mut energy_run, &mut messages);
            }
            energy_run.push((position, target, actual));
            continue;
        }
        match *deviation {
            Deviation::ShortPool {
                requested,
                available,
            } => messages.push(format!(
                "the pool has {} with a key, a tempo and an energy, so the set has {available} instead of {requested}.",
                output::counted(available, "track", "tracks")
            )),
            Deviation::KeyClash { position, relation } => {
                let from = plan.tracks[position - 1].key;
                let to = plan.tracks[position].key;
                let what = match relation {
                    Some(relation) => format!("a {} move", relation.label().to_lowercase()),
                    None => "no harmonic move".to_string(),
                };
                messages.push(format!(
                    "track {} to {} goes from {} to {}, {what}, which --moves does not allow.",
                    counted_position(position - 1),
                    counted_position(position),
                    view.notation.short(from),
                    view.notation.short(to),
                ));
            }
            Deviation::TempoJump {
                position,
                bpm_change,
            } => messages.push(format!(
                "track {} to {} changes tempo by {} BPM, more than {}.",
                counted_position(position - 1),
                counted_position(position),
                signed(bpm_change),
                trimmed(view.options.max_bpm_step)
            )),
            Deviation::RepeatedArtist { position } => messages.push(format!(
                "tracks {} and {} are by the same artist.",
                counted_position(position - 1),
                counted_position(position)
            )),
            Deviation::EnergyOffTarget { .. } => {}
        }
    }
    flush(&mut energy_run, &mut messages);
    messages
}

#[derive(Serialize)]
struct JsonSet<'set> {
    curve: &'set [f32],
    requested_tracks: usize,
    duration_seconds: f32,
    mean_energy_error: f32,
    every_transition_clean: bool,
    cost: f64,
    lower_bound: f64,
    tracks: Vec<JsonTrack<'set>>,
    transitions: Vec<JsonTransition>,
    deviations: Vec<JsonDeviation>,
    left_out: Vec<JsonLeftOut<'set>>,
    failed: Vec<JsonFailure<'set>>,
}

#[derive(Serialize)]
struct JsonTrack<'set> {
    position: usize,
    path: &'set PathBuf,
    artist: Option<&'set str>,
    title: Option<&'set str>,
    key: String,
    key_name: String,
    bpm: f32,
    energy: f32,
    target_energy: f32,
    duration_seconds: f32,
    pinned: bool,
}

#[derive(Serialize)]
struct JsonTransition {
    from: usize,
    to: usize,
    key_move: Option<&'static str>,
    key_move_allowed: bool,
    bpm_change: f32,
    bpm_change_percent: f32,
    tempo_allowed: bool,
    same_artist: bool,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JsonDeviation {
    ShortPool {
        requested: usize,
        available: usize,
    },
    EnergyOffTarget {
        position: usize,
        target: f32,
        actual: f32,
    },
    KeyClash {
        position: usize,
        key_move: Option<&'static str>,
    },
    TempoJump {
        position: usize,
        bpm_change: f32,
    },
    RepeatedArtist {
        position: usize,
    },
}

#[derive(Serialize)]
struct JsonLeftOut<'set> {
    path: &'set PathBuf,
    missing: &'static str,
}

#[derive(Serialize)]
struct JsonFailure<'set> {
    path: &'set PathBuf,
    error: String,
}

fn round_to_tenths(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

fn round_to_hundredths(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

/// The name JSON gives a key move, such as `energy_boost`.
fn move_name(relation: euphoria_core::Relation) -> &'static str {
    use euphoria_core::Relation;
    match relation {
        Relation::Same => "same",
        Relation::Adjacent => "adjacent",
        Relation::Relative => "relative",
        Relation::Diagonal => "diagonal",
        Relation::EnergyBoost => "energy_boost",
        Relation::MoodChange => "mood_change",
        Relation::SemitoneBoost => "semitone_boost",
    }
}

/// Writes the set as one JSON object. Positions count from 1, as in the
/// table.
pub fn write_json(
    writer: &mut impl Write,
    view: &SetView<'_>,
    failures: &[FileResult],
) -> io::Result<()> {
    let plan = view.plan;
    let set = JsonSet {
        curve: view.options.curve.levels(),
        requested_tracks: view.options.track_count,
        duration_seconds: round_to_tenths(view.total_seconds()),
        mean_energy_error: round_to_hundredths(plan.mean_energy_error()),
        every_transition_clean: plan.every_transition_is_clean(),
        cost: (plan.cost * 1000.0).round() / 1000.0,
        lower_bound: (plan.lower_bound * 1000.0).round() / 1000.0,
        tracks: plan
            .tracks
            .iter()
            .enumerate()
            .map(|(position, planned)| {
                let track = view.track(position);
                JsonTrack {
                    position: counted_position(position),
                    path: &track.path,
                    artist: track.artist.as_deref(),
                    title: track.title.as_deref(),
                    key: planned.key.camelot(),
                    key_name: planned.key.to_string(),
                    bpm: round_to_tenths(planned.bpm),
                    energy: planned.energy,
                    target_energy: round_to_hundredths(planned.target_energy),
                    duration_seconds: round_to_tenths(track.analysis.duration_seconds),
                    pinned: planned.pinned,
                }
            })
            .collect(),
        transitions: plan
            .transitions
            .iter()
            .enumerate()
            .map(|(index, transition)| JsonTransition {
                from: counted_position(index),
                to: counted_position(index + 1),
                key_move: transition.relation.map(move_name),
                key_move_allowed: transition.key_allowed,
                bpm_change: round_to_tenths(transition.bpm_change),
                bpm_change_percent: round_to_tenths(transition.bpm_change_percent),
                tempo_allowed: transition.tempo_allowed,
                same_artist: transition.same_artist,
            })
            .collect(),
        deviations: plan
            .deviations
            .iter()
            .map(|deviation| match *deviation {
                Deviation::ShortPool {
                    requested,
                    available,
                } => JsonDeviation::ShortPool {
                    requested,
                    available,
                },
                Deviation::EnergyOffTarget {
                    position,
                    target,
                    actual,
                } => JsonDeviation::EnergyOffTarget {
                    position: counted_position(position),
                    target: round_to_hundredths(target),
                    actual,
                },
                Deviation::KeyClash { position, relation } => JsonDeviation::KeyClash {
                    position: counted_position(position),
                    key_move: relation.map(move_name),
                },
                Deviation::TempoJump {
                    position,
                    bpm_change,
                } => JsonDeviation::TempoJump {
                    position: counted_position(position),
                    bpm_change: round_to_tenths(bpm_change),
                },
                Deviation::RepeatedArtist { position } => JsonDeviation::RepeatedArtist {
                    position: counted_position(position),
                },
            })
            .collect(),
        left_out: plan
            .left_out
            .iter()
            .map(|left| JsonLeftOut {
                path: &view.pool[left.track].path,
                missing: left.missing.name(),
            })
            .collect(),
        failed: failures
            .iter()
            .filter_map(|result| {
                let error = result.outcome.as_ref().err()?;
                Some(JsonFailure {
                    path: &result.path,
                    error: error.chain().to_string(),
                })
            })
            .collect(),
    };
    serde_json::to_writer_pretty(&mut *writer, &set)?;
    writeln!(writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use euphoria_core::{Key, build_set};

    #[test]
    fn set_lengths_show_hours_once_there_are_any() {
        assert_eq!(set_length(900.0), "15:00");
        assert_eq!(set_length(7736.1), "2:08:56");
        assert_eq!(set_length(3600.0), "1:00:00");
    }

    #[test]
    fn energy_maps_onto_eight_blocks() {
        assert_eq!(level_block(1.0), '▁');
        assert_eq!(level_block(10.0), '█');
        assert_eq!(level_block(5.5), '▅');
        assert_eq!(level_block(0.0), '▁');
        assert_eq!(level_block(12.0), '█');
    }

    fn pool_track(code: &str, bpm: f32, energy: u8, artist: &str, title: &str) -> PoolTrack {
        PoolTrack {
            path: PathBuf::from(format!("/music/{title}.flac")),
            analysis: euphoria_core::TrackAnalysis {
                duration_seconds: 300.0,
                key: Some(code.parse::<Key>().unwrap()),
                bpm: Some(bpm),
                energy: Some(energy),
                tuning_cents: None,
                key_timeline: Vec::new(),
                quality: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
            },
            artist: Some(artist.to_string()),
            title: Some(title.to_string()),
        }
    }

    fn with_set<T>(
        pool: &[PoolTrack],
        options: &SetOptions,
        body: impl FnOnce(&SetView<'_>) -> T,
    ) -> T {
        let plan = build_set(pool, options).unwrap();
        let view = SetView {
            pool,
            plan: &plan,
            options,
            notation: KeyNotationChoice::Camelot,
            width: 100,
        };
        body(&view)
    }

    fn first_pool_and_options() -> (Vec<PoolTrack>, SetOptions) {
        let pool = vec![
            pool_track("8A", 124.0, 5, "A", "One"),
            pool_track("9A", 125.0, 6, "B", "Two"),
            pool_track("9A", 126.0, 7, "C", "Three"),
        ];
        let options = SetOptions::new(3, "5,7".parse().unwrap());
        (pool, options)
    }

    #[test]
    fn the_table_lists_each_track_with_its_move_and_the_two_curves() {
        let (pool, options) = first_pool_and_options();
        let text = with_set(&pool, &options, |view| {
            let mut buffer = Vec::new();
            write_table(&mut buffer, &Theme::PLAIN, view).unwrap();
            String::from_utf8(buffer).unwrap()
        });
        let first_line = text.lines().next().unwrap();
        assert!(first_line.starts_with("#"));
        for heading in ["Key", "BPM", "Step", "Move", "Energy", "Target", "Track"] {
            assert!(first_line.contains(heading));
        }
        assert!(text.contains("A - One"));
        assert!(text.contains("B - Two"));
        assert!(text.contains("C - Three"));
        assert!(text.lines().any(|line| line.starts_with("Curve ")));
        assert!(text.lines().any(|line| line.starts_with("Set   ")));
        let second_in_table = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("B - Two") || line.contains("C - Three"))
            .nth(1)
            .unwrap()
            .1;
        assert!(second_in_table.contains("+1.0"));
    }

    #[test]
    fn the_summary_says_every_mix_is_clean() {
        let (pool, options) = first_pool_and_options();
        let text = with_set(&pool, &options, |view| {
            let mut buffer = Vec::new();
            write_summary(&mut buffer, &Theme::PLAIN, view).unwrap();
            String::from_utf8(buffer).unwrap()
        });
        assert!(text.starts_with("3 tracks, 15:00."));
        assert!(text.contains("every mix in key and within 5 BPM"));
    }

    #[test]
    fn tracks_off_the_curve_in_a_row_are_told_together() {
        let pool = [
            pool_track("8A", 124.0, 5, "A", "One"),
            pool_track("8A", 124.0, 5, "B", "Two"),
            pool_track("8A", 124.0, 5, "C", "Three"),
            pool_track("8A", 124.0, 5, "D", "Four"),
        ];
        let options = SetOptions::new(4, "9".parse().unwrap());
        let messages = with_set(&pool, &options, deviation_messages);
        assert_eq!(
            messages,
            vec![
                "tracks 1 to 4 are below the curve by up to 4.0: the pool has nothing closer that mixes in."
            ]
        );
    }

    #[test]
    fn json_counts_positions_from_one() {
        let (pool, options) = first_pool_and_options();
        let value = with_set(&pool, &options, |view| {
            let mut buffer = Vec::new();
            write_json(&mut buffer, view, &[]).unwrap();
            serde_json::from_slice::<serde_json::Value>(&buffer).unwrap()
        });
        assert_eq!(value["tracks"][0]["position"], 1);
        assert_eq!(value["tracks"].as_array().unwrap().len(), 3);
        assert_eq!(value["transitions"][0]["from"], 1);
        assert_eq!(value["transitions"][0]["to"], 2);
        assert_eq!(value["every_transition_clean"], true);
        assert!(value["deviations"].as_array().unwrap().is_empty());
    }
}
