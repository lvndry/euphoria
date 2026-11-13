// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serato cue points and beat grids written through the tag plan: read
//! back, merged with the cues a file already has, and undone byte for byte.

use std::path::{Path, PathBuf};
use std::time::Duration;

use euphoria_core::detect::{BeatGrid, CueSection, SectionCue};
use euphoria_core::tags::{
    GridOutcome, LabelPlacement, SeratoCue, SeratoGrid, SeratoMarkers, SeratoReport, SeratoSkip,
    TagValues, TagWriteOptions, WritePlan, add_serato_markers, apply_plan, describe_serato_value,
    plan_write, undo,
};
use euphoria_core::track_cues::{Cue, HotCue};
use euphoria_core::{Key, TrackAnalysis};

const FORMATS: [&str; 4] = [
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.aiff",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.flac",
];

fn scratch(test: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("serato-{test}"));
    if folder.exists() {
        std::fs::remove_dir_all(&folder).expect("the old scratch folder is removed");
    }
    std::fs::create_dir_all(&folder).expect("the scratch folder is created");
    folder
}

fn copy_fixture(folder: &Path, fixture: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let destination = folder.join(fixture);
    std::fs::copy(source, &destination).expect("the fixture is copied");
    destination
}

fn serato_fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/serato-fixtures")
            .join(name),
    )
    .expect("the Serato fixture is read")
}

/// A plan that changes no other field, so only the Serato fields are
/// written.
fn empty_plan(path: &Path) -> WritePlan {
    let options = TagWriteOptions {
        write_bpm: false,
        label: LabelPlacement::None,
        ..TagWriteOptions::default()
    };
    let plan = plan_write(path, TagValues::default(), &options).expect("the file is planned");
    assert!(plan.is_empty());
    plan
}

fn cue(name: &str, milliseconds: u64) -> SeratoCue {
    SeratoCue::of(Duration::from_millis(milliseconds), name.to_string(), None)
}

fn euphoria_markers() -> SeratoMarkers {
    SeratoMarkers {
        cues: vec![
            cue("Start", 146),
            cue("Drop", 63_500),
            cue("Outro", 400_250),
        ],
        beat_grid: Some(SeratoGrid {
            first_beat: Duration::from_millis(146),
            bpm: 121.297,
        }),
    }
}

fn write(path: &Path, markers: &SeratoMarkers) -> (WritePlan, SeratoReport) {
    let mut plan = empty_plan(path);
    let report = add_serato_markers(&mut plan, markers).expect("the markers are planned");
    (plan, report)
}

/// The description of each Serato field the file has, after a fresh plan
/// for `markers` read them.
fn described(plan: &WritePlan) -> Vec<String> {
    plan.changes()
        .iter()
        .filter_map(|change| describe_serato_value(&change.field, change.new.as_deref()?))
        .collect()
}

#[test]
fn cues_and_grid_are_written_read_back_and_undone_byte_for_byte_in_every_format() {
    let folder = scratch("round-trip");
    for fixture in FORMATS {
        let path = copy_fixture(&folder, fixture);
        let original = std::fs::read(&path).expect("the copy is read");

        let (plan, report) = write(&path, &euphoria_markers());
        assert_eq!(report.skipped, None, "{fixture}");
        assert_eq!(report.grid, GridOutcome::Written, "{fixture}");
        assert_eq!(
            report
                .placed
                .iter()
                .map(|cue| (cue.name.as_str(), cue.pad))
                .collect::<Vec<_>>(),
            [("Start", 0), ("Drop", 1), ("Outro", 2)],
            "{fixture}"
        );
        assert!(!report.has_conflicts());
        let descriptions = described(&plan);
        assert!(
            descriptions.contains(
                &"cue points 1 Start at 0:00.146, 2 Drop at 1:03.500, 3 Outro at 6:40.250"
                    .to_string()
            ),
            "{fixture}: {descriptions:?}"
        );
        assert!(
            descriptions.contains(&"beat grid from 0.146 s at 121.30 BPM".to_string()),
            "{fixture}: {descriptions:?}"
        );
        let backup = apply_plan(&plan).expect("the plan is written");

        // Read back: the same markers are already there, so a second write
        // changes nothing.
        let (again, second) = write(&path, &euphoria_markers());
        assert!(again.is_empty(), "{fixture}: {:?}", again.changes());
        assert_eq!(second.already_there, 3, "{fixture}");
        assert_eq!(second.kept, 3, "{fixture}");
        assert_eq!(second.grid, GridOutcome::AlreadyThere, "{fixture}");

        undo(&backup).expect("the write is undone");
        assert_eq!(
            std::fs::read(&path).expect("the file is read"),
            original,
            "{fixture} is not identical after undo"
        );
    }
}

#[test]
fn your_cues_and_grid_are_kept_and_the_conflicts_reported() {
    let folder = scratch("merge");
    for fixture in FORMATS {
        let path = copy_fixture(&folder, fixture);
        let yours = SeratoMarkers {
            cues: vec![cue("Mine", 5_000)],
            beat_grid: Some(SeratoGrid {
                first_beat: Duration::from_millis(20),
                bpm: 120.0,
            }),
        };
        let (plan, _) = write(&path, &yours);
        apply_plan(&plan).expect("your cue is written");

        let (plan, report) = write(&path, &euphoria_markers());

        assert_eq!(report.kept, 1, "{fixture}");
        assert_eq!(
            report
                .placed
                .iter()
                .map(|cue| (cue.name.as_str(), cue.wanted_pad, cue.pad))
                .collect::<Vec<_>>(),
            [("Start", 0, 1), ("Drop", 1, 2), ("Outro", 2, 3)],
            "{fixture}"
        );
        assert!(
            matches!(report.grid, GridOutcome::KeptYours { bpm, .. } if (bpm - 120.0).abs() < 0.001),
            "{fixture}: {:?}",
            report.grid
        );
        assert!(report.has_conflicts());
        let descriptions = described(&plan);
        assert!(
            descriptions[0].starts_with("cue points 1 Mine at 0:05.000, 2 Start at 0:00.146"),
            "{fixture}: {descriptions:?}"
        );
        assert!(
            !plan
                .changes()
                .iter()
                .any(|change| change.field.to_lowercase().contains("beatgrid")),
            "{fixture}: the grid of yours is not replaced"
        );
    }
}

#[test]
fn a_cue_that_finds_no_free_pad_is_left_out() {
    let folder = scratch("full");
    let path = copy_fixture(&folder, "sine-440hz-mono.mp3");
    let full = SeratoMarkers {
        cues: (0..8).map(|pad| cue("Mine", 1_000 * (pad + 1))).collect(),
        beat_grid: None,
    };
    let (plan, _) = write(&path, &full);
    apply_plan(&plan).expect("your cues are written");

    let (plan, report) = write(&path, &euphoria_markers());

    assert!(report.placed.is_empty());
    assert_eq!(report.no_free_pad, ["Start", "Drop", "Outro"]);
    assert_eq!(report.kept, 8);
    assert!(report.has_conflicts());
    assert!(
        plan.changes()
            .iter()
            .all(|change| !change.field.contains("Markers2"))
    );
}

#[test]
fn wav_files_get_no_serato_data() {
    let folder = scratch("wav");
    let path = copy_fixture(&folder, "sine-440hz-mono.wav");
    let (plan, report) = write(&path, &euphoria_markers());
    assert_eq!(report.skipped, Some(SeratoSkip::Wav));
    assert!(plan.is_empty());
}

/// An MP3 file whose ID3v2.3 tag holds the GEOB frames Serato wrote for a
/// track with five hot cues, from Mixxx's test data.
fn mp3_with_serato_cues(folder: &Path) -> PathBuf {
    let geob = |description: &str, data: &[u8]| {
        let mut content = vec![0];
        content.extend(b"application/octet-stream\0\0");
        content.extend(description.as_bytes());
        content.push(0);
        content.extend(data);
        let mut frame = b"GEOB".to_vec();
        frame.extend((content.len() as u32).to_be_bytes());
        frame.extend([0, 0]);
        frame.extend(content);
        frame
    };
    let cues = "hotcue-positions-00m00s0-03m38s4-01m00s0-00m00s1-00m01s0.octet-stream";
    let mut frames = geob(
        "Serato Markers_",
        &serato_fixture(&format!("mp3/markers_/{cues}")),
    );
    frames.extend(geob(
        "Serato Markers2",
        &serato_fixture(&format!("mp3/markers2/{cues}")),
    ));
    frames.extend(geob(
        "Serato BeatGrid",
        &serato_fixture("mp3/beatgrid/analyzed.octet-stream"),
    ));
    frames.resize(frames.len() + 256, 0);
    let size = frames.len() as u32;
    let mut tag = b"ID3\x03\x00\x00".to_vec();
    tag.extend([
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ]);
    tag.extend(frames);

    let source = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine-440hz-mono.mp3"),
    )
    .expect("the fixture is read");
    let audio_start = if source.starts_with(b"ID3") {
        let size = source[6..10]
            .iter()
            .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
        10 + size
    } else {
        0
    };
    tag.extend(&source[audio_start..]);
    let path = folder.join("serato.mp3");
    std::fs::write(&path, tag).expect("the file is written");
    path
}

#[test]
fn the_cues_serato_wrote_are_kept_and_euphoria_takes_the_free_pads() {
    let folder = scratch("real-serato");
    let path = mp3_with_serato_cues(&folder);
    let original = std::fs::read(&path).expect("the file is read");

    let (plan, report) = write(&path, &euphoria_markers());

    assert_eq!(report.skipped, None);
    assert_eq!(report.kept, 5);
    assert_eq!(
        report
            .placed
            .iter()
            .map(|cue| (cue.wanted_pad, cue.pad))
            .collect::<Vec<_>>(),
        [(0, 5), (1, 6), (2, 7)]
    );
    assert!(matches!(
        report.grid,
        GridOutcome::KeptYours { bpm, .. } if (bpm - 115.0).abs() < 0.001
    ));
    let fields: Vec<&str> = plan
        .changes()
        .iter()
        .map(|change| change.field.as_str())
        .collect();
    // Pads 6 to 8 are not in Serato's older copy of the first five cues,
    // so only the new copy changes.
    assert_eq!(fields, ["GEOB:Serato Markers2"]);
    let description = &described(&plan)[0];
    assert!(
        description.starts_with("cue points 1 at 0:00.000, 2 at 3:38.4"),
        "{description}"
    );
    assert!(
        description.ends_with("6 Start at 0:00.146, 7 Drop at 1:03.500, 8 Outro at 6:40.250"),
        "{description}"
    );

    let backup = apply_plan(&plan).expect("the plan is written");
    undo(&backup).expect("the write is undone");
    assert_eq!(std::fs::read(&path).expect("the file is read"), original);
}

#[test]
fn an_m4a_file_gets_the_older_copy_serato_reads_its_first_cues_from() {
    let folder = scratch("older-copy");
    let path = copy_fixture(&folder, "sine-440hz-mono.m4a");
    let (plan, report) = write(&path, &euphoria_markers());
    assert_eq!(report.placed.len(), 3);
    let older = plan
        .changes()
        .iter()
        .find(|change| change.field == "----:com.serato.dj:markers")
        .and_then(|change| describe_serato_value(&change.field, change.new.as_deref()?));
    assert_eq!(older.as_deref(), Some("3 of the first 5 cue points"));
}

fn analysis_with_grid() -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 300.0,
        key: Some("A minor".parse::<Key>().expect("a key name")),
        bpm: Some(120.0),
        energy: Some(6),
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: None,
        key_score_margin: None,
        tempo_estimate: Some(120.0),
        tempo_score_margin: Some(0.5),
        beat_grid: Some(BeatGrid {
            first_downbeat_seconds: 0.146,
            bpm: 120.0,
            salience: 10.0,
            drift_seconds: None,
        }),
        cues: vec![
            SectionCue {
                seconds: 0.146,
                section: CueSection::Start,
            },
            SectionCue {
                seconds: 63.5,
                section: CueSection::Drop,
            },
        ],
    }
}

#[test]
fn the_cues_a_user_set_win_over_the_analysis_and_keep_their_pads() {
    let folder = scratch("user-cues");
    for fixture in FORMATS {
        let path = copy_fixture(&folder, fixture);
        let original = std::fs::read(&path).expect("the copy is read");
        let analysis = analysis_with_grid();
        let edited = vec![
            Cue {
                seconds: 30.25,
                name: "My break".to_string(),
                hot_cue: Some(HotCue::from_pad(2).expect("pad C")),
            },
            Cue {
                seconds: 5.0,
                name: "memory only".to_string(),
                hot_cue: None,
            },
        ];
        let markers = SeratoMarkers::of_cues(&analysis, Some(edited));
        assert_eq!(
            markers.cues,
            vec![SeratoCue::of(
                Duration::from_millis(30_250),
                "My break".to_string(),
                Some(2)
            )],
            "{fixture}: a memory cue is no cue point, the rest are the analysis'"
        );

        let (plan, report) = write(&path, &markers);
        assert_eq!(report.skipped, None, "{fixture}");
        assert_eq!(
            report
                .placed
                .iter()
                .map(|cue| (cue.name.as_str(), cue.pad))
                .collect::<Vec<_>>(),
            [("My break", 2)],
            "{fixture}: the user's pad is kept, not the next free one"
        );
        let backup = apply_plan(&plan).expect("the plan is written");

        // Read back: the same markers are already there, so a second write
        // changes nothing.
        let (again, second) = write(&path, &markers);
        assert!(again.is_empty(), "{fixture}: {:?}", again.changes());
        assert_eq!(second.already_there, 1, "{fixture}");
        assert_eq!(second.grid, GridOutcome::AlreadyThere, "{fixture}");

        undo(&backup).expect("the write is undone");
        assert_eq!(
            std::fs::read(&path).expect("the file is read"),
            original,
            "{fixture} is not identical after undo"
        );
    }
}

#[test]
fn the_analysis_cues_are_written_when_the_user_has_not_set_any() {
    let analysis = analysis_with_grid();
    let markers = SeratoMarkers::of_cues(&analysis, None);
    assert_eq!(
        markers.cues,
        vec![
            SeratoCue::of(Duration::from_millis(146), "Start".to_string(), None),
            SeratoCue::of(Duration::from_millis(63_500), "Drop".to_string(), None)
        ]
    );

    // A grid rated low confidence carries nothing.
    let mut low_analysis = analysis;
    low_analysis.beat_grid = Some(BeatGrid {
        salience: 5.0,
        ..low_analysis.beat_grid.expect("a grid")
    });
    assert!(SeratoMarkers::of_cues(&low_analysis, None).cues.is_empty());
}

#[test]
fn ogg_files_get_no_serato_data() {
    let folder = scratch("ogg");
    let path = copy_fixture(&folder, "sine-440hz-mono.ogg");
    let (plan, report) = write(&path, &euphoria_markers());
    assert_eq!(report.skipped, Some(SeratoSkip::Ogg));
    assert!(plan.is_empty());
}
