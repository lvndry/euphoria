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

Writing is safe by default. A key or tempo a file already has is kept, a key or tempo Euphoria is unsure of is left out, and the label is not added unless you pick a DJ program. [Safe by default](#safe-by-default) says what changed and how to get the old behaviour back.

A write changes the tag fields it shows and nothing else. Your comments, artwork, cue points, ratings and the audio stay byte for byte what they were, and an undo returns each file to exactly the bytes it had. [Tags](../reference/tags.md) describes how that is done and checked.

The [desktop app](desktop-app.md#write-tags) does the same from its toolbar, with the preview in a window, and shares the backups with these commands: `euphoria tags undo` undoes a write made in the app, and the app undoes one made here.

## What is written

| What | Where | Example |
| --- | --- | --- |
| Key | The key field of the file's tag format | `8A` |
| Tempo | The tempo field | `126` |
| Key and energy | A label in front of the comment, only with a DJ preset or `--label` | `8A - Energy 7` |

The energy rating has no tag field of its own that DJ programs read, so it goes into the comment, in the form Mixed In Key users know: you can sort and search for it in the comment column of any DJ program. With the default `generic` preset no label is written, so the energy is not written either; add `--label comment-start` for it.

## Safe by default

Earlier versions replaced the key and tempo of every file with what the analysis found, and put a label in front of every comment. A tempo from the analysis is only as good as the analysis: a hip hop track tagged 75 BPM could become 150 after the 90 to 180 range folded it, and a key you set by hand could be replaced by a low-confidence guess. Euphoria is meant to never silently damage a tag that is right, so the defaults changed:

| Before | Now | To get the old behaviour back |
| --- | --- | --- |
| A key and tempo already in the file were replaced. | A key or tempo that can be read is kept, and the preview says `key kept (already tagged 8A)`. | `--overwrite` |
| Keys and tempos rated low confidence were written. | They are left out, and the preview says `key not written (rated low confidence)`. A key or tempo you corrected counts as high confidence and is always written. | `--include-low-confidence` |
| The `8A - Energy 7` label went in front of every comment. | The `generic` preset writes no label. The DJ presets (`rekordbox`, `serato`, `traktor`, `engine-dj`, `virtualdj`, `mixxx`) keep writing it at the start of the comment. | `--label comment-start` |
| Writing went ahead while a DJ program was running. | Euphoria warns, and the command needs `--yes` to go on. | `--yes` |

A value that is not readable, such as `o` in the key field, counts as missing and is replaced. A decimal comma counts: a tempo of `127,5` is read as 127.5.

### Values Euphoria wrote itself are updated

Keeping every existing value would stop an update of Euphoria, or a correction you make, from ever reaching files Euphoria tagged earlier. So a value Euphoria's own analysis wrote is not treated as existing, as long as it is still in the file as written. Euphoria tells the two apart with its write log, the same backups that `euphoria tags undo` uses. Each log entry records that the values came from an analysis, and the value written. A key or tempo is Euphoria's when the file still holds exactly what the log says an analysis wrote there. A value you typed with `euphoria tags set`, one typed in another program, and one an analysis wrote before the log recorded this (earlier versions) are not Euphoria's, so they are kept. Undoing a write removes its log, and what it wrote is then no longer known as Euphoria's.

The label is not used for this, because Mixed In Key writes the same `8A - Energy 7` form and a label cannot say who wrote the key next to it.

When the key is left out and the comment already holds a label, the label is left alone rather than rewritten without its key.

### Your comment is kept

The label goes in front of the comment the file already has, separated by ` - `:

| Before | After |
| --- | --- |
| *(no comment)* | `8A - Energy 7` |
| `https://example.com/track/42` | `8A - Energy 7 - https://example.com/track/42` |
| `bought 2019 - warm up` | `8A - Energy 7 - bought 2019 - warm up` |

Writing again never adds a second label. When the key or the energy changed, for example after an update of Euphoria, the label is replaced where it stands and the rest of the comment is left alone. A label in the same form written by another program, such as Mixed In Key, is replaced the same way.

Only text that has exactly the form of a label counts as one: a Camelot code (`8A`), an energy rating (`Energy 7`), or a key followed by an energy rating (`8A - Energy 7`, `Am - Energy 7`, `1m - Energy 7`), at the very start of the comment and followed by ` - ` or by nothing. Everything else is your own text and is kept, including comments such as `A - side`, `Am - my favourite`, `8A-side remix` and `High Energy 7`. A comment of your own that begins with a bare Camelot code and ` - `, such as `8A - my note`, cannot be told apart from a label, and its `8A` is replaced by the new label.

In the desktop app, **Settings > Writing tags** shows a box titled **In your files** under the label choices. It shows, for the selected track or a built-in example, the fields that would change and where the label ends up, and it updates as soon as you change the program, the notation, the label place, the tempo or the replace and low confidence choices. It is the same plan **Write tags** shows, so a key a file already has appears as "kept (already tagged 8A)" when replacing is off. Nothing is written from it.

### Choose where the label goes

| Option | Result |
| --- | --- |
| `--label comment-start` | `8A - Energy 7 - your comment`. The default with a DJ preset. |
| `--label comment-end` | `your comment - 8A - Energy 7` |
| `--label grouping` | The label goes in front of the grouping, and the comment is not touched. |
| `--label none` | No label. The energy is not written, and neither the comment nor the grouping is touched. The default with the `generic` preset. |

When you change this option between two writes, a label with an energy rating at the other end of the same field is removed, so the comment still holds one label. A label is not moved from the comment to the grouping or back: the field you no longer choose is left as it is.

### Other choices

- `--key-notation musical` writes `Am` instead of `8A`, in the key field and in the label. `--key-notation open-key` writes `1m`, the notation Traktor uses for its own keys.
- `--overwrite` replaces a key or tempo the file already has. `--include-low-confidence` also writes keys and tempos rated low confidence.
- `--no-tempo` leaves the tempo field alone. `--tempo` writes it with the Serato preset, which otherwise leaves it out.

## Choose your DJ program

`--preset` picks the tag fields for one program: `rekordbox`, `serato`, `traktor`, `engine-dj`, `virtualdj` or `mixxx`. The default, `generic`, is the right choice when you use several programs or are not sure.

All presets write the key to the same fields: `TKEY` in MP3, WAV and AIFF, both `initialkey` and `KEY` in M4A and MP4, and `INITIALKEY` in FLAC and Ogg Vorbis. They differ in the tempo:

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
  key kept (already tagged 9A)
/Users/you/Music/DJ/warm sun.mp3
  tempo not written (rated low confidence)
340 files to change, 2 already up to date.
Write these changes to 340 files? [y/N]
```

On a terminal, the current values are dimmed and the new ones are green. Answer `y` to write. Any other answer writes nothing.

- `--dry-run` stops after the preview. Nothing is written and nothing is asked.
- `--yes` writes without asking, for use in scripts.
- When the command cannot ask, because its input is not a terminal, and `--yes` was not given, it writes nothing and says so.
- When rekordbox, Serato DJ, Traktor, Engine DJ or VirtualDJ is running, the command warns that the program may save a file during the write and lose the change, and writes nothing until you run it again with `--yes`. It never refuses outright.

Files that already hold the right values are not touched at all.

## Undo

`euphoria tags undo` puts back the tags that the last write replaced and lists the files it restored. Each file returns to the exact bytes it had before the write. Run it again to take back the write before that one.

The backup of every file is saved on disk before the file is changed, so a write that was interrupted, by a crash or a power cut, can be undone too. Backups are kept in Euphoria's data folder, next to the [analysis cache](../reference/cli.md#where-the-cache-is-stored).

If you changed the tags of a file after the write, undo leaves that file alone rather than overwrite your change, reports it, and restores the others. Running `euphoria tags undo` again tries that file again.

## Writing tags automatically after analysis

The desktop app can write each track's key and tempo into its file as soon as the track has been analyzed, so the tags are filled without a click on **Write tags**. It is the setting **Write key and tempo to files after analysis** under **Settings > Writing tags**.

It is on by default, because a key or tempo in the file is what DJ programs read, and the safe defaults below never overwrite one the file already has. The first analysis of a library therefore fills the empty key and tempo fields of every track it analyzes. Turn the setting off to write only when you choose **Write tags**.

When it is on, each newly analyzed track goes through the same planning and writing as **Write tags**, so every safe default above applies:

- A key or tempo the file already has is kept, unless **Replace existing key and tempo** is on. A value Euphoria wrote earlier is still updated.
- A key or tempo rated low confidence is left out, unless **Also write low confidence values** is on. A key or tempo you corrected counts as high confidence, and a correction you make is written the same way.
- Your DJ program and key notation, the energy label place and the tempo choice are respected.
- Every write is logged with its backups, so **Undo this write**, **Tags > Undo Last Tag Write…** and `euphoria tags undo` put the old tags back. An automatic write is undone as one write per batch.

Only new analysis is written: a track whose result was loaded from the [cache](../reference/cli.md#where-the-cache-is-stored) is not touched, while a track analyzed again after an analyzer update, or corrected, is.

**Batches.** Files are not written the moment each track finishes. Tracks wait and are written together when 25 of them are waiting, when the oldest has waited 10 seconds, or when analysis ends, whichever comes first. A long analysis therefore writes a few times a minute, never once per track, and the last tracks are written as soon as analysis is over. The logo in the toolbar shows "Writing tags" while a batch is written.

**A DJ program is running.** Euphoria never writes automatically while rekordbox, Serato DJ, Traktor, Engine DJ or VirtualDJ is running, because the program may save a file at the same moment and lose the change. The tracks are held and a strip says "N tracks have a key and tempo to write into the file. Waiting because <program> is open: close it and choose Write tags, or write now." **Write now** writes them anyway; **Dismiss** forgets them, and **Write tags** can still do them later. The process names Euphoria looks for were not tested against the real programs.

**Errors.** A file that cannot be written, such as a read-only file, a file in a format that takes no tags or a file that is gone, is skipped. When analysis is over one notice says how many tracks were left out and why for the first one. **Write tags** lists each of them with its reason.

## Edit other fields

`euphoria tags set` edits the title, artist, album, genre, year, track number, comment, grouping, key and BPM fields by hand across many files, with find and replace and prefixes and suffixes, through the same preview, safety and undo. The [command reference](../reference/cli.md#euphoria-tags-set) lists its options, and the desktop app does the same with **Edit tags**.

## Limits

A file that cannot be written safely is reported with the reason and skipped. The other files are written, and the command exits with status 1.

- **Files with more than one hard link** are not written. One file that appears under several names cannot be replaced without either leaving the other names with the old contents or risking the file under every name. To tag such a file, make it a file of its own first, by copying it and moving the copy over the name you use.
- **Files changed by another program during the write** are not written. If your DJ software or a tag editor saves a file between the preview and the write, or while Euphoria is writing it, Euphoria notices, leaves the file as the other program left it, and reports it. Run the command again for those files. Euphoria also warns when it finds a DJ program running, since that is the usual cause. The process names it looks for were not tested against the real programs.
- **Tags that cannot be taken apart safely** are not written, for example an ID3v2 tag with an extended header or a fragmented MP4 file. [Tags](../reference/tags.md#when-a-write-is-refused) lists every case.
- The owner, access control lists and extended attributes of a file, such as Finder tags, are not carried over to the rewritten file. Its permission bits are.
- The modification time of a written file changes, on a write and on an undo.
