---
title: Tags
description: Which tag fields Euphoria reads and writes for each file format and DJ program.
---

# Tags

Euphoria writes the key, tempo and energy it detects into the tags of your files, so DJ software can show them. [Write tags into your files](../guides/write-tags.md) shows how to do it from the command line. This page describes what is written and how.

A write always goes through three steps:

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

Last of all, the original is read once more and must still hold the tag the write started from. If another program changed the file in the meantime, replacing it would undo that change.

If any check fails, the write is refused, the new file is deleted, and the original is not touched. Between that last look and the replacement itself there remains a moment, far shorter than the write, in which a change by another program would still be lost; the formats offer no way to close it.

### Undo across runs

The backups of a write can be kept in a log on disk, one log per write, so that a later run of the program can undo it. The backup of a file is in the log, flushed to disk, before the file is changed, which means a write cut short by a crash or a power loss can still be undone: a file whose backup was saved but which was not changed yet is recognised by its fields still holding their original values.

A write that fails or is refused leaves its file unchanged, and the log notes that, so the backup of that file plays no part in a later undo. If the file was changed but noting it in the log failed, for example on a full disk, the write is reported as a change whose record could not be saved, and the backup saved beforehand still undoes it.

A log holds at most 2 GiB, one million lines (two per file, so half a million files) and 4 MiB per line. The backup of a file is normally one or two kilobytes, so a write of fifty thousand files makes a log of about a hundred megabytes. A write that reaches a limit stops taking further files, which are reported and can be written in another run, and a log beyond a limit is refused when undoing. Undo reads a log one entry at a time.

Undoing the last write restores its files one by one. A file that cannot be restored, for example because its tags were edited since, is reported and stays in the log, so the undo can be repeated for it. A log whose files are all restored is deleted. `euphoria tags write` keeps its logs in the `tag-writes` folder of Euphoria's data folder.

### Undo

The backup holds, for each field, the item's original bytes. Undo puts those bytes back, so a restored field is identical to the original, not just equal in value: a comment that was UTF-16 is UTF-16 again. Undo is checked the same way as a write.

The backup also holds the original length of the tag area and of the file. Undo gives the padding its original size again, so the file is byte for byte what it was before the write. The exceptions are padding that held something other than zeros, and a WAV or AIFF file whose last chunk lacked its padding byte; in both cases every item is still restored exactly.

### When a write is refused

The error names the file and the reason, and the file is not touched. `read_tags` does not fail on these files; it returns the fields it can reach, which for the extended header and `meta` cases below is none.

- The tag area holds something that cannot be told apart into items: data that is not a frame, an item that runs past its container, padding that holds data.
- An ID3v2 tag has an extended header, or an ID3v2.2 tag is compressed.
- A file holds more than one ID3v2 tag where one is expected, more than one Vorbis comment block, or more than one `moov`, `udta`, `meta` or `ilst` atom.
- A planned ID3v2 frame is compressed, encrypted or unsynchronised on its own. Such frames are kept untouched when they are not part of the plan.
- The `meta` atom of an MP4 file lacks its version field, as some QuickTime style files have it.
- A fragmented MP4 file would need its audio moved, because padding cannot take up the change in length.
- The size in an ID3v2 tag header is damaged, so the end of the tag cannot be known. `read_tags` fails on such a file too.
- The file has more than one hard link. See below.
- Another program changed the file between the moment its tags were read and the moment the new file was ready. Make a new plan and apply that.

### Links, permissions and times

- A symbolic link is followed. The file it points to is rewritten, and the link stays a link.
- A file with more than one hard link, that is, one file that appears under several names, is not written at all. There is no safe way to do it. Putting a new file in place of one name leaves the other names with the old contents, so the names stop being the same file without anyone having asked for that. Overwriting the shared contents where they are keeps the names together, but an interruption halfway damages the track under every name at once. Refusing keeps the library exactly as it is and says why. To tag such a file, make it a file of its own first, for example by copying it and moving the copy over the name you use.
- The modification time of the file is the time of the write.
- The rewritten file keeps the permission bits of the original. Its owner, access control lists and extended attributes (such as Finder tags) are those of a newly created file, not carried over from the original.

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
| Every preset | `TKEY` | `----:com.apple.iTunes:initialkey` and `----:com.apple.iTunes:KEY` | `INITIALKEY` |

Every preset writes the same key fields. When a FLAC file already has a `KEY` comment, it is given the same value as `INITIALKEY`; a FLAC file without one does not get one. The reasons are in [use Euphoria with your DJ program](../guides/dj-programs.md#the-same-in-every-program).

The key is written as a Camelot code such as `8A`, as a short musical name such as `Am`, or as an Open Key code such as `1m`.

### Tempo

| Preset | ID3v2 | MP4 | Vorbis comments | Decimals |
| --- | --- | --- | --- | --- |
| rekordbox, Traktor, Engine DJ, VirtualDJ | `TBPM` | `tmpo` | `BPM` | No |
| Serato | Not written unless asked for | | | No |
| Mixxx | `TBPM` | `tmpo` and `----:com.apple.iTunes:BPM` | `BPM` | Up to two |
| Generic | `TBPM` | `tmpo` and `----:com.apple.iTunes:BPM` | `BPM` | No |

`tmpo` always holds a whole number, stored in two bytes. With decimals, a tempo is written such as `127.5`; without, it is rounded to a whole number. A tempo field that already holds the same number, for example `128.00`, counts as matching and is not rewritten.

A file that already has `----:com.apple.iTunes:BPM` in M4A, or `TEMPO` in FLAC, gets the same tempo there too, so a field that Mixxx would read first never holds an older value.

### Label

The energy rating has no tag field of its own that DJ programs read. It is written, with the key, as a label such as `8A - Energy 7` in the comment or the grouping. The label takes the key notation of the key field. A track without a key gets `Energy 7`, a track without an energy rating gets `8A`, and a track with neither gets no label.

| Placement | Result for a comment `warm up` |
| --- | --- |
| Front of the comment (the default) | `8A - Energy 7 - warm up` |
| End of the comment | `warm up - 8A - Energy 7` |
| Front of the grouping | The comment is not touched. |
| None | Neither field is touched, and the energy is not written. |

The text the field already holds is kept, separated from the label by ` - `. An empty field holds the label alone.

The label carries no marker, so one written earlier is recognised by its form:

- A label is a Camelot code (`8A`), an energy rating (`Energy 7`), or a key followed by an energy rating (`8A - Energy 7`, `Am - Energy 7`, `1m - Energy 7`). The Camelot and Open Key numbers run from 1 to 12 and the energy from 1 to 10, without a leading zero.
- It counts only at the end of the field it is written at, next to ` - ` or filling the field.
- A label found there is replaced, so writing again never leaves two. A label in the same form from another program is replaced too.
- A musical key name or an Open Key code without an energy rating is not taken for a label, because a comment such as `A - side` or `1m - long intro` begins the same way. Writing the same label twice still changes nothing, whatever its form.
- At the other end of the same field, only a label with an energy rating is removed. That covers a change of placement between two writes without touching a comment that happens to end in a Camelot code.
- Everything else is your own text and is kept: `8A-side remix`, `Am - my favourite`, `High Energy 7`, `Energy 11 - intro`.
- A comment of your own that starts with a bare Camelot code and ` - `, such as `8A - my note`, cannot be told apart from a label, and its code is replaced.

A label is never moved between the comment and the grouping: only the field of the chosen placement is read and written.

| Target | ID3v2 | MP4 | Vorbis comments |
| --- | --- | --- | --- |
| Comment | `COMM` with an empty description | `©cmt` | `COMMENT` |
| Grouping | `TIT1` | `©grp` | `GROUPING` |

An existing ID3v2 comment keeps its language. Comments with a description belong to other programs and are not touched.

## How well each field is confirmed

DJ software vendors do not publish the exact fields their programs read. [Use Euphoria with your DJ program](../guides/dj-programs.md) gives, for each program, what is confirmed, what is only reported, and what is unconfirmed, along with what to set in the program. In short:

| Program | Strongest evidence |
| --- | --- |
| Mixxx | Every field above, confirmed in its source code. |
| rekordbox | Tested in rekordbox 7: the key is read from `TKEY`, M4A `KEY` (not `initialkey`) and FLAC `INITIALKEY`; the comment from every format; WAV title and comment from RIFF INFO only; no tempo from any tag. |
| Serato | Serato's own statements for the key in MP3 and the analysis settings. M4A and FLAC field names are unconfirmed. |
| Traktor | Mixed In Key's guide and user reports for the Key Text column. M4A and FLAC field names are unconfirmed. |
| Engine DJ | The Engine DJ user guide for when tags are read. Field names are unconfirmed. |
| VirtualDJ | Its options list and its CTO's statement about FLAC key fields. MP3 and M4A field names are unconfirmed. |
