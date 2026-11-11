---
title: Write tags into your files
description: Put the key, tempo and energy Euphoria found into the tags of your audio files, with a preview first and an undo afterwards.
---

# Write tags into your files

One command analyzes your music and writes what it found into the files themselves, where DJ software and music players read it:

```
euphoria tags write ~/Music/DJ
```

It shows every change first and asks before it writes anything. Afterwards, one command puts the old tags back:

```
euphoria tags undo
```

A write changes the tag fields it shows and nothing else. Your comments, artwork, cue points, ratings and the audio stay byte for byte what they were, and an undo returns each file to exactly the bytes it had. [Tags](../reference/tags.md) describes how that is done and checked.

The [desktop app](desktop-app.md#write-tags) does the same from its toolbar, with the preview in a window, and shares the backups with these commands: `euphoria tags undo` undoes a write made in the app, and the app undoes one made here.

## What is written

| What | Where | Example |
| --- | --- | --- |
| Key | The key field of the file's tag format | `8A` |
| Tempo | The tempo field | `126` |
| Key and energy | A label in front of the comment | `8A - Energy 7` |

The energy rating has no tag field of its own that DJ programs read, so it goes into the comment, in the form Mixed In Key users know: you can sort and search for it in the comment column of any DJ program.

### Your comment is kept

The label goes in front of the comment the file already has, separated by ` - `:

| Before | After |
| --- | --- |
| *(no comment)* | `8A - Energy 7` |
| `https://example.com/track/42` | `8A - Energy 7 - https://example.com/track/42` |
| `bought 2019 - warm up` | `8A - Energy 7 - bought 2019 - warm up` |

Writing again never adds a second label. When the key or the energy changed, for example after an update of Euphoria, the label is replaced where it stands and the rest of the comment is left alone. A label in the same form written by another program, such as Mixed In Key, is replaced the same way.

Only text that has exactly the form of a label counts as one: a Camelot code (`8A`), an energy rating (`Energy 7`), or a key followed by an energy rating (`8A - Energy 7`, `Am - Energy 7`, `1m - Energy 7`), at the very start of the comment and followed by ` - ` or by nothing. Everything else is your own text and is kept, including comments such as `A - side`, `Am - my favourite`, `8A-side remix` and `High Energy 7`. A comment of your own that begins with a bare Camelot code and ` - `, such as `8A - my note`, cannot be told apart from a label, and its `8A` is replaced by the new label.

### Choose where the label goes

| Option | Result |
| --- | --- |
| `--label comment-start` | `8A - Energy 7 - your comment`. The default. |
| `--label comment-end` | `your comment - 8A - Energy 7` |
| `--label grouping` | The label goes in front of the grouping, and the comment is not touched. |
| `--label none` | No label. The energy is not written, and neither the comment nor the grouping is touched. |

When you change this option between two writes, a label with an energy rating at the other end of the same field is removed, so the comment still holds one label. A label is not moved from the comment to the grouping or back: the field you no longer choose is left as it is.

### Other choices

- `--key-notation musical` writes `Am` instead of `8A`, in the key field and in the label. `--key-notation open-key` writes `1m`, the notation Traktor uses for its own keys.
- `--no-tempo` leaves the tempo field alone. `--tempo` writes it with the Serato preset, which otherwise leaves it out.

## Choose your DJ program

`--preset` picks the tag fields for one program: `rekordbox`, `serato`, `traktor`, `engine-dj`, `virtualdj` or `mixxx`. The default, `generic`, is the right choice when you use several programs or are not sure.

All presets write the key to the same fields: `TKEY` in MP3, WAV and AIFF, both `initialkey` and `KEY` in M4A, and `INITIALKEY` in FLAC. They differ in the tempo:

| Preset | Tempo |
| --- | --- |
| `mixxx` | With up to two decimals where the field allows, such as `127.5`. |
| `serato` | Not written unless you add `--tempo`, so that Serato detects the tempo and builds its beat grid itself. |
| Every other preset | Rounded to a whole number, the only form every program is known to accept. |

[Use Euphoria with your DJ program](dj-programs.md) explains each choice, what to set in each program so that it keeps Euphoria's values, and how much of it is confirmed. No vendor publishes the fields its program reads, and no preset has yet been tried in the program it is named after. After a write, most programs show the new values only once you ask them to read the tags again.

## The preview

Before anything is written, each file that would change is listed with its fields, the value each has now and the value it would get:

```
$ euphoria tags write ~/Music/DJ
/Users/you/Music/DJ/low tide.flac
  INITIALKEY: (none) -> 8A
  BPM: (none) -> 124
  COMMENT: https://example.com/track/42 -> 8A - Energy 7 - https://example.com/track/42
340 files to change, 2 already up to date.
Write these changes to 340 files? [y/N]
```

Answer `y` to write. Any other answer writes nothing.

- `--dry-run` stops after the preview. Nothing is written and nothing is asked.
- `--yes` writes without asking, for use in scripts.
- When the command cannot ask, because its input is not a terminal, and `--yes` was not given, it writes nothing and says so.

Files that already hold the right values are not touched at all.

## Undo

`euphoria tags undo` puts back the tags that the last write replaced and lists the files it restored. Each file returns to the exact bytes it had before the write. Run it again to take back the write before that one.

The backup of every file is saved on disk before the file is changed, so a write that was interrupted, by a crash or a power cut, can be undone too. Backups are kept in Euphoria's data folder, next to the [analysis cache](../reference/cli.md#where-the-cache-is-stored).

If you changed the tags of a file after the write, undo leaves that file alone rather than overwrite your change, reports it, and restores the others. Running `euphoria tags undo` again tries that file again.

## Limits

A file that cannot be written safely is reported with the reason and skipped. The other files are written, and the command exits with status 1.

- **Files with more than one hard link** are not written. One file that appears under several names cannot be replaced without either leaving the other names with the old contents or risking the file under every name. To tag such a file, make it a file of its own first, by copying it and moving the copy over the name you use.
- **Files changed by another program during the write** are not written. If your DJ software or a tag editor saves a file between the preview and the write, or while Euphoria is writing it, Euphoria notices, leaves the file as the other program left it, and reports it. Run the command again for those files. It is still best to close other programs that write tags while Euphoria writes.
- **Tags that cannot be taken apart safely** are not written, for example an ID3v2 tag with an extended header or a fragmented MP4 file. [Tags](../reference/tags.md#when-a-write-is-refused) lists every case.
- The owner, access control lists and extended attributes of a file, such as Finder tags, are not carried over to the rewritten file. Its permission bits are.
- The modification time of a written file changes, on a write and on an undo.
