// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs `euphoria export rekordbox` on the core crate's audio fixtures and
//! reads the file it writes.

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

const AUDIO_FIXTURES: [&str; 10] = [
    "sine-440hz-mono-alac.m4a",
    "sine-440hz-mono.aiff",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.mp4",
    "sine-440hz-mono.ogg",
    "sine-440hz-mono.wav",
    "sine-440hz-opposite-phase-stereo.flac",
    "sine-440hz-stereo.wav",
];
const LOCATION_PREFIX: &str = "file://localhost";
const IMPORT_STEPS: [&str; 4] = ["To import it in rekordbox:", "  1. ", "  2. ", "  3. "];
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

/// Asserts that `lines`, from the first given, are the numbered import
/// steps and the link to the guide.
fn assert_import_steps(lines: &[&str]) {
    assert_eq!(lines.len(), IMPORT_STEPS.len() + 1, "{lines:?}");
    for (line, start) in lines.iter().zip(IMPORT_STEPS) {
        assert!(line.starts_with(start), "{lines:?}");
    }
    assert!(
        lines[IMPORT_STEPS.len()].starts_with(GUIDE_LINE_START),
        "{lines:?}"
    );
}

/// The lines a successful export writes to standard error: the summary,
/// then the import steps and the guide.
fn summary_lines(output: &Output) -> Vec<String> {
    let errors = standard_error(output);
    let lines: Vec<&str> = errors.lines().collect();
    assert_import_steps(&lines[1..]);
    lines.iter().map(|line| line.to_string()).collect()
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
            "Exported 10 tracks in 1 playlist to {}",
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
        summary_lines(&second_run)[0].ends_with("second.xml (10 from the cache)"),
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
    assert_eq!(library.tracks.len(), 8);
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
    assert!(
        lines[0].starts_with(&format!("error: {}: cannot decode ", readme.display())),
        "{errors}"
    );
    assert!(lines[0].contains("unsupported format"), "{errors}");
    assert!(
        lines[1].starts_with("Exported 10 tracks in 1 playlist to "),
        "{errors}"
    );
    assert_eq!(
        lines[2],
        "Left 1 file out of the export; the errors above say why."
    );
    assert_import_steps(&lines[3..]);

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
    assert_eq!(
        lines[0],
        format!(
            "warning: {path}: named after its file, as its title could not be read: \
             cannot open {path}: Permission denied (os error 13)",
            path = unreadable.display()
        )
    );
    assert!(
        lines[1].starts_with("Exported 2 tracks in 1 playlist to ")
            && lines[1].ends_with("(2 from the cache)"),
        "{errors}"
    );
    assert_import_steps(&lines[2..]);

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
    assert!(errors.starts_with("error: "), "{errors}");
    assert!(errors.contains("README.md: "), "{errors}");
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
        format!("Exported 10 tracks to {}", output_file.display())
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

    assert_eq!(
        tonality(&["--key-notation", "open-key"]).as_deref(),
        Some("4d")
    );

    let unknown = euphoria.run(&[
        "export",
        "rekordbox",
        text(&fixture),
        "-o",
        "unused.xml",
        "--key-notation",
        "other",
    ]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(standard_error(&unknown).contains("possible values: camelot, musical, open-key"));
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
fn a_cache_that_cannot_be_used_stops_the_export_until_no_cache_is_passed() {
    let euphoria = Euphoria::new();
    std::fs::write(euphoria.cache_path(), "not a database ".repeat(500))
        .expect("the damaged cache is written");
    let output_folder = TemporaryFolder::new();
    let output_file = output_folder.path().join("library.xml");
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let export = |extra: &[&str]| {
        let mut arguments = vec![
            "export",
            "rekordbox",
            text(&fixture),
            "-o",
            text(&output_file),
        ];
        arguments.extend_from_slice(extra);
        euphoria.run(&arguments)
    };

    let refused = export(&[]);
    assert!(!refused.status.success());
    let errors = standard_error(&refused);
    assert!(
        errors.contains(text(&euphoria.cache_path())) && errors.contains("--no-cache"),
        "{errors}"
    );
    assert!(!output_file.exists(), "nothing is written");

    let exported = export(&["--no-cache"]);
    assert_success(&exported);
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

#[cfg(unix)]
#[test]
fn a_symlinked_track_is_left_out_when_the_collection_lists_its_target() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let real_folder = TemporaryFolder::new();
    let real = resolved(real_folder.path()).join("kicks.wav");
    write_kick_track(&real, 120.0, 0.25);
    let music = resolved(music_folder.path());
    let link = music.join("linked.wav");
    std::os::unix::fs::symlink(&real, &link).expect("the link is created");

    let output_folder = TemporaryFolder::new();
    let output = resolved(output_folder.path());
    let collection = output.join("collection.xml");
    std::fs::write(
        &collection,
        format!(
            "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"1\">\
             <TRACK TrackID=\"1\" Location=\"file://localhost{}\">\
             <POSITION_MARK Name=\"mine\" Type=\"0\" Start=\"4.0\" Num=\"0\"/>\
             </TRACK></COLLECTION></DJ_PLAYLISTS>",
            text(&real)
        ),
    )
    .expect("the collection is written");

    let file = output.join("export.xml");
    let export = euphoria.run(&[
        "export",
        "rekordbox",
        text(&music),
        "-o",
        text(&file),
        "--beat-grid",
        "--cues",
        "--collection",
        text(&collection),
    ]);
    assert_eq!(export.status.code(), Some(1));
    assert!(
        standard_error(&export).contains("every track already has cues"),
        "{}",
        standard_error(&export)
    );
    assert!(!file.exists());
}

#[test]
fn the_cues_the_user_set_replace_the_automatic_ones_in_the_json_and_the_export() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let automatic = analyzed_as_json(&euphoria, music_folder.path());
    assert_eq!(automatic[0]["cues"][0]["hot_cue"], "A");
    assert_eq!(automatic[0]["set_by_user"], serde_json::json!([]));

    let edited = vec![
        euphoria_core::Cue {
            seconds: 2.25,
            name: "My drop".to_string(),
            hot_cue: euphoria_core::HotCue::from_letter('C'),
        },
        euphoria_core::Cue {
            seconds: 10.0,
            name: "Memory only".to_string(),
            hot_cue: None,
        },
    ];
    euphoria_core::Cache::open(&euphoria.cache_path())
        .expect("the cache opens")
        .set_edited_cues(&track, Some(&edited))
        .expect("the cues are kept");

    let analyzed = analyzed_as_json(&euphoria, music_folder.path());
    assert_eq!(
        analyzed[0]["cues"],
        serde_json::json!([
            {"seconds": 2.25, "name": "My drop", "hot_cue": "C"},
            {"seconds": 10.0, "name": "Memory only", "hot_cue": null},
        ])
    );
    assert_eq!(analyzed[0]["set_by_user"], serde_json::json!(["cues"]));

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
    let export = |name: &str, collection: &Path| {
        let file = output.join(name);
        let run = euphoria.run(&[
            "export",
            "rekordbox",
            text(&track),
            "-o",
            text(&file),
            "--beat-grid",
            "--cues",
            "--collection",
            text(collection),
        ]);
        (run, file)
    };

    let (exported, exported_file) = export("edited.xml", &empty_collection);
    assert_success(&exported);
    assert_eq!(
        exported_cues(&exported_file),
        [
            ("My drop".to_string(), "-1".to_string()),
            ("My drop".to_string(), "2".to_string()),
            ("Memory only".to_string(), "-1".to_string()),
        ]
    );

    let (left_out, left_out_file) = export("left-out.xml", &cued_collection);
    assert_eq!(left_out.status.code(), Some(1));
    assert!(
        standard_error(&left_out).contains("every track already has cues"),
        "{}",
        standard_error(&left_out)
    );
    assert!(!left_out_file.exists());
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

#[test]
fn the_collection_is_never_the_output_even_with_force() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let output_folder = TemporaryFolder::new();
    let output = resolved(output_folder.path());
    let collection = output.join("collection.xml");
    let contents = "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"0\"/></DJ_PLAYLISTS>";
    std::fs::write(&collection, contents).expect("the collection is written");
    let link = output.join("link.xml");
    std::os::unix::fs::symlink(&collection, &link).expect("the link is made");

    for spelling in [
        collection.clone(),
        output.join(".").join("collection.xml"),
        output.join("sub").join("..").join("collection.xml"),
        link,
    ] {
        if spelling
            .parent()
            .is_some_and(|parent| parent.ends_with(".."))
        {
            std::fs::create_dir_all(output.join("sub")).expect("the folder is made");
        }
        let refused = euphoria.run(&[
            "export",
            "rekordbox",
            text(&track),
            "-o",
            text(&spelling),
            "--force",
            "--beat-grid",
            "--cues",
            "--collection",
            text(&collection),
        ]);

        assert_eq!(refused.status.code(), Some(1), "{}", spelling.display());
        assert!(
            standard_error(&refused).contains("is the rekordbox collection in"),
            "{}",
            standard_error(&refused)
        );
        assert_eq!(
            std::fs::read_to_string(&collection).expect("the collection is read"),
            contents
        );
    }
}

#[test]
fn the_collection_is_never_the_output_without_cues_either() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let output_folder = TemporaryFolder::new();
    let collection = resolved(output_folder.path()).join("collection.xml");
    let contents = "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"0\"/></DJ_PLAYLISTS>";
    std::fs::write(&collection, contents).expect("the collection is written");

    let refused = euphoria.run(&[
        "export",
        "rekordbox",
        text(&track),
        "-o",
        text(&collection),
        "--force",
        "--beat-grid",
        "--collection",
        text(&collection),
    ]);

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        standard_error(&refused).contains("is the rekordbox collection in"),
        "{}",
        standard_error(&refused)
    );
    assert_eq!(
        std::fs::read_to_string(&collection).expect("the collection is read"),
        contents
    );
}

#[test]
fn cued_tracks_that_match_no_file_stop_the_export_unless_allowed() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let output_folder = TemporaryFolder::new();
    let output = resolved(output_folder.path());
    let collection = output.join("collection.xml");
    std::fs::write(
        &collection,
        "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"2\">\
         <TRACK TrackID=\"1\" Name=\"Elsewhere\" Artist=\"Someone\" \
         Location=\"file://localhost/C:/Music/elsewhere.mp3\">\
         <POSITION_MARK Name=\"\" Type=\"0\" Start=\"1.0\" Num=\"0\"/></TRACK>\
         <TRACK TrackID=\"2\" Name=\"No location\">\
         <POSITION_MARK Name=\"\" Type=\"0\" Start=\"1.0\" Num=\"0\"/></TRACK>\
         </COLLECTION></DJ_PLAYLISTS>",
    )
    .expect("the collection is written");
    let refused_file = output.join("refused.xml");
    let allowed_file = output.join("allowed.xml");
    let export = |file: &Path, extra: &[&str]| {
        let mut arguments = vec![
            "export",
            "rekordbox",
            text(&track),
            "-o",
            text(file),
            "--beat-grid",
            "--cues",
            "--collection",
            text(&collection),
        ];
        arguments.extend_from_slice(extra);
        euphoria.run(&arguments)
    };

    let refused = export(&refused_file, &[]);
    let allowed = export(&allowed_file, &["--allow-unmatched-cued-tracks"]);

    assert_eq!(refused.status.code(), Some(1));
    let message = standard_error(&refused);
    assert!(message.contains("2 tracks with cues in"), "{message}");
    assert!(
        message.contains("Elsewhere by Someone at file://localhost/C:/Music/elsewhere.mp3"),
        "{message}"
    );
    assert!(
        message.contains("No location by (no artist) at (no location)"),
        "{message}"
    );
    assert!(
        message.contains("--allow-unmatched-cued-tracks"),
        "{message}"
    );
    assert!(!refused_file.exists());

    assert_success(&allowed);
    let lines: Vec<String> = standard_error(&allowed)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines[2], "Cues for 1 of 1 track.");
    assert_eq!(
        lines[3],
        "Not protected, as --allow-unmatched-cued-tracks asked: 2 tracks with cues in your rekordbox collection match no file on this computer:"
    );
    assert_eq!(
        lines[4],
        "  Elsewhere by Someone at file://localhost/C:/Music/elsewhere.mp3"
    );
    assert!(!exported_cues(&allowed_file).is_empty());
}

#[test]
fn unmatched_collection_metadata_cannot_rewrite_terminal_messages() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let collection = folder.path().join("collection.xml");
    let output_file = folder.path().join("export.xml");
    std::fs::write(
        &collection,
        "<DJ_PLAYLISTS><COLLECTION>\
        <TRACK Name=\"fake&#13;forged&#10;error: spoofed\" Artist=\"\u{202e}\" \
        Location=\"file://localhost/definitely-absent.mp3\"><POSITION_MARK Start=\"1\"/></TRACK>\
        </COLLECTION></DJ_PLAYLISTS>",
    )
    .unwrap();
    let track = fixtures_directory().join("sine-440hz-mono.flac");
    let output = euphoria.run(&[
        "export",
        "rekordbox",
        text(&track),
        "-o",
        text(&output_file),
        "--beat-grid",
        "--cues",
        "--collection",
        text(&collection),
        "--allow-unmatched-cued-tracks",
    ]);
    assert_success(&output);
    let message = standard_error(&output);
    assert!(!message.contains(['\r', '\u{202e}']), "{message:?}");
    assert!(
        message.contains("fake\\rforged\\nerror: spoofed by \\u{202e}"),
        "{message:?}"
    );
    assert!(
        !message
            .lines()
            .any(|line| line
                == "error: spoofed by \u{202e} at file://localhost/definitely-absent.mp3")
    );
}

/// The NML location Traktor would give `path` on a Mac whose startup
/// volume is `Test HD`.
fn traktor_key(path: &Path) -> String {
    let text = text(path);
    let (volume, rest) = match text.strip_prefix("/Volumes/") {
        Some(rest) => rest.split_once('/').expect("a volume and a path"),
        None => ("Test HD", &text[1..]),
    };
    let (folders, file) = rest.rsplit_once('/').unwrap_or(("", rest));
    let mut key = format!("{volume}/:");
    for folder in folders.split('/').filter(|folder| !folder.is_empty()) {
        key.push_str(folder);
        key.push_str("/:");
    }
    key.push_str(file);
    key
}

fn traktor_entry_key(entry: roxmltree::Node<'_, '_>) -> String {
    let location = child(entry, "LOCATION");
    format!(
        "{}{}{}",
        location.attribute("VOLUME").expect("a volume"),
        location.attribute("DIR").expect("a folder"),
        location.attribute("FILE").expect("a file")
    )
}

#[test]
fn traktor_export_writes_key_tempo_grid_and_a_playlist_per_folder() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let fixture = resolved(&fixtures_directory().join("sine-440hz-mono.flac"));
    let output_folder = TemporaryFolder::new();
    let output = resolved(output_folder.path()).join("euphoria.nml");

    let exported = euphoria.run(&[
        "export",
        "traktor",
        text(&resolved(music_folder.path())),
        text(&fixture),
        "--beat-grid",
        "--key-notation",
        "open-key",
        "--startup-volume",
        "Test HD",
        "-o",
        text(&output),
    ]);

    assert_success(&exported);
    let errors = standard_error(&exported);
    let lines: Vec<&str> = errors.lines().collect();
    assert!(lines[0].starts_with("Exported 2 tracks"), "{lines:?}");
    assert_eq!(
        lines[1],
        "Beat grids for 1 of 2 tracks; the others have no clear beat or a grid rated low confidence, so Traktor analyzes them."
    );
    assert_eq!(lines[2], "To import it in Traktor:");
    assert!(lines[5].starts_with("  3. "), "{lines:?}");
    assert!(lines[6].ends_with("export-to-traktor.md"), "{lines:?}");

    let document = std::fs::read_to_string(&output).expect("the export is read");
    let xml = roxmltree::Document::parse(&document).expect("the export is well formed XML");
    let entries: Vec<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name("ENTRY") && node.attribute("TITLE").is_some())
        .collect();
    let mut entry_keys: Vec<String> = entries
        .iter()
        .map(|entry| traktor_entry_key(*entry))
        .collect();
    entry_keys.sort();
    let mut expected_keys = vec![traktor_key(&track), traktor_key(&fixture)];
    expected_keys.sort();
    assert_eq!(entry_keys, expected_keys);
    let entry_for = |path: &Path| {
        *entries
            .iter()
            .find(|entry| traktor_entry_key(**entry) == traktor_key(path))
            .expect("the track is exported")
    };
    let analyzed = analyzed_as_json(&euphoria, music_folder.path());
    let kicks = entry_for(&track);
    let bpm: f64 = child(kicks, "TEMPO")
        .attribute("BPM")
        .expect("a tempo")
        .parse()
        .expect("a number");
    assert!(
        (bpm - analyzed[0]["beat_grid"]["bpm"]
            .as_f64()
            .expect("a grid tempo"))
        .abs()
            < 0.006,
        "{bpm} against {}",
        analyzed[0]["beat_grid"]
    );
    assert!((bpm - 120.0).abs() < 0.05, "{bpm}");
    let grid = kicks
        .children()
        .find(|node| node.has_tag_name("CUE_V2") && node.attribute("TYPE") == Some("4"))
        .expect("a grid marker");
    let start_seconds: f64 = grid
        .attribute("START")
        .expect("a start")
        .parse::<f64>()
        .expect("a number")
        / 1000.0;
    let first_downbeat = analyzed[0]["beat_grid"]["first_downbeat_seconds"]
        .as_f64()
        .expect("a first downbeat");
    assert!(
        (start_seconds - first_downbeat).abs() < 0.001,
        "{start_seconds} against {first_downbeat}"
    );
    assert_eq!(
        child(grid, "GRID").attribute("BPM"),
        child(kicks, "TEMPO").attribute("BPM")
    );

    let sine = entry_for(&fixture);
    let key_text = child(sine, "INFO").attribute("KEY").expect("a key");
    let open_key: euphoria_core::Key = key_text.parse().expect("an Open Key code");
    assert!(
        key_text.ends_with('m') || key_text.ends_with('d'),
        "{key_text}"
    );
    let value: u8 = child(sine, "MUSICAL_KEY")
        .attribute("VALUE")
        .expect("a key value")
        .parse()
        .expect("a number");
    assert_eq!(
        euphoria_core::export::traktor::key_from_musical_key_value(value),
        Some(open_key)
    );

    let playlist_keys: Vec<String> = xml
        .descendants()
        .filter(|node| node.has_tag_name("PRIMARYKEY"))
        .map(|node| node.attribute("KEY").expect("a key").to_string())
        .collect();
    let mut playlist_keys = playlist_keys;
    playlist_keys.sort();
    assert_eq!(playlist_keys, expected_keys);
}

#[test]
fn traktor_cues_leave_out_tracks_with_cues_of_their_own_in_the_collection() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let cued = resolved(music_folder.path()).join("cued.wav");
    let analyzed_only = resolved(music_folder.path()).join("analyzed.wav");
    write_kick_track(&cued, 120.0, 0.25);
    write_kick_track(&analyzed_only, 124.0, 0.1);
    let output_folder = TemporaryFolder::new();
    let output = resolved(output_folder.path());
    let location = |path: &Path| {
        let key = traktor_key(path);
        let (volume, rest) = key.split_once("/:").expect("a volume");
        let (folders, file) = rest.rsplit_once("/:").expect("a file");
        format!(
            "<LOCATION DIR=\"/:{folders}/:\" FILE=\"{file}\" VOLUME=\"{volume}\" VOLUMEID=\"1\"/>"
        )
    };
    let collection = output.join("collection.nml");
    std::fs::write(
        &collection,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><NML VERSION=\"20\"><COLLECTION ENTRIES=\"2\">\
             <ENTRY TITLE=\"Cued\">{}\
             <CUE_V2 NAME=\"mine\" TYPE=\"0\" START=\"4000.0\" LEN=\"0\" HOTCUE=\"3\"/></ENTRY>\
             <ENTRY TITLE=\"Analyzed\">{}\
             <CUE_V2 NAME=\"AutoGrid\" TYPE=\"4\" START=\"100.0\" LEN=\"0\" HOTCUE=\"-1\"><GRID BPM=\"124.0\"/></CUE_V2>\
             <CUE_V2 NAME=\"AutoGrid\" TYPE=\"0\" START=\"100.0\" LEN=\"0\" HOTCUE=\"0\"/></ENTRY>\
             </COLLECTION></NML>",
            location(&cued),
            location(&analyzed_only)
        ),
    )
    .expect("the collection is written");
    let file = output.join("cues.nml");

    let exported = euphoria.run(&[
        "export",
        "traktor",
        text(&resolved(music_folder.path())),
        "--beat-grid",
        "--cues",
        "--collection",
        text(&collection),
        "--startup-volume",
        "Test HD",
        "-o",
        text(&file),
    ]);
    let refused = euphoria.run(&[
        "export",
        "traktor",
        text(&cued),
        "--beat-grid",
        "--cues",
        "--collection",
        text(&collection),
        "--force",
        "-o",
        text(&collection),
    ]);

    assert_success(&exported);
    let errors = standard_error(&exported);
    assert!(
        errors.contains("Left out 1 track that already has cues in your Traktor collection"),
        "{errors}"
    );
    let document = std::fs::read_to_string(&file).expect("the export is read");
    let xml = roxmltree::Document::parse(&document).expect("the export is well formed XML");
    let entries: Vec<_> = xml
        .descendants()
        .filter(|node| node.has_tag_name("ENTRY") && node.attribute("TITLE").is_some())
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(traktor_entry_key(entries[0]), traktor_key(&analyzed_only));
    let hot_cues: Vec<&str> = entries[0]
        .children()
        .filter(|node| node.has_tag_name("CUE_V2") && node.attribute("TYPE") == Some("0"))
        .map(|node| node.attribute("HOTCUE").expect("a hot cue"))
        .collect();
    assert_eq!(hot_cues.first(), Some(&"0"), "{document}");
    assert!(hot_cues.iter().all(|pad| *pad != "-1"), "{document}");

    assert_eq!(refused.status.code(), Some(1));
    assert!(
        standard_error(&refused).contains("is the Traktor collection in"),
        "{}",
        standard_error(&refused)
    );
    assert!(
        std::fs::read_to_string(&collection)
            .expect("the collection is read")
            .contains("NAME=\"mine\"")
    );
}

#[test]
fn unreadable_saved_cues_are_reported_as_unchecked_edits() {
    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let track = music.path().join("track.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.wav"), &track)
        .expect("the fixture is copied");
    assert_success(&euphoria.run(&["analyze", "--json", text(&track)]));
    euphoria_core::Cache::open(&euphoria.cache_path())
        .expect("the cache opens")
        .set_edited_cues(&track, Some(&[]))
        .expect("only a cue edit is saved");
    rusqlite::Connection::open(euphoria.cache_path())
        .expect("the database opens")
        .execute("UPDATE edited_cues SET cues = 'not json'", [])
        .expect("the saved cue set is made unreadable");
    let output = music.path().join("library.xml");
    let exported = euphoria.run(&["export", "rekordbox", text(&track), "-o", text(&output)]);
    assert!(!exported.status.success());
    let errors = standard_error(&exported);
    assert!(errors.contains("cannot check the saved edits"), "{errors}");
    assert!(!errors.contains("key or tempo"), "{errors}");
    assert!(!output.exists(), "the unchecked track is not exported");
}

#[test]
fn cues_saved_before_the_analysis_are_kept_within_the_track_on_every_surface() {
    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let track = music.path().join("track.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.wav"), &track)
        .expect("the fixture is copied");
    // The cache knows no length yet, so both cues are stored.
    euphoria_core::Cache::open(&euphoria.cache_path())
        .expect("the cache opens")
        .set_edited_cues(
            &track,
            Some(&[
                euphoria_core::Cue {
                    seconds: 0.5,
                    name: "In the track".to_string(),
                    hot_cue: None,
                },
                euphoria_core::Cue {
                    seconds: 999.0,
                    name: "Past the end".to_string(),
                    hot_cue: None,
                },
            ]),
        )
        .expect("the cues are saved");

    // Analyzing records the 1 s length, and the cue past the end is kept
    // out of the stored set.
    let json = euphoria.run(&["analyze", "--json", text(&track)]);
    assert_success(&json);
    let report = String::from_utf8_lossy(&json.stdout).into_owned();
    assert!(report.contains("\"In the track\""), "{report}");
    assert!(!report.contains("\"Past the end\""), "{report}");
    let stored = euphoria_core::Cache::open(&euphoria.cache_path())
        .expect("the cache opens")
        .edited_cues_for(&track)
        .expect("the cues are read")
        .expect("a set is stored");
    assert_eq!(stored.len(), 1, "{stored:?}");

    // Export shows the same set as the report.
    let collection = music.path().join("empty-collection.xml");
    std::fs::write(
        &collection,
        "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"0\"/></DJ_PLAYLISTS>",
    )
    .expect("the collection is written");
    let output = music.path().join("library.xml");
    let exported = euphoria.run(&[
        "export",
        "rekordbox",
        text(&track),
        "-o",
        text(&output),
        "--beat-grid",
        "--cues",
        "--collection",
        text(&collection),
    ]);
    assert_success(&exported);
    let xml = std::fs::read_to_string(&output).expect("the export is read");
    assert!(xml.contains("In the track"), "{xml}");
    assert!(!xml.contains("Past the end"), "{xml}");
}

/// An Engine DJ 5.0 library in its own folder holding `track`, analysed by
/// Engine DJ at 44.1 kHz, and the path of its database.
fn engine_library(track: &Path) -> (TemporaryFolder, PathBuf, PathBuf) {
    use euphoria_core::export::engine_blobs::{
        EMPTY_QUICK_CUE_OFFSET, QuickCue, QuickCues, TrackData, compress,
    };
    let folder = TemporaryFolder::new();
    let library = resolved(folder.path()).join("Engine Library");
    let database = library.join("Database2").join("m.db");
    std::fs::create_dir_all(database.parent().expect("a folder")).expect("the folder is made");
    let schema = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/euphoria-core/tests/fixtures/engine/engine-desktop-5.0.0-m.db.sql"),
    )
    .expect("the schema is read");
    let connection = rusqlite::Connection::open(&database).expect("the library opens");
    connection
        .execute_batch(&schema)
        .expect("the library is built");
    let relative = pathdiff(track, &library);
    connection
        .execute(
            "INSERT INTO Track (path, filename, length, bpm, bpmAnalyzed, isAnalyzed, isAvailable) VALUES (?1, 'kicks.wav', 30, 100, 100.0, 1, 1)",
            [&relative],
        )
        .expect("the track is added");
    let track_id = connection.last_insert_rowid();
    let track_data = TrackData {
        sample_rate: 44_100.0,
        samples: 44_100 * 30,
        key: 0,
        average_loudness: [0.5; 3],
        extra: Vec::new(),
    };
    let quick_cues = QuickCues {
        cues: (0..8)
            .map(|_| QuickCue {
                label: String::new(),
                sample_offset: EMPTY_QUICK_CUE_OFFSET,
                colour: [0; 4],
            })
            .collect(),
        adjusted_main_cue: EMPTY_QUICK_CUE_OFFSET,
        is_main_cue_adjusted: 0,
        default_main_cue: EMPTY_QUICK_CUE_OFFSET,
        extra: Vec::new(),
    };
    connection
        .execute(
            "UPDATE PerformanceData SET trackData = ?1, quickCues = ?2 WHERE trackId = ?3",
            rusqlite::params![
                compress(&track_data.encode()),
                compress(&quick_cues.encode()),
                track_id
            ],
        )
        .expect("the performance data is set");
    (folder, library, database)
}

/// `path` relative to `base`, both absolute, with `..` where they part.
fn pathdiff(path: &Path, base: &Path) -> String {
    let path_parts: Vec<_> = path.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let shared = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for _ in shared..base_parts.len() {
        relative.push("..");
    }
    for part in &path_parts[shared..] {
        relative.push(part);
    }
    relative.to_string_lossy().into_owned()
}

fn engine_backups(library: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(library.parent().expect("a folder"))
        .expect("the folder is read")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("Engine Library backup by Euphoria")
            })
        })
        .collect()
}

#[test]
fn engine_writes_key_tempo_and_grid_and_keeps_a_backup() {
    use euphoria_core::export::engine_blobs::{BeatData, decompress};
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let (_folder, library, database) = engine_library(&track);

    let exported = euphoria.run(&[
        "export",
        "engine",
        "--library",
        text(&library),
        "--beat-grid",
        text(&track),
    ]);

    assert_success(&exported);
    let connection = rusqlite::Connection::open(&database).expect("the library opens");
    let (bpm, beat_data): (f64, Vec<u8>) = connection
        .query_row(
            "SELECT bpmAnalyzed, beatData FROM Track JOIN PerformanceData ON Track.id = PerformanceData.trackId",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the track is read");
    assert!((bpm - 120.0).abs() < 0.05, "{bpm}");
    let beat_data =
        BeatData::decode(&decompress(&beat_data).expect("it inflates")).expect("it decodes");
    assert_eq!(beat_data.adjusted_grid[0].beat_number, -4);
    let backups = engine_backups(&library);
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert!(backups[0].join("Database2").join("m.db").is_file());
    assert!(standard_error(&exported).contains("Updated 1 track in"));
}

#[test]
fn engine_dry_run_writes_nothing() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let (_folder, library, database) = engine_library(&track);
    let before = std::fs::read(&database).expect("the library is read");

    let dry_run = euphoria.run(&[
        "export",
        "engine",
        "--library",
        text(&library),
        "--dry-run",
        text(&track),
    ]);

    assert_success(&dry_run);
    assert!(standard_error(&dry_run).contains("Dry run: 1 track"));
    assert_eq!(
        std::fs::read(&database).expect("the library is read"),
        before
    );
    assert!(engine_backups(&library).is_empty());
}

#[test]
fn engine_refuses_an_unknown_schema_before_analysing() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let (_folder, library, database) = engine_library(&track);
    rusqlite::Connection::open(&database)
        .and_then(|connection| {
            connection.execute("UPDATE Information SET schemaVersionMinor = 9", [])
        })
        .expect("the version is changed");
    let before = std::fs::read(&database).expect("the library is read");

    let exported = euphoria.run(&[
        "export",
        "engine",
        "--library",
        text(&library),
        text(&track),
    ]);

    assert!(!exported.status.success());
    assert!(standard_error(&exported).contains("schema version 3.9.2"));
    assert_eq!(
        std::fs::read(&database).expect("the library is read"),
        before
    );
    assert!(engine_backups(&library).is_empty());
}

/// A VirtualDJ database in its own folder holding a song for `track`, at
/// 120 BPM by VirtualDJ's scan, and a song for another file.
fn virtualdj_database(track: &Path) -> (TemporaryFolder, PathBuf, String) {
    let folder = TemporaryFolder::new();
    let database = resolved(folder.path()).join("database.xml");
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<VirtualDJ_Database Version=\"2026\">\r\n <Song FilePath=\"{}\" FileSize=\"1\">\r\n  <Tags Title=\"Kicks\" />\r\n  <Scan Version=\"801\" Bpm=\"0.500000\" Flag=\"32768\" />\r\n </Song>\r\n <Song FilePath=\"/elsewhere/other.mp3\" FileSize=\"2\">\r\n  <Tags Title=\"Other\" />\r\n </Song>\r\n</VirtualDJ_Database>\r\n",
        track.display()
    );
    std::fs::write(&database, &text).expect("the database is written");
    (folder, database, text)
}

fn backups_next_to(database: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(database.parent().expect("a folder"))
        .expect("the folder is read")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("database.xml.euphoria-backup-")
            })
        })
        .collect()
}

#[test]
fn virtualdj_updates_the_song_of_a_track_and_keeps_a_backup() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let (_database_folder, database, original) = virtualdj_database(&track);

    let exported = euphoria.run(&[
        "export",
        "virtualdj",
        "--database",
        text(&database),
        "--beat-grid",
        text(&track),
    ]);

    assert_success(&exported);
    let updated = std::fs::read_to_string(&database).expect("the database is read");
    let document = roxmltree::Document::parse(&updated).expect("well-formed XML");
    let song = document
        .descendants()
        .find(|node| node.attribute("FilePath") == Some(text(&track)))
        .expect("the song");
    let scan = song
        .children()
        .find(|node| node.has_tag_name("Scan"))
        .expect("the scan");
    assert!(scan.attribute("Phase").is_some(), "{updated}");
    let seconds_per_beat: f64 = scan
        .attribute("Bpm")
        .expect("a tempo")
        .parse()
        .expect("a number");
    assert!((60.0 / seconds_per_beat - 120.0).abs() < 0.05, "{updated}");
    assert!(
        song.children()
            .any(|node| node.attribute("Type") == Some("beatgrid"))
    );
    let other_song = " <Song FilePath=\"/elsewhere/other.mp3\" FileSize=\"2\">\r\n  <Tags Title=\"Other\" />\r\n </Song>";
    assert!(updated.contains(other_song));
    let backups = backups_next_to(&database);
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(
        std::fs::read_to_string(&backups[0]).expect("the backup is read"),
        original
    );
    assert!(standard_error(&exported).contains("Updated 1 song in"));
}

#[test]
fn virtualdj_dry_run_writes_nothing() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let (_database_folder, database, original) = virtualdj_database(&track);

    let dry_run = euphoria.run(&[
        "export",
        "virtualdj",
        "--database",
        text(&database),
        "--dry-run",
        text(&track),
    ]);

    assert_success(&dry_run);
    assert!(standard_error(&dry_run).contains("Dry run: 1 song"));
    assert_eq!(
        std::fs::read_to_string(&database).expect("the database is read"),
        original
    );
    assert!(backups_next_to(&database).is_empty());
}

#[test]
fn virtualdj_reports_tracks_that_are_not_in_the_database() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let (_database_folder, database, _) = virtualdj_database(Path::new("/elsewhere/a.mp3"));
    let original = std::fs::read_to_string(&database).expect("the database is read");

    let exported = euphoria.run(&[
        "export",
        "virtualdj",
        "--database",
        text(&database),
        text(&track),
    ]);

    assert!(!exported.status.success());
    let errors = standard_error(&exported);
    assert!(errors.contains("No track is in"), "{errors}");
    assert!(
        errors.contains("1 track is not in this database"),
        "{errors}"
    );
    assert_eq!(
        std::fs::read_to_string(&database).expect("the database is read"),
        original
    );
    assert!(backups_next_to(&database).is_empty());
}

#[test]
fn virtualdj_refuses_a_file_that_is_not_a_virtualdj_database() {
    let euphoria = Euphoria::new();
    let music_folder = TemporaryFolder::new();
    let track = resolved(music_folder.path()).join("kicks.wav");
    write_kick_track(&track, 120.0, 0.25);
    let database_folder = TemporaryFolder::new();
    let database = resolved(database_folder.path()).join("database.xml");
    std::fs::write(&database, "<rekordbox/>").expect("the file is written");

    let exported = euphoria.run(&[
        "export",
        "virtualdj",
        "--database",
        text(&database),
        text(&track),
    ]);

    assert!(!exported.status.success());
    assert!(standard_error(&exported).contains("not <VirtualDJ_Database>"));
    assert_eq!(
        std::fs::read_to_string(&database).expect("the file is read"),
        "<rekordbox/>"
    );
}
