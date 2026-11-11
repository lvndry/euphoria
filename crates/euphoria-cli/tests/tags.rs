// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs `euphoria tags write` and `euphoria tags undo` on copies of the
//! core crate's audio fixtures and reads the files back.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use euphoria_core::tags::{TrackTags, read_tags};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

const FIXTURES: [&str; 5] = [
    "sine-440hz-mono.aiff",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.wav",
];
const USER_COMMENT: &str = "https://example.com/track/42?a=b - bought 2019";

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn standard_output(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A folder with a copy of every fixture.
struct Library {
    folder: TemporaryFolder,
}

impl Library {
    fn new() -> Self {
        let folder = TemporaryFolder::new();
        for fixture in FIXTURES {
            std::fs::copy(
                fixtures_directory().join(fixture),
                folder.path().join(fixture),
            )
            .expect("the fixture is copied");
        }
        Self { folder }
    }

    fn path(&self) -> &Path {
        self.folder.path()
    }

    fn tracks(&self) -> Vec<PathBuf> {
        FIXTURES
            .iter()
            .map(|fixture| self.path().join(fixture))
            .collect()
    }

    fn contents(&self) -> Vec<Vec<u8>> {
        self.tracks()
            .iter()
            .map(|track| std::fs::read(track).expect("the track is read"))
            .collect()
    }

    fn tags(&self) -> Vec<TrackTags> {
        self.tracks()
            .iter()
            .map(|track| read_tags(track).expect("the tags are read"))
            .collect()
    }
}

/// Gives the WAV file at `path` an ID3v2 tag whose only frame is a comment
/// holding `comment`, the way another program would have left it.
fn add_comment_to_wav(path: &Path, comment: &str) {
    let mut frame_content = b"\x00eng\x00".to_vec();
    frame_content.extend(comment.as_bytes());
    let mut tag = b"ID3\x03\x00\x00".to_vec();
    let body_length = 10 + frame_content.len() as u32;
    tag.extend([
        (body_length >> 21) as u8 & 0x7F,
        (body_length >> 14) as u8 & 0x7F,
        (body_length >> 7) as u8 & 0x7F,
        body_length as u8 & 0x7F,
    ]);
    tag.extend(b"COMM");
    tag.extend((frame_content.len() as u32).to_be_bytes());
    tag.extend([0, 0]);
    tag.extend(frame_content);
    if tag.len() % 2 == 1 {
        // One byte of padding inside the tag keeps the chunk even.
        tag.push(0);
        let padded_length = body_length + 1;
        tag[9] = padded_length as u8 & 0x7F;
        tag[8] = (padded_length >> 7) as u8 & 0x7F;
    }

    let mut bytes = std::fs::read(path).expect("the track is read");
    bytes.extend(b"ID3 ");
    bytes.extend((tag.len() as u32).to_le_bytes());
    bytes.extend(&tag);
    let container_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&container_size.to_le_bytes());
    std::fs::write(path, bytes).expect("the track is written");
}

/// Runs the binary with nothing on standard input, as a script or a pipe
/// would.
fn run(euphoria: &Euphoria, arguments: &[&str]) -> Output {
    euphoria
        .command(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("the euphoria binary runs")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "the command ended with {}\nstandard error:\n{}\nstandard output:\n{}",
        output.status,
        standard_error(output),
        standard_output(output)
    );
}

fn log_count(euphoria: &Euphoria) -> usize {
    std::fs::read_dir(euphoria.data_directory().join("tag-writes"))
        .map(|entries| entries.count())
        .unwrap_or(0)
}

/// What every written file must hold: a key, a tempo, and a comment that
/// starts with the key and an energy rating.
fn assert_written(tags: &TrackTags) {
    let key = tags.key.as_deref().expect("a key");
    assert!(tags.bpm.is_some(), "{tags:?}");
    let comment = tags.comment.as_deref().expect("a comment");
    let label_start = format!("{key} - Energy ");
    assert!(comment.starts_with(&label_start), "{comment:?}");
}

#[test]
fn dry_run_shows_the_changes_and_writes_nothing() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let original = library.contents();

    let output = run(
        &euphoria,
        &["tags", "write", "--dry-run", text(library.path())],
    );
    assert_success(&output);
    assert!(library.contents() == original);
    assert_eq!(log_count(&euphoria), 0);

    let preview = standard_output(&output);
    for track in library.tracks() {
        assert!(preview.contains(text(&track)), "{preview}");
    }
    for expected in [
        "  TKEY: (none) -> ",
        "  INITIALKEY: (none) -> ",
        "  tmpo: (none) -> ",
        "  COMM: (none) -> ",
        " - Energy ",
    ] {
        assert!(
            preview.contains(expected),
            "{expected:?} is not in:\n{preview}"
        );
    }
    let report = standard_error(&output);
    assert!(report.contains("5 files to change."), "{report}");
    assert!(report.contains("Nothing was written."), "{report}");
}

#[test]
fn without_a_terminal_and_without_yes_nothing_is_written() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let original = library.contents();

    let output = run(&euphoria, &["tags", "write", text(library.path())]);
    assert!(!output.status.success());
    assert!(library.contents() == original);
    assert_eq!(log_count(&euphoria), 0);
    let report = standard_error(&output);
    assert!(
        report.contains("Nothing was written: standard input is not a terminal"),
        "{report}"
    );
    assert!(report.contains("--yes"), "{report}");
    // The preview is still shown.
    assert!(standard_output(&output).contains(" - Energy "));
}

#[test]
fn an_answer_on_a_pipe_does_not_confirm() {
    use std::io::Write;

    let euphoria = Euphoria::new();
    let library = Library::new();
    let original = library.contents();

    let mut child = euphoria
        .command(&["tags", "write", text(library.path())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the euphoria binary runs");
    child
        .stdin
        .take()
        .expect("standard input is piped")
        .write_all(b"y\nyes\n")
        .expect("the answer is sent");
    let output = child.wait_with_output().expect("the command ends");
    assert!(!output.status.success());
    assert!(library.contents() == original);
}

#[test]
fn write_then_undo_gives_back_the_exact_files() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    add_comment_to_wav(&library.path().join("sine-440hz-mono.wav"), USER_COMMENT);
    let original = library.contents();
    let original_tags = library.tags();

    let output = run(&euphoria, &["tags", "write", "--yes", text(library.path())]);
    assert_success(&output);
    let report = standard_error(&output);
    assert!(report.contains("Wrote the tags of 5 files."), "{report}");
    assert!(report.contains("euphoria tags undo"), "{report}");
    assert_eq!(log_count(&euphoria), 1);
    for tags in library.tags() {
        assert_written(&tags);
    }
    let wav_comment = library.tags()[4].comment.clone().unwrap();
    assert!(
        wav_comment.ends_with(&format!(" - {USER_COMMENT}")),
        "{wav_comment:?}"
    );

    let output = run(&euphoria, &["tags", "undo"]);
    assert_success(&output);
    assert!(library.contents() == original);
    assert_eq!(library.tags(), original_tags);
    assert_eq!(log_count(&euphoria), 0);
    let restored = standard_output(&output);
    for track in library.tracks() {
        assert!(restored.contains(text(&track)), "{restored}");
    }
    let report = standard_error(&output);
    assert!(report.contains("Restored the tags of 5 files."), "{report}");

    let output = run(&euphoria, &["tags", "undo"]);
    assert_success(&output);
    assert!(standard_error(&output).contains("There is no tag write to undo."));
    assert!(library.contents() == original);
}

#[test]
fn writing_again_changes_nothing_and_does_not_stack_the_label() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    add_comment_to_wav(&library.path().join("sine-440hz-mono.wav"), USER_COMMENT);

    assert_success(&run(
        &euphoria,
        &["tags", "write", "--yes", text(library.path())],
    ));
    let after_first = library.contents();
    let output = run(&euphoria, &["tags", "write", "--yes", text(library.path())]);
    assert_success(&output);
    assert!(library.contents() == after_first);
    assert_eq!(standard_output(&output), "");
    let report = standard_error(&output);
    assert!(
        report.contains("0 files to change, 5 already up to date."),
        "{report}"
    );
    assert_eq!(log_count(&euphoria), 1);

    // Asking for another notation rewrites the label in place.
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--key-notation",
            "musical",
            text(library.path()),
        ],
    );
    assert_success(&output);
    for tags in library.tags() {
        assert_written(&tags);
        let comment = tags.comment.unwrap();
        assert_eq!(comment.matches("Energy").count(), 1, "{comment:?}");
    }
    let wav_comment = library.tags()[4].comment.clone().unwrap();
    assert!(
        wav_comment.ends_with(&format!(" - {USER_COMMENT}")),
        "{wav_comment:?}"
    );
    assert_eq!(wav_comment.matches(USER_COMMENT).count(), 1);
}

#[test]
fn options_choose_what_is_written_and_where() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--no-tempo",
            "--label",
            "grouping",
            "--preset",
            "serato",
            text(library.path()),
        ],
    );
    assert_success(&output);
    for tags in library.tags() {
        let key = tags.key.as_deref().expect("a key");
        assert_eq!(tags.bpm, None, "{tags:?}");
        assert_eq!(tags.comment, None, "{tags:?}");
        let grouping = tags.grouping.as_deref().expect("a grouping");
        assert!(
            grouping.starts_with(&format!("{key} - Energy ")),
            "{grouping:?}"
        );
    }
    // No preset creates KEY next to INITIALKEY in FLAC.
    let preview = standard_output(&output);
    assert!(preview.contains("  INITIALKEY: "), "{preview}");
    assert!(!preview.contains("  KEY: "), "{preview}");

    let library = Library::new();
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--label",
            "none",
            text(library.path()),
        ],
    );
    assert_success(&output);
    for tags in library.tags() {
        assert!(tags.key.is_some() && tags.bpm.is_some(), "{tags:?}");
        assert_eq!((tags.comment, tags.grouping), (None, None));
    }

    let library = Library::new();
    add_comment_to_wav(&library.path().join("sine-440hz-mono.wav"), USER_COMMENT);
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--label",
            "comment-end",
            text(library.path()),
        ],
    );
    assert_success(&output);
    let wav_comment = library.tags()[4].comment.clone().unwrap();
    assert!(
        wav_comment.starts_with(&format!("{USER_COMMENT} - ")),
        "{wav_comment:?}"
    );
    assert!(wav_comment.contains(" - Energy "), "{wav_comment:?}");
}

#[test]
fn the_serato_preset_writes_the_tempo_only_when_asked() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--preset",
            "serato",
            text(library.path()),
        ],
    );
    assert_success(&output);
    for tags in library.tags() {
        assert!(tags.key.is_some(), "{tags:?}");
        assert_eq!(tags.bpm, None, "{tags:?}");
    }

    let library = Library::new();
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--preset",
            "serato",
            "--tempo",
            text(library.path()),
        ],
    );
    assert_success(&output);
    for tags in library.tags() {
        assert!(tags.bpm.is_some(), "{tags:?}");
    }

    let both = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--tempo",
            "--no-tempo",
            "--dry-run",
            text(library.path()),
        ],
    );
    assert!(!both.status.success());
}

#[test]
fn every_program_has_a_preset_and_open_key_is_a_notation() {
    for preset in [
        "generic",
        "rekordbox",
        "serato",
        "traktor",
        "engine-dj",
        "virtualdj",
        "mixxx",
    ] {
        let euphoria = Euphoria::new();
        let library = Library::new();
        let output = run(
            &euphoria,
            &[
                "tags",
                "write",
                "--dry-run",
                "--preset",
                preset,
                text(library.path()),
            ],
        );
        assert_success(&output);
    }

    let euphoria = Euphoria::new();
    let library = Library::new();
    let output = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--preset",
            "traktor",
            "--key-notation",
            "open-key",
            text(library.path()),
        ],
    );
    assert_success(&output);
    for tags in library.tags() {
        let key = tags.key.as_deref().expect("a key");
        assert!(key.ends_with('m') || key.ends_with('d'), "{key}");
        let comment = tags.comment.as_deref().expect("a comment");
        assert!(
            comment.starts_with(&format!("{key} - Energy ")),
            "{comment}"
        );
    }
}

#[test]
fn a_file_that_fails_is_reported_and_the_others_are_written() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let not_audio = library.path().join("notes.mp3");
    std::fs::write(&not_audio, "This is a text file, not audio.\n".repeat(64)).unwrap();

    let output = run(&euphoria, &["tags", "write", "--yes", text(library.path())]);
    assert!(!output.status.success());
    let report = standard_error(&output);
    assert!(
        report.contains(&format!("error: {}: ", text(&not_audio))),
        "{report}"
    );
    assert!(
        report.contains("5 files to change, 1 left out because of errors."),
        "{report}"
    );
    assert!(report.contains("Wrote the tags of 5 files."), "{report}");
    for tags in library.tags() {
        assert_written(&tags);
    }
    assert_eq!(
        std::fs::read_to_string(&not_audio).unwrap(),
        "This is a text file, not audio.\n".repeat(64)
    );
}

#[cfg(unix)]
#[test]
fn a_file_the_module_refuses_is_reported_with_the_reason() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let linked = library.path().join("sine-440hz-mono.flac");
    let elsewhere = TemporaryFolder::new();
    let other_name = elsewhere.path().join("same-file.flac");
    std::fs::hard_link(&linked, &other_name).unwrap();
    let original = std::fs::read(&linked).unwrap();

    let output = run(&euphoria, &["tags", "write", "--yes", text(library.path())]);
    assert!(!output.status.success());
    let report = standard_error(&output);
    assert!(
        report.contains(&format!("error: {}: ", text(&linked))),
        "{report}"
    );
    assert!(report.contains("more than one hard link"), "{report}");
    assert!(report.contains("Wrote the tags of 4 files."), "{report}");
    assert!(std::fs::read(&linked).unwrap() == original);
    assert!(std::fs::read(&other_name).unwrap() == original);
}

#[test]
fn a_file_changed_after_the_write_is_reported_by_undo_and_can_be_retried() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let original = library.contents();
    assert_success(&run(
        &euphoria,
        &["tags", "write", "--yes", text(library.path())],
    ));

    // Another program rewrites the tags of one file.
    let edited = library.tracks().remove(1);
    let written = std::fs::read(&edited).unwrap();
    let edit = run(
        &euphoria,
        &[
            "tags",
            "write",
            "--yes",
            "--key-notation",
            "musical",
            text(&edited),
        ],
    );
    assert_success(&edit);
    let edited_contents = std::fs::read(&edited).unwrap();
    // Forget that write, so the first one is the last on record.
    let logs = euphoria.data_directory().join("tag-writes");
    let mut log_files: Vec<PathBuf> = std::fs::read_dir(&logs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    log_files.sort();
    std::fs::remove_file(log_files.pop().unwrap()).unwrap();

    let output = run(&euphoria, &["tags", "undo"]);
    assert!(!output.status.success());
    let report = standard_error(&output);
    assert!(
        report.contains(&format!("error: {}: ", text(&edited))),
        "{report}"
    );
    assert!(
        report.contains("Restored the tags of 4 files, 1 could not be restored"),
        "{report}"
    );
    assert!(std::fs::read(&edited).unwrap() == edited_contents);

    std::fs::write(&edited, written).unwrap();
    assert_success(&run(&euphoria, &["tags", "undo"]));
    assert!(library.contents() == original);
}

#[test]
fn yes_and_dry_run_cannot_be_combined() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let output = run(
        &euphoria,
        &["tags", "write", "--yes", "--dry-run", text(library.path())],
    );
    assert!(!output.status.success());
    assert!(standard_error(&output).contains("cannot be used with"));
}

/// No carriage return, escape character or other control character apart
/// from the line ends the program itself writes.
fn assert_cannot_act_on_a_terminal(text: &str) {
    for line in text.lines() {
        assert!(
            !line.chars().any(char::is_control),
            "a control character reaches the terminal in {line:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn text_from_files_cannot_act_on_the_terminal() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    // A name and a comment that try to erase the preview line and replace
    // it with something harmless looking.
    let track = folder.path().join("track\r\u{1b}[2Kinnocent.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.wav"), &track).unwrap();
    add_comment_to_wav(&track, "note\r\u{1b}[1A\u{1b}[2Knothing to see\u{7}");
    let not_audio = folder.path().join("broken\r\u{1b}[2K.mp3");
    std::fs::write(&not_audio, "This is a text file, not audio.\n".repeat(64)).unwrap();

    let outputs = [
        run(
            &euphoria,
            &["tags", "write", "--dry-run", text(folder.path())],
        ),
        run(&euphoria, &["tags", "write", text(folder.path())]),
        run(&euphoria, &["tags", "write", "--yes", text(folder.path())]),
        run(&euphoria, &["tags", "undo"]),
        run(&euphoria, &["analyze", text(folder.path())]),
        run(
            &euphoria,
            &[
                "analyze",
                text(&folder.path().join("missing\r\u{1b}[2K.mp3")),
            ],
        ),
    ];
    for output in &outputs {
        assert_cannot_act_on_a_terminal(&standard_output(output));
        assert_cannot_act_on_a_terminal(&standard_error(output));
    }
    let preview = standard_output(&outputs[0]);
    assert!(
        preview.contains("track\\r\\u{1b}[2Kinnocent.wav"),
        "{preview}"
    );
    assert!(
        preview.contains("note\\r\\u{1b}[1A\\u{1b}[2Knothing to see\\u{7}"),
        "{preview}"
    );
    let report = standard_error(&outputs[0]);
    assert!(report.contains("error: "), "{report}");
    assert!(report.contains("broken\\r\\u{1b}[2K.mp3"), "{report}");
    assert!(standard_output(&outputs[3]).contains("track\\r\\u{1b}[2Kinnocent.wav"));

    // What is written to the file is the real text, not the escaped form.
    let written = run(&euphoria, &["tags", "write", "--yes", text(folder.path())]);
    assert_cannot_act_on_a_terminal(&standard_output(&written));
    let comment = read_tags(&track).unwrap().comment.unwrap();
    assert!(
        comment.ends_with(" - note\r\u{1b}[1A\u{1b}[2Knothing to see\u{7}"),
        "{comment:?}"
    );

    // JSON keeps the real path, escaped the JSON way.
    let json = run(&euphoria, &["analyze", "--json", text(&track)]);
    assert!(standard_output(&json).contains("track\\r\\u001b[2Kinnocent.wav"));
}

#[test]
fn written_and_restored_files_come_from_the_cache_and_are_not_analyzed_again() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let analyze = ["analyze", "--json", text(library.path())];
    let first = run(&euphoria, &analyze);
    assert_success(&first);
    assert!(!standard_error(&first).contains("came from the cache"));

    let written = run(&euphoria, &["tags", "write", "--yes", text(library.path())]);
    assert_success(&written);
    assert!(
        !standard_error(&written).contains("warning"),
        "{}",
        standard_error(&written)
    );
    let after_write = run(&euphoria, &analyze);
    assert_success(&after_write);
    assert_eq!(
        standard_error(&after_write),
        "5 of 5 tracks came from the cache.\n"
    );
    assert_eq!(after_write.stdout, first.stdout);

    let undone = run(&euphoria, &["tags", "undo"]);
    assert_success(&undone);
    let after_undo = run(&euphoria, &analyze);
    assert_success(&after_undo);
    assert_eq!(
        standard_error(&after_undo),
        "5 of 5 tracks came from the cache.\n"
    );
    assert_eq!(after_undo.stdout, first.stdout);
}

#[test]
fn a_write_without_the_cache_leaves_no_cache_behind() {
    let euphoria = Euphoria::new();
    let library = Library::new();
    let written = run(
        &euphoria,
        &["tags", "write", "--yes", "--no-cache", text(library.path())],
    );
    assert_success(&written);
    assert!(!euphoria.cache_path().exists());
    assert_success(&run(&euphoria, &["tags", "undo"]));
    assert!(!euphoria.cache_path().exists());
}
