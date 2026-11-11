# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# requires-python = ">=3.12"
# dependencies = ["mutagen"]
# ///
"""Builds a kit for checking by hand which tags and XML values rekordbox reads.

    cargo build --release -p euphoria-cli
    uv run bench/rekordbox_test_kit.py --clips bench/data/key/audio --output /tmp/euphoria-kit

The kit is a folder of short audio files cut from benchmark clips, each
tagged in one variant (a key field name, a key notation, a tempo with or
without decimals, a comment), a rekordbox XML file that lists a few of them
with chosen values, and CHECKLIST.md: the steps to follow in rekordbox and
a table to fill in. Every file's title starts with its test number, such as
"EUPHORIA T01", so the rows are easy to find in rekordbox.

One WAV file, T11, is tagged by Euphoria itself, with `euphoria tags write`,
to check that rekordbox shows the label Euphoria writes into the RIFF INFO
comment. The script uses the euphoria binary given with --euphoria, or the
one built in target/release.

The script reads the clips and writes only into the output folder, which
must not exist yet, and removes it again if building the kit fails. It never touches a rekordbox library or its settings.
It needs ffmpeg on the PATH.
"""

import argparse
import os
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
REPOSITORY = Path(__file__).resolve().parent.parent
EUPHORIA_COMMENT = "RIFF comment kept by Euphoria"
# Stands in the checklist for the RIFF comment Euphoria wrote into T11.
WRITTEN_COMMENT = "{written_comment}"

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
    ("T11", "wav-euphoria", {"riff_comment": EUPHORIA_COMMENT}),
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


def riff_comment(path):
    """The ICMT value of the file's RIFF INFO list, read from the chunks
    directly: ffprobe shows the ID3 comment instead when there is one."""
    data = path.read_bytes()
    offset = 12
    while offset + 8 <= len(data):
        name = data[offset:offset + 4]
        size = int.from_bytes(data[offset + 4:offset + 8], "little")
        if name == b"LIST" and data[offset + 8:offset + 12] == b"INFO":
            value_offset = offset + 12
            while value_offset + 8 <= offset + 8 + size:
                value_name = data[value_offset:value_offset + 4]
                value_size = int.from_bytes(data[value_offset + 4:value_offset + 8], "little")
                if value_name == b"ICMT":
                    text = data[value_offset + 8:value_offset + 8 + value_size]
                    return text.split(b"\0")[0].decode("utf-8", "replace")
                value_offset += 8 + value_size + value_size % 2
        offset += 8 + size + size % 2
    return ""


def tag_with_euphoria(euphoria, target, data_folder):
    """Lets Euphoria analyze the file and write its tags, as a user would
    with the rekordbox preset. Its data, the analysis cache and the backup
    of the write, goes into a folder of the kit, which is removed after."""
    command = [str(euphoria), "tags", "write", "--yes", "--no-cache", "--preset", "rekordbox", str(target)]
    environment = {**os.environ, "EUPHORIA_DATA_DIR": str(data_folder)}
    result = subprocess.run(command, capture_output=True, text=True, check=False, env=environment)
    shutil.rmtree(data_folder, ignore_errors=True)
    if result.returncode != 0:
        raise SystemExit(f"euphoria failed: {' '.join(command)}\n{result.stderr}")


def make_tag_track(number, file_format, values, clip, folder, euphoria):
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
    elif file_format == "wav-euphoria":
        target = folder / f"{number}.wav"
        riff = [("title", f"EUPHORIA {number} RIFF title"), ("comment", values["riff_comment"])]
        cut(clip, target, "wav", riff)
        audio = WAVE(target)
        audio.add_tags()
        audio.tags.add(TIT2(encoding=3, text=title))
        audio.save(v2_version=3)
        tag_with_euphoria(euphoria, target, folder.parent / "euphoria-data")
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
             "flac": "FLAC", "wav": "WAV", "wav-euphoria": "WAV by Euphoria"}[file_format]
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

Files: `tags/` holds the tracks T01 to T11, `xml/` holds X01 and X02, and
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
| T11 | WAV tagged by `euphoria tags write --preset rekordbox`: RIFF title `...RIFF title`, RIFF comment `{written_comment}` | | | | |

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


def build(output, clips, euphoria):
    tag_folder = output / "tags"
    xml_folder = output / "xml"
    tag_folder.mkdir()
    xml_folder.mkdir()
    made = {}
    for (number, file_format, values), clip in zip(TAG_TRACKS, clips):
        made[number] = make_tag_track(number, file_format, values, clip, tag_folder, euphoria)
    xml_entries = []
    for (number, file_format, values), clip in zip(XML_TRACKS, clips[len(TAG_TRACKS):]):
        target = xml_folder / f"{number}.{file_format}"
        cut(clip, target, file_format)
        xml_entries.append((target, f"EUPHORIA {number} {file_format.upper()} from the XML", values))
    existing_number, existing_values = EXISTING_IN_XML
    existing_path, existing_title = made[existing_number]
    xml_entries.append((existing_path, existing_title, existing_values))
    write_xml(output / "rekordbox.xml", xml_entries)
    euphoria_path, _ = made["T11"]
    written_comment = riff_comment(euphoria_path)
    if not written_comment.endswith(f" - {EUPHORIA_COMMENT}"):
        raise SystemExit(f"Euphoria did not keep the RIFF comment of T11: {written_comment!r}")
    checklist = CHECKLIST.replace(WRITTEN_COMMENT, written_comment)
    (output / "CHECKLIST.md").write_text(checklist, encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--clips", required=True, type=Path, help="folder of MP3 clips to cut from")
    parser.add_argument("--output", required=True, type=Path, help="new folder for the kit")
    parser.add_argument("--euphoria", type=Path, default=REPOSITORY / "target" / "release" / "euphoria",
                        help="the euphoria binary that tags T11 (default: the release build)")
    arguments = parser.parse_args()

    if shutil.which("ffmpeg") is None:
        parser.error("ffmpeg is not on the PATH")
    if not arguments.euphoria.is_file():
        parser.error(f"{arguments.euphoria} does not exist; run cargo build --release -p euphoria-cli or pass --euphoria")
    if arguments.output.exists():
        parser.error(f"{arguments.output} exists; choose a new folder")
    clips = sorted(arguments.clips.glob("*.mp3"))
    needed = len(TAG_TRACKS) + len(XML_TRACKS)
    if len(clips) < needed:
        parser.error(f"{arguments.clips} holds {len(clips)} MP3 clips; the kit needs {needed}")

    output = arguments.output.resolve()
    output.mkdir(parents=True)
    try:
        build(output, clips, arguments.euphoria.resolve())
    except BaseException:
        shutil.rmtree(output, ignore_errors=True)
        raise
    print(f"Kit written to {output}. Follow {output / 'CHECKLIST.md'}.")


if __name__ == "__main__":
    sys.exit(main())
