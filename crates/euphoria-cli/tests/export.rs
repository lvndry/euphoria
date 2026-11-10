// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs `euphoria export rekordbox` on the core crate's audio fixtures and
//! reads the file it writes.

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

const AUDIO_FIXTURES: [&str; 7] = [
    "sine-440hz-mono.aiff",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.wav",
    "sine-440hz-opposite-phase-stereo.flac",
    "sine-440hz-stereo.wav",
];
const LOCATION_PREFIX: &str = "file://localhost";
const IMPORT_STEPS_START: &str = "In rekordbox: 1. ";
const GUIDE_LINE_START: &str = "Full guide: https://";

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn tagged_fixtures_directory() -> PathBuf {
    fixtures_directory().join("../tagged-fixtures")
}

fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().expect("the path resolves")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", standard_error(output));
}

/// The lines a successful export writes to standard error: the summary and
/// the two lines of import steps.
fn summary_lines(output: &Output) -> Vec<String> {
    let lines: Vec<String> = standard_error(output).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].starts_with(IMPORT_STEPS_START), "{lines:?}");
    assert!(lines[2].starts_with(GUIDE_LINE_START), "{lines:?}");
    lines
}

fn percent_decoded(encoded: &str) -> String {
    let mut bytes = Vec::new();
    let mut rest = encoded.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let code = std::str::from_utf8(&after[..2]).expect("two hexadecimal digits");
            bytes.push(u8::from_str_radix(code, 16).expect("a hexadecimal number"));
            rest = &after[2..];
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    String::from_utf8(bytes).expect("a UTF-8 path")
}

/// One `TRACK` of the collection.
#[derive(Debug, Clone, PartialEq)]
struct ExportedTrack {
    id: String,
    path: PathBuf,
    name: Option<String>,
    artist: Option<String>,
    total_time: String,
    tempo: Option<String>,
    tonality: Option<String>,
}

/// A playlist or folder: its name, and its tracks or its children.
#[derive(Debug, Clone, PartialEq)]
enum ExportedNode {
    Playlist(String, Vec<PathBuf>),
    Folder(String, Vec<ExportedNode>),
}

fn child<'document, 'input>(
    parent: roxmltree::Node<'document, 'input>,
    name: &str,
) -> roxmltree::Node<'document, 'input> {
    parent
        .children()
        .find(|node| node.has_tag_name(name))
        .unwrap_or_else(|| panic!("a {name} element"))
}

fn attribute(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name).map(str::to_string)
}

struct ExportedLibrary {
    tracks: Vec<ExportedTrack>,
    playlists: Vec<ExportedNode>,
}

impl ExportedLibrary {
    fn read(file: &Path) -> Self {
        let document = std::fs::read_to_string(file).expect("the export is read");
        let xml = roxmltree::Document::parse(&document).expect("the export is well formed XML");
        let collection = child(xml.root_element(), "COLLECTION");
        let tracks: Vec<ExportedTrack> = collection
            .children()
            .filter(|node| node.has_tag_name("TRACK"))
            .map(|node| {
                let location = node.attribute("Location").expect("a location");
                let encoded_path = location
                    .strip_prefix(LOCATION_PREFIX)
                    .expect("a file://localhost location");
                ExportedTrack {
                    id: attribute(node, "TrackID").expect("a track number"),
                    path: PathBuf::from(percent_decoded(encoded_path)),
                    name: attribute(node, "Name"),
                    artist: attribute(node, "Artist"),
                    total_time: attribute(node, "TotalTime").expect("a duration"),
                    tempo: attribute(node, "AverageBpm"),
                    tonality: attribute(node, "Tonality"),
                }
            })
            .collect();
        assert_eq!(
            collection.attribute("Entries"),
            Some(tracks.len().to_string().as_str())
        );

        let root = child(child(xml.root_element(), "PLAYLISTS"), "NODE");
        assert_eq!(root.attribute("Name"), Some("ROOT"));
        let playlists = Self::read_nodes(root, &tracks);
        Self { tracks, playlists }
    }

    fn read_nodes(parent: roxmltree::Node<'_, '_>, tracks: &[ExportedTrack]) -> Vec<ExportedNode> {
        parent
            .children()
            .filter(|node| node.has_tag_name("NODE"))
            .map(|node| {
                let name = node.attribute("Name").expect("a name").to_string();
                if node.attribute("Type") == Some("0") {
                    return ExportedNode::Folder(name, Self::read_nodes(node, tracks));
                }
                let paths = node
                    .children()
                    .filter(|entry| entry.has_tag_name("TRACK"))
                    .map(|entry| {
                        let key = entry.attribute("Key").expect("a track number");
                        let track = tracks
                            .iter()
                            .find(|track| track.id == key)
                            .expect("the playlist entry is in the collection");
                        track.path.clone()
                    })
                    .collect();
                ExportedNode::Playlist(name, paths)
            })
            .collect()
    }

    fn track(&self, path: &Path) -> &ExportedTrack {
        self.tracks
            .iter()
            .find(|track| track.path == path)
            .unwrap_or_else(|| panic!("{} is in the export", path.display()))
    }
}

/// What `euphoria analyze --json` reports for a folder, by file.
fn analyzed_as_json(euphoria: &Euphoria, folder: &Path) -> Vec<serde_json::Value> {
    let output = euphoria.run(&["analyze", "--json", text(folder)]);
    assert_success(&output);
    let tracks: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the output is JSON");
    tracks.as_array().expect("an array").clone()
}

#[test]
fn exports_every_fixture_with_its_location_key_and_tempo() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = resolved(output_folder.path()).join("library.xml");
    let fixtures = resolved(&fixtures_directory());

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixtures),
        "--output",
        text(&output_file),
    ]);

    assert_success(&exported);
    assert!(exported.stdout.is_empty());
    assert_eq!(
        summary_lines(&exported)[0],
        format!(
            "Exported 7 tracks in 1 playlist to {}",
            output_file.display()
        )
    );

    let library = ExportedLibrary::read(&output_file);
    assert_eq!(library.tracks.len(), AUDIO_FIXTURES.len());
    let analyzed = analyzed_as_json(&euphoria, &fixtures);
    assert_eq!(analyzed.len(), AUDIO_FIXTURES.len());
    for (name, analysis) in AUDIO_FIXTURES.iter().zip(&analyzed) {
        let path = fixtures.join(name);
        assert_eq!(analysis["path"], text(&path));
        let track = library.track(&path);
        assert!(track.path.is_file(), "{track:?}");
        assert_eq!(track.total_time, "1", "{track:?}");
        assert_eq!(
            track.name.as_deref(),
            name.rsplit_once('.').map(|(stem, _)| stem),
            "{track:?}"
        );
        assert_eq!(track.artist, None, "{track:?}");
        assert_eq!(
            track.tonality.as_deref(),
            analysis["key"]["camelot"].as_str(),
            "{track:?}"
        );
        match (&track.tempo, analysis["bpm"].as_f64()) {
            (None, None) => {}
            (Some(tempo), Some(analyzed_tempo)) => {
                let tempo: f64 = tempo.parse().expect("a number");
                // The JSON rounds to one decimal, the export to two.
                assert!((tempo - analyzed_tempo).abs() < 0.06, "{track:?}");
            }
            _ => panic!("{track:?} does not match {analysis}"),
        }
    }
    assert!(
        library.tracks.iter().any(|track| track.tonality.is_some())
            && library.tracks.iter().any(|track| track.tempo.is_some()),
        "some fixture has a key and some fixture has a tempo"
    );
    assert_eq!(
        library.playlists,
        vec![ExportedNode::Playlist(
            "fixtures".to_string(),
            AUDIO_FIXTURES
                .iter()
                .map(|name| fixtures.join(name))
                .collect()
        )]
    );
    assert_eq!(
        std::fs::read_dir(output_folder.path())
            .expect("the folder is read")
            .count(),
        1,
        "only the export is in the output folder"
    );
}

#[test]
fn a_second_run_comes_from_the_cache_and_writes_the_same_file() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let first_file = output_folder.path().join("first.xml");
    let second_file = output_folder.path().join("second.xml");
    let fixtures = fixtures_directory();

    let first_run = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixtures),
        "-o",
        text(&first_file),
    ]);
    let second_run = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixtures),
        "-o",
        text(&second_file),
    ]);

    assert_success(&first_run);
    assert_success(&second_run);
    assert!(!summary_lines(&first_run)[0].contains("from the cache"));
    assert!(
        summary_lines(&second_run)[0].ends_with("second.xml (7 from the cache)"),
        "{}",
        standard_error(&second_run)
    );
    assert_eq!(
        std::fs::read(&first_file).expect("the first export is read"),
        std::fs::read(&second_file).expect("the second export is read")
    );
}

#[test]
fn titles_and_artists_come_from_the_tags_of_the_files() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("tagged.xml");
    let tagged_fixtures = resolved(&tagged_fixtures_directory());

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        text(&tagged_fixtures),
        "-o",
        text(&output_file),
    ]);

    assert_success(&exported);
    let library = ExportedLibrary::read(&output_file);
    assert_eq!(library.tracks.len(), 5);
    for track in &library.tracks {
        assert_eq!(track.name.as_deref(), Some("Low Tide"), "{track:?}");
        assert_eq!(track.artist.as_deref(), Some("Saffron & Co"), "{track:?}");
    }
}

#[test]
fn an_existing_output_file_is_kept_unless_force_is_given() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");
    std::fs::write(&output_file, "mine").expect("the existing file is written");
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let arguments = [
        "export",
        "rekordbox",
        text(&fixture),
        "-o",
        text(&output_file),
    ];

    let refused = euphoria.run(&arguments);

    assert!(!refused.status.success());
    let errors = standard_error(&refused);
    assert!(errors.starts_with("error: "), "{errors}");
    assert!(errors.contains("library.xml already exists"), "{errors}");
    assert!(errors.contains("--force"), "{errors}");
    assert_eq!(
        std::fs::read_to_string(&output_file).expect("the file is read"),
        "mine"
    );
    assert!(
        !euphoria.cache_path().exists(),
        "the export stops before analyzing anything"
    );

    let mut forced_arguments = arguments.to_vec();
    forced_arguments.push("--force");
    let forced = euphoria.run(&forced_arguments);

    assert_success(&forced);
    assert_eq!(ExportedLibrary::read(&output_file).tracks.len(), 1);
}

#[test]
fn a_file_that_fails_is_reported_and_the_others_are_exported() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");
    let fixtures = resolved(&fixtures_directory());
    let readme = fixtures.join("README.md");

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixtures),
        text(&readme),
        "-o",
        text(&output_file),
    ]);

    assert_eq!(exported.status.code(), Some(1));
    assert!(exported.stdout.is_empty());
    let errors = standard_error(&exported);
    let lines: Vec<&str> = errors.lines().collect();
    assert_eq!(lines.len(), 5, "{errors}");
    assert_eq!(lines[0], "Left 1 file out of the export:");
    assert!(
        lines[1].starts_with(&format!("  {}: cannot decode ", readme.display())),
        "{errors}"
    );
    assert!(lines[1].contains("unsupported format"), "{errors}");
    assert!(
        lines[2].starts_with("Exported 7 tracks in 1 playlist to "),
        "{errors}"
    );
    assert!(lines[3].starts_with(IMPORT_STEPS_START), "{errors}");
    assert!(lines[4].starts_with(GUIDE_LINE_START), "{errors}");

    let library = ExportedLibrary::read(&output_file);
    assert_eq!(library.tracks.len(), AUDIO_FIXTURES.len());
    assert!(library.tracks.iter().all(|track| track.path != readme));
}

#[test]
fn nothing_is_written_when_no_file_can_be_exported() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");
    let readme = fixtures_directory().join("README.md");
    let empty_folder = TemporaryFolder::new();

    let all_failed = euphoria.run(&[
        "export",
        "rekordbox",
        text(&readme),
        "-o",
        text(&output_file),
    ]);
    let none_found = euphoria.run(&[
        "export",
        "rekordbox",
        text(empty_folder.path()),
        "-o",
        text(&output_file),
    ]);

    assert_eq!(all_failed.status.code(), Some(1));
    let errors = standard_error(&all_failed);
    assert!(
        errors.starts_with("Left 1 file out of the export:"),
        "{errors}"
    );
    assert!(
        errors.contains("error: none of the files could be exported"),
        "{errors}"
    );
    assert_eq!(none_found.status.code(), Some(1));
    assert_eq!(
        standard_error(&none_found),
        "error: no audio files found, so nothing was exported\n"
    );
    assert!(!output_file.exists());
}

#[test]
fn no_playlists_exports_the_collection_only() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = resolved(output_folder.path()).join("library.xml");
    let fixtures = fixtures_directory();

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        "--no-playlists",
        text(&fixtures),
        "-o",
        text(&output_file),
    ]);

    assert_success(&exported);
    assert_eq!(
        summary_lines(&exported)[0],
        format!("Exported 7 tracks to {}", output_file.display())
    );
    let library = ExportedLibrary::read(&output_file);
    assert_eq!(library.tracks.len(), AUDIO_FIXTURES.len());
    assert_eq!(library.playlists, vec![]);
}

#[test]
fn playlists_mirror_the_folders_and_named_files_get_their_own() {
    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let music_folder = resolved(music.path());
    let library_folder = music_folder.join("My Library");
    let copy_fixture = |fixture: &str, destination: &str| {
        let destination = music_folder.join(destination);
        std::fs::create_dir_all(destination.parent().expect("a parent folder"))
            .expect("the folder is created");
        std::fs::copy(fixtures_directory().join(fixture), &destination)
            .expect("the fixture is copied");
        destination
    };
    let house = copy_fixture("sine-440hz-mono.flac", "My Library/House/Low Tide #1.flac");
    let deep_techno = copy_fixture("sine-440hz-mono.wav", "My Library/Techno/Deep/été.wav");
    let techno = copy_fixture("sine-440hz-mono.mp3", "My Library/Techno/peak.mp3");
    let named_directly = copy_fixture("sine-440hz-stereo.wav", "Downloads/new.wav");
    std::fs::create_dir_all(library_folder.join("Empty")).expect("the folder is created");
    std::fs::write(library_folder.join("House").join("cover.jpg"), b"")
        .expect("the cover is written");
    let output_file = music_folder.join("friday set.xml");

    let exported = euphoria
        .command(&[
            "export",
            "rekordbox",
            "My Library",
            "Downloads/../Downloads/new.wav",
            "--output",
            "friday set.xml",
        ])
        .current_dir(&music_folder)
        .output()
        .expect("the euphoria binary runs");

    assert_success(&exported);
    assert_eq!(
        summary_lines(&exported)[0],
        format!(
            "Exported 4 tracks in 4 playlists to {}",
            output_file.display()
        )
    );
    let library = ExportedLibrary::read(&output_file);
    assert_eq!(
        library.playlists,
        vec![
            ExportedNode::Folder(
                "My Library".to_string(),
                vec![
                    ExportedNode::Playlist("House".to_string(), vec![house]),
                    ExportedNode::Folder(
                        "Techno".to_string(),
                        vec![
                            ExportedNode::Playlist("Techno".to_string(), vec![techno]),
                            ExportedNode::Playlist("Deep".to_string(), vec![deep_techno.clone()]),
                        ]
                    ),
                ]
            ),
            ExportedNode::Playlist("friday set".to_string(), vec![named_directly]),
        ]
    );
    assert_eq!(library.track(&deep_techno).name.as_deref(), Some("été"));
}

#[test]
fn key_notation_chooses_how_keys_are_written() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let tonality = |notation_arguments: &[&str]| {
        let output_file = output_folder.path().join("library.xml");
        let mut arguments = vec![
            "export",
            "rekordbox",
            text(&fixture),
            "--force",
            "-o",
            text(&output_file),
        ];
        arguments.extend(notation_arguments);
        assert_success(&euphoria.run(&arguments));
        ExportedLibrary::read(&output_file).tracks[0]
            .tonality
            .clone()
    };

    assert_eq!(tonality(&[]).as_deref(), Some("11B"));
    assert_eq!(
        tonality(&["--key-notation", "camelot"]).as_deref(),
        Some("11B")
    );
    assert_eq!(
        tonality(&["--key-notation", "musical"]).as_deref(),
        Some("A")
    );

    let unknown = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixture),
        "-o",
        "unused.xml",
        "--key-notation",
        "open-key",
    ]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(standard_error(&unknown).contains("possible values: camelot, musical"));
}

#[test]
fn folders_nested_too_deeply_are_refused_and_nothing_is_written() {
    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let mut deepest_folder = music.path().to_path_buf();
    for level in 0..34 {
        deepest_folder.push(format!("level {level}"));
    }
    std::fs::create_dir_all(&deepest_folder).expect("the folders are created");
    for folder in [music.path(), &deepest_folder] {
        std::fs::copy(
            fixtures_directory().join("sine-440hz-mono.wav"),
            folder.join("track.wav"),
        )
        .expect("the fixture is copied");
    }
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        text(music.path()),
        "-o",
        text(&output_file),
    ]);

    assert_eq!(exported.status.code(), Some(1));
    assert_eq!(
        standard_error(&exported),
        "error: cannot export folder \"level 31\": nested more than 32 folders deep\n"
    );
    assert!(!output_file.exists());
    assert_eq!(
        std::fs::read_dir(output_folder.path())
            .expect("the folder is read")
            .count(),
        0
    );
}

#[test]
fn a_cache_that_cannot_be_used_is_a_warning_and_the_export_is_written() {
    let euphoria = Euphoria::new();
    std::fs::write(euphoria.cache_path(), "not a database ".repeat(500))
        .expect("the damaged cache is written");
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixture),
        "-o",
        text(&output_file),
    ]);

    assert_success(&exported);
    let errors = standard_error(&exported);
    assert!(
        errors.starts_with("warning: analyzing without the cache: "),
        "{errors}"
    );
    assert!(errors.contains("Exported 1 track in 1 playlist to "));
    assert_eq!(ExportedLibrary::read(&output_file).tracks.len(), 1);
}

#[test]
fn no_cache_neither_reads_nor_creates_the_cache() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        "--no-cache",
        "--jobs",
        "1",
        text(&fixture),
        "-o",
        text(&output_file),
    ]);

    assert_success(&exported);
    assert!(!euphoria.cache_path().exists());
    assert_eq!(ExportedLibrary::read(&output_file).tracks.len(), 1);
}

#[test]
fn paths_that_cannot_be_used_are_errors() {
    let euphoria = Euphoria::new();
    let output_folder = TemporaryFolder::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");

    let missing_input = euphoria.run(&[
        "export",
        "rekordbox",
        "/definitely/not/here",
        "-o",
        text(&output_folder.path().join("library.xml")),
    ]);
    assert_eq!(missing_input.status.code(), Some(1));
    assert!(standard_error(&missing_input).contains("cannot read /definitely/not/here"));

    let missing_output_folder = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixture),
        "-o",
        text(&output_folder.path().join("missing/library.xml")),
    ]);
    assert_eq!(missing_output_folder.status.code(), Some(1));
    assert!(standard_error(&missing_output_folder).contains("does not exist"));

    let output_is_a_folder = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixture),
        "--force",
        "-o",
        text(output_folder.path()),
    ]);
    assert_eq!(output_is_a_folder.status.code(), Some(1));
    assert!(standard_error(&output_is_a_folder).contains("is a folder"));

    let no_output = euphoria.run(&["export", "rekordbox", text(&fixture)]);
    assert_eq!(no_output.status.code(), Some(2));
    assert!(standard_error(&no_output).contains("--output <FILE>"));
}
