// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Round trips of the VirtualDJ database merge.

use std::path::{Path, PathBuf};
use std::time::Duration;

use euphoria_core::EuphoriaError;
use euphoria_core::export::rekordbox::{CueKind, CuePoint, Track};
use euphoria_core::export::virtualdj::{
    DatabaseMerge, VirtualDjOptions, VirtualDjTrack, bpm_from_seconds_per_beat, merge_database,
    update_database_file,
};
use euphoria_core::tags::{KeyNotation, LabelPlacement};

const BEFORE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<VirtualDJ_Database Version="2026">
 <Song FilePath="/music/a.mp3" FileSize="1">
  <Tags Author="A" Title="T" Flag="1" />
  <Infos SongLength="300.0" PlayCount="7" />
  <Comment>my note</Comment>
  <Scan Version="801" Bpm="0.500000" Key="C" Flag="32768" />
  <Poi Pos="0.100000" Type="beatgrid" />
  <Poi Pos="10.000000" Type="automix" Point="fadeStart" />
 </Song>
</VirtualDJ_Database>
"#;

const AFTER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<VirtualDJ_Database Version="2026">
 <Song FilePath="/music/a.mp3" FileSize="1">
  <Tags Author="A" Title="T" Flag="1" Bpm="0.468750" Key="Am" />
  <Infos SongLength="300.0" PlayCount="7" />
  <Comment>8A - Energy 7 - my note</Comment>
  <Scan Version="801" Bpm="0.468750" Key="Am" Flag="32768" Phase="0.250000" />
  <Poi Pos="0.250000" Type="beatgrid" />
  <Poi Pos="10.000000" Type="automix" Point="fadeStart" />
  <Poi Name="Drop" Pos="30.000000" Num="2" />
 </Song>
</VirtualDJ_Database>
"#;

fn cue(name: &str, seconds: u64, kind: CueKind) -> CuePoint {
    CuePoint {
        name: name.to_string(),
        time: Duration::from_secs(seconds),
        kind,
    }
}

fn track_at(path: &str) -> Track {
    Track {
        key: Some("A minor".parse().expect("a key")),
        bpm: Some(128.0),
        first_beat: Some(Duration::from_millis(250)),
        cue_points: vec![
            cue("Drop", 30, CueKind::Memory),
            cue("Drop", 30, CueKind::Hot(1)),
        ],
        ..Track::new(path, Duration::from_secs(300))
    }
}

fn merge_with(database: &str, tracks: &[&Track], options: VirtualDjOptions) -> DatabaseMerge {
    let entries: Vec<VirtualDjTrack<'_>> = tracks
        .iter()
        .map(|track| VirtualDjTrack {
            track,
            energy: Some(7),
        })
        .collect();
    merge_database(
        database,
        Path::new("database.xml"),
        &entries,
        &options,
        |_| None,
    )
    .expect("the database is merged")
}

fn merge(database: &str, track: &Track) -> DatabaseMerge {
    merge_with(database, &[track], VirtualDjOptions::default())
}

fn first_song<'document>(
    document: &'document roxmltree::Document<'document>,
) -> roxmltree::Node<'document, 'document> {
    document
        .descendants()
        .find(|node| node.has_tag_name("Song"))
        .expect("a song")
}

fn child<'document>(
    song: roxmltree::Node<'document, 'document>,
    name: &str,
) -> roxmltree::Node<'document, 'document> {
    song.children()
        .find(|node| node.has_tag_name(name))
        .unwrap_or_else(|| panic!("a {name} element"))
}

#[test]
fn the_example_database_is_updated_exactly() {
    let track = track_at("/music/a.mp3");
    let merged = merge(BEFORE, &track);
    assert_eq!(merged.document, AFTER);
    assert_eq!(merged.updated, vec![PathBuf::from("/music/a.mp3")]);
    assert!(merged.not_in_database.is_empty());
    assert_eq!(merged.cues_without_pad, 0);
}

#[test]
fn reading_back_gives_the_values_written() {
    let track = track_at("/music/a.mp3");
    let merged = merge(BEFORE, &track);
    let document = roxmltree::Document::parse(&merged.document).expect("well-formed XML");
    let song = first_song(&document);
    let scan = child(song, "Scan");
    let bpm = bpm_from_seconds_per_beat(scan.attribute("Bpm").expect("a tempo")).expect("a tempo");
    assert!((bpm - 128.0).abs() < 0.0001, "{bpm}");
    assert_eq!(scan.attribute("Key"), Some("Am"));
    assert_eq!(child(song, "Tags").attribute("Key"), Some("Am"));
    let grids: Vec<f64> = song
        .children()
        .filter(|node| node.attribute("Type") == Some("beatgrid"))
        .map(|node| {
            node.attribute("Pos")
                .expect("a position")
                .parse()
                .expect("a number")
        })
        .collect();
    assert_eq!(grids, vec![0.25]);
}

#[test]
fn unknown_elements_attributes_and_other_songs_are_kept_byte_for_byte() {
    let changed_song = " <Song FilePath=\"/music/a.mp3\" FileSize=\"1\" Future=\"x\">\r\n  <Tags Title=\"T\" Mood=\"dark\" />\r\n  <Future A=\"1\"><Inner/></Future>\r\n  <CustomMix>x</CustomMix>\r\n </Song>";
    let other_song = " <Song FilePath=\"/music/b.mp3\" FileSize=\"2\">\r\n  <Tags Title=\"B\" Key=\"C\" />\r\n </Song>";
    let database = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<VirtualDJ_Database Version=\"8.4\">\r\n{changed_song}\r\n <!-- note -->\r\n{other_song}\r\n</VirtualDJ_Database>\r\n"
    );
    let track = track_at("/music/a.mp3");
    let merged = merge(&database, &track);
    let output = &merged.document;
    assert!(output.contains(other_song));
    assert!(output.contains("<!-- note -->"));
    assert!(output.contains("<Future A=\"1\"><Inner/></Future>"));
    assert!(output.contains("<CustomMix>x</CustomMix>"));
    assert!(output.contains("Future=\"x\""));
    assert!(output.contains("Mood=\"dark\""));
    assert!(
        !output.replace("\r\n", "").contains('\n'),
        "every line ends with CR LF"
    );
    let outside = |text: &str| {
        let start = text
            .find("<Song FilePath=\"/music/a.mp3\"")
            .expect("the song");
        let end = text[start..].find("</Song>").expect("its end") + start + "</Song>".len();
        format!("{}{}", &text[..start], &text[end..])
    };
    assert_eq!(outside(output), outside(&database));
}

#[test]
fn exporting_twice_changes_nothing_the_second_time() {
    let track = track_at("/music/a.mp3");
    let once = merge(BEFORE, &track);
    let twice = merge(&once.document, &track);
    assert_eq!(once.document, twice.document);
    assert_eq!(twice.document.matches("Energy 7").count(), 1);
}

#[test]
fn a_song_with_its_own_cues_keeps_them() {
    let database = BEFORE.replace(
        "  <Poi Pos=\"10.000000\"",
        "  <Poi Name=\"Mine\" Pos=\"5.000000\" Num=\"1\" />\n  <Poi Pos=\"10.000000\"",
    );
    let track = track_at("/music/a.mp3");
    let merged = merge(&database, &track);
    assert!(!merged.document.contains("Name=\"Drop\""));
    assert!(merged.document.contains("Name=\"Mine\""));
    assert!(
        merged
            .document
            .contains("<Poi Pos=\"0.250000\" Type=\"beatgrid\" />")
    );
    assert_eq!(merged.kept_own_cues, vec![PathBuf::from("/music/a.mp3")]);
}

#[test]
fn memory_cues_without_a_pad_are_counted_and_left_out() {
    let track = Track {
        cue_points: vec![cue("Intro", 4, CueKind::Memory)],
        ..track_at("/music/a.mp3")
    };
    let merged = merge(BEFORE, &track);
    assert_eq!(merged.cues_without_pad, 1);
    assert!(!merged.document.contains("Intro"));
}

#[test]
fn label_placements() {
    let track = track_at("/music/a.mp3");
    let with_label = |label, label_notation, database: &str| {
        merge_with(
            database,
            &[&track],
            VirtualDjOptions {
                label,
                label_notation,
            },
        )
        .document
    };
    assert!(
        with_label(LabelPlacement::CommentEnd, KeyNotation::Camelot, BEFORE)
            .contains("<Comment>my note - 8A - Energy 7</Comment>")
    );
    let grouping = with_label(LabelPlacement::Grouping, KeyNotation::Camelot, BEFORE);
    assert!(grouping.contains("Grouping=\"8A - Energy 7\""));
    assert!(grouping.contains("<Comment>my note</Comment>"));
    assert!(
        with_label(LabelPlacement::None, KeyNotation::Camelot, BEFORE)
            .contains("<Comment>my note</Comment>")
    );
    let without_comment = BEFORE.replace("  <Comment>my note</Comment>\n", "");
    assert!(
        with_label(
            LabelPlacement::CommentStart,
            KeyNotation::Camelot,
            &without_comment
        )
        .contains(
            "  <Infos SongLength=\"300.0\" PlayCount=\"7\" />\n  <Comment>8A - Energy 7</Comment>\n"
        )
    );
    assert!(
        with_label(LabelPlacement::CommentStart, KeyNotation::Musical, BEFORE)
            .contains("<Comment>Am - Energy 7 - my note</Comment>")
    );
}

#[test]
fn songs_are_matched_by_path_and_others_are_reported() {
    let elsewhere = track_at("/elsewhere/z.mp3");
    let merged = merge(BEFORE, &elsewhere);
    assert_eq!(merged.document, BEFORE);
    assert_eq!(
        merged.not_in_database,
        vec![PathBuf::from("/elsewhere/z.mp3")]
    );

    let windows = BEFORE.replace("/music/a.mp3", r"C:\Music\A.mp3");
    let merged = merge(&windows, &track_at(r"c:\music\a.mp3"));
    assert_eq!(merged.updated.len(), 1);

    let linked = BEFORE.replace("/music/a.mp3", "/link/a.mp3");
    let real = track_at("/real/a.mp3");
    let entries = [VirtualDjTrack {
        track: &real,
        energy: None,
    }];
    let merged = merge_database(
        &linked,
        Path::new("database.xml"),
        &entries,
        &VirtualDjOptions::default(),
        |file_path| (file_path == "/link/a.mp3").then(|| PathBuf::from("/real/a.mp3")),
    )
    .expect("the database is merged");
    assert_eq!(merged.updated, vec![PathBuf::from("/real/a.mp3")]);
}

#[test]
fn text_that_needs_escaping_is_escaped() {
    let database = BEFORE.replace("my note", "x &amp; y");
    let track = Track {
        cue_points: vec![cue("A & B <\"x\">", 30, CueKind::Hot(0))],
        ..track_at("/music/a.mp3")
    };
    let merged = merge(&database, &track);
    let document = roxmltree::Document::parse(&merged.document).expect("well-formed XML");
    let song = first_song(&document);
    assert_eq!(child(song, "Comment").text(), Some("8A - Energy 7 - x & y"));
    let names: Vec<&str> = song
        .children()
        .filter_map(|node| node.attribute("Name"))
        .collect();
    assert_eq!(names, vec!["A & B <\"x\">"]);
}

#[test]
fn a_database_that_is_not_virtualdj_is_refused() {
    let track = track_at("/music/a.mp3");
    let entries = [VirtualDjTrack {
        track: &track,
        energy: None,
    }];
    for database in ["<rekordbox/>", "not xml"] {
        let result = merge_database(
            database,
            Path::new("database.xml"),
            &entries,
            &VirtualDjOptions::default(),
            |_| None,
        );
        assert!(
            matches!(result, Err(EuphoriaError::VirtualDjDatabase { .. })),
            "{database}"
        );
    }
}

#[test]
fn a_byte_order_mark_is_kept() {
    let track = track_at("/music/a.mp3");
    let merged = merge(&format!("\u{feff}{BEFORE}"), &track);
    assert_eq!(merged.document, format!("\u{feff}{AFTER}"));
}

#[test]
fn a_song_without_children_gets_them() {
    let track = Track {
        cue_points: Vec::new(),
        first_beat: None,
        ..track_at("/music/a.mp3")
    };
    for song in [
        "<Song FilePath=\"/music/a.mp3\" FileSize=\"1\"></Song>",
        "<Song FilePath=\"/music/a.mp3\" FileSize=\"1\" />",
    ] {
        let database = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VirtualDJ_Database Version=\"8.4\">\n {song}\n</VirtualDJ_Database>\n"
        );
        let merged = merge(&database, &track);
        let document = roxmltree::Document::parse(&merged.document).expect("well-formed XML");
        let tags = child(first_song(&document), "Tags");
        assert_eq!(tags.attribute("Bpm"), Some("0.468750"), "{song}");
        assert_eq!(tags.attribute("Key"), Some("Am"), "{song}");
    }
}

struct ScratchFolder(PathBuf);

impl ScratchFolder {
    fn new(name: &str) -> Self {
        let folder =
            std::env::temp_dir().join(format!("euphoria-virtualdj-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("the folder is made");
        Self(folder)
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn update_database_file_backs_up_and_replaces() {
    let folder = ScratchFolder::new("file");
    let database = folder.0.join("database.xml");
    let backup = folder.0.join("database.xml.bak");
    std::fs::write(&database, BEFORE).expect("the database is written");
    let track = track_at("/music/a.mp3");
    let entries = [VirtualDjTrack {
        track: &track,
        energy: Some(7),
    }];
    let options = VirtualDjOptions::default();

    update_database_file(&database, &backup, &entries, &options).expect("the update");
    assert_eq!(
        std::fs::read_to_string(&backup).expect("the backup"),
        BEFORE
    );
    assert_eq!(
        std::fs::read_to_string(&database).expect("the database"),
        AFTER
    );

    let again = update_database_file(&database, &backup, &entries, &options);
    assert!(matches!(
        again,
        Err(EuphoriaError::ExportDestinationExists { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(&database).expect("the database"),
        AFTER
    );

    let unmatched_backup = folder.0.join("unmatched.bak");
    let elsewhere = track_at("/elsewhere/z.mp3");
    let unmatched = [VirtualDjTrack {
        track: &elsewhere,
        energy: None,
    }];
    let merged = update_database_file(&database, &unmatched_backup, &unmatched, &options)
        .expect("nothing to do");
    assert!(merged.updated.is_empty());
    assert!(!unmatched_backup.exists());
}
