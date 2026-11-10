// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rekordbox XML export: the exact document for a small library, how
//! paths and text are encoded, what is left out when nothing is known, and
//! the libraries that are refused.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use euphoria_core::export::rekordbox::{
    CueKind, CuePoint, ExportOptions, Folder, KeyNotation, Library, Playlist, PlaylistNode, Track,
    write_xml,
};
use euphoria_core::{EuphoriaError, ExportProblem, Key, Mode};

const AROUND_THE_WORLD: &str = "/Users/dj/Music/Daft Punk - Around the World.mp3";
const THEME_REMIX: &str = "/Users/dj/Music/Beyoncé & JAY-Z/100% #1 <Remix>.flac";
const WARM_UP: &str = r"C:\Music\Sets\Warm Up.wav";

fn golden_library() -> Library {
    let around_the_world = Track {
        title: Some("Around the World".to_string()),
        artist: Some("Daft Punk".to_string()),
        key: Some(Key::new(9, Mode::Minor)),
        bpm: Some(121.3),
        first_beat: Some(Duration::from_millis(146)),
        cue_points: vec![
            CuePoint {
                name: "Intro".to_string(),
                time: Duration::from_millis(146),
                kind: CueKind::Memory,
            },
            CuePoint {
                name: "Drop".to_string(),
                time: Duration::from_millis(63_500),
                kind: CueKind::Hot(0),
            },
        ],
        ..Track::new(AROUND_THE_WORLD, Duration::from_millis(429_600))
    };
    let theme_remix = Track {
        title: Some("Tom & Jerry's \"Theme\" <Remix>".to_string()),
        artist: Some("Beyoncé".to_string()),
        key: Some(Key::new(6, Mode::Major)),
        bpm: Some(98.0),
        ..Track::new(THEME_REMIX, Duration::from_secs(200))
    };
    let warm_up = Track::new(WARM_UP, Duration::from_millis(61_400));

    Library {
        tracks: vec![around_the_world, theme_remix, warm_up],
        playlists: vec![
            PlaylistNode::Folder(Folder {
                name: "Club & Bar".to_string(),
                children: vec![
                    PlaylistNode::Playlist(Playlist {
                        name: "Peak <Time>".to_string(),
                        tracks: vec![PathBuf::from(THEME_REMIX), PathBuf::from(AROUND_THE_WORLD)],
                    }),
                    PlaylistNode::Folder(Folder {
                        name: "Later".to_string(),
                        children: Vec::new(),
                    }),
                ],
            }),
            PlaylistNode::Playlist(Playlist {
                name: "Everything".to_string(),
                tracks: vec![
                    PathBuf::from(AROUND_THE_WORLD),
                    PathBuf::from(THEME_REMIX),
                    PathBuf::from(WARM_UP),
                ],
            }),
            PlaylistNode::Playlist(Playlist {
                name: "Unsorted".to_string(),
                tracks: Vec::new(),
            }),
        ],
    }
}

fn export_with(library: &Library, options: &ExportOptions) -> String {
    let mut output = Vec::new();
    write_xml(&mut output, library, options).expect("the library exports");
    String::from_utf8(output).expect("the export is UTF-8")
}

fn export(library: &Library) -> String {
    export_with(library, &ExportOptions::default())
}

fn single_track_library(track: Track) -> Library {
    Library {
        tracks: vec![track],
        playlists: Vec::new(),
    }
}

fn untagged_track(path: impl Into<PathBuf>) -> Track {
    Track::new(path, Duration::from_secs(180))
}

fn collection_tracks<'input>(
    document: &'input roxmltree::Document<'input>,
) -> Vec<roxmltree::Node<'input, 'input>> {
    document
        .descendants()
        .find(|node| node.has_tag_name("COLLECTION"))
        .expect("the document has a COLLECTION")
        .children()
        .filter(|node| node.has_tag_name("TRACK"))
        .collect()
}

fn exported_location(path: impl Into<PathBuf>) -> String {
    let xml = export(&single_track_library(untagged_track(path)));
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
    collection_tracks(&document)[0]
        .attribute("Location")
        .expect("every track has a Location")
        .to_string()
}

fn export_problem(library: &Library) -> (PathBuf, ExportProblem) {
    let mut output = Vec::new();
    let error = write_xml(&mut output, library, &ExportOptions::default())
        .expect_err("the library is refused");
    assert!(output.is_empty(), "a refused library writes nothing");
    match error {
        EuphoriaError::Export { path, problem } => (path, problem),
        other => panic!("expected an export error, got {other:?}"),
    }
}

#[test]
fn small_library_matches_the_golden_file() {
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("rekordbox")
        .join("expected.xml");
    let expected = std::fs::read_to_string(golden_path)
        .expect("the golden file is readable")
        .replace("{version}", env!("CARGO_PKG_VERSION"));

    assert_eq!(export(&golden_library()), expected);
}

#[test]
fn export_parses_as_well_formed_xml_with_the_documented_structure() {
    let xml = export(&golden_library());
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");

    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "DJ_PLAYLISTS");
    assert_eq!(root.attribute("Version"), Some("1.0.0"));
    let sections: Vec<&str> = root
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|node| node.tag_name().name())
        .collect();
    assert_eq!(sections, ["PRODUCT", "COLLECTION", "PLAYLISTS"]);

    let tracks = collection_tracks(&document);
    let collection = tracks[0].parent().unwrap();
    assert_eq!(collection.attribute("Entries"), Some("3"));
    assert_eq!(tracks.len(), 3);
    assert_eq!(
        tracks[1].attribute("Name"),
        Some("Tom & Jerry's \"Theme\" <Remix>")
    );
    assert_eq!(tracks[1].attribute("Artist"), Some("Beyoncé"));
}

#[test]
fn location_percent_encodes_everything_but_unreserved_characters() {
    let cases = [
        ("/Music/plain.mp3", "file://localhost/Music/plain.mp3"),
        (
            "/Music/Two Words/a b.mp3",
            "file://localhost/Music/Two%20Words/a%20b.mp3",
        ),
        (
            "/Music/Café/Señor Ünïcode.mp3",
            "file://localhost/Music/Caf%C3%A9/Se%C3%B1or%20%C3%9Cn%C3%AFcode.mp3",
        ),
        (
            "/Music/東京/夜.flac",
            "file://localhost/Music/%E6%9D%B1%E4%BA%AC/%E5%A4%9C.flac",
        ),
        (
            "/Music/Simon & Garfunkel.mp3",
            "file://localhost/Music/Simon%20%26%20Garfunkel.mp3",
        ),
        (
            "/Music/Track #1.mp3",
            "file://localhost/Music/Track%20%231.mp3",
        ),
        (
            "/Music/100% Pure.mp3",
            "file://localhost/Music/100%25%20Pure.mp3",
        ),
        (
            "/Music/already%20encoded.mp3",
            "file://localhost/Music/already%2520encoded.mp3",
        ),
        (
            "/Music/who? [live] (edit) + 'quote'.mp3",
            "file://localhost/Music/who%3F%20%5Blive%5D%20%28edit%29%20%2B%20%27quote%27.mp3",
        ),
        (
            "/Music/keep-these_as.they~are.mp3",
            "file://localhost/Music/keep-these_as.they~are.mp3",
        ),
        (
            r"/Music/back\slash.mp3",
            "file://localhost/Music/back%5Cslash.mp3",
        ),
    ];
    for (path, location) in cases {
        assert_eq!(exported_location(path), location, "{path}");
    }
}

#[test]
fn location_keeps_the_drive_letter_of_a_windows_path() {
    let cases = [
        (
            r"C:\Music\PioneerDJ\Demo Tracks\Demo Track 1.mp3",
            "file://localhost/C:/Music/PioneerDJ/Demo%20Tracks/Demo%20Track%201.mp3",
        ),
        (
            "D:/Sets/R&B #2.wav",
            "file://localhost/D:/Sets/R%26B%20%232.wav",
        ),
    ];
    for (path, location) in cases {
        assert_eq!(exported_location(path), location, "{path}");
    }
}

#[cfg(unix)]
#[test]
fn location_encodes_path_bytes_that_are_not_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let path = PathBuf::from(OsStr::from_bytes(b"/Music/caf\xE9.mp3"));
    assert_eq!(exported_location(path), "file://localhost/Music/caf%E9.mp3");
}

#[test]
fn track_without_key_or_tempo_omits_those_attributes() {
    let xml = export(&single_track_library(untagged_track("/Music/unknown.mp3")));
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
    let track = collection_tracks(&document)[0];

    let attribute_names: Vec<&str> = track
        .attributes()
        .map(|attribute| attribute.name())
        .collect();
    assert_eq!(attribute_names, ["TrackID", "TotalTime", "Location"]);
    assert_eq!(track.attribute("TotalTime"), Some("180"));
    assert!(!track.has_children());
}

#[test]
fn beat_grid_needs_both_a_tempo_and_a_first_beat() {
    let tempo_only = Track {
        bpm: Some(128.0),
        ..untagged_track("/Music/tempo-only.mp3")
    };
    let first_beat_only = Track {
        first_beat: Some(Duration::from_millis(25)),
        ..untagged_track("/Music/first-beat-only.mp3")
    };
    let both = Track {
        bpm: Some(127.996),
        first_beat: Some(Duration::from_micros(25_400)),
        ..untagged_track("/Music/both.mp3")
    };
    let xml = export(&Library {
        tracks: vec![tempo_only, first_beat_only, both],
        playlists: Vec::new(),
    });
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
    let tracks = collection_tracks(&document);

    assert_eq!(tracks[0].attribute("AverageBpm"), Some("128.00"));
    assert!(!tracks[0].has_children());
    assert_eq!(tracks[1].attribute("AverageBpm"), None);
    assert!(!tracks[1].has_children());

    let grid: Vec<_> = tracks[2]
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    assert_eq!(grid.len(), 1);
    assert_eq!(grid[0].tag_name().name(), "TEMPO");
    assert_eq!(grid[0].attribute("Inizio"), Some("0.025"));
    assert_eq!(grid[0].attribute("Bpm"), Some("128.00"));
    assert_eq!(grid[0].attribute("Metro"), Some("4/4"));
    assert_eq!(grid[0].attribute("Battito"), Some("1"));
}

#[test]
fn cue_points_are_written_as_memory_cues_and_numbered_hot_cues() {
    let track = Track {
        cue_points: vec![
            CuePoint {
                name: String::new(),
                time: Duration::ZERO,
                kind: CueKind::Memory,
            },
            CuePoint {
                name: "Break & build".to_string(),
                time: Duration::from_millis(95_250),
                kind: CueKind::Hot(2),
            },
        ],
        ..untagged_track("/Music/cues.mp3")
    };
    let xml = export(&single_track_library(track));
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
    let marks: Vec<_> = collection_tracks(&document)[0]
        .children()
        .filter(|node| node.has_tag_name("POSITION_MARK"))
        .collect();

    assert_eq!(marks.len(), 2);
    assert_eq!(marks[0].attribute("Name"), Some(""));
    assert_eq!(marks[0].attribute("Type"), Some("0"));
    assert_eq!(marks[0].attribute("Start"), Some("0.000"));
    assert_eq!(marks[0].attribute("Num"), Some("-1"));
    assert_eq!(marks[0].attribute("End"), None);
    assert_eq!(marks[1].attribute("Name"), Some("Break & build"));
    assert_eq!(marks[1].attribute("Type"), Some("0"));
    assert_eq!(marks[1].attribute("Start"), Some("95.250"));
    assert_eq!(marks[1].attribute("Num"), Some("2"));
}

#[test]
fn key_notation_chooses_the_tonality_text() {
    let expectations = [
        (Key::new(9, Mode::Minor), "8A", "Am"),
        (Key::new(0, Mode::Major), "8B", "C"),
        (Key::new(6, Mode::Minor), "11A", "F#m"),
        (Key::new(6, Mode::Major), "2B", "F#"),
        (Key::new(1, Mode::Minor), "12A", "Dbm"),
        (Key::new(8, Mode::Minor), "1A", "Abm"),
        (Key::new(10, Mode::Major), "6B", "Bb"),
    ];
    for (key, camelot, musical) in expectations {
        let library = single_track_library(Track {
            key: Some(key),
            ..untagged_track("/Music/keyed.mp3")
        });
        for (key_notation, tonality) in [
            (KeyNotation::Camelot, camelot),
            (KeyNotation::Musical, musical),
        ] {
            let xml = export_with(&library, &ExportOptions { key_notation });
            let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
            assert_eq!(
                collection_tracks(&document)[0].attribute("Tonality"),
                Some(tonality),
                "{key} as {key_notation:?}"
            );
        }
    }
}

#[test]
fn default_key_notation_is_camelot() {
    assert_eq!(ExportOptions::default().key_notation, KeyNotation::Camelot);
}

#[test]
fn playlists_reference_tracks_by_their_track_id() {
    let xml = export(&golden_library());
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");

    let location_of_track_id = |track_id: &str| {
        collection_tracks(&document)
            .into_iter()
            .find(|track| track.attribute("TrackID") == Some(track_id))
            .and_then(|track| track.attribute("Location"))
            .expect("the key names a track of the collection")
            .to_string()
    };
    let playlist_locations = |name: &str| -> Vec<String> {
        let playlist = document
            .descendants()
            .find(|node| node.has_tag_name("NODE") && node.attribute("Name") == Some(name))
            .expect("the playlist exists");
        assert_eq!(playlist.attribute("Type"), Some("1"));
        assert_eq!(playlist.attribute("KeyType"), Some("0"));
        let locations: Vec<String> = playlist
            .children()
            .filter(|node| node.has_tag_name("TRACK"))
            .map(|entry| location_of_track_id(entry.attribute("Key").unwrap()))
            .collect();
        assert_eq!(
            playlist.attribute("Entries"),
            Some(locations.len().to_string().as_str())
        );
        locations
    };

    let around_the_world = exported_location(AROUND_THE_WORLD);
    let theme_remix = exported_location(THEME_REMIX);
    let warm_up = exported_location(WARM_UP);
    assert_eq!(
        playlist_locations("Peak <Time>"),
        [theme_remix.clone(), around_the_world.clone()]
    );
    assert_eq!(
        playlist_locations("Everything"),
        [around_the_world, theme_remix, warm_up]
    );
    assert!(playlist_locations("Unsorted").is_empty());
}

#[test]
fn folders_count_their_direct_children() {
    let xml = export(&golden_library());
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
    let folder_count = |name: &str| {
        let folder = document
            .descendants()
            .find(|node| node.has_tag_name("NODE") && node.attribute("Name") == Some(name))
            .expect("the folder exists");
        assert_eq!(folder.attribute("Type"), Some("0"));
        let children = folder
            .children()
            .filter(roxmltree::Node::is_element)
            .count();
        assert_eq!(
            folder.attribute("Count"),
            Some(children.to_string().as_str())
        );
        children
    };

    assert_eq!(folder_count("ROOT"), 3);
    assert_eq!(folder_count("Club & Bar"), 2);
    assert_eq!(folder_count("Later"), 0);
}

#[test]
fn text_survives_a_round_trip_through_an_xml_parser() {
    let title = "5 < 6 & 7 > \"six\" 'n' <!-- --> ]]> &amp;\ttab\nline\rreturn";
    let track = Track {
        title: Some(title.to_string()),
        artist: Some("bell\u{7}and null\u{0}removed".to_string()),
        ..untagged_track("/Music/text.mp3")
    };
    let xml = export(&single_track_library(track));
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");
    let exported = collection_tracks(&document)[0];

    assert_eq!(exported.attribute("Name"), Some(title));
    assert_eq!(exported.attribute("Artist"), Some("belland nullremoved"));
}

#[test]
fn empty_library_is_a_complete_document() {
    let xml = export(&Library::default());
    let document = roxmltree::Document::parse(&xml).expect("the export is well formed");

    assert!(collection_tracks_or_empty(&document).is_empty());
    let root_node = document
        .descendants()
        .find(|node| node.has_tag_name("NODE"))
        .expect("the playlist tree has a root");
    assert_eq!(root_node.attribute("Name"), Some("ROOT"));
    assert_eq!(root_node.attribute("Count"), Some("0"));
}

fn collection_tracks_or_empty<'input>(
    document: &'input roxmltree::Document<'input>,
) -> Vec<roxmltree::Node<'input, 'input>> {
    let collection = document
        .descendants()
        .find(|node| node.has_tag_name("COLLECTION"))
        .expect("the document has a COLLECTION");
    assert_eq!(collection.attribute("Entries"), Some("0"));
    collection
        .children()
        .filter(roxmltree::Node::is_element)
        .collect()
}

#[test]
fn relative_and_network_paths_are_refused() {
    for path in [
        "Music/relative.mp3",
        "relative.mp3",
        "",
        r"\\server\share\a.mp3",
        "C:relative.mp3",
    ] {
        let library = single_track_library(untagged_track(path));
        assert_eq!(
            export_problem(&library),
            (PathBuf::from(path), ExportProblem::NotAbsolutePath),
            "{path:?}"
        );
    }
}

#[test]
fn a_path_listed_twice_is_refused() {
    let library = Library {
        tracks: vec![
            untagged_track("/Music/once.mp3"),
            untagged_track("/Music/twice.mp3"),
            untagged_track("/Music/twice.mp3"),
        ],
        playlists: Vec::new(),
    };
    assert_eq!(
        export_problem(&library),
        (
            PathBuf::from("/Music/twice.mp3"),
            ExportProblem::DuplicatePath
        )
    );
}

#[test]
fn a_playlist_entry_outside_the_collection_is_refused() {
    let library = Library {
        tracks: vec![untagged_track("/Music/known.mp3")],
        playlists: vec![PlaylistNode::Folder(Folder {
            name: "Folder".to_string(),
            children: vec![PlaylistNode::Playlist(Playlist {
                name: "Playlist".to_string(),
                tracks: vec![
                    PathBuf::from("/Music/known.mp3"),
                    PathBuf::from("/Music/missing.mp3"),
                ],
            })],
        })],
    };
    assert_eq!(
        export_problem(&library),
        (
            PathBuf::from("/Music/missing.mp3"),
            ExportProblem::NotInCollection
        )
    );
}

#[test]
fn a_tempo_that_is_not_a_positive_number_is_refused() {
    for bpm in [0.0, -120.0, f64::NAN, f64::INFINITY] {
        let library = single_track_library(Track {
            bpm: Some(bpm),
            ..untagged_track("/Music/tempo.mp3")
        });
        assert_eq!(
            export_problem(&library),
            (
                PathBuf::from("/Music/tempo.mp3"),
                ExportProblem::InvalidTempo
            ),
            "{bpm}"
        );
    }
}

struct FullDisk;

impl io::Write for FullDisk {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::StorageFull, "disk is full"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_failing_writer_is_reported_with_its_cause() {
    let error = write_xml(FullDisk, &golden_library(), &ExportOptions::default())
        .expect_err("the writer fails");

    assert!(matches!(
        &error,
        EuphoriaError::ExportWrite { source } if source.kind() == io::ErrorKind::StorageFull
    ));
    assert_eq!(error.chain(), "cannot write the export: disk is full");
}

#[test]
fn export_errors_name_the_path_and_the_problem() {
    let library = single_track_library(untagged_track("relative.mp3"));
    let error = write_xml(Vec::new(), &library, &ExportOptions::default())
        .expect_err("the library is refused");
    assert_eq!(
        error.to_string(),
        "cannot export relative.mp3: not an absolute local path"
    );
}
