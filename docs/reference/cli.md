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
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version. |

Each worker can use about 2 GB of memory on a full length track, which is why the default stops at 4. Lower `--jobs` on a machine with little memory.

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

The command exits with status 0 when every file was analyzed. If any file failed, the table is followed by a list of the failed files and the reason, and the command exits with status 1.
