---
title: Command line
description: Every command, option, output format and exit status of the euphoria command.
---

# Command line

## euphoria analyze

Detects the key, tempo and energy of audio files.

```
euphoria analyze [OPTIONS] <PATHS>...
```

Each path is a file or a folder. Folders are searched recursively for [supported audio files](supported-formats.md). Hidden files and folders are skipped, including the `._` copies macOS leaves on external drives. A file named directly is always analyzed, so an unsupported file is reported as a failure instead of being ignored.

A symbolic link to an audio file is analyzed. A symbolic link to a folder is not entered, so a scan stays inside the folders you named; to include a linked folder, name it on the command line.

```
$ euphoria analyze ~/Music/DJ
Key             BPM  Energy  Time  File
8A   A minor  124.0       6  6:48  /Users/you/Music/DJ/Low Tide.flac
-                 -       -  1:00  /Users/you/Music/DJ/Intro.wav
```

A `-` means nothing was detected, for example in silence or a track shorter than one second.

The Energy column is the [energy rating](../concepts/energy.md) from 1 to 10. It is Euphoria's own scale, so do not compare it with the energy numbers of other tools.

The table holds what you act on while mixing. The tuning of a track and its key over time are in the [JSON output](#json-output) only: most tracks are at concert pitch and stay in one key, so a column for either would be empty or zero nearly everywhere.

### Options

| Option | Effect |
| --- | --- |
| `--json` | Print a JSON array instead of a table. |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

Analyzing one seven minute track peaks near 0.6 GB of memory. On full length tracks, 4 workers peaked near 2 GB in total and 8 workers between 4.8 and 6.2 GB, which is why the default stops at 8. Lower `--jobs` on a machine with 8 GB of memory or less.

### JSON output

One object per file, in path order, including files that failed:

```json
[
  {
    "path": "/Users/you/Music/DJ/Low Tide.flac",
    "duration_seconds": 408.4,
    "key": { "camelot": "8A", "name": "A minor" },
    "bpm": 124.0,
    "energy": 6,
    "tuning_cents": -31.3,
    "key_timeline": [
      {
        "start_seconds": 0.0,
        "end_seconds": 250.0,
        "key": { "camelot": "8A", "name": "A minor" }
      },
      {
        "start_seconds": 250.0,
        "end_seconds": 408.4,
        "key": { "camelot": "9A", "name": "E minor" }
      }
    ],
    "error": null
  },
  {
    "path": "/Users/you/Music/DJ/notes.txt",
    "duration_seconds": null,
    "key": null,
    "bpm": null,
    "energy": null,
    "tuning_cents": null,
    "key_timeline": [],
    "error": "cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: unsupported feature: core (probe): no suitable format reader found"
  }
]
```

`key`, `bpm` and `energy` are `null` when nothing was detected. `energy` is a whole number from 1 to 10. `error` is set only when the file could not be analyzed.

`tuning_cents` is how far the track is tuned from A = 440 Hz, in cents from -50 to 50, where 100 cents is a semitone. Positive is sharp: at 30 the A above middle C sounds at 447.7 Hz, and at -31.3 at 432.1 Hz. It is `null` when the tuning could not be measured, which is the case for silence, for noise and for many tracks that are mostly drums. When it is 20 or more either way, the key is detected after bringing the track back to 440 Hz, so `key` names the nearest key at concert pitch.

`key_timeline` is the key over time, as sections in order that together cover the track from its start to `duration_seconds`. A track in which no change of key was found has one section, in the key of the track. A track with no key has none. When there are several sections, each carries the key that fits that stretch best, while `key` stays the key that fits the whole track, so `key` is usually but not always one of them. Section boundaries are accurate to a few seconds.

[Accuracy](../concepts/accuracy.md#tuning-and-key-over-time) describes how both are measured and how far to trust them.

### Progress and exit status

A progress bar is shown when the output goes to a terminal, and hidden when it is piped to another program.

When some results came from the cache, a line such as `120 of 124 tracks came from the cache.` is written to standard error, so piped output holds only the table or the JSON.

The command exits with status 0 when every file was analyzed. If any file failed, the table is followed by a list of the failed files and the reason, and the command exits with status 1. It also exits with status 1, after printing the results, if saving them to the cache failed partway through the run. A cache that cannot be used at all only causes a [warning](#when-the-cache-cannot-be-used) and does not change the exit status.

## euphoria export rekordbox

Analyzes audio files and writes the results as a rekordbox XML file, which rekordbox imports with keys, tempos and playlists. [Export to rekordbox](../guides/export-to-rekordbox.md) walks through the export and the import.

```
euphoria export rekordbox [OPTIONS] --output <FILE> <PATHS>...
```

Paths are searched as in [`euphoria analyze`](#euphoria-analyze), and results come from the [cache](#the-analysis-cache) when the tracks were analyzed before. Each path is turned into a full path first, taken from the current directory when it is relative, with any symbolic link in it followed, because rekordbox finds tracks by their full path.

```
$ euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml (336 from the cache)
In rekordbox: 1. Preferences > View > Layout: tick rekordbox xml. 2. Preferences > Advanced > Database: choose this file. 3. Refresh rekordbox xml in the tree, then import its tracks or playlists.
Full guide: https://github.com/lvndry/euphoria/blob/main/docs/guides/export-to-rekordbox.md
```

### Options

| Option | Effect |
| --- | --- |
| `-o`, `--output <FILE>` | The file to write. Required. |
| `--force` | Replace the output file if it exists. Without it, an existing file is an error and nothing is analyzed. |
| `--key-notation <NOTATION>` | `camelot` (the default) writes keys such as `8A`, `musical` writes keys such as `Am`. |
| `--no-playlists` | Write the tracks only, without playlists. |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |

### What is written

Each track has its full path, its duration, its key and tempo when Euphoria found them, its energy rating as the comment `Energy 7` when it has one, and its title and artist. Title and artist are read from the file's own tags: ID3 in MP3 and AIFF, Vorbis comments in FLAC, MP4 metadata in M4A and the `INFO` list in WAV. A file with no title is given its file name without the extension. So is a file whose tags cannot be read, which is exported without an artist.

Playlists mirror the folders:

- Each folder named on the command line becomes one entry at the top level, in the order given.
- A folder holding only tracks becomes a playlist. A folder holding other folders with tracks becomes a playlist folder, and tracks sitting directly in it go into a playlist of the same name, first inside it.
- A folder with no tracks anywhere inside is left out.
- Files named directly go into one last playlist, named after the output file without its extension.
- When one named folder is inside another, each gets its own entry.
- An entry that would have the same name as an earlier entry beside it gets ` (2)`, ` (3)` and so on after its name.

The file is written under a temporary name in the folder of the output file and moved into place once it is complete, so the output file is never half written. Without `--force` it only takes a name that is free: a file that another program creates at the output path while the export runs is kept, and the export fails.

### Output and exit status

Nothing is written to standard output. Standard error gets the progress bar when it is a terminal, one line saying how many tracks and playlists were exported and where, and two lines on importing the file into rekordbox. The summary ends with how many tracks came from the cache when some did.

Only a file that could not be analyzed is left out of the export. Such files are listed on standard error with the reason, before the summary, and the command exits with status 1 after writing the file for the other tracks:

```
Left 1 file out of the export:
  /Users/you/Music/DJ/notes.txt: cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: unsupported feature: core (probe): no suitable format reader found
Exported 339 tracks in 12 playlists to /Users/you/Music/euphoria.xml
```

A track whose key and tempo are known but whose tags cannot be read is still exported, under its file name and without an artist. This happens when the result came from the cache and the file can no longer be opened, for example after its permissions changed. Such tracks are listed in a note before the summary, and the note alone does not change the exit status:

```
Could not read the title of 1 file, used its file name:
  /Users/you/Music/DJ/Low Tide.flac: cannot open /Users/you/Music/DJ/Low Tide.flac: Permission denied (os error 13)
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml (340 from the cache)
```

A damaged tag does not cause the note. Either the file still opens and is exported under its file name, or the damage stops the file from being decoded and it is listed as left out.

The command also exits with status 1, and writes no file, when:

- the output file exists and `--force` was not given, or its folder does not exist,
- the output file is one of the audio files to export, reached by any path or symbolic link, or it has an audio file extension and is inside a folder to export. `--force` does not change this, so an export never replaces a track,
- no audio file was found, or none could be exported,
- folders are nested more than 32 levels deep, the most a rekordbox XML file can hold. The error names the first folder beyond the limit.

As with `euphoria analyze`, a cache that cannot be used only causes a [warning](#when-the-cache-cannot-be-used), and a failure to save results to the cache partway through gives status 1 after the file is written.

## euphoria tags write

Analyzes audio files and writes the key, tempo and energy into their tags, after showing every change and asking for confirmation. [Write tags into your files](../guides/write-tags.md) walks through it, and [Tags](tags.md) lists the fields and describes how a file is rewritten.

```
euphoria tags write [OPTIONS] <PATHS>...
```

Paths are searched as in [`euphoria analyze`](#euphoria-analyze), and results come from the [cache](#the-analysis-cache) when the tracks were analyzed before.

```
$ euphoria tags write ~/Music/DJ
/Users/you/Music/DJ/low tide.flac
  INITIALKEY: (none) -> 8A
  KEY: (none) -> 8A
  BPM: (none) -> 124.04
  COMMENT: https://example.com/track/42 -> 8A - Energy 7 - https://example.com/track/42
340 files to change, 2 already up to date.
Write these changes to 340 files? [y/N] y
Wrote the tags of 340 files.
To put the old tags back: euphoria tags undo
```

### Options

| Option | Effect |
| --- | --- |
| `--dry-run` | Show the preview and write nothing. Cannot be combined with `--yes`. |
| `-y`, `--yes` | Write without asking for confirmation. |
| `--key-notation <NOTATION>` | `camelot` (the default) writes keys such as `8A`, `musical` writes keys such as `Am`. Applies to the key field and to the label. |
| `--no-tempo` | Leave the tempo field alone. |
| `--label <WHERE>` | Where the key and energy label, such as `8A - Energy 7`, goes: `comment-start` (the default), `comment-end`, `grouping`, or `none` to write no label. The text already in the field is kept. |
| `--preset <PROGRAM>` | The DJ program whose tag fields are written: `generic` (the default), `rekordbox`, `serato`, `traktor` or `engine-dj`. |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 8. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |

### Preview and confirmation

The preview goes to standard output: the path of each file that would change, then one line per field with the value it has and the value it would get. `(none)` stands for a field the file does not have. Files that already hold the values are not listed and are never touched. Producing the preview only reads the files.

Standard error then gets one line with the number of files to change, the number already up to date and the number left out because of errors, and the question. Only `y` or `yes` writes. With `--dry-run` there is no question. When there is nothing to change there is no question either.

When standard input is not a terminal and `--yes` was not given, nobody can answer the question. The command then writes nothing, says so, and exits with status 1. An answer sent through a pipe is not read.

### What is written and how

Each file is written as described in [Tags](tags.md#how-a-file-is-rewritten): only the fields in the preview change, every other byte of the file is carried over, and the new file is checked against the original before it takes its place. The backup of each file is saved in the `tag-writes` folder of the [data folder](#where-the-cache-is-stored) before the file is changed.

### Exit status

Paths, tag values and error messages come from the files, so before they are shown, characters that could act on the terminal, such as a carriage return, an escape sequence or a text direction override, are replaced by visible escapes such as `\r` and `\u{1b}`. A file cannot erase or disguise the preview you are asked to confirm. The same holds for the table of `euphoria analyze` and the messages of `euphoria export`. What is written into the tags is the real text. JSON output carries the real text with JSON's own escapes.

A file that fails does not stop the others. A file that cannot be analyzed, or whose tags cannot be read or written, is listed on standard error as `error: <path>: <reason>` and left unchanged. That includes the files the tag writer refuses, such as a file with more than one hard link or a file another program changed during the write.

If a file was changed but the log of the write could not note it, for example because the disk filled up, the file is listed as `warning: <path>: the file was changed, but the record of it could not be saved`. Its backup was saved before the change, so `euphoria tags undo` still restores it.

The command exits with status 0 when every file was written or was already up to date, and also when you answered no. It exits with status 1 when any file failed, when a change could not be recorded, when it could not ask for confirmation, and, after writing, when saving analysis results to the cache failed partway.

## euphoria tags undo

Puts back the tags that the last `euphoria tags write` replaced.

```
$ euphoria tags undo
/Users/you/Music/DJ/low tide.flac
...
Restored the tags of 340 files.
```

Each restored file is listed on standard output and is byte for byte what it was before the write. Running the command again undoes the write before that one. When there is nothing to undo it says so and exits with status 0.

A file whose tags changed after the write is not restored, so that the later change is not lost. It is listed on standard error with the reason, the other files are restored, and the command exits with status 1. The backup of such a file is kept, and running the command again tries it again. An earlier write cannot be undone while such a file remains.

A write that was interrupted can be undone as well. Files it had not changed yet are counted in the summary as not changed by the write.

## The analysis cache

`euphoria analyze` and `euphoria export` remember every result in a SQLite database on your computer, so a track is analyzed once. A later run uses the remembered result instead of analyzing the file again.

A track is recognised by its content, not by its name or folder:

- A file that was renamed or moved is still recognised, and is not analyzed again.
- A file whose content changed is analyzed again. Editing tags changes the content, so a retagged file is analyzed once more.
- A file that could not be analyzed is not remembered, and is tried again on the next run.
- Results from a version of Euphoria that detected keys or tempo differently, or rated energy with a different version of its scale, are ignored and replaced. This includes results stored before Euphoria rated energy, results stored before tempo was reported between 90 and 180 BPM, which changed the tempo of many tracks and the energy rating of some, and results stored before the tuning and the key over time were measured. The first run after such an update analyzes every track once more.
- A file that changes while it is being analyzed is not remembered, and is analyzed again on the next run.

A file found where it was last seen, with the same size and modification time, is not read at all. Any other file is read once to compute a BLAKE3 hash of its content, which takes a small fraction of the time an analysis does.

### Where the cache is stored

| System | Location |
| --- | --- |
| macOS | `~/Library/Application Support/euphoria/analysis-cache.sqlite3` |
| Linux | `$XDG_DATA_HOME/euphoria/analysis-cache.sqlite3`, or `~/.local/share/euphoria/analysis-cache.sqlite3` when `XDG_DATA_HOME` is not set |

Set the `EUPHORIA_DATA_DIR` environment variable to keep the database in another folder:

```
EUPHORIA_DATA_DIR=/Volumes/Music/.euphoria euphoria analyze /Volumes/Music
```

A relative value is taken from the directory the command is run in, so `EUPHORIA_DATA_DIR=data` run in `/Users/you` means `/Users/you/data`. `euphoria cache path` prints the absolute location that results.

Several `euphoria analyze` and `euphoria export` commands can run at the same time and share one database.

### When the cache cannot be used

The cache only saves time, so a problem with it never stops an analysis. If the database cannot be opened or read, for example because the file is damaged, its folder is read only, or it was written by a newer version of Euphoria, `euphoria analyze` and `euphoria export` print a warning on standard error and analyze every file without the cache:

```
warning: analyzing without the cache: cannot use the analysis cache at /Users/you/Library/Application Support/euphoria/analysis-cache.sqlite3: file is not a database
  Run `euphoria cache clear` to reset the cache, or pass --no-cache to skip it.
```

The results and the exit status are the same as with `--no-cache`. Nothing is read from or saved to the cache in that run.

## euphoria cache

Manages the analysis cache.

```
euphoria cache <COMMAND>
```

| Command | Effect |
| --- | --- |
| `path` | Print the location of the cache database. |
| `clear` | Forget every cached result. Every track is analyzed again on the next run. |

`clear` empties the database and keeps the file, so it is safe while an analysis is running: that analysis carries on, and the results it saves after the clear are kept. A database that cannot be opened, such as a damaged file, is deleted instead, and the next run creates a new one. Unlike `euphoria analyze` and `euphoria export`, both commands report a problem as an error and exit with status 1.

```
$ euphoria cache path
/Users/you/Library/Application Support/euphoria/analysis-cache.sqlite3
```
