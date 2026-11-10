---
title: Tags
description: Which tag fields Euphoria reads and writes for each file format and DJ program.
---

# Tags

Euphoria writes the key and tempo it detects into the tags of your files, so DJ software can show them. A write always goes through three steps:

1. A plan lists every field that would change, with its current and new value. Making a plan only reads the file. A file that already holds the right values gets an empty plan and is left alone.
2. Applying the plan changes exactly those fields. Every other byte of the file stays what it was. If a field changed after the plan was made, nothing is written.
3. Each write returns a backup of what it replaced. Undo puts it back and removes the fields the write created. If a field was edited after the write, undo stops instead of overwriting the edit.

## How a file is rewritten

Euphoria does not read a tag into a model and write the model back, because that rewrites items nobody asked to change: a one byte rating becomes four bytes, an empty genre disappears, a text encoding changes. Instead, every item of a tag is kept as the bytes it was read from. A write exchanges the bytes of the planned items and copies everything else.

| Tag format | What counts as one item |
| --- | --- |
| ID3v2 | A frame, header included |
| MP4 | A child of the `ilst` atom, with all its `data` atoms |
| Vorbis comments | One `NAME=value` comment |

What stays byte for byte the same:

- Every item the plan does not name, in its place: artwork (`APIC` frames, `covr`, FLAC picture blocks), ratings, sort names, gapless and volume data, the private data of other programs, and items Euphoria does not know.
- An item the plan does name keeps what it can: an ID3v2 frame keeps its flags and its text encoding when the new text fits in it, a comment frame keeps its language, a Vorbis comment and a freeform MP4 item keep the spelling of their name.
- Tags of other kinds in the same file: ID3v1 and APE tags in MP3, RIFF INFO in WAV, text chunks in AIFF, and an ID3v2 tag in front of a FLAC stream.
- Everything else in the container, such as other MP4 atoms and FLAC metadata blocks, and the audio.

What may change besides the planned items is padding, the empty space formats keep for tags to grow into. When there is enough, it takes up the difference and nothing else in the file moves. When there is not, the tag area grows and the audio moves back. In an MP4 file whose `moov` atom comes before the audio, the chunk offsets (`stco`, `co64`) are then moved by the same distance.

The new file is built next to the original, flushed to disk, and checked before it takes the original's place:

- Every byte outside the rewritten tag area is compared with the original.
- The new file is read again from scratch. Its items, apart from the planned ones, must be the original items, byte for byte and in the same order. Each planned item must hold exactly what was meant to be written.
- For MP4, the rest of `moov` must be unchanged, and every chunk offset must still point at the same audio.

If any check fails, the write is refused and the file is not touched.

### Undo

The backup holds, for each field, the item's original bytes. Undo puts those bytes back, so a restored field is identical to the original, not just equal in value: a comment that was UTF-16 is UTF-16 again. Undo is checked the same way as a write.

The backup also holds the original length of the tag area and of the file. Undo gives the padding its original size again, so the file is byte for byte what it was before the write. The exceptions are padding that held something other than zeros, and a WAV or AIFF file whose last chunk lacked its padding byte; in both cases every item is still restored exactly.

### When a write is refused

The error names the file and the reason, and the file is not touched. `read_tags` still reads these files.

- The tag area holds something that cannot be told apart into items: data that is not a frame, an item that runs past its container, padding that holds data.
- An ID3v2 tag has an extended header, or an ID3v2.2 tag is compressed.
- A file holds more than one ID3v2 tag where one is expected, more than one Vorbis comment block, or more than one `moov`, `udta`, `meta` or `ilst` atom.
- A planned ID3v2 frame is compressed, encrypted or unsynchronised on its own. Such frames are kept untouched when they are not part of the plan.
- A fragmented MP4 file would need its audio moved.

### Links, permissions and times

- A symbolic link is followed. The file it points to is rewritten, and the link stays a link.
- A file with several hard links is overwritten where it is, so all its names keep sharing the same contents. This one case is not a single step: if it is interrupted, the complete new file is left next to the original, named `.<file name>.<numbers>.euphoria-tmp`.
- The modification time of the file is the time of the write.
- The rewritten file keeps the permission bits of the original. Its owner, access control lists and extended attributes (such as Finder tags) are those of a newly created file, not carried over from the original. A file with several hard links is the exception: it is overwritten in place and keeps all of them.

## Tag formats

| File format | Tag format |
| --- | --- |
| MP3, WAV, AIFF | ID3v2 |
| M4A | MP4 atoms |
| FLAC | Vorbis comments |

An existing ID3v2 tag keeps its version, whether ID3v2.2, ID3v2.3 or ID3v2.4, along with frames that belong to another version. A file with no ID3v2 tag gets an ID3v2.3 tag, the version the widest range of DJ software accepts. In WAV and AIFF the tag lives in an `ID3 ` chunk, and an existing chunk keeps the spelling of its name.

New text is stored as Latin-1 when it fits, and otherwise as UTF-16 in ID3v2.2 and ID3v2.3 and as UTF-8 in ID3v2.4.

An M4A file without metadata gets the `udta`, `meta` and `ilst` atoms that iTunes style tags live in.

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
