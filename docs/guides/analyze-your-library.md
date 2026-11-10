---
title: Analyze your library
description: Analyze a whole music library, handle failed files and use the results elsewhere.
---

# Analyze your library

## Analyze several folders at once

`euphoria analyze` takes any number of files and folders:

```
euphoria analyze ~/Music/DJ "/Volumes/USB DRIVE/Promos" ~/Downloads/new-track.flac
```

Folders are searched recursively. Hidden files are skipped, including the `._` copies macOS leaves on external drives, so a USB stick prepared on a Mac does not show every track twice.

## Large libraries and memory

Euphoria analyzes several tracks at once: as many as your computer has cores, up to 4. Each one can use 2 to 3 GB of memory on a full length track. On a computer with 8 GB of memory or less, analyze fewer tracks at once:

```
euphoria analyze --jobs 2 ~/Music/DJ
```

## When a file fails

Euphoria keeps going when a file cannot be read. At the end it lists the files it could not analyze and why:

```
Could not analyze 1 file:
  /Users/you/Music/DJ/notes.txt: cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: ...
```

Common reasons are a file in a format Euphoria does not read, a file that is damaged, or a download that did not finish. The command then exits with status 1, which scripts can check.

## When no key or tempo is shown

A `-` in the Key or BPM column means Euphoria found nothing to measure, for example in silence or in a clip shorter than one second. It is not an error.

## Save the results

`--json` prints the results in a form spreadsheets and scripts can read:

```
euphoria analyze --json ~/Music/DJ > library.json
```

The [command reference](../reference/cli.md#json-output) describes every field.
