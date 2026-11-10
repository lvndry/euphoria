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
    "error": null
  },
  {
    "path": "/Users/you/Music/DJ/notes.txt",
    "duration_seconds": null,
    "key": null,
    "bpm": null,
    "energy": null,
    "error": "cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: unsupported feature: core (probe): no suitable format reader found"
  }
]
```

`key`, `bpm` and `energy` are `null` when nothing was detected. `energy` is a whole number from 1 to 10. `error` is set only when the file could not be analyzed.

### Progress and exit status

A progress bar is shown when the output goes to a terminal, and hidden when it is piped to another program.

When some results came from the cache, a line such as `120 of 124 tracks came from the cache.` is written to standard error, so piped output holds only the table or the JSON.

The command exits with status 0 when every file was analyzed. If any file failed, the table is followed by a list of the failed files and the reason, and the command exits with status 1. It also exits with status 1, after printing the results, if saving them to the cache failed partway through the run. A cache that cannot be used at all only causes a [warning](#when-the-cache-cannot-be-used) and does not change the exit status.

## The analysis cache

`euphoria analyze` remembers every result in a SQLite database on your computer, so a track is analyzed once. A later run prints the remembered result instead of analyzing the file again.

A track is recognised by its content, not by its name or folder:

- A file that was renamed or moved is still recognised, and is not analyzed again.
- A file whose content changed is analyzed again. Editing tags changes the content, so a retagged file is analyzed once more.
- A file that could not be analyzed is not remembered, and is tried again on the next run.
- Results from a version of Euphoria that detected keys or tempo differently, or rated energy with a different version of its scale, are ignored and replaced. This includes results stored before Euphoria rated energy, and results stored before tempo detection dropped its multi-resolution pass, which changed the tempo of some tracks. The first run after such an update analyzes every track once more.
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

Several `euphoria analyze` commands can run at the same time and share one database.

### When the cache cannot be used

The cache only saves time, so a problem with it never stops an analysis. If the database cannot be opened or read, for example because the file is damaged, its folder is read only, or it was written by a newer version of Euphoria, `euphoria analyze` prints a warning on standard error and analyzes every file without the cache:

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

`clear` empties the database and keeps the file, so it is safe while an analysis is running: that analysis carries on, and the results it saves after the clear are kept. A database that cannot be opened, such as a damaged file, is deleted instead, and the next run creates a new one. Unlike `euphoria analyze`, both commands report a problem as an error and exit with status 1.

```
$ euphoria cache path
/Users/you/Library/Application Support/euphoria/analysis-cache.sqlite3
```
