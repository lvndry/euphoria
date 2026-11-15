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

While it works, each track being analyzed has a line saying what Euphoria is doing with it, such as decoding or finding the key, and after a few seconds how long it has taken. A bar above counts the finished tracks.

## Read the table

Each row is a track: its key, coloured by its place on the Camelot wheel, its tempo, its energy from 1 to 10, its sound quality in a word, its length and its file. The dots after the key and the tempo say how sure Euphoria is: `●●●` sure, `●●○` fairly sure, `●○○` worth checking by ear. In a narrow window, the less needed columns are left out.

Below the table, a summary says what the numbers count, for example:

```
Key and tempo: 3 of 124 tracks rated low confidence (●○○), worth checking by ear.
Sound quality: 86 tracks good, 31 weak on a big system, 7 likely re-encoded.
hint: --needs-review lists only the tracks to check by ear.
hint: euphoria quality lists the files worth replacing, worst first, with why.
```

## Look at one track closely

Analyze a single file and Euphoria shows everything it found: the key with its runner up, the tempo with its half or double, the energy, why the sound quality got its verdict, the loudness, the tuning and the length.

```
euphoria analyze ~/Music/DJ/new-track.flac
```

`--detail` shows every track of a folder that way.

## Find the tracks worth checking or replacing

```
euphoria analyze --needs-review ~/Music/DJ
euphoria analyze --quality weak,reencoded ~/Music/DJ
```

The first lists the tracks whose key or tempo is rated low, or whose tags name another key than the detected one, the second the files whose sound quality is weak on a big system or that were likely made from a lower quality file. The summary says how many of the tracks are shown.

`euphoria quality ~/Music/DJ` lists the files worth replacing worst first, damaged and clipped ones included, each with why. See [find the files to replace](../concepts/audio-quality.md#find-the-files-to-replace).

## Analyze again after adding music

Euphoria remembers every result, so run the same command again whenever you add tracks. Only the new ones are analyzed, and the rest are printed straight away:

```
$ euphoria analyze ~/Music/DJ
...
120 of 124 tracks came from the cache.
```

You can rename files and reorganize folders freely. A track is recognised by its content, so it is not analyzed again after it moves. Editing a file's tags in another program changes its content, so a track retagged that way is analyzed once more. Tags written by Euphoria, and undone by it, do not cost an analysis.

To analyze tracks again whatever is remembered, for example after a result looked wrong:

```
euphoria analyze --reanalyze ~/Music/DJ/track.flac
```

If the remembered results cannot be used, for example because their file is damaged, Euphoria says so in a warning and analyzes every track anyway. `euphoria cache clear` fixes a damaged file.

`--no-cache` analyzes without reading or saving anything. `euphoria cache path` prints where the results are kept and `euphoria cache clear` forgets all of them. The [command reference](../reference/cli.md#the-analysis-cache) has the details.

## Large libraries and memory

Euphoria analyzes several tracks at once: as many as your computer has cores, up to 8. Each one peaks near 0.6 GB of memory on a seven minute track, and 8 at once peaked between 4.8 and 6.2 GB on full length tracks. The minutes of audio in flight are limited as well, so a folder of long mixes cannot use more memory than a folder of tracks (see [long files and mixes](#long-files-and-mixes)). On a computer with 8 GB of memory or less, analyze fewer tracks at once:

```
euphoria analyze --jobs 4 ~/Music/DJ
```

### Long files and mixes

A count of workers does not bound memory when files are long: eight one hour mixes at once would need about 40 GB. Euphoria therefore also limits the minutes of audio in flight. A file waits until the files being analyzed plus its own length fit in 60 minutes of audio, about 5 GB; seven minute tracks never wait, and one hour mixes are analyzed one at a time. Change the limit with `--audio-budget`:

```
euphoria analyze --audio-budget 30 ~/Music/Mixes
```

A single file longer than the limit is analyzed alone, and still needs memory for its whole length, near 86 MB for each minute: about 5 GB for a one hour mix and 15 GB for three hours. The desktop app uses the same limit. Analyzing key, tempo and energy on a part of a very long file would cut this, but the beat grid and sections need the whole track, so it is not done yet.

## When a folder cannot be read or a file is skipped

A folder that cannot be read, because its permission is denied or a network drive fails partway through, no longer stops the scan. Euphoria analyzes everything else and ends with a warning that counts the folders and gives the reason for each, such as `2 folders could not be read: ...`. The command then exits with status 3 when nothing else failed, so a script knows the scan was incomplete. The desktop app shows the same sentence as a notice.

Audio in formats Euphoria cannot decode yet, such as `.opus` and `.wma`, is counted and reported too, as in `14 files skipped: 9 .opus, 5 .wma, which Euphoria can't decode yet`. See [supported formats](../reference/supported-formats.md).

## Damaged files

A truncated download or a corrupt file is analyzed as far as it can be and marked damaged, with how much is missing, in the detailed view, in the JSON and in the desktop inspector. A file that has lost 2% or more of its audio gets no beat grid and no cues, since they would be wrong. See [damaged files](../concepts/audio-quality.md#damaged-files).

## When a file fails

Euphoria keeps going when a file cannot be read. At the end it lists the files it could not analyze and why:

```
error: /Users/you/Music/DJ/notes.txt: cannot decode /Users/you/Music/DJ/notes.txt: unsupported format: ...
```

Common reasons are a file in a format Euphoria does not read, a file that is damaged, or a download that did not finish. The command then exits with status 1, which scripts can check.

## When no key or tempo is shown

A `-` in the Key, BPM or Energy column means Euphoria found nothing to measure, for example in silence or in a clip shorter than one second. It is not an error.

## Save the results

`--json` prints the results in a form spreadsheets and scripts can read:

```
euphoria analyze --json ~/Music/DJ > library.json
```

Without `--json`, output sent to a file or another program is a plain table with fixed columns and no colour, which scripts can read too. `--color never` turns colour off on a terminal as well, and so does setting `NO_COLOR`.

The [command reference](../reference/cli.md#json-output) describes every field.
