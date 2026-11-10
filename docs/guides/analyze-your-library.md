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

## Analyze again after adding music

Euphoria remembers every result, so run the same command again whenever you add tracks. Only the new ones are analyzed, and the rest are printed straight away:

```
$ euphoria analyze ~/Music/DJ
...
120 of 124 tracks came from the cache.
```

You can rename files and reorganize folders freely. A track is recognised by its content, so it is not analyzed again after it moves. Editing a file's tags changes its content, so a retagged track is analyzed once more.

To analyze tracks again whatever is remembered, for example after a result looked wrong:

```
euphoria analyze --reanalyze ~/Music/DJ/track.flac
```

If the remembered results cannot be used, for example because their file is damaged, Euphoria says so in a warning and analyzes every track anyway. `euphoria cache clear` fixes a damaged file.

`--no-cache` analyzes without reading or saving anything. `euphoria cache path` prints where the results are kept and `euphoria cache clear` forgets all of them. The [command reference](../reference/cli.md#the-analysis-cache) has the details.

## Large libraries and memory

Euphoria analyzes several tracks at once: as many as your computer has cores, up to 8. Each one peaks near 0.6 GB of memory on a seven minute track, and 8 at once peaked between 4.8 and 6.2 GB on full length tracks. On a computer with 8 GB of memory or less, analyze fewer tracks at once:

```
euphoria analyze --jobs 4 ~/Music/DJ
```

## When a file fails

Euphoria keeps going when a file cannot be read. At the end it lists the files it could not analyze and why:

```
Could not analyze 1 file:
  /Users/you/Music/DJ/notes.txt: cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: ...
```

Common reasons are a file in a format Euphoria does not read, a file that is damaged, or a download that did not finish. The command then exits with status 1, which scripts can check.

## When no key or tempo is shown

A `-` in the Key, BPM or Energy column means Euphoria found nothing to measure, for example in silence or in a clip shorter than one second. It is not an error.

## Save the results

`--json` prints the results in a form spreadsheets and scripts can read:

```
euphoria analyze --json ~/Music/DJ > library.json
```

The [command reference](../reference/cli.md#json-output) describes every field.
