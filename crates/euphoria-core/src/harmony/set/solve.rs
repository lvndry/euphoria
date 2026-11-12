// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cmp::Ordering;

use super::{
    Deviation, LeftOut, MissingFeature, Pin, PinPosition, PlannedTrack, SetError, SetOptions,
    SetPlan, SetTrack, Transition, normalised_position,
};
use crate::harmony::relation::Relation;
use crate::key::Key;

/// Added for each broken rule: a key move that is not allowed, a tempo step
/// past the limit or an artist twice in a row. It is larger than the energy
/// errors of any whole set, so the search breaks a rule only when no set
/// avoids it.
const RULE_BREAK: f32 = 10_000.0;

/// The cost of a tempo step as large as the limit. Smaller steps cost the
/// square of their share of it.
const TEMPO_STEP_WEIGHT: f32 = 0.5;

/// How many partial sets ending on the same track the beam keeps at least,
/// so that it holds different endings rather than many orders of one. A
/// small pool gets more, so that the beam stays full.
const STATES_PER_LAST_TRACK: usize = 2;

/// How many polishing moves are made at most, a bound that is never reached
/// in practice since each move lowers the cost.
const MAXIMUM_POLISH_MOVES: usize = 1_000;

/// Energy errors and tempo steps this close to the limit count as within it,
/// so that rounding never turns an exact fit into a deviation.
const ROUNDING_ALLOWANCE: f32 = 1e-4;

/// A cost improvement smaller than this is rounding, not an improvement.
const IMPROVEMENT_EPSILON: f64 = 1e-9;

/// A track of the pool with everything a set needs.
struct Candidate {
    pool_index: usize,
    key: Key,
    bpm: f32,
    energy: f32,
    artists: Vec<String>,
}

pub(super) fn build<Track: SetTrack>(
    pool: &[Track],
    options: &SetOptions,
) -> Result<SetPlan, SetError> {
    if options.track_count == 0 {
        return Err(SetError::NoTracks);
    }
    let mut candidates = Vec::new();
    let mut left_out = Vec::new();
    let mut candidate_of_pool = vec![None; pool.len()];
    for (pool_index, track) in pool.iter().enumerate() {
        match candidate(pool_index, track) {
            Ok(candidate) => {
                candidate_of_pool[pool_index] = Some(candidates.len());
                candidates.push(candidate);
            }
            Err(missing) => left_out.push(LeftOut {
                track: pool_index,
                missing,
            }),
        }
    }
    for pin in &options.pins {
        if pin.track >= pool.len() {
            return Err(SetError::PinnedTrackMissing { track: pin.track });
        }
        if let Some(left) = left_out.iter().find(|left| left.track == pin.track) {
            return Err(SetError::PinnedTrackUnusable {
                track: pin.track,
                missing: left.missing,
            });
        }
    }
    if candidates.is_empty() {
        return Err(SetError::NothingUsable);
    }

    let track_count = options.track_count.min(candidates.len());
    let pins = resolve_pins(&options.pins, &candidate_of_pool, track_count)?;
    let problem = Problem::new(&candidates, options, track_count, &pins);
    let searched = problem.search();
    let order = problem.polish(searched);
    let cost = problem.total_cost(&order);
    Ok(problem.plan(&order, &pins, left_out, cost))
}

pub(super) fn evaluate<Track: SetTrack>(
    pool: &[Track],
    order: &[usize],
    options: &SetOptions,
) -> Result<SetPlan, SetError> {
    if order.is_empty() {
        return Err(SetError::NoTracks);
    }
    let mut candidates = Vec::with_capacity(order.len());
    for (position, &pool_index) in order.iter().enumerate() {
        let track = pool
            .get(pool_index)
            .ok_or(SetError::PinnedTrackMissing { track: pool_index })?;
        if order[..position].contains(&pool_index) {
            return Err(SetError::ConflictingPins {
                position,
                track: pool_index,
            });
        }
        let candidate =
            candidate(pool_index, track).map_err(|missing| SetError::PinnedTrackUnusable {
                track: pool_index,
                missing,
            })?;
        candidates.push(candidate);
    }
    let count = order.len();
    let fixed = SetOptions {
        track_count: count,
        ..options.clone()
    };
    let every_position: Vec<(usize, usize)> =
        (0..count).map(|position| (position, position)).collect();
    let problem = Problem::new(&candidates, &fixed, count, &every_position);
    let identity: Vec<usize> = (0..count).collect();
    let pinned: Vec<(usize, usize)> = options
        .pins
        .iter()
        .filter_map(|pin| {
            let position = match pin.position {
                PinPosition::At(position) => position,
                PinPosition::Last => count - 1,
            };
            (order.get(position) == Some(&pin.track)).then_some((position, position))
        })
        .collect();
    let cost = problem.total_cost(&identity);
    Ok(problem.plan(&identity, &pinned, Vec::new(), cost))
}

fn candidate<Track: SetTrack>(
    pool_index: usize,
    track: &Track,
) -> Result<Candidate, MissingFeature> {
    let key = track.key().ok_or(MissingFeature::Key)?;
    let bpm = track
        .bpm()
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
        .ok_or(MissingFeature::Tempo)?;
    let energy = track
        .energy()
        .filter(|energy| energy.is_finite())
        .ok_or(MissingFeature::Energy)?;
    Ok(Candidate {
        pool_index,
        key,
        bpm,
        energy,
        artists: track.artist().map(artist_names).unwrap_or_default(),
    })
}

/// The artists a tag names, lower case: "A feat. B" and "A & B" name both
/// A and B, so a track by A followed by one by A and B repeats an artist.
fn artist_names(artist: &str) -> Vec<String> {
    const SEPARATORS: [&str; 11] = [
        " featuring ",
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " vs. ",
        " vs ",
        " x ",
        " and ",
        "&",
        ",",
    ];
    let mut names = vec![format!(" {} ", artist.trim().to_lowercase())];
    for separator in SEPARATORS {
        names = names
            .iter()
            .flat_map(|name| {
                name.split(separator)
                    .map(|part| format!(" {} ", part.trim()))
            })
            .collect();
    }
    let mut names: Vec<String> = names
        .into_iter()
        .flat_map(|name| {
            name.split([';', '/'])
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The position of each pin in the set, as `(position, candidate index)`
/// sorted by position.
fn resolve_pins(
    pins: &[Pin],
    candidate_of_pool: &[Option<usize>],
    track_count: usize,
) -> Result<Vec<(usize, usize)>, SetError> {
    let mut resolved: Vec<(usize, usize)> = Vec::new();
    for pin in pins {
        let position = match pin.position {
            PinPosition::At(position) => position,
            PinPosition::Last => track_count - 1,
        };
        if position >= track_count {
            return Err(SetError::PinPastTheEnd {
                position,
                track_count,
            });
        }
        let Some(candidate) = candidate_of_pool[pin.track] else {
            return Err(SetError::PinnedTrackMissing { track: pin.track });
        };
        if resolved
            .iter()
            .any(|&(other_position, other)| other_position == position || other == candidate)
        {
            return Err(SetError::ConflictingPins {
                position,
                track: pin.track,
            });
        }
        resolved.push((position, candidate));
    }
    resolved.sort_unstable();
    Ok(resolved)
}

/// The search problem: costs of every track at every position and of every
/// transition, over the usable tracks.
struct Problem<'pool> {
    candidates: &'pool [Candidate],
    options: &'pool SetOptions,
    track_count: usize,
    targets: Vec<f32>,
    /// The cost of candidate `track` at `position`, at
    /// `position * candidates + track`: the squared energy error, or
    /// infinity where a pin forbids it.
    placement: Vec<f32>,
    /// The cost of going from candidate `from` to candidate `to`, at
    /// `from * candidates + to`; infinity from a track to itself.
    transition: Vec<f32>,
    /// The lowest cost of finishing the set after candidate `track` is at
    /// `position`, at `position * candidates + track`, when tracks may
    /// repeat.
    remaining: Vec<f32>,
    pinned_positions: Vec<bool>,
}

impl<'pool> Problem<'pool> {
    fn new(
        candidates: &'pool [Candidate],
        options: &'pool SetOptions,
        track_count: usize,
        pins: &[(usize, usize)],
    ) -> Self {
        let width = candidates.len();
        let targets = options.curve.targets(track_count);
        let mut pinned_positions = vec![false; track_count];
        let mut pinned_tracks = vec![false; width];
        for &(position, track) in pins {
            pinned_positions[position] = true;
            pinned_tracks[track] = true;
        }

        let mut placement = vec![f32::INFINITY; track_count * width];
        for (position, target) in targets.iter().enumerate() {
            let row = &mut placement[position * width..(position + 1) * width];
            if pinned_positions[position] {
                if let Some(&(_, track)) = pins.iter().find(|(pinned, _)| *pinned == position) {
                    let error = candidates[track].energy - target;
                    row[track] = error * error;
                }
            } else {
                for (track, candidate) in candidates.iter().enumerate() {
                    if !pinned_tracks[track] {
                        let error = candidate.energy - target;
                        row[track] = error * error;
                    }
                }
            }
        }

        let mut transition = vec![f32::INFINITY; width * width];
        for (from_index, from) in candidates.iter().enumerate() {
            for (to_index, to) in candidates.iter().enumerate() {
                if from_index != to_index {
                    transition[from_index * width + to_index] =
                        transition_cost(&explain(from, to, options), from, to, options);
                }
            }
        }

        let mut problem = Self {
            candidates,
            options,
            track_count,
            targets,
            placement,
            transition,
            remaining: Vec::new(),
            pinned_positions,
        };
        problem.remaining = problem.costs_to_finish();
        problem
    }

    fn width(&self) -> usize {
        self.candidates.len()
    }

    fn placement_cost(&self, position: usize, track: usize) -> f32 {
        self.placement[position * self.width() + track]
    }

    fn transition_cost(&self, from: usize, to: usize) -> f32 {
        self.transition[from * self.width() + to]
    }

    fn remaining_cost(&self, position: usize, track: usize) -> f32 {
        self.remaining[position * self.width() + track]
    }

    /// The exact lowest cost of finishing the set from each track at each
    /// position when tracks may repeat, by dynamic programming from the
    /// last position back. It never exceeds the true cost of finishing, so
    /// it is a safe guide for the beam and a bound on the whole set.
    fn costs_to_finish(&self) -> Vec<f32> {
        let width = self.width();
        let mut remaining = vec![0.0; self.track_count * width];
        let mut ahead = vec![0.0f32; width];
        for position in (0..self.track_count.saturating_sub(1)).rev() {
            let next = position + 1;
            for (track, cost) in ahead.iter_mut().enumerate() {
                *cost = self.placement_cost(next, track) + remaining[next * width + track];
            }
            for track in 0..width {
                let row = &self.transition[track * width..(track + 1) * width];
                remaining[position * width + track] = row
                    .iter()
                    .zip(&ahead)
                    .map(|(step, after)| step + after)
                    .fold(f32::INFINITY, f32::min);
            }
        }
        remaining
    }

    fn lower_bound(&self) -> f64 {
        (0..self.width())
            .map(|track| f64::from(self.placement_cost(0, track) + self.remaining_cost(0, track)))
            .fold(f64::INFINITY, f64::min)
    }

    /// The best order the beam finds, as candidate indices.
    fn search(&self) -> Vec<usize> {
        let width = self.width();
        let words = width.div_ceil(64);
        let beam_width = self.options.beam_width.max(1);
        let per_last_track = STATES_PER_LAST_TRACK.max(beam_width.div_ceil(width));

        let mut layers: Vec<Vec<(usize, usize)>> = Vec::with_capacity(self.track_count);
        let mut costs: Vec<f64> = Vec::new();
        let mut used: Vec<u64> = Vec::new();
        let mut expansions: Vec<Expansion> = Vec::new();

        for position in 0..self.track_count {
            expansions.clear();
            if position == 0 {
                for track in 0..width {
                    let placed = self.placement_cost(0, track);
                    if placed.is_finite() {
                        let cost = f64::from(placed);
                        expansions.push(Expansion {
                            score: cost + f64::from(self.remaining_cost(0, track)),
                            cost,
                            parent: 0,
                            track,
                        });
                    }
                }
            } else {
                let previous = &layers[position - 1];
                for (parent, &(last, _)) in previous.iter().enumerate() {
                    let parent_used = &used[parent * words..(parent + 1) * words];
                    let row = &self.transition[last * width..(last + 1) * width];
                    let placements = &self.placement[position * width..(position + 1) * width];
                    let remaining = &self.remaining[position * width..(position + 1) * width];
                    for track in 0..width {
                        if parent_used[track / 64] & (1 << (track % 64)) != 0 {
                            continue;
                        }
                        let step = row[track] + placements[track];
                        if !step.is_finite() {
                            continue;
                        }
                        let cost = costs[parent] + f64::from(step);
                        expansions.push(Expansion {
                            score: cost + f64::from(remaining[track]),
                            cost,
                            parent,
                            track,
                        });
                    }
                }
            }

            let kept = keep_best(&mut expansions, beam_width, per_last_track);
            let mut next_costs = Vec::with_capacity(kept.len());
            let mut next_used = Vec::with_capacity(kept.len() * words);
            let mut layer = Vec::with_capacity(kept.len());
            for expansion in kept {
                if position == 0 {
                    next_used.extend(std::iter::repeat_n(0u64, words));
                } else {
                    next_used.extend_from_slice(
                        &used[expansion.parent * words..(expansion.parent + 1) * words],
                    );
                }
                let start = next_used.len() - words;
                next_used[start + expansion.track / 64] |= 1 << (expansion.track % 64);
                next_costs.push(expansion.cost);
                layer.push((expansion.track, expansion.parent));
            }
            costs = next_costs;
            used = next_used;
            layers.push(layer);
        }

        let best = costs
            .iter()
            .enumerate()
            .min_by(|(first_index, first), (second_index, second)| {
                first
                    .total_cmp(second)
                    .then_with(|| first_index.cmp(second_index))
            })
            .map_or(0, |(index, _)| index);
        let mut order = vec![0; self.track_count];
        let mut state = best;
        for position in (0..self.track_count).rev() {
            let (track, parent) = layers[position][state];
            order[position] = track;
            state = parent;
        }
        order
    }

    fn total_cost(&self, order: &[usize]) -> f64 {
        let placed: f64 = order
            .iter()
            .enumerate()
            .map(|(position, &track)| f64::from(self.placement_cost(position, track)))
            .sum();
        let stepped: f64 = order
            .windows(2)
            .map(|pair| f64::from(self.transition_cost(pair[0], pair[1])))
            .sum();
        placed + stepped
    }

    /// The part of the cost of `order` that depends on which track is at
    /// `position`, were it `track`: its placement and the transitions into
    /// and out of it.
    fn local_cost(&self, order: &[usize], position: usize, track: usize) -> f64 {
        let mut cost = f64::from(self.placement_cost(position, track));
        if position > 0 {
            cost += f64::from(self.transition_cost(order[position - 1], track));
        }
        if let Some(&next) = order.get(position + 1) {
            cost += f64::from(self.transition_cost(track, next));
        }
        cost
    }

    /// Lowers the cost of `order` by the best of three kinds of move until
    /// none helps: putting an unused track in place of one in the set,
    /// swapping two tracks, and moving one track elsewhere. Pinned tracks
    /// stay where they are.
    fn polish(&self, mut order: Vec<usize>) -> Vec<usize> {
        let count = order.len();
        let mut in_set = vec![false; self.width()];
        for &track in &order {
            in_set[track] = true;
        }
        let mut cost = self.total_cost(&order);
        let mut trial = order.clone();
        for _ in 0..MAXIMUM_POLISH_MOVES {
            let mut best: Option<(f64, Vec<usize>)> = None;
            let mut consider = |candidate: &[usize], candidate_cost: f64| {
                if candidate_cost < cost - IMPROVEMENT_EPSILON
                    && best
                        .as_ref()
                        .is_none_or(|(best_cost, _)| candidate_cost < *best_cost)
                {
                    best = Some((candidate_cost, candidate.to_vec()));
                }
            };

            for position in 0..count {
                if self.pinned_positions[position] {
                    continue;
                }
                let current = order[position];
                let removed = self.local_cost(&order, position, current);
                for (track, _) in in_set.iter().enumerate().filter(|(_, used)| !**used) {
                    let candidate_cost = cost - removed + self.local_cost(&order, position, track);
                    if candidate_cost < cost - IMPROVEMENT_EPSILON {
                        trial.copy_from_slice(&order);
                        trial[position] = track;
                        consider(&trial, self.total_cost(&trial));
                    }
                }
            }
            for first in 0..count {
                for second in first + 1..count {
                    if self.pinned_positions[first] || self.pinned_positions[second] {
                        continue;
                    }
                    trial.copy_from_slice(&order);
                    trial.swap(first, second);
                    let candidate_cost = self.total_cost(&trial);
                    consider(&trial, candidate_cost);
                }
            }
            for from in 0..count {
                for to in 0..count {
                    let low = from.min(to);
                    let high = from.max(to);
                    if high - low < 2
                        || self.pinned_positions[low..=high]
                            .iter()
                            .any(|&pinned| pinned)
                    {
                        continue;
                    }
                    trial.copy_from_slice(&order);
                    let moved = trial.remove(from);
                    trial.insert(to, moved);
                    let candidate_cost = self.total_cost(&trial);
                    consider(&trial, candidate_cost);
                }
            }

            let Some((better_cost, better)) = best else {
                break;
            };
            for &track in &order {
                in_set[track] = false;
            }
            for &track in &better {
                in_set[track] = true;
            }
            order = better;
            cost = better_cost;
        }
        order
    }

    fn plan(
        &self,
        order: &[usize],
        pins: &[(usize, usize)],
        left_out: Vec<LeftOut>,
        cost: f64,
    ) -> SetPlan {
        let count = order.len();
        let tracks: Vec<PlannedTrack> = order
            .iter()
            .enumerate()
            .map(|(position, &track)| {
                let candidate = &self.candidates[track];
                PlannedTrack {
                    track: candidate.pool_index,
                    position: normalised_position(position, count),
                    target_energy: self.targets[position],
                    energy: candidate.energy,
                    key: candidate.key,
                    bpm: candidate.bpm,
                    pinned: pins.iter().any(|&(pinned, _)| pinned == position),
                }
            })
            .collect();
        let transitions: Vec<Transition> = order
            .windows(2)
            .map(|pair| {
                explain(
                    &self.candidates[pair[0]],
                    &self.candidates[pair[1]],
                    self.options,
                )
            })
            .collect();

        let mut deviations = Vec::new();
        if count < self.options.track_count {
            deviations.push(Deviation::ShortPool {
                requested: self.options.track_count,
                available: count,
            });
        }
        for (position, track) in tracks.iter().enumerate() {
            if position > 0 {
                let transition = &transitions[position - 1];
                if !transition.key_allowed {
                    deviations.push(Deviation::KeyClash {
                        position,
                        relation: transition.relation,
                    });
                }
                if !transition.tempo_allowed {
                    deviations.push(Deviation::TempoJump {
                        position,
                        bpm_change: transition.bpm_change,
                    });
                }
                if transition.same_artist && !self.options.allow_repeated_artists {
                    deviations.push(Deviation::RepeatedArtist { position });
                }
            }
            if track.energy_error().abs() > self.options.energy_tolerance + ROUNDING_ALLOWANCE {
                deviations.push(Deviation::EnergyOffTarget {
                    position,
                    target: track.target_energy,
                    actual: track.energy,
                });
            }
        }

        SetPlan {
            tracks,
            transitions,
            deviations,
            left_out,
            cost,
            lower_bound: self.lower_bound(),
        }
    }
}

/// A partial set one track longer than one in the beam.
#[derive(Debug, Clone, Copy)]
struct Expansion {
    /// The cost so far plus the lowest cost of finishing.
    score: f64,
    /// The cost so far.
    cost: f64,
    /// The index of the partial set it extends.
    parent: usize,
    /// The candidate it adds.
    track: usize,
}

fn expansion_order(first: &Expansion, second: &Expansion) -> Ordering {
    first
        .score
        .total_cmp(&second.score)
        .then_with(|| first.parent.cmp(&second.parent))
        .then_with(|| first.track.cmp(&second.track))
}

/// The best `beam_width` expansions in order, at most `per_last_track`
/// ending on each track. The order is total, so the choice never depends on
/// how the expansions were found.
fn keep_best(
    expansions: &mut Vec<Expansion>,
    beam_width: usize,
    per_last_track: usize,
) -> Vec<Expansion> {
    let shortlist = beam_width.saturating_mul(3);
    if expansions.len() > shortlist {
        expansions.select_nth_unstable_by(shortlist, expansion_order);
        expansions.truncate(shortlist);
    }
    expansions.sort_unstable_by(expansion_order);
    let mut per_track: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut kept = Vec::with_capacity(beam_width);
    for expansion in expansions.iter() {
        let count = per_track.entry(expansion.track).or_insert(0);
        if *count < per_last_track {
            *count += 1;
            kept.push(*expansion);
            if kept.len() == beam_width {
                break;
            }
        }
    }
    kept
}

fn explain(from: &Candidate, to: &Candidate, options: &SetOptions) -> Transition {
    let relation = from.key.relation_to(&to.key);
    let bpm_change = to.bpm - from.bpm;
    Transition {
        relation,
        key_allowed: relation.is_some_and(|relation| options.relations.contains(&relation)),
        bpm_change,
        bpm_change_percent: bpm_change * 100.0 / from.bpm,
        tempo_allowed: bpm_change.abs() <= options.max_bpm_step + ROUNDING_ALLOWANCE,
        same_artist: from.artists.iter().any(|name| to.artists.contains(name)),
    }
}

/// How much a key move costs when it is allowed: nothing for the same key,
/// rising with how bold the move is.
fn relation_cost(relation: Relation) -> f32 {
    match relation {
        Relation::Same => 0.0,
        Relation::Adjacent => 0.1,
        Relation::Relative => 0.15,
        Relation::Diagonal => 0.3,
        Relation::EnergyBoost => 0.4,
        Relation::MoodChange => 0.6,
        Relation::SemitoneBoost => 0.8,
    }
}

fn transition_cost(
    transition: &Transition,
    from: &Candidate,
    to: &Candidate,
    options: &SetOptions,
) -> f32 {
    let key_cost = match transition.relation {
        Some(relation) if transition.key_allowed => relation_cost(relation),
        Some(relation) => RULE_BREAK + relation_cost(relation),
        None => RULE_BREAK + 1.0,
    };
    let step = (to.bpm - from.bpm).abs();
    let tempo_cost = if transition.tempo_allowed {
        if options.max_bpm_step > 0.0 {
            TEMPO_STEP_WEIGHT * (step / options.max_bpm_step).powi(2)
        } else {
            0.0
        }
    } else {
        RULE_BREAK + TEMPO_STEP_WEIGHT + (step - options.max_bpm_step.max(0.0))
    };
    let artist_cost = if transition.same_artist && !options.allow_repeated_artists {
        RULE_BREAK
    } else {
        0.0
    };
    key_cost + tempo_cost + artist_cost
}

#[cfg(test)]
mod tests;
