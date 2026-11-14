// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The beat grid read from a rekordbox collection: a constant grid, variable
//! ones, a track with no grid, a track missing from the collection, who wins
//! between a correction and the collection, and cues placed on the imported
//! grid. The collection is XML written to Pioneer's published description of
//! the format, not exported by rekordbox.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use euphoria_core::rekordbox_grid::{
    CollectionError, CollectionGrid, GridChoice, GridSource, RekordboxGrid, VariableReason,
    choose_grid, read_collection_grids, rekordbox_location_path,
};
use euphoria_core::{AudioBuffer, BeatGrid, Correction, Key, Mode, TrackAnalysis};

const COLLECTION: &str = include_str!("fixtures/rekordbox/grids.xml");

fn grid_of(track_file: &str) -> Option<RekordboxGrid> {
    let wanted = PathBuf::from(format!("/Users/dj/Music/{track_file}"));
    read_collection_grids(COLLECTION)
        .expect("the fixture is a collection")
        .into_iter()
        .find(|track| track.path.as_deref() == Some(Path::new(&wanted)))
        .map(|track: CollectionGrid| track.grid)
}

fn constant(track_file: &str) -> BeatGrid {
    grid_of(track_file)
        .expect("the fixture has the track")
        .usable()
        .expect("the track has a constant grid")
}

#[test]
fn a_constant_grid_gives_the_first_downbeat_and_the_tempo() {
    let grid = constant("constant.wav");
    assert_eq!(grid.bpm, 120.0);
    assert!((grid.first_downbeat_seconds - 0.5).abs() < 1e-9);
}

#[test]
fn a_marker_on_the_third_beat_is_counted_back_to_the_downbeat() {
    let grid = constant("offbeat.wav");
    assert_eq!(grid.bpm, 120.0);
    assert!(
        (grid.first_downbeat_seconds - 0.0).abs() < 1e-9,
        "{}",
        grid.first_downbeat_seconds
    );
}

#[test]
fn markers_with_one_tempo_on_the_same_grid_are_constant() {
    assert_eq!(constant("repeated.wav"), constant("constant.wav"));
}

#[test]
fn a_grid_that_changes_tempo_is_reported_as_variable_and_not_used() {
    let grid = grid_of("variable.wav").unwrap();
    assert_eq!(
        grid,
        RekordboxGrid::Variable {
            markers: 2,
            slowest_bpm: 120.0,
            fastest_bpm: 126.0,
            reason: VariableReason::TempoChanges,
        }
    );
    assert_eq!(grid.usable(), None);
}

#[test]
fn a_grid_that_shifts_without_changing_tempo_is_variable_too() {
    let grid = grid_of("shifted.wav").unwrap();
    assert!(matches!(
        grid,
        RekordboxGrid::Variable {
            reason: VariableReason::GridShift,
            ..
        }
    ));
    assert_eq!(grid.usable(), None);
}

#[test]
fn a_meter_other_than_four_four_is_not_used() {
    assert_eq!(
        grid_of("waltz.wav"),
        Some(RekordboxGrid::UnsupportedMeter {
            meter: "3/4".to_string()
        })
    );
}

#[test]
fn a_track_with_no_markers_has_no_grid_and_a_broken_marker_is_unreadable() {
    assert_eq!(grid_of("none.wav"), Some(RekordboxGrid::Missing));
    assert_eq!(grid_of("broken.wav"), Some(RekordboxGrid::Unreadable));
}

#[test]
fn a_track_that_is_not_in_the_collection_is_not_found() {
    assert_eq!(grid_of("elsewhere.wav"), None);
}

#[test]
fn a_file_that_is_not_a_collection_is_refused() {
    assert!(matches!(
        read_collection_grids("not xml"),
        Err(CollectionError::NotXml(_))
    ));
    assert!(matches!(
        read_collection_grids("<DJ_PLAYLISTS/>"),
        Err(CollectionError::NotACollection)
    ));
}

#[test]
fn locations_decode_percent_escapes_drive_letters_and_refuse_broken_ones() {
    assert_eq!(
        rekordbox_location_path("file://localhost/Volumes/My%20Disk/k%C3%AFt.wav"),
        Some(PathBuf::from("/Volumes/My Disk/kït.wav"))
    );
    assert_eq!(
        rekordbox_location_path("file://localhost/D:/DJ/My%20Track.flac"),
        Some(PathBuf::from("D:/DJ/My Track.flac"))
    );
    assert_eq!(rekordbox_location_path("file://localhost/a%2"), None);
    assert_eq!(rekordbox_location_path("file://localhost/a%zz"), None);
    assert_eq!(rekordbox_location_path("file:///a.wav"), None);
}

#[test]
fn a_correction_beats_the_rekordbox_grid_which_beats_the_detected_one() {
    let usable = grid_of("constant.wav").unwrap();
    let variable = grid_of("variable.wav").unwrap();
    let tempo_set = Correction::of_bpm(100.0).unwrap();
    let key_set = Correction::of_key(Key::new(9, Mode::Minor));

    assert_eq!(choose_grid(&tempo_set, Some(&usable)), GridChoice::User);
    assert_eq!(choose_grid(&tempo_set, None), GridChoice::User);
    assert_eq!(
        choose_grid(&key_set, Some(&usable)).source(),
        GridSource::Rekordbox
    );
    assert_eq!(
        choose_grid(&Correction::default(), Some(&usable)),
        GridChoice::Rekordbox(constant("constant.wav"))
    );
    assert_eq!(
        choose_grid(&Correction::default(), Some(&variable)),
        GridChoice::Detected
    );
    assert_eq!(
        choose_grid(&Correction::default(), Some(&RekordboxGrid::Missing)),
        GridChoice::Detected
    );
    assert_eq!(
        choose_grid(&Correction::default(), None),
        GridChoice::Detected
    );
}

#[test]
fn the_sources_are_named_as_they_are_shown() {
    assert_eq!(GridSource::Rekordbox.label(), "rekordbox");
    assert_eq!(GridSource::Detected.label(), "detected");
    assert_eq!(GridSource::User.label(), "you");
}

const SAMPLE_RATE: u32 = 44_100;

fn four_on_the_floor(first_kick_seconds: f64, bars: usize) -> AudioBuffer {
    let beat_seconds = 0.5;
    let length = ((first_kick_seconds + bars as f64 * 2.0 + 1.0) * f64::from(SAMPLE_RATE)) as usize;
    let mut samples = vec![0.0_f32; length];
    for beat in 0..bars * 4 {
        let start =
            ((first_kick_seconds + beat as f64 * beat_seconds) * f64::from(SAMPLE_RATE)) as usize;
        for offset in 0..(beat_seconds * f64::from(SAMPLE_RATE)) as usize {
            let time = offset as f64 / f64::from(SAMPLE_RATE);
            let value = 0.6 * (std::f64::consts::TAU * 55.0 * time).sin() * (-time / 0.05).exp();
            if let Some(sample) = samples.get_mut(start + offset) {
                *sample = value as f32;
            }
        }
    }
    AudioBuffer::new(
        samples,
        NonZeroU32::new(SAMPLE_RATE).expect("a sample rate"),
    )
}

fn detected_analysis() -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 70.0,
        key: None,
        bpm: Some(119.0),
        energy: None,
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: None,
        key_score_margin: None,
        tempo_estimate: Some(119.0),
        tempo_score_margin: Some(0.001),
        beat_grid: None,
        cues: Vec::new(),
    }
}

#[test]
fn cues_land_on_the_bars_of_the_imported_grid() {
    let audio = four_on_the_floor(0.5, 32);
    let grid = constant("constant.wav");
    let placed = detected_analysis().on_imported_grid(grid, &audio);

    assert_eq!(placed.beat_grid, Some(grid));
    assert_eq!(placed.bpm, Some(120.0));
    assert_eq!(placed.tempo_estimate, None);
    assert!(!placed.cues.is_empty());
    for cue in &placed.cues {
        let bars = (cue.seconds - grid.first_downbeat_seconds) / grid.bar_seconds();
        assert!(
            (bars - bars.round()).abs() < 1e-9 && bars >= 0.0,
            "a cue at {} s is off the bars of the grid",
            cue.seconds
        );
    }
}

#[test]
fn cues_follow_the_imported_grid_when_it_differs_from_the_audio() {
    let audio = four_on_the_floor(0.5, 32);
    let grid = BeatGrid::imported(0.75, 120.0);
    let placed = detected_analysis().on_imported_grid(grid, &audio);
    assert!(!placed.cues.is_empty());
    for cue in &placed.cues {
        let bars = (cue.seconds - 0.75) / 2.0;
        assert!((bars - bars.round()).abs() < 1e-9, "{}", cue.seconds);
    }
}
