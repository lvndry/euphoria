---
title: Export to Traktor
description: Bring the keys, tempos, beat grids and cue points Euphoria found into Traktor Pro with an NML collection file.
---

# Export to Traktor

One command analyzes your music and writes the keys, tempos and energy ratings as a Traktor collection file (NML):

```
euphoria export traktor ~/Music/DJ --output ~/Music/euphoria.nml
```

Traktor imports that file into your collection with **Import Another Collection**. Your audio files are never modified, and Euphoria never writes your own `collection.nml`.

**Tested in Traktor: no.** Traktor is not installed on the machine Euphoria is developed on. The file follows the layout of a real Traktor Pro 4 `collection.nml` published by the [traktor-nml-utils](https://github.com/wolkenarchitekt/traktor-nml-utils) project, and the NML writer of [dj-data-converter](https://github.com/digital-dj-tools/dj-data-converter), whose authors checked it in Traktor. Euphoria's tests write a library, read it back and compare, and check that the file uses only elements and attributes found in that real collection. That is not the same as opening it in Traktor. Steps marked **(not verified)** come from those projects and from Native Instruments' manual, not from a test. If you try it, please report what you see.

## Export your library

```
$ euphoria export traktor ~/Music/DJ --output ~/Music/euphoria.nml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.nml
To import it in Traktor:
  1. Back up collection.nml in Documents/Native Instruments/Traktor <version>.
  2. In the browser tree, right click Track Collection > Import Another Collection.
  3. Choose this file. Lock the beat grid or untick BPM before analyzing these tracks.
Full guide: https://github.com/lvndry/euphoria/blob/main/docs/guides/export-to-traktor.md
```

The options are those of [the rekordbox export](export-to-rekordbox.md#export-your-library), with the same playlists that mirror your folders:

- `--force` replaces an output file that exists.
- `--key-notation camelot` (the default), `musical` or `open-key` chooses how the key is spelled in Traktor's **Key Text** column.
- `--no-playlists` exports the tracks without playlists.
- `--beat-grid` adds Euphoria's [beat grid](#beat-grids).
- `--cues --collection <collection.nml>` adds [cue points](#cue-points).
- `--playlist <NAME>` exports a playlist built in the [desktop app](desktop-app.md#playlists).
- `--startup-volume <NAME>` names the Mac disk your system is on. See [file locations](#file-locations).

The [command reference](../reference/cli.md#euphoria-export-traktor) lists every option.

## What the file contains

| Information | Written as | When |
| --- | --- | --- |
| File location | `LOCATION`: volume, folders and file name | Always |
| Title and artist | `ENTRY TITLE` and `ARTIST`, from the file's tags | Title always (the file name when the tags have none), artist when the tags have one |
| Duration | `INFO PLAYTIME` in seconds and `PLAYTIME_FLOAT` | Always |
| Key | `INFO KEY`, the **Key Text** column, in the notation you chose, and `MUSICAL_KEY`, the **Key** column | When Euphoria found one |
| Tempo | `TEMPO BPM`, with six decimals | When Euphoria found one |
| Energy | `INFO COMMENT`, as `Energy 7` | When Euphoria rated the track |
| Beat grid | A grid marker (`CUE_V2` type 4) at the first downbeat, with the grid's tempo | With `--beat-grid`, for grids rated medium or high |
| Cue points | Hot cues 1 to 8 (`CUE_V2` type 0) named Start, Main, Breakdown, Drop and Outro, or Section 2, Section 3 and so on | With `--cues`, on tracks with a grid in the file and no cues in your collection |

Traktor's own **Key** column is filled from `MUSICAL_KEY`, numbered 0 to 11 for the major keys from C and 12 to 23 for the minor keys from C minor. That numbering was checked against five tracks of the real Traktor Pro 4 collection, where `INFO KEY` and `MUSICAL_KEY` are both present. Mixxx's Traktor reader uses a numbering one semitone off from it; Euphoria follows the real file.

Euphoria writes no `AUDIO_ID`, `VOLUMEID`, loudness, waveform or stem data. Traktor computes those itself when it analyzes the tracks. dj-data-converter writes NML without them too.

## File locations

Traktor names every file by its volume, its folders separated by `/:`, and its file name, and finds tracks that way:

| Path on your computer | Volume in the file | Folders in the file |
| --- | --- | --- |
| `/Users/you/Music/a.mp3` on a Mac | `Macintosh HD`, the name of the disk your system is on | `/:Users/:you/:Music/:` |
| `/Volumes/USB Stick/Sets/b.flac` | `USB Stick` | `/:Sets/:` |
| `C:\Music\c.wav` on Windows | `C:` | `/:Music/:` |

Euphoria finds the name of your system disk by itself, from the entry of `/Volumes` that leads to `/`. If you renamed the disk, that is the name it finds. Pass `--startup-volume` only when you export for another computer. **(not verified)**: Traktor's handling of a location written by another program has not been tested here; dj-data-converter writes the same layout.

## Beat grids

With `--beat-grid`, each track whose [beat grid](../concepts/accuracy.md#beat-grid) is rated medium or high gets a grid marker at its first downbeat, with the grid's tempo. Traktor holds one tempo per track, which is what Euphoria's grids are. A grid rated low is left out, and Traktor draws its own when it analyzes the track.

The tempo is written with six decimals, as Traktor writes its own, so it does not have the rounding the rekordbox export has.

Traktor's analysis replaces an imported grid. Before analyzing imported tracks, either lock them (the lock icon, or **Lock** in the track's context menu) or analyze with **Analyze (Async) > Special** and untick **BPM**, as dj-data-converter advises. **(not verified)**

## Cue points

With `--cues`, each track whose beat grid is written gets hot cues 1 to 8 at the starts of its sections: Start, Main, Breakdown, Drop and Outro, or numbered sections on a track without that arc, at most 8, each on the first beat of a bar. Traktor lists hot cues among its cues, so the export does not add a second, memory copy of each one, as the rekordbox export does. dj-data-converter does the same.

Euphoria never changes cues you set. `--cues` needs `--collection`, naming your Traktor `collection.nml`, which is only read:

```
euphoria export traktor ~/Music/DJ --output ~/Music/euphoria.nml --beat-grid --cues \
  --collection ~/Documents/"Native Instruments"/"Traktor 3.11.1"/collection.nml
```

A track listed there with any cue, hot cue or loop of its own is left out of the file entirely, so importing the file cannot touch its cues. Grid markers, and the `AutoGrid` hot cue Traktor sets on a grid marker when it analyzes a track, do not count as cues of your own. A track with cues in the collection whose file is not on this computer stops the export, as for rekordbox; `--allow-unmatched-cued-tracks` lets it go ahead. When `--collection` is supplied, Euphoria refuses to use that file as the output, even with `--force`. Without `--collection`, `--force` replaces the existing file at `--output`; use a separate file for the export.

The cues are right only where the bars of the grid are. On about half of the benchmark excerpts the first downbeat is off by one to three beats, and every cue with it. Look at a few tracks first.

## Import into Traktor

These steps follow Native Instruments' Traktor Pro manual and dj-data-converter's guide. They were not tried in Traktor here. **(not verified)**

1. Quit Traktor and back up `collection.nml`. It is in `Documents/Native Instruments/Traktor <version>/` on both macOS and Windows. Copy it somewhere safe.
2. Open Traktor. In **Preferences > File Management**, set tag writing to write no tags or only Traktor's own tags if you do not want Traktor to rewrite the files' key and tempo tags. In **Preferences > Analyze Options**, choose how the key is shown; the **Key Text** column shows Euphoria's text as written.
3. In the browser tree, right click **Track Collection** and choose **Import Another Collection**. Choose the file Euphoria exported.
4. The tracks appear in the collection with their key, tempo, comment, grid and cues, and the playlists appear under **Playlists**.
5. Lock the imported tracks, or untick **BPM** when you analyze them, so that Traktor does not replace the grid.

Never copy Euphoria's file over your `collection.nml`. That would replace your whole collection with the exported tracks.

## What happens to tracks you already have

**Unconfirmed.** No source read for Euphoria says field by field what **Import Another Collection** does to a track that is already in the collection. Expect it to replace the key, tempo, comment, grid and cues of that track with the file's. That is why `--cues` leaves out every track with cues of its own in your collection. Try the import on a few tracks first, with your backup at hand.

## If something looks wrong

| What you see | Likely cause |
| --- | --- |
| Tracks show as missing | The volume name in the file is not the one Traktor knows, or the files moved. Check the disk's name in Finder and pass `--startup-volume`, or export again from the files' current place. |
| The Key column is empty but Key Text is filled | Traktor read `INFO KEY` but not `MUSICAL_KEY`. Analyze the key in Traktor, or report it. |
| The grid moved after analysis | Traktor's analysis replaced it. Lock the tracks before analyzing, or untick BPM. |
