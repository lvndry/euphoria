---
title: Command line
description: Every command, option, output format and exit status of the euphoria command.
---

# Command line

## euphoria analyze

Detects the key and tempo of audio files.

```
euphoria analyze [OPTIONS] <PATHS>...
```

Each path is a file or a folder. Folders are searched recursively for [supported audio files](supported-formats.md). Hidden files and folders are skipped, including the `._` copies macOS leaves on external drives. A file named directly is always analyzed, so an unsupported file is reported as a failure instead of being ignored.

A symbolic link to an audio file is analyzed. A symbolic link to a folder is not entered, so a scan stays inside the folders you named; to include a linked folder, name it on the command line.

```
$ euphoria analyze ~/Music/DJ
Key             BPM  Time  File
8A   A minor  124.0  6:48  /Users/you/Music/DJ/Low Tide.flac
-                 -  1:00  /Users/you/Music/DJ/Intro.wav
```

A `-` means nothing was detected, for example in silence or a track shorter than one second.

### Options

| Option | Effect |
| --- | --- |
| `--json` | Print a JSON array instead of a table. |
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 4. |
| `--no-cache` | Analyze every file, and neither read nor update the [cache](#the-analysis-cache). |
| `--reanalyze` | Analyze every file again and replace its cached result. Cannot be combined with `--no-cache`. |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

Each worker can use 2 to 3 GB of memory on a full length track, which is why the default stops at 4. Lower `--jobs` on a machine with little memory.

### JSON output

One object per file, in path order, including files that failed:

```json
[
  {
    "path": "/Users/you/Music/DJ/Low Tide.flac",
    "duration_seconds": 408.4,
    "key": { "camelot": "8A", "name": "A minor" },
    "bpm": 124.0,
    "error": null
  },
  {
    "path": "/Users/you/Music/DJ/notes.txt",
    "duration_seconds": null,
    "key": null,
    "bpm": null,
    "error": "cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: unsupported feature: core (probe): no suitable format reader found"
  }
]
```

`key` and `bpm` are `null` when nothing was detected. `error` is set only when the file could not be analyzed.

### Progress and exit status

A progress bar is shown when the output goes to a terminal, and hidden when it is piped to another program.

When some results came from the cache, a line such as `120 of 124 tracks came from the cache.` is written to standard error, so piped output holds only the table or the JSON.

The command exits with status 0 when every file was analyzed. If any file failed, the table is followed by a list of the failed files and the reason, and the command exits with status 1. It also exits with status 1, after printing the results, if they could not be saved to the cache.

## The analysis cache

`euphoria analyze` remembers every result in a SQLite database on your computer, so a track is analyzed once. A later run prints the remembered result instead of analyzing the file again.

A track is recognised by its content, not by its name or folder:

- A file that was renamed or moved is still recognised, and is not analyzed again.
- A file whose content changed is analyzed again. Editing tags changes the content, so a retagged file is analyzed once more.
- A file that could not be analyzed is not remembered, and is tried again on the next run.
- Results from a version of Euphoria that detected keys or tempo differently are ignored and replaced.

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

If the database cannot be opened, for example because the file is damaged, the command stops with an error that names the file. `euphoria cache clear` deletes it.

## euphoria cache

Manages the analysis cache.

```
euphoria cache <COMMAND>
```

| Command | Effect |
| --- | --- |
| `path` | Print the location of the cache database. |
| `clear` | Delete the cache database. Every track is analyzed again on the next run. |

```
$ euphoria cache path
/Users/you/Library/Application Support/euphoria/analysis-cache.sqlite3
```
