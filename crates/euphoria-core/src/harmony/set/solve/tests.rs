// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use super::*;
use crate::harmony::mix::MixTrack;
use crate::harmony::set::{
    CurvePreset, DEFAULT_MAX_BPM_STEP, EnergyCurve, build_set, evaluate_set,
};
use crate::key::{Key, Mode};

#[derive(Debug, Clone)]
struct PoolTrack {
    key: Option<Key>,
    bpm: Option<f32>,
    energy: Option<f32>,
    artist: Option<String>,
}

impl MixTrack for PoolTrack {
    fn key(&self) -> Option<Key> {
        self.key
    }

    fn bpm(&self) -> Option<f32> {
        self.bpm
    }

    fn energy(&self) -> Option<f32> {
        self.energy
    }
}

impl SetTrack for PoolTrack {
    fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }
}

fn track(code: &str, bpm: f32, energy: f32) -> PoolTrack {
    PoolTrack {
        key: Some(code.parse().unwrap()),
        bpm: Some(bpm),
        energy: Some(energy),
        artist: None,
    }
}

fn by(artist: &str, mut track: PoolTrack) -> PoolTrack {
    track.artist = Some(artist.to_string());
    track
}

/// A pseudo random generator with a fixed seed, so every run sees the same
/// pool.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }
}

/// A pool like a real crate of house music: any key, 118 to 130 BPM, energy
/// 2 to 9 with most tracks in the middle, forty artists.
fn library(size: usize, seed: u64) -> Vec<PoolTrack> {
    let mut generator = Generator(seed);
    (0..size)
        .map(|_| {
            let tonic = generator.below(12) as u8;
            let mode = if generator.below(2) == 0 {
                Mode::Minor
            } else {
                Mode::Major
            };
            let bpm = 118.0 + generator.below(25) as f32 / 2.0;
            let energy = (2 + generator.below(4) + generator.below(5)) as f32;
            PoolTrack {
                key: Some(Key::new(tonic, mode)),
                bpm: Some(bpm),
                energy: Some(energy),
                artist: Some(format!("Artist {}", generator.below(40))),
            }
        })
        .collect()
}

fn options(track_count: usize, curve: &str) -> SetOptions {
    SetOptions::new(track_count, curve.parse().unwrap())
}

fn order(plan: &SetPlan) -> Vec<usize> {
    plan.tracks.iter().map(|planned| planned.track).collect()
}

#[test]
fn follows_a_feasible_curve_with_every_transition_clean() {
    let pool = library(500, 7);
    let plan = build_set(&pool, &options(20, "4,6,8,9,7,5")).unwrap();

    assert_eq!(plan.tracks.len(), 20);
    assert_eq!(plan.transitions.len(), 19);
    assert!(plan.deviations.is_empty(), "{:?}", plan.deviations);
    assert!(plan.every_transition_is_clean());
    assert!(
        plan.mean_energy_error() <= 0.5,
        "{}",
        plan.mean_energy_error()
    );
    for transition in &plan.transitions {
        assert!(transition.is_clean(false));
        assert!(transition.bpm_change.abs() <= DEFAULT_MAX_BPM_STEP);
    }
    let mut seen = order(&plan);
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 20, "a track is used twice");
    assert!(plan.cost >= plan.lower_bound - 1e-6);
}

#[test]
fn an_unreachable_peak_gives_the_closest_set_and_says_where_it_falls_short() {
    let mut generator = Generator(11);
    let pool: Vec<PoolTrack> = (0..120)
        .map(|index| {
            let mut planned = track(
                &format!("{}A", 1 + generator.below(12)),
                124.0,
                (3 + index % 4) as f32,
            );
            planned.artist = Some(format!("Artist {index}"));
            planned
        })
        .collect();
    let plan = build_set(&pool, &options(9, "4,6,9,6,4")).unwrap();

    assert!(plan.every_transition_is_clean(), "{:?}", plan.deviations);
    let off_target: Vec<usize> = plan
        .deviations
        .iter()
        .filter_map(|deviation| match deviation {
            Deviation::EnergyOffTarget {
                position,
                target,
                actual,
            } => {
                assert!(*target > 7.0);
                assert_eq!(*actual, 6.0, "the peak gets the most energetic tracks");
                Some(*position)
            }
            _ => None,
        })
        .collect();
    assert_eq!(off_target, [3, 4, 5]);
    for planned in &plan.tracks {
        let closest = planned.target_energy.clamp(3.0, 6.0);
        assert!(
            (planned.energy - closest).abs() <= 0.5 + 1e-6,
            "{planned:?}"
        );
    }
}

#[test]
fn keys_that_cannot_chain_break_the_fewest_rules_and_say_where() {
    let pool = [
        track("1A", 124.0, 5.0),
        track("7A", 124.0, 5.0),
        track("1A", 124.0, 5.0),
        track("7A", 124.0, 5.0),
    ];
    let plan = build_set(&pool, &options(4, "5")).unwrap();
    let clashes: Vec<&Deviation> = plan
        .deviations
        .iter()
        .filter(|deviation| matches!(deviation, Deviation::KeyClash { .. }))
        .collect();
    assert_eq!(clashes.len(), 1, "{:?}", plan.deviations);
    assert!(matches!(
        clashes[0],
        Deviation::KeyClash {
            position: 2,
            relation: None
        }
    ));
    assert!(!plan.every_transition_is_clean());
}

#[test]
fn a_tempo_gap_is_crossed_once_and_reported() {
    let pool = [
        track("8A", 100.0, 5.0),
        track("8A", 140.0, 5.0),
        track("8A", 101.0, 5.0),
        track("8A", 139.0, 5.0),
    ];
    let plan = build_set(&pool, &options(4, "5")).unwrap();
    let jumps: Vec<&Deviation> = plan
        .deviations
        .iter()
        .filter(|deviation| matches!(deviation, Deviation::TempoJump { .. }))
        .collect();
    assert_eq!(jumps.len(), 1, "{:?}", plan.deviations);
    let Deviation::TempoJump { bpm_change, .. } = jumps[0] else {
        unreachable!()
    };
    assert_eq!(bpm_change.abs(), 38.0, "the smallest jump across the gap");
}

#[test]
fn rising_energy_prefers_the_smoother_key_move() {
    let pool = [
        track("8A", 124.0, 4.0),
        track("9A", 124.0, 6.0),
        track("3A", 124.0, 6.0),
    ];
    let mut options = options(2, "4,6");
    options.relations = Relation::ALL.to_vec();
    let plan = build_set(&pool, &options).unwrap();
    assert_eq!(order(&plan), [0, 1]);
    assert_eq!(plan.transitions[0].relation, Some(Relation::Adjacent));
}

#[test]
fn only_the_allowed_key_moves_count_as_clean() {
    let pool = [track("8A", 124.0, 5.0), track("10A", 124.0, 5.0)];
    let mut strict = options(2, "5");
    strict.relations = vec![Relation::Same, Relation::Adjacent];
    let plan = build_set(&pool, &strict).unwrap();
    assert_eq!(order(&plan), [0, 1], "the energy boost goes up, not down");
    assert_eq!(
        plan.deviations,
        [Deviation::KeyClash {
            position: 1,
            relation: Some(Relation::EnergyBoost)
        }]
    );

    let plan = build_set(&pool, &options(2, "5")).unwrap();
    assert!(plan.deviations.is_empty());
}

#[test]
fn pinned_tracks_stay_first_last_and_in_the_middle() {
    let pool = library(200, 3);
    let mut options = options(12, "journey");
    options.pins = vec![
        Pin {
            track: 17,
            position: PinPosition::At(0),
        },
        Pin {
            track: 42,
            position: PinPosition::Last,
        },
        Pin {
            track: 99,
            position: PinPosition::At(5),
        },
    ];
    let plan = build_set(&pool, &options).unwrap();
    let tracks = order(&plan);
    assert_eq!(tracks[0], 17);
    assert_eq!(tracks[5], 99);
    assert_eq!(tracks[11], 42);
    let pinned: Vec<bool> = plan.tracks.iter().map(|planned| planned.pinned).collect();
    assert_eq!(pinned.iter().filter(|&&pinned| pinned).count(), 3);
    assert!(pinned[0] && pinned[5] && pinned[11]);
    assert_eq!(tracks.iter().filter(|&&index| index == 17).count(), 1);
}

#[test]
fn an_artist_never_plays_twice_in_a_row_unless_allowed() {
    let pool = [
        by("Someone", track("8A", 124.0, 5.0)),
        by("Someone feat. Guest", track("8A", 124.0, 6.0)),
        by("Other", track("8A", 124.0, 9.0)),
    ];
    let plan = build_set(&pool, &options(3, "5,6,6")).unwrap();
    assert!(plan.every_transition_is_clean(), "{:?}", plan.deviations);
    assert_eq!(order(&plan), [0, 2, 1]);

    let mut relaxed = options(3, "5,6,6");
    relaxed.allow_repeated_artists = true;
    let plan = build_set(&pool, &relaxed).unwrap();
    assert!(
        plan.deviations
            .iter()
            .all(|deviation| !matches!(deviation, Deviation::RepeatedArtist { .. }))
    );
    assert!(
        plan.transitions
            .iter()
            .any(|transition| transition.same_artist)
    );
}

#[test]
fn artist_tags_are_split_into_names() {
    assert_eq!(artist_names("Someone feat. Guest"), ["guest", "someone"]);
    assert_eq!(artist_names("A & B, C"), ["a", "b", "c"]);
    assert_eq!(artist_names("  Solo  "), ["solo"]);
    assert_eq!(artist_names("Xavier"), ["xavier"]);
    assert_eq!(artist_names("AC/DC"), ["ac", "dc"]);
}

#[test]
fn a_short_pool_gives_a_shorter_set_and_lists_what_it_left_out() {
    let pool = vec![
        track("8A", 124.0, 5.0),
        PoolTrack {
            key: None,
            ..track("8A", 124.0, 5.0)
        },
        track("9A", 125.0, 6.0),
        PoolTrack {
            bpm: Some(0.0),
            ..track("8A", 124.0, 5.0)
        },
        PoolTrack {
            energy: None,
            ..track("8A", 124.0, 5.0)
        },
    ];
    let plan = build_set(&pool, &options(10, "5,6")).unwrap();
    assert_eq!(plan.tracks.len(), 2);
    assert_eq!(
        plan.deviations[0],
        Deviation::ShortPool {
            requested: 10,
            available: 2
        }
    );
    assert_eq!(
        plan.left_out,
        [
            LeftOut {
                track: 1,
                missing: MissingFeature::Key
            },
            LeftOut {
                track: 3,
                missing: MissingFeature::Tempo
            },
            LeftOut {
                track: 4,
                missing: MissingFeature::Energy
            },
        ]
    );
}

#[test]
fn impossible_requests_are_errors() {
    let pool = [track("8A", 124.0, 5.0), track("9A", 124.0, 5.0)];
    assert_eq!(build_set(&pool, &options(0, "5")), Err(SetError::NoTracks));
    let empty: [PoolTrack; 0] = [];
    assert_eq!(
        build_set(&empty, &options(3, "5")),
        Err(SetError::NothingUsable)
    );

    let pinned = |pins: Vec<Pin>| {
        let mut options = options(2, "5");
        options.pins = pins;
        build_set(&pool, &options)
    };
    assert_eq!(
        pinned(vec![Pin {
            track: 5,
            position: PinPosition::At(0)
        }]),
        Err(SetError::PinnedTrackMissing { track: 5 })
    );
    assert_eq!(
        pinned(vec![Pin {
            track: 0,
            position: PinPosition::At(2)
        }]),
        Err(SetError::PinPastTheEnd {
            position: 2,
            track_count: 2
        })
    );
    assert_eq!(
        pinned(vec![
            Pin {
                track: 0,
                position: PinPosition::At(1)
            },
            Pin {
                track: 1,
                position: PinPosition::Last
            }
        ]),
        Err(SetError::ConflictingPins {
            position: 1,
            track: 1
        })
    );
    let unusable = [PoolTrack {
        key: None,
        ..track("8A", 124.0, 5.0)
    }];
    let mut options = options(1, "5");
    options.pins = vec![Pin {
        track: 0,
        position: PinPosition::At(0),
    }];
    assert_eq!(
        build_set(&unusable, &options),
        Err(SetError::PinnedTrackUnusable {
            track: 0,
            missing: MissingFeature::Key
        })
    );
}

#[test]
fn the_same_input_always_gives_the_same_set() {
    let pool = library(300, 21);
    let options = SetOptions::new(25, EnergyCurve::preset(CurvePreset::PeakTime));
    let first = build_set(&pool, &options).unwrap();
    for _ in 0..3 {
        assert_eq!(build_set(&pool, &options).unwrap(), first);
    }
}

/// The lowest cost over every ordered choice of `count` tracks of the pool.
fn exhaustive_best(problem: &Problem<'_>, count: usize) -> f64 {
    fn extend(problem: &Problem<'_>, order: &mut Vec<usize>, count: usize, best: &mut f64) {
        if order.len() == count {
            *best = best.min(problem.total_cost(order));
            return;
        }
        for track in 0..problem.width() {
            if !order.contains(&track) {
                order.push(track);
                extend(problem, order, count, best);
                order.pop();
            }
        }
    }
    let mut best = f64::INFINITY;
    extend(problem, &mut Vec::new(), count, &mut best);
    best
}

#[test]
fn finds_the_best_set_of_small_pools_that_can_be_checked_exhaustively() {
    for seed in 0..12 {
        let pool = library(8, 100 + seed);
        let candidates: Vec<Candidate> = pool
            .iter()
            .enumerate()
            .map(|(index, track)| candidate(index, track).unwrap())
            .collect();
        let mut options = options(5, "3,6,8,5");
        options.max_bpm_step = 3.0;
        let problem = Problem::new(&candidates, &options, 5, &[]);
        let best = exhaustive_best(&problem, 5);
        let plan = build_set(&pool, &options).unwrap();
        assert!(
            (plan.cost - best).abs() < 1e-6,
            "seed {seed}: found {} where the best is {best}",
            plan.cost
        );
        assert!(plan.lower_bound <= best + 1e-6);
    }
}

#[test]
fn orders_five_hundred_tracks_into_thirty_quickly() {
    let pool = library(500, 5);
    let options = options(30, "4,6,8,9,7,5");
    // A release build on an M5 with every core busy took 30 to 60 ms, and
    // 180 ms at its slowest; the bound is ten times the slowest run. A debug
    // build is about a hundred times slower, so it checks the result only.
    let runs = if cfg!(debug_assertions) { 1 } else { 3 };
    let elapsed = (0..runs)
        .map(|_| {
            let started = Instant::now();
            let plan = build_set(&pool, &options).unwrap();
            let elapsed = started.elapsed();
            assert_eq!(plan.tracks.len(), 30);
            assert!(plan.every_transition_is_clean());
            elapsed
        })
        .min()
        .unwrap();
    if !cfg!(debug_assertions) {
        assert!(elapsed < Duration::from_millis(1_800), "took {elapsed:?}");
    }
}

#[test]
fn a_reordered_set_is_explained_as_given() {
    let pool = library(120, 9);
    let options = options(10, "4,6,8,9,7,5");
    let built = build_set(&pool, &options).unwrap();
    let mut order = order(&built);
    let again = evaluate_set(&pool, &order, &options).unwrap();
    assert_eq!(again.tracks, built.tracks);
    assert_eq!(again.transitions, built.transitions);
    assert_eq!(again.deviations, built.deviations);
    assert!((again.cost - built.cost).abs() < 1e-6);

    order.swap(0, 6);
    let swapped = evaluate_set(&pool, &order, &options).unwrap();
    assert_eq!(swapped.tracks[0].track, order[0]);
    assert!(swapped.cost >= built.cost);
    assert!(
        swapped
            .deviations
            .iter()
            .any(|deviation| matches!(deviation, Deviation::EnergyOffTarget { .. }))
    );

    let mut pinned = options.clone();
    pinned.pins = vec![Pin {
        track: order[3],
        position: PinPosition::At(3),
    }];
    let explained = evaluate_set(&pool, &order, &pinned).unwrap();
    let flags: Vec<bool> = explained.tracks.iter().map(|track| track.pinned).collect();
    assert_eq!(flags.iter().filter(|&&flag| flag).count(), 1);
    assert!(flags[3]);

    assert_eq!(
        evaluate_set(&pool, &[order[0], order[0]], &options),
        Err(SetError::ConflictingPins {
            position: 1,
            track: order[0]
        })
    );
    assert_eq!(evaluate_set(&pool, &[], &options), Err(SetError::NoTracks));
}
