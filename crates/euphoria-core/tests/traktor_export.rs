// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Traktor NML export: a library written and parsed back holds what it
//! was given, the document uses only the elements and attributes of a real
//! Traktor Pro 4 collection, and the libraries Traktor cannot hold are
//! refused.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use euphoria_core::export::rekordbox::{
    CueKind, CuePoint, Folder, Library, Playlist, PlaylistNode, Track,
};
use euphoria_core::export::traktor::{
    HOT_CUE_PADS, ModifiedTime, TraktorOptions, key_from_musical_key_value, write_nml,
};
use euphoria_core::tags::KeyNotation;
use euphoria_core::{EuphoriaError, ExportProblem, Key, Mode};

const ON_THE_STARTUP_DISK: &str = "/Users/dj/Music/Daft Punk - Around the World.mp3";
const ON_A_STICK: &str = "/Volumes/USB Stick/Sets/Beyoncé & JAY-Z/100% #1 <Remix>.flac";
const ON_WINDOWS: &str = r"C:\Music\Sets\Warm Up.wav";

fn options() -> TraktorOptions {
    TraktorOptions {
        key_notation: KeyNotation::Camelot,
        startup_volume: "Macintosh HD".to_string(),
        modified: ModifiedTime {
            year: 2026,
            month: 11,
            day: 11,
            seconds_of_day: 3_600,
        },
    }
}

fn cue(name: &str, milliseconds: u64, kind: CueKind) -> CuePoint {
    CuePoint {
        name: name.to_string(),
        time: Duration::from_millis(milliseconds),
        kind,
    }
}

fn library() -> Library {
    let around_the_world = Track {
        title: Some("Around the World".to_string()),
        artist: Some("Daft Punk".to_string()),
        key: Some(Key::new(9, Mode::Minor)),
        bpm: Some(121.297_531),
        comment: Some("Energy 7".to_string()),
        first_beat: Some(Duration::from_millis(146)),
        cue_points: vec![
            cue("Start", 146, CueKind::Memory),
            cue("Start", 146, CueKind::Hot(0)),
            cue("Drop", 63_500, CueKind::Memory),
            cue("Drop", 63_500, CueKind::Hot(1)),
            cue("Mine", 90_000, CueKind::Memory),
            cue("Outro", 400_250, CueKind::Hot(7)),
        ],
        ..Track::new(ON_THE_STARTUP_DISK, Duration::from_millis(429_600))
    };
    let theme_remix = Track {
        title: Some("Tom & Jerry's \"Theme\" <Remix>".to_string()),
        artist: Some("Beyoncé".to_string()),
        key: Some(Key::new(6, Mode::Major)),
        bpm: Some(98.0),
        ..Track::new(ON_A_STICK, Duration::from_secs(200))
    };
    let warm_up = Track::new(ON_WINDOWS, Duration::from_millis(61_400));
    Library {
        tracks: vec![around_the_world, theme_remix, warm_up],
        playlists: vec![
            PlaylistNode::Folder(Folder {
                name: "Club & Bar".to_string(),
                children: vec![
                    PlaylistNode::Playlist(Playlist {
                        name: "Peak <Time>".to_string(),
                        tracks: vec![
                            PathBuf::from(ON_A_STICK),
                            PathBuf::from(ON_THE_STARTUP_DISK),
                        ],
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
                    PathBuf::from(ON_THE_STARTUP_DISK),
                    PathBuf::from(ON_A_STICK),
                    PathBuf::from(ON_WINDOWS),
                ],
            }),
        ],
    }
}

fn export(library: &Library, options: &TraktorOptions) -> String {
    let mut output = Vec::new();
    write_nml(&mut output, library, options).expect("the library exports");
    String::from_utf8(output).expect("the export is UTF-8")
}

/// What a track looks like once read back from an NML file.
#[derive(Debug, PartialEq)]
struct ReadTrack {
    primary_key: String,
    title: Option<String>,
    artist: Option<String>,
    playtime_milliseconds: u64,
    key_text: Option<String>,
    musical_key: Option<Key>,
    bpm_micro: Option<u64>,
    comment: Option<String>,
    grid: Option<(u64, u64)>,
    cues: Vec<(String, u64, i16)>,
}

fn micro(value: f64) -> u64 {
    (value * 1_000_000.0).round() as u64
}

fn parse_back(document: &str) -> (Vec<ReadTrack>, Vec<String>) {
    let document = roxmltree::Document::parse(document).expect("the export is well formed");
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "NML");
    let collection = root
        .children()
        .find(|node| node.has_tag_name("COLLECTION"))
        .expect("the export has the expected element or value");
    let entries: Vec<_> = collection
        .children()
        .filter(|node| node.has_tag_name("ENTRY"))
        .collect();
    assert_eq!(
        collection.attribute("ENTRIES"),
        Some(entries.len().to_string().as_str())
    );
    fn child<'input>(
        entry: roxmltree::Node<'input, 'input>,
        name: &str,
    ) -> Option<roxmltree::Node<'input, 'input>> {
        entry.children().find(|node| node.has_tag_name(name))
    }
    let tracks = entries
        .iter()
        .map(|entry| {
            let location =
                child(*entry, "LOCATION").expect("the export has the expected element or value");
            let info = child(*entry, "INFO").expect("the export has the expected element or value");
            let cues: Vec<_> = entry
                .children()
                .filter(|node| node.has_tag_name("CUE_V2"))
                .collect();
            let start = |cue: &roxmltree::Node<'_, '_>| {
                micro(
                    cue.attribute("START")
                        .expect("START is present")
                        .parse::<f64>()
                        .expect("a numeric attribute")
                        / 1000.0,
                )
            };
            ReadTrack {
                primary_key: format!(
                    "{}{}{}",
                    location.attribute("VOLUME").expect("VOLUME is present"),
                    location.attribute("DIR").expect("DIR is present"),
                    location.attribute("FILE").expect("FILE is present")
                ),
                title: entry.attribute("TITLE").map(str::to_string),
                artist: entry.attribute("ARTIST").map(str::to_string),
                playtime_milliseconds: (info
                    .attribute("PLAYTIME_FLOAT")
                    .expect("the export has the expected element or value")
                    .parse::<f64>()
                    .expect("the export has the expected element or value")
                    * 1000.0)
                    .round() as u64,
                key_text: info.attribute("KEY").map(str::to_string),
                musical_key: child(*entry, "MUSICAL_KEY").map(|node| {
                    key_from_musical_key_value(
                        node.attribute("VALUE")
                            .expect("VALUE is present")
                            .parse()
                            .expect("a valid numeric attribute"),
                    )
                    .expect("the export has the expected element or value")
                }),
                bpm_micro: child(*entry, "TEMPO").map(|node| {
                    micro(
                        node.attribute("BPM")
                            .expect("BPM is present")
                            .parse()
                            .expect("a valid numeric attribute"),
                    )
                }),
                comment: info.attribute("COMMENT").map(str::to_string),
                grid: cues
                    .iter()
                    .find(|cue| cue.attribute("TYPE") == Some("4"))
                    .map(|cue| {
                        let grid = child(*cue, "GRID")
                            .expect("the export has the expected element or value");
                        (
                            start(cue),
                            micro(
                                grid.attribute("BPM")
                                    .expect("BPM is present")
                                    .parse()
                                    .expect("a valid numeric attribute"),
                            ),
                        )
                    }),
                cues: cues
                    .iter()
                    .filter(|cue| cue.attribute("TYPE") == Some("0"))
                    .map(|cue| {
                        (
                            cue.attribute("NAME").expect("NAME is present").to_string(),
                            start(cue),
                            cue.attribute("HOTCUE")
                                .expect("HOTCUE is present")
                                .parse()
                                .expect("a valid numeric attribute"),
                        )
                    })
                    .collect(),
            }
        })
        .collect();

    let mut playlists = Vec::new();
    fn walk(node: roxmltree::Node<'_, '_>, prefix: &str, playlists: &mut Vec<String>) {
        let name = node.attribute("NAME").expect("NAME is present");
        let path = format!("{prefix}/{name}");
        match node.attribute("TYPE") {
            Some("FOLDER") => {
                let subnodes = node
                    .children()
                    .find(|child| child.has_tag_name("SUBNODES"))
                    .expect("the export has the expected element or value");
                let children: Vec<_> = subnodes
                    .children()
                    .filter(|child| child.has_tag_name("NODE"))
                    .collect();
                assert_eq!(
                    subnodes.attribute("COUNT"),
                    Some(children.len().to_string().as_str())
                );
                playlists.push(format!("{path}/"));
                for child in children {
                    walk(child, &path, playlists);
                }
            }
            Some("PLAYLIST") => {
                let playlist = node
                    .children()
                    .find(|child| child.has_tag_name("PLAYLIST"))
                    .expect("the export has the expected element or value");
                assert_eq!(playlist.attribute("TYPE"), Some("LIST"));
                assert_eq!(
                    playlist.attribute("UUID").expect("UUID is present").len(),
                    32
                );
                let keys: Vec<&str> = playlist
                    .descendants()
                    .filter(|child| child.has_tag_name("PRIMARYKEY"))
                    .map(|key| {
                        assert_eq!(key.attribute("TYPE"), Some("TRACK"));
                        key.attribute("KEY").expect("KEY is present")
                    })
                    .collect();
                assert_eq!(
                    playlist.attribute("ENTRIES"),
                    Some(keys.len().to_string().as_str())
                );
                playlists.push(format!("{path} = {}", keys.join(" | ")));
            }
            other => panic!("unexpected node type {other:?}"),
        }
    }
    let root_node = root
        .children()
        .find(|node| node.has_tag_name("PLAYLISTS"))
        .expect("the export has the expected element or value")
        .children()
        .find(|node| node.has_tag_name("NODE"))
        .expect("the export has the expected element or value");
    assert_eq!(root_node.attribute("NAME"), Some("$ROOT"));
    walk(root_node, "", &mut playlists);
    (tracks, playlists)
}

#[test]
fn a_library_read_back_holds_what_was_exported() {
    let (tracks, playlists) = parse_back(&export(&library(), &options()));

    let startup_key = "Macintosh HD/:Users/:dj/:Music/:Daft Punk - Around the World.mp3";
    let stick_key = "USB Stick/:Sets/:Beyoncé & JAY-Z/:100% #1 <Remix>.flac";
    let windows_key = "C:/:Music/:Sets/:Warm Up.wav";
    assert_eq!(
        tracks,
        vec![
            ReadTrack {
                primary_key: startup_key.to_string(),
                title: Some("Around the World".to_string()),
                artist: Some("Daft Punk".to_string()),
                playtime_milliseconds: 429_600,
                key_text: Some("8A".to_string()),
                musical_key: Some(Key::new(9, Mode::Minor)),
                bpm_micro: Some(121_297_531),
                comment: Some("Energy 7".to_string()),
                grid: Some((146_000, 121_297_531)),
                // The memory cues that repeat a hot cue are left out.
                cues: vec![
                    ("Start".to_string(), 146_000, 0),
                    ("Drop".to_string(), 63_500_000, 1),
                    ("Mine".to_string(), 90_000_000, -1),
                    ("Outro".to_string(), 400_250_000, 7),
                ],
            },
            ReadTrack {
                primary_key: stick_key.to_string(),
                title: Some("Tom & Jerry's \"Theme\" <Remix>".to_string()),
                artist: Some("Beyoncé".to_string()),
                playtime_milliseconds: 200_000,
                key_text: Some("2B".to_string()),
                musical_key: Some(Key::new(6, Mode::Major)),
                bpm_micro: Some(98_000_000),
                comment: None,
                grid: None,
                cues: Vec::new(),
            },
            ReadTrack {
                primary_key: windows_key.to_string(),
                title: None,
                artist: None,
                playtime_milliseconds: 61_400,
                key_text: None,
                musical_key: None,
                bpm_micro: None,
                comment: None,
                grid: None,
                cues: Vec::new(),
            },
        ]
    );
    assert_eq!(
        playlists,
        vec![
            "/$ROOT/".to_string(),
            "/$ROOT/Club & Bar/".to_string(),
            format!("/$ROOT/Club & Bar/Peak <Time> = {stick_key} | {startup_key}"),
            "/$ROOT/Club & Bar/Later/".to_string(),
            format!("/$ROOT/Everything = {startup_key} | {stick_key} | {windows_key}"),
        ]
    );
}

#[test]
fn every_notation_is_written_in_key_text_and_the_key_column_is_the_same() {
    let a_minor = Track {
        key: Some(Key::new(9, Mode::Minor)),
        ..Track::new("/a.mp3", Duration::from_secs(1))
    };
    let library = Library {
        tracks: vec![a_minor],
        playlists: Vec::new(),
    };
    for (notation, text) in [
        (KeyNotation::Camelot, "8A"),
        (KeyNotation::Musical, "Am"),
        (KeyNotation::OpenKey, "1m"),
    ] {
        let options = TraktorOptions {
            key_notation: notation,
            ..options()
        };
        let (tracks, _) = parse_back(&export(&library, &options));
        assert_eq!(tracks[0].key_text.as_deref(), Some(text));
        assert_eq!(tracks[0].musical_key, Some(Key::new(9, Mode::Minor)));
    }
}

#[test]
fn a_small_tempo_that_stays_positive_at_six_decimals_is_kept() {
    let track = Track {
        bpm: Some(0.000_001),
        first_beat: Some(Duration::from_millis(100)),
        ..Track::new("/a.mp3", Duration::from_secs(10))
    };
    let text = export(&one_track(track), &options());
    let document =
        roxmltree::Document::parse(&text).expect("the export has the expected element or value");
    for name in ["TEMPO", "GRID"] {
        let element = document
            .descendants()
            .find(|node| node.has_tag_name(name))
            .expect("the export has the expected element or value");
        assert_eq!(element.attribute("BPM"), Some("0.000001"));
    }
}

#[test]
fn a_renamed_startup_disk_and_the_same_playlist_again_are_stable() {
    let options = TraktorOptions {
        startup_volume: "Studio".to_string(),
        ..options()
    };
    let first = export(&library(), &options);
    let (tracks, _) = parse_back(&first);
    assert!(tracks[0].primary_key.starts_with("Studio/:Users/:"));
    assert_eq!(first, export(&library(), &options));
}

/// The elements and attributes found in the Traktor Pro 4 `collection.nml`
/// of traktor-nml-utils
/// (<https://github.com/wolkenarchitekt/traktor-nml-utils/blob/master/xml_to_xsd/collection.nml>),
/// for the elements the export writes: (parent, element, attributes).
const REAL_TRAKTOR_VOCABULARY: &[(&str, &str, &[&str])] = &[
    ("", "NML", &["VERSION"]),
    ("NML", "COLLECTION", &["ENTRIES"]),
    (
        "COLLECTION",
        "ENTRY",
        &[
            "ARTIST",
            "AUDIO_ID",
            "LOCK",
            "LOCK_MODIFICATION_TIME",
            "MODIFIED_DATE",
            "MODIFIED_TIME",
            "TITLE",
        ],
    ),
    ("ENTRY", "LOCATION", &["DIR", "FILE", "VOLUME", "VOLUMEID"]),
    (
        "ENTRY",
        "INFO",
        &[
            "BITRATE",
            "COMMENT",
            "COVERARTID",
            "FILESIZE",
            "FLAGS",
            "GENRE",
            "IMPORT_DATE",
            "KEY",
            "KEY_LYRICS",
            "LABEL",
            "LAST_PLAYED",
            "PLAYCOUNT",
            "PLAYTIME",
            "PLAYTIME_FLOAT",
            "PRODUCER",
            "RANKING",
            "RATING",
            "RELEASE_DATE",
            "REMIXER",
        ],
    ),
    (
        "ENTRY",
        "TEMPO",
        &["BPM", "BPM_QUALITY", "BPM_TRANSIENTCOHERENCE"],
    ),
    ("ENTRY", "MUSICAL_KEY", &["VALUE"]),
    (
        "ENTRY",
        "CUE_V2",
        &[
            "COLOR",
            "DISPL_ORDER",
            "HOTCUE",
            "LEN",
            "NAME",
            "REPEATS",
            "START",
            "TYPE",
        ],
    ),
    ("CUE_V2", "GRID", &["BPM"]),
    ("NML", "PLAYLISTS", &[]),
    ("PLAYLISTS", "NODE", &["NAME", "TYPE"]),
    ("SUBNODES", "NODE", &["NAME", "TYPE"]),
    ("NODE", "SUBNODES", &["COUNT"]),
    ("NODE", "PLAYLIST", &["ENTRIES", "TYPE", "UUID"]),
    ("PLAYLIST", "ENTRY", &[]),
    ("ENTRY", "PRIMARYKEY", &["KEY", "TYPE"]),
];

#[test]
fn the_export_uses_only_the_elements_and_attributes_of_a_real_collection() {
    let known: BTreeMap<(&str, &str), BTreeSet<&str>> = REAL_TRAKTOR_VOCABULARY
        .iter()
        .map(|(parent, element, attributes)| {
            ((*parent, *element), attributes.iter().copied().collect())
        })
        .collect();
    let document = export(&library(), &options());
    let document = roxmltree::Document::parse(&document)
        .expect("the export has the expected element or value");
    for node in document.descendants().filter(|node| node.is_element()) {
        let parent = node
            .parent_element()
            .map(|parent| parent.tag_name().name())
            .unwrap_or("");
        let element = node.tag_name().name();
        let attributes = known
            .get(&(parent, element))
            .unwrap_or_else(|| panic!("{element} inside {parent:?} is not in a real collection"));
        for attribute in node.attributes() {
            assert!(
                attributes.contains(attribute.name()),
                "{element} has {}, which a real collection does not",
                attribute.name()
            );
        }
    }
}

fn refusal(library: &Library) -> (PathBuf, ExportProblem) {
    let mut output = Vec::new();
    let error = write_nml(&mut output, library, &options()).expect_err("the library is refused");
    assert!(output.is_empty(), "a refused library writes nothing");
    match error {
        EuphoriaError::Export { path, problem } => (path, problem),
        other => panic!("expected an export error, got {other:?}"),
    }
}

fn one_track(track: Track) -> Library {
    Library {
        tracks: vec![track],
        playlists: Vec::new(),
    }
}

#[test]
fn libraries_traktor_cannot_hold_are_refused() {
    let past_the_last_pad = Track {
        cue_points: vec![cue("Late", 1_000, CueKind::Hot(HOT_CUE_PADS))],
        ..Track::new("/a.mp3", Duration::from_secs(10))
    };
    assert_eq!(
        refusal(&one_track(past_the_last_pad)),
        (PathBuf::from("/a.mp3"), ExportProblem::InvalidHotCue)
    );

    let no_tempo = Track {
        bpm: Some(f64::NAN),
        ..Track::new("/a.mp3", Duration::from_secs(10))
    };
    assert_eq!(refusal(&one_track(no_tempo)).1, ExportProblem::InvalidTempo);

    let rounds_to_zero = Track {
        bpm: Some(0.000_000_1),
        first_beat: Some(Duration::from_millis(100)),
        ..Track::new("/a.mp3", Duration::from_secs(10))
    };
    assert_eq!(
        refusal(&one_track(rounds_to_zero)).1,
        ExportProblem::InvalidTempo
    );

    assert_eq!(
        refusal(&one_track(Track::new(
            "Music/a.mp3",
            Duration::from_secs(1)
        )))
        .1,
        ExportProblem::NotAbsolutePath
    );

    // The startup disk is reachable under /Volumes too, and Traktor names
    // both paths the same way.
    let twice = Library {
        tracks: vec![
            Track::new("/a.mp3", Duration::from_secs(1)),
            Track::new("/Volumes/Macintosh HD/a.mp3", Duration::from_secs(1)),
        ],
        playlists: Vec::new(),
    };
    assert_eq!(
        refusal(&twice),
        (
            PathBuf::from("/Volumes/Macintosh HD/a.mp3"),
            ExportProblem::DuplicatePath
        )
    );

    let not_in_collection = Library {
        tracks: Vec::new(),
        playlists: vec![PlaylistNode::Playlist(Playlist {
            name: "Lost".to_string(),
            tracks: vec![PathBuf::from("/b.mp3")],
        })],
    };
    assert_eq!(
        refusal(&not_in_collection).1,
        ExportProblem::NotInCollection
    );
}

#[cfg(unix)]
#[test]
fn a_file_name_xml_cannot_represent_is_refused_instead_of_changed() {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("traktor-name-\u{1}.mp3");
    std::fs::write(&path, b"scratch audio").expect("the scratch file can be written");
    let mut document = Vec::new();
    let result = write_nml(
        &mut document,
        &one_track(Track::new(&path, Duration::from_secs(1))),
        &options(),
    );
    std::fs::remove_file(&path).expect("the scratch file can be removed");
    assert!(matches!(
        result,
        Err(EuphoriaError::Export {
            problem: ExportProblem::PathNotXml,
            ..
        })
    ));
    assert!(document.is_empty());
}

#[test]
fn xml_whitespace_in_file_names_is_preserved() {
    let file = "name\t\n\r.mp3";
    let library = one_track(Track::new(format!("/Music/{file}"), Duration::from_secs(1)));
    let text = export(&library, &options());
    let document =
        roxmltree::Document::parse(&text).expect("the export has the expected element or value");
    let location = document
        .descendants()
        .find(|node| node.has_tag_name("LOCATION"))
        .expect("the export has the expected element or value");
    assert_eq!(location.attribute("FILE"), Some(file));
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_unicode_is_refused() {
    use std::os::unix::ffi::OsStrExt;
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/Music/\xff.mp3"));
    assert_eq!(
        refusal(&one_track(Track::new(path, Duration::from_secs(1)))).1,
        ExportProblem::PathNotUnicode
    );
}
