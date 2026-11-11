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
    assert!(
        output.status.success(),
        "the command ended with {}\nstandard error:\n{}\nstandard output:\n{}",
        output.status,
        standard_error(output),
        String::from_utf8_lossy(&output.stdout)
    );
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
    comment: Option<String>,
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
                    comment: attribute(node, "Comments"),
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
        assert_eq!(
            track.comment,
            analysis["energy"]
                .as_u64()
                .map(|energy| format!("Energy {energy}")),
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
            && library.tracks.iter().any(|track| track.comment.is_some())
            && library.tracks.iter().any(|track| track.tempo.is_some()),
        "some fixture has a key, some an energy rating and some a tempo"
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

#[cfg(unix)]
#[test]
fn a_track_whose_tags_cannot_be_read_is_exported_under_its_file_name() {
    use std::os::unix::fs::PermissionsExt;

    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let music_folder = resolved(music.path());
    let readable = music_folder.join("readable.flac");
    let unreadable = music_folder.join("Locked Away.flac");
    // The two files differ in content. The cache keeps one place per content,
    // so of two identical files only one is found again without being opened.
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.flac"), &readable)
        .expect("the fixture is copied");
    std::fs::copy(tagged_fixtures_directory().join("tagged.flac"), &unreadable)
        .expect("the fixture is copied");
    let output_file = music_folder.join("library.xml");
    let arguments = [
        "export",
        "rekordbox",
        text(&music_folder),
        "--force",
        "-o",
        text(&output_file),
    ];
    assert_success(&euphoria.run(&arguments));
    let before = ExportedLibrary::read(&output_file);
    let track_before = before.track(&unreadable);
    assert_eq!(track_before.name.as_deref(), Some("Low Tide"));
    assert_eq!(track_before.artist.as_deref(), Some("Saffron & Co"));
    let set_mode = |mode| {
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(mode))
            .expect("the file's permissions are set");
    };

    // The analysis comes from the cache, which does not open a file it has
    // seen at the same place with the same size and modification time.
    set_mode(0o000);
    let exported = euphoria.run(&arguments);
    set_mode(0o644);

    assert_success(&exported);
    assert!(exported.stdout.is_empty());
    let errors = standard_error(&exported);
    let lines: Vec<&str> = errors.lines().collect();
    assert_eq!(lines.len(), 5, "{errors}");
    assert_eq!(
        lines[0],
        "Could not read the title of 1 file, used its file name:"
    );
    assert_eq!(
        lines[1],
        format!(
            "  {path}: cannot open {path}: Permission denied (os error 13)",
            path = unreadable.display()
        )
    );
    assert!(
        lines[2].starts_with("Exported 2 tracks in 1 playlist to ")
            && lines[2].ends_with("(2 from the cache)"),
        "{errors}"
    );

    let library = ExportedLibrary::read(&output_file);
    let renamed_track = library.track(&unreadable);
    assert_eq!(renamed_track.name.as_deref(), Some("Locked Away"));
    assert_eq!(renamed_track.artist, None);
    assert!(renamed_track.tonality.is_some());
    assert_eq!(renamed_track.tonality, track_before.tonality);
    assert_eq!(renamed_track.tempo, track_before.tempo);
    assert_eq!(renamed_track.comment, track_before.comment);
    assert_eq!(renamed_track.total_time, track_before.total_time);
    assert_eq!(library.track(&readable), before.track(&readable));
    assert_eq!(
        library.playlists,
        vec![ExportedNode::Playlist(
            text(music_folder.file_name().map(Path::new).expect("a name")).to_string(),
            vec![unreadable, readable]
        )]
    );
}

#[test]
fn an_output_that_is_one_of_the_audio_files_is_refused_even_with_force() {
    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let music_folder = resolved(music.path());
    let library_folder = music_folder.join("library");
    std::fs::create_dir_all(&library_folder).expect("the folder is created");
    let song = library_folder.join("song.flac");
    let original = std::fs::read(fixtures_directory().join("sine-440hz-mono.flac"))
        .expect("the fixture is read");
    std::fs::write(&song, &original).expect("the song is written");
    let assert_refused = |arguments: &[&str], expected: &str| {
        let refused = euphoria
            .command(arguments)
            .current_dir(&music_folder)
            .output()
            .expect("the euphoria binary runs");
        let errors = standard_error(&refused);
        assert_eq!(refused.status.code(), Some(1), "{arguments:?}: {errors}");
        assert!(errors.starts_with("error: "), "{arguments:?}: {errors}");
        assert!(errors.contains(expected), "{arguments:?}: {errors}");
        assert_eq!(
            std::fs::read(&song).expect("the song is read"),
            original,
            "{arguments:?}"
        );
        assert!(
            !euphoria.cache_path().exists(),
            "{arguments:?}: the export stops before analyzing anything"
        );
    };
    let is_an_input = "is one of the files to export";

    assert_refused(
        &[
            "export",
            "rekordbox",
            text(&song),
            "--force",
            "-o",
            text(&song),
        ],
        is_an_input,
    );
    assert_refused(
        &[
            "export",
            "rekordbox",
            "library/song.flac",
            "--force",
            "-o",
            "library/../library/./song.flac",
        ],
        is_an_input,
    );
    assert_refused(
        &[
            "export",
            "rekordbox",
            "library",
            "--force",
            "-o",
            "library/song.flac",
        ],
        is_an_input,
    );
    assert_refused(
        &[
            "export",
            "rekordbox",
            "library",
            "--force",
            "-o",
            "library/new.MP3",
        ],
        "has the extension of an audio file and is inside",
    );
    assert!(!library_folder.join("new.MP3").exists());

    #[cfg(unix)]
    {
        let link_to_song = music_folder.join("link.xml");
        std::os::unix::fs::symlink(&song, &link_to_song).expect("the link is created");
        assert_refused(
            &[
                "export",
                "rekordbox",
                "library",
                "--force",
                "-o",
                "link.xml",
            ],
            is_an_input,
        );
        assert!(link_to_song.is_symlink());

        let link_to_folder = music_folder.join("linked-library");
        std::os::unix::fs::symlink(&library_folder, &link_to_folder).expect("the link is created");
        assert_refused(
            &[
                "export",
                "rekordbox",
                "library",
                "--force",
                "-o",
                "linked-library/song.flac",
            ],
            is_an_input,
        );
        assert_refused(
            &[
                "export",
                "rekordbox",
                "linked-library",
                "--force",
                "-o",
                "library/new.wav",
            ],
            "has the extension of an audio file and is inside",
        );
    }

    let allowed = euphoria
        .command(&[
            "export",
            "rekordbox",
            "library",
            "-o",
            "library/library.xml",
        ])
        .current_dir(&music_folder)
        .output()
        .expect("the euphoria binary runs");
    assert_success(&allowed);
    assert_eq!(
        ExportedLibrary::read(&library_folder.join("library.xml")).tracks[0].path,
        song
    );
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

/// Writes 30 seconds of a 16 bit mono WAV file holding a decaying low tone
/// on every beat at `tempo`, starting `first_beat` seconds in.
fn write_kick_track(path: &Path, tempo: f64, first_beat: f64) {
    write_kicks(path, first_beat, |_| tempo);
}

/// Writes 30 seconds of a 16 bit mono WAV file holding a decaying low tone
/// on every beat from `first_beat`, each beat following the one before at
/// `tempo_at` its time.
fn write_kicks(path: &Path, first_beat: f64, tempo_at: impl Fn(f64) -> f64) {
    const SAMPLE_RATE: u32 = 44_100;
    let sample_count = 30 * SAMPLE_RATE as usize;
    let mut samples = vec![0.0_f64; sample_count];
    let mut beat_start = first_beat;
    while beat_start < 30.0 {
        let first_sample = (beat_start * f64::from(SAMPLE_RATE)) as usize;
        for (offset, sample) in samples
            .iter_mut()
            .skip(first_sample)
            .take(8_000)
            .enumerate()
        {
            let time = offset as f64 / f64::from(SAMPLE_RATE);
            *sample += (std::f64::consts::TAU * 60.0 * time).sin() * (-time / 0.04).exp();
        }
        beat_start += 60.0 / tempo_at(beat_start);
    }
    let data_length = (sample_count * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + sample_count * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for sample in samples {
        let scaled = (sample.clamp(-1.0, 1.0) * 0.8 * f64::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&scaled.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("the track is written");
}

#[test]
fn beat_grid_writes_the_grid_of_a_track_with_a_clear_beat() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let output_folder = TemporaryFolder::new();
    let with_grid = resolved(output_folder.path()).join("with-grid.xml");
    let without_grid = resolved(output_folder.path()).join("without-grid.xml");

    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        "--beat-grid",
        text(&track),
        "-o",
        text(&with_grid),
    ]);
    let plain = euphoria.run(&[
        "export",
        "rekordbox",
        text(&track),
        "-o",
        text(&without_grid),
    ]);

    assert_success(&exported);
    assert_eq!(
        standard_error(&exported).lines().nth(1),
        Some("Beat grids for 1 of 1 track.")
    );
    let document = std::fs::read_to_string(&with_grid).expect("the export is read");
    let xml = roxmltree::Document::parse(&document).expect("the export is well formed XML");
    let tempo = xml
        .descendants()
        .find(|node| node.has_tag_name("TEMPO"))
        .expect("the track has a beat grid");
    let start: f64 = tempo
        .attribute("Inizio")
        .expect("a start")
        .parse()
        .expect("a number");
    let bpm: f64 = tempo
        .attribute("Bpm")
        .expect("a tempo")
        .parse()
        .expect("a number");
    assert!((bpm - 120.0).abs() < 0.05, "{bpm}");
    let beat_offset = (start - 0.25).rem_euclid(0.5);
    assert!(
        beat_offset.min(0.5 - beat_offset) < 0.03,
        "first downbeat at {start}"
    );
    assert_eq!(
        ExportedLibrary::read(&with_grid).tracks[0].tempo.as_deref(),
        tempo.attribute("Bpm")
    );

    assert_success(&plain);
    let document = std::fs::read_to_string(&without_grid).expect("the export is read");
    assert!(!document.contains("<TEMPO"));
    let analyzed = analyzed_as_json(&euphoria, music_folder.path());
    let grid = &analyzed[0]["beat_grid"];
    assert!((grid["bpm"].as_f64().expect("a tempo") - 120.0).abs() < 0.05);
    assert!(grid["first_downbeat_seconds"].is_f64());
    assert!(grid["confidence"].is_string());
}

/// The cue points of the only track in an exported file, as (name, number).
fn exported_cues(file: &Path) -> Vec<(String, String)> {
    let document = std::fs::read_to_string(file).expect("the export is read");
    let xml = roxmltree::Document::parse(&document).expect("the export is well formed XML");
    xml.descendants()
        .filter(|node| node.has_tag_name("POSITION_MARK"))
        .map(|node| {
            (
                attribute(node, "Name").expect("a name"),
                attribute(node, "Num").expect("a number"),
            )
        })
        .collect()
}

#[test]
fn cues_are_written_only_for_tracks_without_cues_in_the_collection() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let output_folder = TemporaryFolder::new();
    let output = resolved(output_folder.path());
    let empty_collection = output.join("empty-collection.xml");
    std::fs::write(
        &empty_collection,
        "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"0\"/></DJ_PLAYLISTS>",
    )
    .expect("the collection is written");
    let cued_collection = output.join("cued-collection.xml");
    std::fs::write(
        &cued_collection,
        format!(
            "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"1\">\
             <TRACK TrackID=\"1\" Location=\"file://localhost{}\">\
             <POSITION_MARK Name=\"mine\" Type=\"0\" Start=\"4.0\" Num=\"0\"/>\
             </TRACK></COLLECTION></DJ_PLAYLISTS>",
            text(&track)
        ),
    )
    .expect("the collection is written");
    let export = |name: &str, extra: &[&str]| {
        let file = output.join(name);
        let mut arguments = vec!["export", "rekordbox", text(&track), "-o", text(&file)];
        arguments.extend_from_slice(extra);
        (euphoria.run(&arguments), file)
    };

    let (without_collection, _) = export("refused.xml", &["--beat-grid", "--cues"]);
    let (without_grid, _) = export(
        "refused-too.xml",
        &["--cues", "--collection", text(&empty_collection)],
    );
    let (new_track, new_track_file) = export(
        "new.xml",
        &[
            "--beat-grid",
            "--cues",
            "--collection",
            text(&empty_collection),
        ],
    );
    let (cued_track, cued_track_file) = export(
        "cued.xml",
        &[
            "--beat-grid",
            "--cues",
            "--collection",
            text(&cued_collection),
        ],
    );

    assert_eq!(without_collection.status.code(), Some(2));
    assert!(standard_error(&without_collection).contains("--collection"));
    assert_eq!(without_grid.status.code(), Some(2));
    assert!(standard_error(&without_grid).contains("--beat-grid"));

    assert_success(&new_track);
    assert_eq!(
        standard_error(&new_track).lines().nth(2),
        Some("Cues for 1 of 1 track.")
    );
    let cues = exported_cues(&new_track_file);
    assert_eq!(
        cues[..2],
        [
            ("Start".to_string(), "-1".to_string()),
            ("Start".to_string(), "0".to_string())
        ]
    );
    let analyzed = analyzed_as_json(&euphoria, music_folder.path());
    assert_eq!(analyzed[0]["cues"][0]["name"], "Start");
    assert_eq!(
        analyzed[0]["cues"].as_array().expect("cues").len() * 2,
        cues.len()
    );

    assert_eq!(cued_track.status.code(), Some(1));
    assert!(
        standard_error(&cued_track).contains("every track already has cues"),
        "{}",
        standard_error(&cued_track)
    );
    assert!(!cued_track_file.exists());

    let other = resolved(music_folder.path()).join("other.wav");
    write_kick_track(&other, 124.0, 0.1);
    let mixed_file = output.join("mixed.xml");
    let mixed = euphoria.run(&[
        "export",
        "rekordbox",
        text(&resolved(music_folder.path())),
        "-o",
        text(&mixed_file),
        "--beat-grid",
        "--cues",
        "--collection",
        text(&cued_collection),
    ]);
    assert_success(&mixed);
    let lines: Vec<String> = standard_error(&mixed).lines().map(str::to_string).collect();
    assert!(
        lines[0].starts_with("Exported 1 track in 1 playlist"),
        "{lines:?}"
    );
    assert_eq!(lines[2], "Cues for 1 of 1 track.");
    assert_eq!(
        lines[3],
        "Left out 1 track that already has cues in your rekordbox collection, so importing this file cannot change them. Export again without --cues to update its key and tempo."
    );
    let library = ExportedLibrary::read(&mixed_file);
    assert_eq!(
        library
            .tracks
            .iter()
            .map(|track| track.path.clone())
            .collect::<Vec<_>>(),
        vec![other.clone()]
    );
    assert_eq!(
        library.playlists,
        vec![ExportedNode::Playlist(
            music_folder
                .path()
                .file_name()
                .expect("a folder name")
                .to_string_lossy()
                .into_owned(),
            vec![other]
        )]
    );
}

#[test]
fn a_grid_rated_low_is_left_out_and_not_counted() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let steady = resolved(music_folder.path()).join("steady.wav");
    let drifting = resolved(music_folder.path()).join("drifting.wav");
    write_kick_track(&steady, 120.0, 0.25);
    // The tempo rises from 120 to 132 BPM over the 30 seconds.
    write_kicks(&drifting, 0.25, |time| 120.0 + 12.0 * time / 30.0);
    let output_folder = TemporaryFolder::new();
    let output_file = resolved(output_folder.path()).join("library.xml");

    let analyzed = analyzed_as_json(&euphoria, music_folder.path());
    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        "--beat-grid",
        "--no-playlists",
        text(music_folder.path()),
        "-o",
        text(&output_file),
    ]);

    let confidence_of = |name: &str| {
        analyzed
            .iter()
            .find(|track| {
                track["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(name))
            })
            .expect("the track is analyzed")["beat_grid"]["confidence"]
            .clone()
    };
    assert_eq!(confidence_of("drifting.wav"), "low");
    assert_ne!(confidence_of("steady.wav"), "low");
    assert_success(&exported);
    assert_eq!(
        standard_error(&exported).lines().nth(1),
        Some(
            "Beat grids for 1 of 2 tracks; the others have no clear beat or a grid rated low confidence, so rekordbox analyzes them."
        )
    );
    let document = std::fs::read_to_string(&output_file).expect("the export is read");
    let xml = roxmltree::Document::parse(&document).expect("the export is well formed XML");
    let with_tempo: Vec<String> = xml
        .descendants()
        .filter(|node| node.has_tag_name("TRACK") && node.attribute("Location").is_some())
        .filter(|node| node.children().any(|child| child.has_tag_name("TEMPO")))
        .map(|node| attribute(node, "Location").expect("a location"))
        .collect();
    assert_eq!(with_tempo.len(), 1);
    assert!(with_tempo[0].ends_with("steady.wav"), "{with_tempo:?}");
}
