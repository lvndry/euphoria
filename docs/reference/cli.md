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
| `-j`, `--jobs <N>` | Analyze N files at once. The default is the number of cores, at most 4. |
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

## The analysis cache

`euphoria analyze` and `euphoria export` remember every result in a SQLite database on your computer, so a track is analyzed once. A later run uses the remembered result instead of analyzing the file again.

A track is recognised by its content, not by its name or folder:

- A file that was renamed or moved is still recognised, and is not analyzed again.
- A file whose content changed is analyzed again. Editing tags changes the content, so a retagged file is analyzed once more.
- A file that could not be analyzed is not remembered, and is tried again on the next run.
- Results from a version of Euphoria that detected keys or tempo differently, or rated energy with a different version of its scale, are ignored and replaced. This includes results stored before Euphoria rated energy, so the first run after that update analyzes every track once more.
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
