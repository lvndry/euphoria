// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing the analysis does not damage tags that are right: a key or
//! tempo already in the file is kept unless Euphoria wrote it itself, and a
//! value rated low confidence is left out.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use euphoria_core::tags::{
    FieldEdit, LabelPlacement, LoggedWrite, SkipReason, SkippedKind, TagField, TagValues,
    TagWriteOptions, TextChange, WriteLog, WrittenValues, apply_plan, plan_edit, plan_write,
    plan_write_with_history, read_tags,
};
use euphoria_core::{Confidence, Key, Mode};

fn scratch(test: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("keep-existing-{test}"));
    if folder.exists() {
        std::fs::remove_dir_all(&folder).unwrap();
    }
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn track(folder: &Path) -> PathBuf {
    let path = folder.join("track.mp3");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sine-440hz-mono.mp3"),
        &path,
    )
    .unwrap();
    path
}

fn a_minor(bpm: f32) -> TagValues {
    TagValues {
        key: Some(Key::new(9, Mode::Minor)),
        bpm: Some(bpm),
        energy: Some(7),
        key_confidence: Some(Confidence::High),
        bpm_confidence: Some(Confidence::High),
    }
}

fn e_minor(bpm: f32) -> TagValues {
    TagValues {
        key: Some(Key::new(4, Mode::Minor)),
        ..a_minor(bpm)
    }
}

fn replacing() -> TagWriteOptions {
    TagWriteOptions::default()
}

fn safe() -> TagWriteOptions {
    TagWriteOptions {
        keep_existing: true,
        skip_low_confidence: true,
        ..TagWriteOptions::default()
    }
}

fn seed(path: &Path, values: TagValues) {
    apply_plan(&plan_write(path, values, &replacing()).unwrap()).unwrap();
}

#[test]
fn a_key_and_tempo_the_file_has_are_kept_and_reported() {
    let folder = scratch("kept");
    let path = track(&folder);
    seed(&path, a_minor(75.0));

    let plan = plan_write(&path, e_minor(150.0), &safe()).unwrap();
    assert!(plan.is_empty(), "{:?}", plan.changes());
    let lines: Vec<String> = plan
        .skipped()
        .iter()
        .map(|skipped| skipped.describe())
        .collect();
    assert_eq!(
        lines,
        [
            "key kept (already tagged 8A)",
            "tempo kept (already tagged 75)"
        ]
    );
    assert_eq!(plan.skipped()[0].kind, SkippedKind::Key);
    assert_eq!(
        plan.skipped()[0].reason,
        SkipReason::AlreadyTagged("8A".to_string())
    );
}

#[test]
fn overwriting_replaces_what_the_file_has() {
    let folder = scratch("overwrite");
    let path = track(&folder);
    seed(&path, a_minor(75.0));

    let plan = plan_write(&path, e_minor(150.0), &replacing()).unwrap();
    apply_plan(&plan).unwrap();
    let tags = read_tags(&path).unwrap();
    assert_eq!(tags.key.as_deref(), Some("9A"));
    assert_eq!(tags.bpm.as_deref(), Some("150"));
}

#[test]
fn a_value_that_agrees_with_the_file_is_not_reported_as_kept() {
    let folder = scratch("agree");
    let path = track(&folder);
    seed(&path, a_minor(126.0));
    let plan = plan_write(&path, a_minor(126.0), &safe()).unwrap();
    assert!(plan.is_empty());
    assert!(plan.skipped().is_empty());
}

fn edit(path: &Path, field: TagField, text: &str) -> euphoria_core::tags::WritePlan {
    plan_edit(
        path,
        &[FieldEdit {
            field,
            change: TextChange::Set(text.to_string()),
        }],
    )
    .unwrap()
}

#[test]
fn an_unreadable_key_is_replaced() {
    let folder = scratch("unreadable");
    let path = track(&folder);
    apply_plan(&edit(&path, TagField::Key, "o")).unwrap();
    let plan = plan_write(&path, a_minor(126.0), &safe()).unwrap();
    apply_plan(&plan).unwrap();
    assert_eq!(read_tags(&path).unwrap().key.as_deref(), Some("8A"));
}

#[test]
fn a_decimal_comma_in_the_file_is_read_and_compared_by_value() {
    let folder = scratch("comma");
    let path = track(&folder);
    apply_plan(&edit(&path, TagField::Bpm, "127,5")).unwrap();
    assert_eq!(read_tags(&path).unwrap().bpm.as_deref(), Some("127.5"));

    let same = TagValues {
        key: None,
        energy: None,
        key_confidence: None,
        ..a_minor(127.5)
    };
    let mixxx = TagWriteOptions {
        preset: euphoria_core::tags::DjSoftware::Mixxx,
        ..replacing()
    };
    assert!(plan_write(&path, same, &mixxx).unwrap().is_empty());

    let plan = plan_write(&path, same, &safe()).unwrap();
    assert!(plan.is_empty());
}

#[test]
fn a_value_euphoria_wrote_is_updated_but_one_typed_or_foreign_is_kept() {
    let folder = scratch("history");
    let logs = folder.join("logs");
    let path = track(&folder);

    let mut log = WriteLog::new(&logs);
    let plan = plan_write(&path, a_minor(126.0), &replacing()).unwrap();
    assert!(matches!(log.apply(&plan).unwrap(), LoggedWrite::Recorded));

    let history = WrittenValues::load(&logs, std::slice::from_ref(&path));
    let plan = plan_write_with_history(&path, e_minor(128.0), &safe(), &history).unwrap();
    let fields: Vec<&str> = plan
        .changes()
        .iter()
        .map(|change| change.field.as_str())
        .collect();
    assert!(
        fields.contains(&"TKEY") && fields.contains(&"TBPM"),
        "{fields:?}"
    );
    assert!(plan.skipped().is_empty());

    let without_history = plan_write(&path, e_minor(128.0), &safe()).unwrap();
    assert!(
        without_history.is_empty(),
        "no log, so the value is not Euphoria's"
    );

    let other_logs = folder.join("other-logs");
    let mut typed_log = WriteLog::new(&other_logs);
    let typed = edit(&path, TagField::Key, "5B");
    assert!(matches!(
        typed_log.apply(&typed).unwrap(),
        LoggedWrite::Recorded
    ));
    let typed_history = WrittenValues::load(&other_logs, std::slice::from_ref(&path));
    let plan = plan_write_with_history(&path, e_minor(128.0), &safe(), &typed_history).unwrap();
    assert_eq!(
        plan.skipped()[0].kind,
        SkippedKind::Key,
        "a hand edit is the user's"
    );

    let stale = WrittenValues::load(&logs, std::slice::from_ref(&path));
    apply_plan(&edit(&path, TagField::Key, "7B")).unwrap();
    let plan = plan_write_with_history(&path, e_minor(128.0), &safe(), &stale).unwrap();
    assert_eq!(
        plan.skipped()
            .iter()
            .filter(|skipped| skipped.kind == SkippedKind::Key)
            .count(),
        1,
        "the key changed since Euphoria wrote it"
    );
}

#[test]
fn a_low_confidence_value_is_left_out_and_a_correction_is_written() {
    let folder = scratch("confidence");
    let path = track(&folder);
    let low = TagValues {
        key_confidence: Some(Confidence::Low),
        bpm_confidence: Some(Confidence::Low),
        ..a_minor(75.0)
    };
    let plan = plan_write(&path, low, &safe()).unwrap();
    assert!(
        plan.changes()
            .iter()
            .all(|change| change.field != "TKEY" && change.field != "TBPM")
    );
    let lines: Vec<String> = plan
        .skipped()
        .iter()
        .map(|skipped| skipped.describe())
        .collect();
    assert_eq!(
        lines,
        [
            "key not written (rated low confidence)",
            "tempo not written (rated low confidence)"
        ]
    );
    let default_label = plan_write(
        &path,
        low,
        &TagWriteOptions {
            label: LabelPlacement::CommentStart,
            ..safe()
        },
    )
    .unwrap();
    let label_text = default_label
        .changes()
        .iter()
        .find_map(|change| change.new.clone().filter(|text| text.contains("Energy")))
        .unwrap();
    assert_eq!(
        label_text, "Energy 7",
        "a key that is left out is not in the label"
    );

    let corrected = TagValues {
        key_confidence: None,
        bpm_confidence: None,
        ..low
    };
    let plan = plan_write(&path, corrected, &safe()).unwrap();
    assert!(plan.skipped().is_empty());
    apply_plan(&plan).unwrap();
    assert_eq!(read_tags(&path).unwrap().key.as_deref(), Some("8A"));

    let other = scratch("confidence-included").join("track.mp3");
    std::fs::copy(&path, &other).unwrap();
    let permissive = TagWriteOptions {
        skip_low_confidence: false,
        ..safe()
    };
    let plan = plan_write(&other, low, &permissive).unwrap();
    assert!(plan.skipped().is_empty());

    let medium = TagValues {
        key_confidence: Some(Confidence::Medium),
        bpm_confidence: Some(Confidence::Medium),
        ..low
    };
    let fresh = track(&scratch("confidence-medium"));
    assert!(
        plan_write(&fresh, medium, &safe())
            .unwrap()
            .skipped()
            .is_empty()
    );
}
