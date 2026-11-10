---
title: Tags
description: Which tag fields Euphoria reads and writes for each file format and DJ program.
---

# Tags

Euphoria writes the key and tempo it detects into the tags of your files, so DJ software can show them. A write always goes through three steps:

1. A plan lists every field that would change, with its current and new value. Making a plan only reads the file. A file that already holds the right values gets an empty plan and is left alone.
2. Applying the plan writes those fields and nothing else. Every other tag, including the cue points and beat grids other programs store, and the audio itself are kept. If a field changed after the plan was made, nothing is written.
3. Each write returns a backup of the values it replaced. Undo puts them back and removes the fields the write created. If a field was edited after the write, undo stops instead of overwriting the edit.

## How a file is rewritten

The new contents are built in a private copy next to the file. The copy is flushed to disk, read back, and compared with the original. It replaces the file only when nothing but the planned fields differs, so an interrupted, failed or refused write leaves the file as it was.

What is kept:

- Every tag item the plan does not name, including artwork (ID3v2 `APIC` frames, MP4 `covr`, FLAC picture blocks) and the private data other programs store.
- Tags of other kinds in the same file: ID3v1 and APE tags in MP3, RIFF INFO in WAV, text chunks in AIFF, and an ID3v2 tag in front of a FLAC stream.
- Everything else in the container, such as other MP4 atoms and FLAC metadata blocks, and the audio, byte for byte.

When a write is refused. The error names the file and the reason, and the file is not touched:

- The tag holds something that cannot be parsed, such as a damaged frame. A tag library skips such data when reading and would drop it when writing. `read_tags` still reads these files.
- The rewritten copy does not match the original outside the planned fields.
- An MP3 holds more than one ID3v2 tag.

Links:

- A symbolic link is followed. The file it points to is rewritten, and the link stays a link.
- A file with several hard links is overwritten where it is, so all its names keep sharing the same contents. This one case is not a single step: if it is interrupted, the complete copy with the new tags is left next to the file, named `.<file name>.<numbers>.euphoria-tmp`.

What changes besides the tags:

- The modification time of the file is the time of the write.
- The rewritten file keeps the permission bits of the original. Its owner, access control lists and extended attributes (such as Finder tags) are those of a newly created file, not carried over from the original. A file with several hard links is the exception: it is overwritten in place and keeps all of them.
- Padding inside the tag area may grow or shrink, so the file size can differ after a write and after an undo.

## Tag formats

| File format | Tag format |
| --- | --- |
| MP3, WAV, AIFF | ID3v2 |
| M4A | MP4 atoms |
| FLAC | Vorbis comments |

An existing ID3v2 tag keeps its version: an ID3v2.4 tag is written back as ID3v2.4, older tags as ID3v2.3. A file with no ID3v2 tag gets an ID3v2.3 tag, the version the widest range of DJ software accepts.

There is one exception. Some programs put frames that only exist in ID3v2.4, such as the sort order frames `TSOP` and `TSOA`, into ID3v2.3 tags. An ID3v2.3 write would drop them, so such a tag is written as ID3v2.4 with every frame kept.

## Fields read

When several fields are listed, the first one present is used.

| Value | ID3v2 | MP4 | Vorbis comments |
| --- | --- | --- | --- |
| Title | `TIT2` | `©nam` | `TITLE` |
| Artist | `TPE1` | `©ART` | `ARTIST` |
| Key | `TKEY` | `----:com.apple.iTunes:initialkey`, `----:com.apple.iTunes:KEY` | `INITIALKEY`, `KEY` |
| Tempo | `TBPM` | `----:com.apple.iTunes:BPM`, `tmpo` | `BPM`, `TEMPO` |
| Comment | `COMM` with an empty description | `©cmt` | `COMMENT`, `DESCRIPTION` |
| Grouping | `TIT1`, `GRP1` | `©grp` | `GROUPING` |

Freeform MP4 names are matched in any letter case. WAV and AIFF files are read through their ID3v2 tag only, not through RIFF INFO or AIFF text chunks.

## Fields written

### Key

| Preset | ID3v2 | MP4 | Vorbis comments |
| --- | --- | --- | --- |
| rekordbox | `TKEY` | `----:com.apple.iTunes:initialkey` | `INITIALKEY` |
| Serato | `TKEY` | `----:com.apple.iTunes:initialkey` | `INITIALKEY` |
| Traktor | `TKEY` | `----:com.apple.iTunes:initialkey` | `INITIALKEY` |
| Engine DJ | `TKEY` | `----:com.apple.iTunes:initialkey` | `INITIALKEY` |
| Generic | `TKEY` | `----:com.apple.iTunes:initialkey` | `INITIALKEY` and `KEY` |

The key is written as a Camelot code such as `8A`, or as a short musical name such as `Am`.

### Tempo

The tempo is written only when asked for.

| Preset | ID3v2 | MP4 | Vorbis comments |
| --- | --- | --- | --- |
| rekordbox | `TBPM` | `tmpo` | `BPM` |
| Serato | `TBPM` | `tmpo` | `BPM` |
| Traktor | `TBPM` | `tmpo` | `BPM` |
| Engine DJ | `TBPM` | `tmpo` | `BPM` |
| Generic | `TBPM` | `tmpo` and `----:com.apple.iTunes:BPM` | `BPM` |

`tmpo` always holds a whole number, stored in two bytes. In the other fields, the Generic preset keeps up to two decimals, such as `127.5`, and the program presets round to a whole number. A tempo field that already holds the same number, for example `128.00`, counts as matching and is not rewritten.

### Label

An optional label, such as the Camelot code, can be put in front of the comment or the grouping. It is written between braces, followed by a space and the text the field already holds: a comment `warm up` becomes `{8A} warm up`, and an empty field becomes `{8A}`.

The braces are how Euphoria recognises its own label later:

- Writing the same label again changes nothing.
- Writing a different label replaces the one in braces and keeps the rest: `{Energy 7} warm up` becomes `{Energy 8} warm up`. A field never holds more than one label.
- Only `{...}` at the very start of the field, followed by a space or by the end of the field, counts as a label. Anything else is your own text and is kept after the new label, including text such as `8A - warm up`, `[8A] warm up`, `warm {8A} up` or `{8A}warm up`.
- A comment you wrote yourself in exactly that shape, such as `{demo} warm up`, cannot be told apart from a label, and its braced part is replaced.

The label text cannot contain a brace. Braces are plain ASCII, so the label is stored unchanged in all three tag formats and displays as written in DJ software columns.

| Target | ID3v2 | MP4 | Vorbis comments |
| --- | --- | --- | --- |
| Comment | `COMM` with an empty description | `©cmt` | `COMMENT` |
| Grouping | `TIT1` | `©grp` | `GROUPING` |

An existing ID3v2 comment keeps its language. Comments with a description belong to other programs and are not touched.

## How well each field is confirmed

DJ software vendors do not publish the exact fields their programs read, so the presets rest on evidence of varying strength.

| Program | What is known | Strength |
| --- | --- | --- |
| Mixxx | Reads every field in the tables above. The Generic preset follows its tag reader. | Confirmed in its source code |
| rekordbox | Reads the key from MP3, AIFF and M4A tags. Does not read the key from FLAC or WAV tags, and does not read the tempo from tags in any format. Reads comments in every format and does not read the grouping. The field names are not published. | Vendor compatibility table from 2019 |
| Serato | Reads `TKEY` and `TBPM` from MP3. Recommends ID3v2.3. Key display for FLAC has a history of bugs and conflicting reports. The MP4 and Vorbis field names are not confirmed. | Forum reports |
| Traktor | Reads `TKEY` and `TBPM` from MP3. The MP4 and Vorbis field names are not confirmed. | Forum reports |
| Engine DJ | Keeps key and tempo tags found on MP3 files. The field names for every format are not confirmed. | Forum reports |

Where a field name is not confirmed, the preset writes the name that Mixxx reads and that other tagging tools write. For a program that does not show the key from a format, such as rekordbox with FLAC, putting the key in the comment as a label is the usual workaround.
