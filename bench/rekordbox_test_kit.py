# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# requires-python = ">=3.12"
# dependencies = ["mutagen"]
# ///
"""Builds a kit for checking by hand which tags and XML values rekordbox reads.

    uv run bench/rekordbox_test_kit.py --clips bench/data/key/audio --output /tmp/euphoria-kit

The kit is a folder of short audio files cut from benchmark clips, each
tagged in one variant (a key field name, a key notation, a tempo with or
without decimals, a comment), a rekordbox XML file that lists a few of them
with chosen values, and CHECKLIST.md: the steps to follow in rekordbox and
a table to fill in. Every file's title starts with its test number, such as
"EUPHORIA T01", so the rows are easy to find in rekordbox.

The script reads the clips and writes only into the output folder, which
must not exist yet, and removes it again if building the kit fails. It never touches a rekordbox library or its settings.
It needs ffmpeg on the PATH.
"""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote
from xml.sax.saxutils import quoteattr

from mutagen.aiff import AIFF
from mutagen.flac import FLAC
from mutagen.id3 import COMM, ID3, TBPM, TIT2, TKEY
from mutagen.mp4 import MP4, AtomDataType, MP4FreeForm
from mutagen.wave import WAVE

CLIP_SECONDS = 30
LABEL = "8A - Energy 7"

# Tracks whose tags are imported. Each entry: number, file format, and the
# fields that differ from the other tracks. What each one decides is in the
# checklist.
TAG_TRACKS = [
    ("T01", "mp3", {"key": "8A", "bpm": "128", "comment": LABEL}),
    ("T02", "mp3", {"key": "Am", "bpm": "123.5"}),
    ("T03", "mp3", {"key": "1m", "bpm": "128"}),
    ("T04", "mp3-v24", {"key": "8A", "bpm": "128", "comment": LABEL}),
    ("T05", "aiff", {"key": "8A", "bpm": "128", "comment": LABEL}),
    ("T06", "m4a", {"m4a_key_field": "initialkey", "key": "8A", "bpm": "128", "comment": LABEL}),
    ("T07", "m4a", {"m4a_key_field": "KEY", "key": "8A", "bpm": "128"}),
    ("T08", "flac", {"flac_key_field": "INITIALKEY", "key": "8A", "bpm": "128", "comment": LABEL}),
    ("T09", "flac", {"flac_key_field": "KEY", "key": "8A", "bpm": "123.5"}),
    ("T10", "wav", {"key": "8A", "bpm": "128", "comment": "ID3 comment", "riff_comment": "RIFF comment"}),
]

# Tracks that reach rekordbox through the XML only, with the values the XML
# gives them. X03 is the same file as T01, with values that differ from its
# tags, to see what importing an existing track replaces.
XML_TRACKS = [
    ("X01", "mp3", {"tonality": "8A", "bpm": "123.50", "comment": LABEL}),
    ("X02", "flac", {"tonality": "Am", "bpm": "128.00", "comment": "Am - Energy 4"}),
]
EXISTING_IN_XML = ("T01", {"tonality": "3B", "bpm": "100.00", "comment": "from the xml"})


def run_ffmpeg(arguments):
    command = ["ffmpeg", "-loglevel", "error", "-y", *arguments]
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise SystemExit(f"ffmpeg failed: {' '.join(command)}\n{result.stderr}")


def cut(clip, target, file_format, metadata=()):
    codec = {
        "mp3": ["-c:a", "libmp3lame", "-b:a", "192k"],
        "m4a": ["-c:a", "aac", "-b:a", "192k"],
        "flac": ["-c:a", "flac"],
        "aiff": ["-c:a", "pcm_s16be"],
        "wav": ["-c:a", "pcm_s16le"],
    }[file_format]
    tags = [option for name, value in metadata for option in ("-metadata", f"{name}={value}")]
    run_ffmpeg(["-i", str(clip), "-t", str(CLIP_SECONDS), "-map_metadata", "-1", "-bitexact", *tags, *codec, str(target)])


def tag_id3(tags, title, values):
    tags.add(TIT2(encoding=3, text=title))
    if "key" in values:
        tags.add(TKEY(encoding=3, text=values["key"]))
    if "bpm" in values:
        tags.add(TBPM(encoding=3, text=values["bpm"]))
    if "comment" in values:
        tags.add(COMM(encoding=3, lang="eng", desc="", text=values["comment"]))


def make_tag_track(number, file_format, values, clip, folder):
    title = f"EUPHORIA {number} {describe(file_format, values)}"
    extension = {"mp3-v24": "mp3"}.get(file_format, file_format)
    target = folder / f"{number}.{extension}"
    if file_format in ("mp3", "mp3-v24"):
        cut(clip, target, "mp3")
        tags = ID3()
        tag_id3(tags, title, values)
        tags.save(target, v2_version=4 if file_format == "mp3-v24" else 3)
    elif file_format == "aiff":
        cut(clip, target, "aiff")
        audio = AIFF(target)
        audio.add_tags()
        tag_id3(audio.tags, title, values)
        audio.save(v2_version=3)
    elif file_format == "wav":
        riff = [("title", f"EUPHORIA {number} RIFF title"), ("comment", values["riff_comment"])]
        cut(clip, target, "wav", riff)
        audio = WAVE(target)
        audio.add_tags()
        tag_id3(audio.tags, title, values)
        audio.save(v2_version=3)
    elif file_format == "m4a":
        cut(clip, target, "m4a")
        audio = MP4(target)
        audio["©nam"] = [title]
        name = f"----:com.apple.iTunes:{values['m4a_key_field']}"
        audio[name] = [MP4FreeForm(values["key"].encode(), dataformat=AtomDataType.UTF8)]
        audio["tmpo"] = [round(float(values["bpm"]))]
        if "comment" in values:
            audio["©cmt"] = [values["comment"]]
        audio.save()
    elif file_format == "flac":
        cut(clip, target, "flac")
        audio = FLAC(target)
        audio["TITLE"] = title
        audio[values["flac_key_field"]] = values["key"]
        audio["BPM"] = values["bpm"]
        if "comment" in values:
            audio["COMMENT"] = values["comment"]
        audio.save()
    return target, title


def describe(file_format, values):
    parts = {"mp3": "MP3 ID3v2.3", "mp3-v24": "MP3 ID3v2.4", "aiff": "AIFF", "m4a": "M4A",
             "flac": "FLAC", "wav": "WAV"}[file_format]
    if "m4a_key_field" in values:
        parts += f" {values['m4a_key_field']}"
    if "flac_key_field" in values:
        parts += f" {values['flac_key_field']}"
    return parts


def location(path):
    return "file://localhost" + quote(str(path.resolve()), safe="/")


def track_element(track_id, path, title, values):
    attributes = [
        ("TrackID", str(track_id)),
        ("Name", title),
        ("TotalTime", str(CLIP_SECONDS)),
        ("AverageBpm", values["bpm"]),
        ("Location", location(path)),
        ("Tonality", values["tonality"]),
        ("Comments", values["comment"]),
    ]
    text = " ".join(f"{name}={quoteattr(value)}" for name, value in attributes)
    return f"    <TRACK {text}/>"


def write_xml(target, entries):
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<DJ_PLAYLISTS Version="1.0.0">',
        '  <PRODUCT Name="Euphoria test kit" Version="1" Company="The Euphoria contributors"/>',
        f'  <COLLECTION Entries="{len(entries)}">',
    ]
    for track_id, (path, title, values) in enumerate(entries, start=1):
        lines.append(track_element(track_id, path, title, values))
    lines += [
        "  </COLLECTION>",
        "  <PLAYLISTS>",
        '    <NODE Type="0" Name="ROOT" Count="1">',
        f'      <NODE Name="Euphoria test kit" Type="1" KeyType="0" Entries="{len(entries)}">',
    ]
    lines += [f'        <TRACK Key="{track_id}"/>' for track_id in range(1, len(entries) + 1)]
    lines += ["      </NODE>", "    </NODE>", "  </PLAYLISTS>", "</DJ_PLAYLISTS>", ""]
    target.write_text("\n".join(lines), encoding="utf-8")


CHECKLIST = """# rekordbox test kit

This kit answers which tags and XML values rekordbox reads, so that
Euphoria's rekordbox preset and its documentation can rest on a test
instead of guesses. It takes about ten minutes. Write what you see into
the tables and send them back.

Files: `tags/` holds the tracks T01 to T10, `xml/` holds X01 and X02, and
`rekordbox.xml` lists X01, X02 and T01. Every title starts with
`EUPHORIA` and the test number, so search the collection for `EUPHORIA`.

rekordbox version: ____________

## 1. Prepare (2 minutes)

1. **File > Library > Backup Library**, so the collection can be put back.
2. **Preferences > Analysis > Track Analysis**: note the current Auto
   Analysis setting here: ________. Then set Auto Analysis so that new
   tracks are not analysed (untick it or choose Disable). Analysis would
   hide what came from the tags.
3. **Preferences > View**: note the key display format here (Classic,
   Alphanumeric, or Display key information on the database): ________.
4. In the collection, show the **Key**, **BPM** and **Comments** columns
   (right click a column header).

## 2. Tags (4 minutes)

Drag the `tags` folder from the kit onto **Collection**. Search for
`EUPHORIA`. For each track write what the columns show, exactly, or `-`
when the column is empty. If the title differs from the one listed,
write the title shown.

| Track | What it holds | Title shown | Key | BPM | Comments |
| --- | --- | --- | --- | --- | --- |
| T01 | MP3 ID3v2.3: TKEY `8A`, TBPM `128`, COMM `8A - Energy 7` | | | | |
| T02 | MP3 ID3v2.3: TKEY `Am`, TBPM `123.5` | | | | |
| T03 | MP3 ID3v2.3: TKEY `1m`, TBPM `128` | | | | |
| T04 | MP3 ID3v2.4: TKEY `8A`, TBPM `128`, COMM `8A - Energy 7` | | | | |
| T05 | AIFF: TKEY `8A`, TBPM `128`, COMM `8A - Energy 7` | | | | |
| T06 | M4A: `initialkey` `8A`, tmpo `128`, comment `8A - Energy 7` | | | | |
| T07 | M4A: `KEY` `8A`, tmpo `128` | | | | |
| T08 | FLAC: INITIALKEY `8A`, BPM `128`, COMMENT `8A - Energy 7` | | | | |
| T09 | FLAC: KEY `8A`, BPM `123.5` | | | | |
| T10 | WAV: ID3 title `...WAV`, comment `ID3 comment`; RIFF title `...RIFF title`, comment `RIFF comment` | | | | |

Now change the key display format in **Preferences > View** to the other
of Classic and Alphanumeric, and note the Key column again:

| Track | Key with the other display format (name it: ________) |
| --- | --- |
| T01 | |
| T02 | |
| T03 | |

## 3. XML (3 minutes)

1. **Preferences > View > Layout**: tick **rekordbox xml**.
2. **Preferences > Advanced > Database > rekordbox xml**: set
   **Imported Library** to the kit's `rekordbox.xml`.
3. In the tree, open **rekordbox xml** and press its refresh button.
4. Under **rekordbox xml > Playlists**, right click **Euphoria test kit**
   and choose **Import Playlist**. Answer any question rekordbox asks,
   and write the question here: ____________
5. Look at T01 in the collection. The XML gives it Key `3B`, BPM `100.00`
   and the comment `from the xml`.

| Step | T01 Key | T01 BPM | T01 Comments |
| --- | --- | --- | --- |
| After Import Playlist | | | |

6. Under **rekordbox xml > All Tracks**, select T01, right click,
   **Import To Collection**. Write any question rekordbox asks: ____________

| Step | T01 Key | T01 BPM | T01 Comments |
| --- | --- | --- | --- |
| After Import To Collection of the track | | | |

7. X01 and X02 have no tags of their own; everything comes from the XML.

| Track | The XML gives | Key | BPM | Comments |
| --- | --- | --- | --- | --- |
| X01 | Tonality `8A`, AverageBpm `123.50`, Comments `8A - Energy 7` | | | |
| X02 | Tonality `Am`, AverageBpm `128.00`, Comments `Am - Energy 4` | | | |

## 4. Analysis (1 minute)

Select T01, right click, **Analyze Track** with **KEY** ticked in
**Preferences > Analysis > Track Analysis Setting**.

| After analysis | T01 Key | T01 BPM |
| --- | --- | --- |
| | | |

## 5. Clean up

1. Search for `EUPHORIA`, select all the kit tracks, right click,
   **Remove from Collection**. Delete the **Euphoria test kit** playlist.
2. Put Auto Analysis, the key display format and the **rekordbox xml**
   Imported Library back as they were.
3. If anything else changed, **File > Library > Restore Library** from the
   backup made in step 1.
"""


def build(output, clips):
    tag_folder = output / "tags"
    xml_folder = output / "xml"
    tag_folder.mkdir()
    xml_folder.mkdir()
    made = {}
    for (number, file_format, values), clip in zip(TAG_TRACKS, clips):
        made[number] = make_tag_track(number, file_format, values, clip, tag_folder)
    xml_entries = []
    for (number, file_format, values), clip in zip(XML_TRACKS, clips[len(TAG_TRACKS):]):
        target = xml_folder / f"{number}.{file_format}"
        cut(clip, target, file_format)
        xml_entries.append((target, f"EUPHORIA {number} {file_format.upper()} from the XML", values))
    existing_number, existing_values = EXISTING_IN_XML
    existing_path, existing_title = made[existing_number]
    xml_entries.append((existing_path, existing_title, existing_values))
    write_xml(output / "rekordbox.xml", xml_entries)
    (output / "CHECKLIST.md").write_text(CHECKLIST, encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--clips", required=True, type=Path, help="folder of MP3 clips to cut from")
    parser.add_argument("--output", required=True, type=Path, help="new folder for the kit")
    arguments = parser.parse_args()

    if shutil.which("ffmpeg") is None:
        parser.error("ffmpeg is not on the PATH")
    if arguments.output.exists():
        parser.error(f"{arguments.output} exists; choose a new folder")
    clips = sorted(arguments.clips.glob("*.mp3"))
    needed = len(TAG_TRACKS) + len(XML_TRACKS)
    if len(clips) < needed:
        parser.error(f"{arguments.clips} holds {len(clips)} MP3 clips; the kit needs {needed}")

    output = arguments.output.resolve()
    output.mkdir(parents=True)
    try:
        build(output, clips)
    except BaseException:
        shutil.rmtree(output, ignore_errors=True)
        raise
    print(f"Kit written to {output}. Follow {output / 'CHECKLIST.md'}.")


if __name__ == "__main__":
    sys.exit(main())
