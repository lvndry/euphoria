// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing tag fields by hand on copies of the fixtures, in every format:
//! each field reads back as written, a comment keeps its key and energy
//! label once, the safety rules of analysis writes hold, and an undo
//! through the write log gives back the original bytes.

use std::path::{Path, PathBuf};

use euphoria_core::tags::{
    FieldEdit, LabelPlacement, LoggedWrite, TagField, TagValues, TagWriteOptions, TextChange,
    WriteLog, apply_plan, plan_edit, plan_write, read_editable_tags, read_tags, undo,
    undo_last_write,
};
use euphoria_core::{EuphoriaError, Key, Mode};

const FIXTURES: [&str; 8] = [
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.wav",
    "sine-440hz-mono.aiff",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.ogg",
    "sine-440hz-mono-alac.m4a",
    "sine-440hz-mono.mp4",
];

const TAGGED: [&str; 8] = [
    "tagged.mp3",
    "tagged.wav",
    "tagged.aiff",
    "tagged.m4a",
    "tagged.flac",
    "tagged.ogg",
    "tagged-alac.m4a",
    "tagged.mp4",
];

fn scratch_copy(folder: &str, fixture: &str, test: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(folder)
        .join(fixture);
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tags-edit-{test}"));
    std::fs::create_dir_all(&directory).expect("the scratch folder is made");
    let path = directory.join(fixture);
    std::fs::copy(source, &path).expect("the fixture is copied");
    path
}

fn set(field: TagField, text: &str) -> FieldEdit {
    FieldEdit {
        field,
        change: TextChange::Set(text.to_string()),
    }
}

fn every_field() -> Vec<FieldEdit> {
    vec![
        set(TagField::Title, "Night Drive"),
        set(TagField::Artist, "Ana Sol"),
        set(TagField::Album, "Late Hours"),
        set(TagField::Genre, "Deep House"),
        set(TagField::Year, "2024"),
        set(TagField::TrackNumber, "7/12"),
        set(TagField::Comment, "warm up, été"),
        set(TagField::Grouping, "Sunday"),
        set(TagField::Key, "8A"),
        set(TagField::Bpm, "122"),
    ]
}

#[test]
fn every_field_is_written_read_back_and_undone_to_the_original_bytes() {
    for folder_and_fixtures in [("fixtures", FIXTURES), ("tagged-fixtures", TAGGED)] {
        let (folder, fixtures) = folder_and_fixtures;
        for fixture in fixtures {
            let path = scratch_copy(folder, fixture, "every-field");
            let original = std::fs::read(&path).expect("the copy is read");

            let plan = plan_edit(&path, &every_field()).expect("the edit is planned");
            assert!(!plan.is_empty(), "{fixture}");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                original,
                "planning writes nothing"
            );
            let backup = apply_plan(&plan).expect("the edit is written");

            let tags = read_editable_tags(&path).expect("the tags are read");
            for edit in every_field() {
                let TextChange::Set(expected) = &edit.change else {
                    unreachable!()
                };
                assert_eq!(
                    tags.get(edit.field),
                    Some(expected.as_str()),
                    "{fixture} {}",
                    edit.field
                );
            }
            let read = read_tags(&path).expect("the tags are read");
            assert_eq!(read.title.as_deref(), Some("Night Drive"), "{fixture}");
            assert_eq!(read.key.as_deref(), Some("8A"), "{fixture}");
            assert!(
                plan_edit(&path, &every_field()).unwrap().is_empty(),
                "{fixture}: the same edit again changes nothing"
            );

            undo(&backup).expect("the edit is undone");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                original,
                "{fixture} is byte identical after the undo"
            );
        }
    }
}

#[test]
fn find_and_replace_and_prefixes_change_only_what_they_match() {
    for fixture in FIXTURES {
        let path = scratch_copy("fixtures", fixture, "replace");
        apply_plan(
            &plan_edit(
                &path,
                &[
                    set(TagField::Genre, "deep house"),
                    set(TagField::Title, "Track"),
                ],
            )
            .unwrap(),
        )
        .unwrap();
        let edits = [
            FieldEdit {
                field: TagField::Genre,
                change: TextChange::Replace {
                    find: "HOUSE".to_string(),
                    replace: "House".to_string(),
                    match_case: false,
                },
            },
            FieldEdit {
                field: TagField::Genre,
                change: TextChange::Replace {
                    find: "deep".to_string(),
                    replace: "Deep".to_string(),
                    match_case: true,
                },
            },
            FieldEdit {
                field: TagField::Title,
                change: TextChange::AddSuffix(" (Edit)".to_string()),
            },
            FieldEdit {
                field: TagField::Album,
                change: TextChange::RemovePrefix("The ".to_string()),
            },
        ];
        let plan = plan_edit(&path, &edits).unwrap();
        apply_plan(&plan).unwrap();
        let tags = read_editable_tags(&path).unwrap();
        assert_eq!(tags.get(TagField::Genre), Some("Deep House"), "{fixture}");
        assert_eq!(tags.get(TagField::Title), Some("Track (Edit)"), "{fixture}");
        assert_eq!(tags.get(TagField::Album), None, "{fixture}");
    }
}

#[test]
fn a_comment_edit_keeps_the_euphoria_label_once() {
    for fixture in FIXTURES {
        let path = scratch_copy("fixtures", fixture, "label");
        let values = TagValues {
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(124.0),
            energy: Some(7),
        };
        let options = TagWriteOptions {
            label: LabelPlacement::CommentStart,
            ..TagWriteOptions::default()
        };
        apply_plan(&plan_write(&path, values, &options).unwrap()).unwrap();
        assert_eq!(
            read_tags(&path).unwrap().comment.as_deref(),
            Some("8A - Energy 7"),
            "{fixture}"
        );
        assert_eq!(
            read_editable_tags(&path).unwrap().get(TagField::Comment),
            None
        );

        apply_plan(&plan_edit(&path, &[set(TagField::Comment, "Peak opener")]).unwrap()).unwrap();
        assert_eq!(
            read_tags(&path).unwrap().comment.as_deref(),
            Some("8A - Energy 7 - Peak opener"),
            "{fixture}"
        );
        assert_eq!(
            read_editable_tags(&path).unwrap().get(TagField::Comment),
            Some("Peak opener")
        );
        assert!(
            plan_write(&path, values, &options).unwrap().is_empty(),
            "{fixture}: writing the analysis again finds the label in place"
        );

        apply_plan(
            &plan_edit(
                &path,
                &[FieldEdit {
                    field: TagField::Comment,
                    change: TextChange::Clear,
                }],
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            read_tags(&path).unwrap().comment.as_deref(),
            Some("8A - Energy 7"),
            "{fixture}: clearing the comment keeps the label"
        );
    }
}

#[test]
fn values_a_field_cannot_hold_are_refused_before_anything_is_written() {
    let path = scratch_copy("fixtures", "sine-440hz-mono.m4a", "refused");
    let original = std::fs::read(&path).unwrap();
    for edit in [
        set(TagField::Bpm, "fast"),
        set(TagField::TrackNumber, "A1"),
        set(TagField::Bpm, "0"),
    ] {
        assert!(
            matches!(
                plan_edit(&path, std::slice::from_ref(&edit)),
                Err(EuphoriaError::TagWrite { .. })
            ),
            "{edit:?}"
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn an_m4a_file_keeps_its_integer_tempo_and_track_number_in_agreement() {
    let path = scratch_copy("tagged-fixtures", "tagged.m4a", "m4a");
    apply_plan(
        &plan_edit(
            &path,
            &[set(TagField::Bpm, "126.4"), set(TagField::TrackNumber, "3")],
        )
        .unwrap(),
    )
    .unwrap();
    let tags = read_editable_tags(&path).unwrap();
    assert_eq!(tags.get(TagField::TrackNumber), Some("3"));
    let bpm: f32 = tags.get(TagField::Bpm).unwrap().parse().unwrap();
    assert!(
        (bpm - 126.4).abs() < 0.01 || (bpm - 126.0).abs() < 0.01,
        "{bpm}"
    );
}

#[cfg(unix)]
#[test]
fn a_file_with_several_links_is_refused() {
    let path = scratch_copy("fixtures", "sine-440hz-mono.mp3", "links");
    let link = path.with_file_name("linked.mp3");
    let _ = std::fs::remove_file(&link);
    std::fs::hard_link(&path, &link).unwrap();
    let refused = plan_edit(&path, &[set(TagField::Genre, "House")]);
    std::fs::remove_file(&link).unwrap();
    assert!(
        matches!(refused, Err(EuphoriaError::TagWrite { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_change_made_after_planning_stops_the_write() {
    let path = scratch_copy("fixtures", "sine-440hz-mono.flac", "conflict");
    let plan = plan_edit(&path, &[set(TagField::Genre, "House")]).unwrap();
    apply_plan(&plan_edit(&path, &[set(TagField::Genre, "Techno")]).unwrap()).unwrap();
    assert!(matches!(
        apply_plan(&plan),
        Err(EuphoriaError::TagConflict { .. })
    ));
    assert_eq!(
        read_editable_tags(&path).unwrap().get(TagField::Genre),
        Some("Techno")
    );
}

#[test]
fn an_edit_written_through_the_log_is_undone_by_the_last_write_undo() {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tags-edit-log");
    let _ = std::fs::remove_dir_all(&directory);
    let files: Vec<PathBuf> = FIXTURES
        .iter()
        .map(|fixture| scratch_copy("fixtures", fixture, "log"))
        .collect();
    let originals: Vec<Vec<u8>> = files
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let mut log = WriteLog::new(&directory.join("tag-writes"));
    for path in &files {
        let plan = plan_edit(
            path,
            &[set(TagField::Genre, "House"), set(TagField::Year, "1999")],
        )
        .unwrap();
        assert!(matches!(log.apply(&plan).unwrap(), LoggedWrite::Recorded));
    }
    let undone = undo_last_write(&directory.join("tag-writes"))
        .unwrap()
        .expect("there is a write to undo");
    assert_eq!(undone.restored.len(), files.len());
    for (path, original) in files.iter().zip(&originals) {
        assert_eq!(
            &std::fs::read(path).unwrap(),
            original,
            "{}",
            path.display()
        );
    }
}

#[test]
fn a_wav_title_and_artist_reach_the_info_list_other_programs_read() {
    let path = scratch_copy("fixtures", "sine-440hz-mono.wav", "wav-info");
    let plan = plan_edit(
        &path,
        &[
            set(TagField::Title, "Night Drive"),
            set(TagField::Artist, "Ana Sol"),
        ],
    )
    .unwrap();
    let fields: Vec<&str> = plan
        .changes()
        .iter()
        .map(|change| change.field.as_str())
        .collect();
    assert!(
        fields.contains(&"INFO INAM") && fields.contains(&"INFO IART"),
        "{fields:?}"
    );
    apply_plan(&plan).unwrap();
    let name = euphoria_core::read_track_name(&path).unwrap();
    assert_eq!(name.title.as_deref(), Some("Night Drive"));
    assert_eq!(name.artist.as_deref(), Some("Ana Sol"));
}
