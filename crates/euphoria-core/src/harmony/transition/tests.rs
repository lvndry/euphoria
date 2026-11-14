// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::analysis::TrackAnalysis;
use crate::detect::{BeatGrid, CueSection, SectionCue};
use crate::harmony::Relation;
use crate::key::{Key, Mode};

const BPM: f64 = 120.0;
const BAR_SECONDS: f64 = 2.0;

struct Synthetic {
    analysis: TrackAnalysis,
    sections: Vec<PlanSection>,
    energy: Vec<EnergyPoint>,
    stems: Option<StemLevels>,
}

impl Synthetic {
    fn track(&self) -> PlanTrack<'_> {
        PlanTrack {
            analysis: &self.analysis,
            sections: &self.sections,
            energy: &self.energy,
            stems: self.stems.as_ref(),
        }
    }
}

fn analysis(bpm: f64, bar_count: usize, key: Option<Key>) -> TrackAnalysis {
    let grid = BeatGrid {
        first_downbeat_seconds: 0.0,
        bpm,
        salience: 3.0,
        drift_seconds: None,
    };
    TrackAnalysis {
        duration_seconds: (bar_count as f64 * grid.bar_seconds()) as f32,
        key,
        bpm: Some(bpm as f32),
        energy: Some(6),
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: None,
        key_score_margin: None,
        tempo_estimate: Some(bpm as f32),
        tempo_score_margin: None,
        beat_grid: Some(grid),
        cues: Vec::new(),
    }
}

fn stem_levels(
    bar_count: usize,
    vocals: impl Fn(usize) -> bool,
    bass: impl Fn(usize) -> bool,
    drums: impl Fn(usize) -> bool,
) -> StemLevels {
    let per_bar = (BAR_SECONDS / STEM_LEVEL_HOP_SECONDS) as usize;
    let lane = |present: &dyn Fn(usize) -> bool| -> Vec<f32> {
        (0..bar_count * per_bar)
            .map(|frame| {
                if present(frame / per_bar) {
                    0.2
                } else {
                    0.0005
                }
            })
            .collect()
    };
    StemLevels {
        hop_seconds: STEM_LEVEL_HOP_SECONDS,
        vocals: lane(&vocals),
        bass: lane(&bass),
        drums: lane(&drums),
    }
}

fn sections(starts: &[(usize, SectionLabel)]) -> Vec<PlanSection> {
    starts
        .iter()
        .map(|(start_bar, label)| PlanSection {
            start_bar: *start_bar,
            label: *label,
        })
        .collect()
}

fn song_a() -> Synthetic {
    Synthetic {
        analysis: analysis(BPM, 128, Some(Key::new(9, Mode::Minor))),
        sections: sections(&[
            (0, SectionLabel::Start),
            (16, SectionLabel::Main),
            (64, SectionLabel::Breakdown),
            (80, SectionLabel::Drop),
            (112, SectionLabel::Outro),
        ]),
        energy: Vec::new(),
        stems: Some(stem_levels(
            128,
            |bar| (16..48).contains(&bar),
            |bar| (16..64).contains(&bar) || (80..112).contains(&bar),
            |_| true,
        )),
    }
}

fn song_b() -> Synthetic {
    Synthetic {
        analysis: analysis(BPM, 128, Some(Key::new(9, Mode::Minor))),
        sections: sections(&[
            (0, SectionLabel::Start),
            (16, SectionLabel::Main),
            (96, SectionLabel::Outro),
        ]),
        energy: Vec::new(),
        stems: Some(stem_levels(
            128,
            |bar| (32..64).contains(&bar),
            |bar| bar >= 16,
            |_| true,
        )),
    }
}

fn plan(a: &Synthetic, b: &Synthetic) -> TransitionPlan {
    plan_transition(&a.track(), &b.track(), &PlanOptions { window_count: 1000 }).unwrap()
}

fn window(plan: &TransitionPlan, a_start: usize, b_start: usize, bars: usize) -> &CandidateWindow {
    plan.windows
        .iter()
        .find(|window| {
            window.a_start_bar == a_start && window.b_start_bar == b_start && window.bars == bars
        })
        .unwrap_or_else(|| panic!("no window at A {a_start}, B {b_start}, {bars} bars"))
}

#[test]
fn automatic_cues_become_sections_on_the_bars_of_the_grid() {
    let mut track = analysis(BPM, 64, None);
    track.cues = vec![
        SectionCue {
            seconds: 0.0,
            section: CueSection::Start,
        },
        SectionCue {
            seconds: 32.0,
            section: CueSection::Main,
        },
        SectionCue {
            seconds: 96.1,
            section: CueSection::Breakdown,
        },
    ];
    assert_eq!(
        AutomaticCues.sections(&track),
        sections(&[
            (0, SectionLabel::Start),
            (16, SectionLabel::Main),
            (48, SectionLabel::Breakdown)
        ])
    );
    track.beat_grid = None;
    assert!(AutomaticCues.sections(&track).is_empty());
}

#[test]
fn a_stem_sounds_above_a_share_of_its_own_body_level_and_above_the_floor() {
    let per_bar = (BAR_SECONDS / STEM_LEVEL_HOP_SECONDS) as usize;
    let level_of_bar = |bar: usize| match bar {
        0..=7 => 0.4,
        8 => 0.15,
        9 => 0.05,
        10 => 0.005,
        _ => 0.0,
    };
    let stems = StemLevels {
        hop_seconds: STEM_LEVEL_HOP_SECONDS,
        vocals: (0..12 * per_bar)
            .map(|frame| level_of_bar(frame / per_bar))
            .collect(),
        bass: vec![0.0; 12 * per_bar],
        drums: vec![0.0; 12 * per_bar],
    };
    let track = Synthetic {
        analysis: analysis(BPM, 12, None),
        sections: Vec::new(),
        energy: Vec::new(),
        stems: Some(stems),
    };
    let lanes = TrackLanes::of(&track.track(), &track.analysis.beat_grid.unwrap());
    let vocals: Vec<Presence> = lanes.bars.iter().map(|bar| bar.vocals).collect();
    assert!(
        vocals[..9]
            .iter()
            .all(|presence| *presence == Presence::Present)
    );
    assert!(
        vocals[9..]
            .iter()
            .all(|presence| *presence == Presence::Absent)
    );
    assert!(lanes.bars.iter().all(|bar| bar.bass == Presence::Absent));
    assert!(lanes.stems_known);
}

#[test]
fn lanes_carry_the_section_and_the_energy_of_every_bar() {
    let mut track = song_a();
    track.energy = vec![
        EnergyPoint {
            start_seconds: 0.0,
            duration_seconds: 4.0,
            energy: Some(3.0),
        },
        EnergyPoint {
            start_seconds: 32.0,
            duration_seconds: 4.0,
            energy: Some(8.0),
        },
    ];
    let lanes = TrackLanes::of(&track.track(), &track.analysis.beat_grid.unwrap());
    assert_eq!(lanes.bars.len(), 128);
    assert_eq!(lanes.bars[0].section, Some(SectionLabel::Start));
    assert_eq!(lanes.bars[15].section, Some(SectionLabel::Start));
    assert_eq!(lanes.bars[16].section, Some(SectionLabel::Main));
    assert_eq!(lanes.bars[127].section, Some(SectionLabel::Outro));
    assert_eq!(lanes.bars[0].energy, Some(3.0));
    assert_eq!(lanes.bars[16].energy, Some(8.0));
    assert_eq!(lanes.bars[100].energy, None);
    assert_eq!(lanes.bars[3].start_seconds, 3.0 * BAR_SECONDS);
}

#[test]
fn a_clean_window_is_found_and_ranked_first() {
    let plan = plan(&song_a(), &song_b());
    let best = &plan.windows[0];
    assert!(best.clean, "{best:?}");
    assert!(best.clash_strip.iter().all(|clash| *clash == Clash::Clear));
    assert_eq!(best.rank, 1);
    assert!(best.a_has_room);
    assert!(best.reason.contains("no vocal overlap"), "{}", best.reason);
    assert!(
        plan.windows
            .windows(2)
            .all(|pair| pair[0].score >= pair[1].score)
    );
}

#[test]
fn a_vocal_clash_is_avoided() {
    let mut a = song_a();
    a.stems = Some(stem_levels(
        128,
        |bar| (16..112).contains(&bar),
        |bar| (16..112).contains(&bar),
        |_| true,
    ));
    let mut b = song_b();
    b.stems = Some(stem_levels(128, |_| true, |bar| bar >= 16, |_| true));
    let plan = plan(&a, &b);
    let singing_over_singing = window(&plan, 96, 0, 16);
    assert_eq!(singing_over_singing.clash_counts.vocals, 16);
    assert!(!singing_over_singing.clean);
    let best = &plan.windows[0];
    assert_eq!(best.clash_counts.vocals, 0, "{best:?}");
    assert_eq!(best.a_start_bar, 112);
    assert!(best.score > singing_over_singing.score);
}

#[test]
fn the_bass_swap_is_the_first_bar_from_which_both_never_play_bass_again() {
    let mut a = song_a();
    a.analysis = analysis(BPM, 48, None);
    a.sections = sections(&[
        (0, SectionLabel::Start),
        (8, SectionLabel::Main),
        (32, SectionLabel::Outro),
    ]);
    a.stems = Some(stem_levels(48, |_| false, |bar| bar < 40, |_| true));
    let mut b = song_b();
    b.stems = Some(stem_levels(128, |_| false, |_| true, |_| true));
    let plan = plan(&a, &b);
    let swapped = window(&plan, 32, 0, 16);
    assert_eq!(swapped.clash_counts.bass, 8);
    assert_eq!(swapped.bass_swap_bar, Some(9));
    assert!(
        swapped.reason.contains("bass swap at bar 9"),
        "{}",
        swapped.reason
    );
    let swap_step = swapped
        .steps
        .iter()
        .find(|step| step.kind == StepKind::SwapBass)
        .unwrap();
    assert_eq!(
        (swap_step.window_bar, swap_step.a_bar, swap_step.b_bar),
        (9, 40, 8)
    );
    let kinds: Vec<StepKind> = swapped.steps.iter().map(|step| step.kind).collect();
    assert_eq!(
        kinds,
        [StepKind::StartB, StepKind::SwapBass, StepKind::AEnds]
    );
    assert_eq!(swapped.steps.last().unwrap().window_bar, 16);
}

#[test]
fn no_bass_swap_is_offered_when_both_play_bass_to_the_end_of_the_window() {
    let mut a = song_a();
    a.stems = Some(stem_levels(128, |_| false, |_| true, |_| true));
    let mut b = song_b();
    b.stems = Some(stem_levels(128, |_| false, |_| true, |_| true));
    let plan = plan(&a, &b);
    let stuck = window(&plan, 32, 16, 16);
    assert_eq!(stuck.clash_counts.bass, 16);
    assert_eq!(stuck.bass_swap_bar, None);
    assert!(stuck.reason.contains("no clean swap"), "{}", stuck.reason);
}

#[test]
fn b_vocals_come_in_after_the_last_bar_where_both_would_sing() {
    let mut a = song_a();
    a.stems = Some(stem_levels(128, |bar| bar < 104, |_| false, |_| true));
    let mut b = song_b();
    b.stems = Some(stem_levels(128, |_| true, |_| false, |_| true));
    let plan = plan(&a, &b);
    let crossing = window(&plan, 96, 16, 16);
    assert_eq!(crossing.clash_counts.vocals, 8);
    assert_eq!(crossing.vocals_in_bar, Some(9));
    assert!(crossing.steps.iter().any(|step| {
        step.kind == StepKind::BringInVocals && step.window_bar == 9 && step.b_bar == 24
    }));
}

#[test]
fn no_plan_when_the_tempos_are_out_of_reach() {
    let mut slow = song_a();
    slow.analysis = analysis(60.0, 64, None);
    let mut fast = song_b();
    fast.analysis = analysis(150.0, 64, None);
    let error = plan_transition(&slow.track(), &fast.track(), &PlanOptions::default()).unwrap_err();
    assert!(matches!(error, PlanError::TempoOutOfReach { rate } if (rate - 0.4).abs() < 1e-9));
    assert!(error.to_string().contains("0.40"));
    let error = plan_transition(&fast.track(), &slow.track(), &PlanOptions::default()).unwrap_err();
    assert!(matches!(error, PlanError::TempoOutOfReach { rate } if (rate - 2.5).abs() < 1e-9));
}

#[test]
fn the_edge_of_what_a_deck_reaches_still_plans() {
    let mut slow = song_b();
    slow.analysis = analysis(60.0, 64, None);
    let plan = plan_transition(&song_a().track(), &slow.track(), &PlanOptions::default()).unwrap();
    assert_eq!(plan.alignment.rate, 2.0);
    assert_eq!(plan.alignment.tempo_change_percent, 100.0);
}

#[test]
fn a_track_without_a_beat_grid_cannot_be_planned() {
    let mut gridless = song_b();
    gridless.analysis.beat_grid = None;
    assert_eq!(
        plan_transition(
            &song_a().track(),
            &gridless.track(),
            &PlanOptions::default()
        )
        .unwrap_err(),
        PlanError::NoBeatGrid(Side::B)
    );
    assert_eq!(
        plan_transition(
            &gridless.track(),
            &song_a().track(),
            &PlanOptions::default()
        )
        .unwrap_err(),
        PlanError::NoBeatGrid(Side::A)
    );
}

#[test]
fn b_is_put_on_the_bars_of_a_at_a_s_tempo() {
    let mut b = song_b();
    b.analysis = analysis(126.0, 128, None);
    let plan = plan_transition(&song_a().track(), &b.track(), &PlanOptions::default()).unwrap();
    assert!((plan.alignment.rate - 120.0 / 126.0).abs() < 1e-12);
    assert!((plan.alignment.tempo_change_percent + 100.0 / 21.0).abs() < 1e-9);
    assert_eq!(plan.alignment.bar_seconds, BAR_SECONDS);
    assert!((plan.lanes_b.bar_seconds - 240.0 / 126.0).abs() < 1e-9);
}

#[test]
fn missing_stems_are_unknown_and_never_guessed() {
    let mut a = song_a();
    a.stems = None;
    let mut b = song_b();
    b.stems = None;
    let plan = plan(&a, &b);
    assert!(!plan.lanes_a.stems_known);
    assert!(plan.lanes_a.bars.iter().all(|bar| {
        bar.vocals == Presence::Unknown
            && bar.bass == Presence::Unknown
            && bar.drums == Presence::Unknown
    }));
    let best = &plan.windows[0];
    assert!(!best.clean);
    assert_eq!(best.clash_counts.unknown, best.bars);
    assert!(
        best.clash_strip
            .iter()
            .all(|clash| *clash == Clash::Unknown)
    );
    assert_eq!(best.breakdown.clash, None);
    assert_eq!(best.bass_swap_bar, None);
    assert_eq!(best.vocals_in_bar, None);
    assert!(best.reason.contains("stems missing"), "{}", best.reason);
}

#[test]
fn stems_on_one_side_only_clear_a_bar_where_the_known_side_is_silent() {
    let a = song_a();
    let mut b = song_b();
    b.stems = None;
    let plan = plan(&a, &b);
    let in_the_outro = &plan.lanes_a.bars[112];
    let somewhere_in_b = &plan.lanes_b.bars[20];
    assert_eq!(Clash::of(in_the_outro, somewhere_in_b), Clash::Clear);
    let in_the_main = &plan.lanes_a.bars[20];
    assert_eq!(Clash::of(in_the_main, somewhere_in_b), Clash::Unknown);
}

#[test]
fn a_window_that_outlasts_a_scores_lower() {
    let mut a = song_a();
    a.analysis = analysis(BPM, 120, None);
    let mut b = song_b();
    b.analysis.key = None;
    a.stems = None;
    b.stems = None;
    let plan = plan(&a, &b);
    let fits = window(&plan, 96, 0, 16);
    let overruns = window(&plan, 112, 0, 16);
    assert!(fits.a_has_room);
    assert!(!overruns.a_has_room);
    assert_eq!(overruns.breakdown.room, 0.0);
    assert!(fits.score > overruns.score);
    assert!(overruns.reason.contains("A ends before the window does"));
}

#[test]
fn closer_energy_and_a_closer_key_rank_higher() {
    let point = |start_seconds: f32, energy: f32| EnergyPoint {
        start_seconds,
        duration_seconds: 2.0,
        energy: Some(energy),
    };
    let flat = |energy: f32| {
        (0..128)
            .map(|bar| point(bar as f32 * 2.0, energy))
            .collect::<Vec<_>>()
    };
    let mut a = song_a();
    a.energy = flat(8.0);
    let mut calm = song_b();
    calm.energy = flat(2.0);
    let mut matching = song_b();
    matching.energy = flat(8.0);
    let calm_plan = plan(&a, &calm);
    let matching_plan = plan(&a, &matching);
    let calm_window = window(&calm_plan, 112, 0, 16);
    let matching_window = window(&matching_plan, 112, 0, 16);
    assert_eq!(matching_window.breakdown.energy, Some(1.0));
    assert!((calm_window.breakdown.energy.unwrap() - (1.0 - 6.0 / 9.0)).abs() < 1e-6);
    assert!(matching_window.score > calm_window.score);

    let mut unrelated = song_b();
    unrelated.analysis.key = Some(Key::new(2, Mode::Major));
    let unrelated_plan = plan(&song_a(), &unrelated);
    let same_plan = plan(&song_a(), &song_b());
    let key_clash = window(&unrelated_plan, 112, 0, 16);
    let same_key = window(&same_plan, 112, 0, 16);
    assert_eq!(same_key.key_relation, Some(Relation::Same));
    assert_eq!(same_key.breakdown.key, Some(1.0));
    assert_eq!(key_clash.breakdown.key, Some(0.0));
    assert!(same_key.score > key_clash.score);
}

#[test]
fn windows_start_on_phrase_boundaries_and_are_16_32_or_64_bars() {
    let plan = plan(&song_a(), &song_b());
    assert!(!plan.windows.is_empty());
    for window in &plan.windows {
        assert!(WINDOW_LENGTHS.contains(&window.bars));
        let phrase = if window.bars == 16 {
            PHRASE_BARS
        } else {
            LONG_PHRASE_BARS
        };
        assert_eq!(window.a_start_bar % phrase, 0, "{window:?}");
        assert_ne!(window.a_section, Some(SectionLabel::Start));
        assert_ne!(window.b_section, Some(SectionLabel::Outro));
        assert_eq!(window.clash_strip.len(), window.bars);
        assert!(window.b_start_bar + window.bars <= 128);
    }
    let pairs: std::collections::HashSet<(usize, usize)> = plan
        .windows
        .iter()
        .map(|window| (window.a_start_bar, window.b_start_bar))
        .collect();
    assert_eq!(pairs.len(), plan.windows.len());
}

#[test]
fn the_top_window_gets_its_cues_on_both_tracks() {
    let mut a = song_a();
    a.stems = Some(stem_levels(128, |_| false, |bar| bar < 120, |_| true));
    let mut b = song_b();
    b.stems = Some(stem_levels(128, |_| false, |_| true, |_| true));
    let plan = plan(&a, &b);
    let top = &plan.windows[0];
    let cues = plan.cues.as_ref().unwrap();
    assert_eq!(cues.a[0].name, "Mix out");
    assert_eq!(cues.a[0].seconds, top.a_start_bar as f64 * BAR_SECONDS);
    assert_eq!(cues.b[0].name, "Mix in");
    assert_eq!(cues.b[0].seconds, top.b_start_bar as f64 * BAR_SECONDS);
    if let Some(bar) = top.bass_swap_bar {
        let swap = cues.b.iter().find(|cue| cue.name == "Bass swap").unwrap();
        assert_eq!(
            swap.seconds,
            (top.b_start_bar + bar - 1) as f64 * BAR_SECONDS
        );
    }
    let as_cues = cues.b_cues();
    assert_eq!(as_cues[0].hot_cue, None);
    assert_eq!(as_cues[0].name, "Mix in");
}

#[test]
fn planning_twice_gives_the_same_plan() {
    assert_eq!(plan(&song_a(), &song_b()), plan(&song_a(), &song_b()));
}

#[test]
fn tracks_too_short_for_any_window_give_no_window_and_no_cues() {
    let mut a = song_a();
    a.analysis = analysis(BPM, 12, None);
    a.sections = Vec::new();
    a.stems = None;
    let mut b = song_b();
    b.analysis = analysis(BPM, 12, None);
    b.sections = Vec::new();
    b.stems = None;
    let plan = plan(&a, &b);
    assert!(plan.windows.is_empty());
    assert!(plan.cues.is_none());
}

#[test]
fn a_track_whose_only_section_is_its_start_may_mix_out_anywhere_on_a_phrase() {
    let mut a = song_a();
    a.sections = sections(&[(0, SectionLabel::Start)]);
    a.stems = None;
    let mut b = song_b();
    b.stems = None;
    let plan = plan(&a, &b);
    assert!(plan.windows.iter().any(|window| window.a_start_bar == 112));
    assert!(
        plan.windows
            .iter()
            .all(|window| window.a_start_bar % PHRASE_BARS == 0)
    );
}
