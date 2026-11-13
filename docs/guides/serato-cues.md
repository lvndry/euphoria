---
title: Cue points and beat grids for Serato
description: Write Euphoria's cue points and beat grid into the tags Serato DJ reads, keep the cues you already set, and undo it byte for byte.
---

# Cue points and beat grids for Serato

Serato DJ keeps its cue points and beat grids in the audio file itself, not in its library. One option of `euphoria tags write` puts Euphoria's cue points and beat grid there, next to the key:

```
euphoria tags write --preset serato --serato-markers ~/Music/DJ
```

Like every tag write, it shows each change first, asks before writing, and `euphoria tags undo` puts every file back exactly as it was, byte for byte.

**Tested in Serato: no.** Serato DJ is not installed on the machine Euphoria is developed on. Serato does not document this data. Euphoria follows the description that the [serato-tags](https://github.com/Holzhaus/serato-tags) project worked out by reading files Serato wrote, and the reader and writer in [Mixxx](https://github.com/mixxxdj/mixxx/tree/main/src/track/serato). Euphoria's tests read 37 fields that Serato DJ itself wrote, from Mixxx's test data, and write each one back. Every one comes back identical to the byte. Euphoria's own writes are then read back and merged with those real cues. That is as close as Euphoria can get without Serato. If you try it, please report what you see.

## What is written

| | MP3 and AIFF | M4A | FLAC |
| --- | --- | --- | --- |
| Cue points, with Serato's pad colours | `Serato Markers2` (ID3 `GEOB` frame) | `----:com.serato.dj:markersv2` | `SERATO_MARKERS_V2` |
| Beat grid | `Serato BeatGrid` | `----:com.serato.dj:beatgrid` | `SERATO_BEATGRID` |
| Older copy of the first five cues | `Serato Markers_`, only when the file has one | `----:com.serato.dj:markers`, added when missing | None |
| Key | `TKEY` | `initialkey` and `KEY` | `INITIALKEY` |

- **Cue points**: up to 8 hot cues at the starts of the track's sections, named Start, Main, Breakdown, Drop and Outro. Each one sits on the first beat of a bar at a phrase boundary, as described on the [accuracy](../concepts/accuracy.md#cue-points) page. Each pad gets the colour Serato gives it by default: red, orange, blue, yellow, green, magenta, cyan and purple.
- **Beat grid**: one marker at the first downbeat with the grid's tempo. Only grids rated medium or high are written. A track whose grid is rated low gets neither a grid nor cues, since the cues sit on its bars.
- **Key**: written in the key field by the preset, as without `--serato-markers`. See [use Euphoria with your DJ program](dj-programs.md#serato-dj).
- **Older copy**: Serato also keeps the first five cues in an older field, and prefers it when the file has one. A cue that is in the newer field only is ignored there. So when a file has that field, Euphoria's cues on pads 1 to 5 are added to it too. In M4A files, Serato reads the first five cues only from that field, so Euphoria adds it when it is missing.

The preview names what changes:

```
/Users/you/Music/DJ/low tide.flac
  INITIALKEY: (none) -> 8A
  Serato cue points: (none) -> cue points 1 Start at 0:00.146, 2 Drop at 1:03.500, 3 Outro at 6:40.250
  Serato beat grid: (none) -> beat grid from 0.146 s at 124.04 BPM
Serato: 3 cue points to add, 0 cue points of yours kept.
```

Not written: WAV files, where Serato reads no tags (as Serato's own staff said), and Ogg Vorbis files, where Serato uses a layout that is not known well enough to write. ID3v2.2 tags, an old version with no room for this data, are skipped too. Each one is listed as a warning, and its key is still written.

## Your cues are kept

Euphoria never removes, moves or changes a cue point, loop, flip, track colour or grid lock the file already has. Every entry of Serato's data that Euphoria does not add is written back as it was.

- **A pad that holds a cue of yours** stays yours. Euphoria's cue goes to the next free pad, and the command says so:

  ```
  warning: /Users/you/Music/DJ/low tide.mp3: Drop went to pad 4, since pad 2 holds a cue of yours
  ```

- **No free pad**: when all 8 pads hold your cues, Euphoria's cues are left out, and the command lists them.
- **A cue at the same place**: a cue of Euphoria's at the same position as one the file has, to the millisecond, is not added again. Running the command twice changes nothing the second time.
- **A beat grid of yours** is kept, never replaced. The command says so and gives its tempo:

  ```
  warning: /Users/you/Music/DJ/low tide.mp3: your Serato beat grid (124.00 BPM from 0.120 s) is kept, not replaced
  ```

  To use Euphoria's grid instead, clear the grid in Serato, save, and run the command again.
- **Data that cannot be read**: when a file's Serato data does not match the known layout, Euphoria writes no Serato data to that file and says which field it could not read.

The summary line counts the cues added and the cues of yours that were kept.

## Bring them into Serato

These steps follow Serato's documentation and Mixed In Key's Serato guide. They were not tried in Serato here. **(not verified)**

1. Close Serato DJ, or at least do not play the tracks while Euphoria writes them.
2. Run `euphoria tags write --preset serato --serato-markers` on your music folder, and check the preview before you answer yes.
3. Open Serato DJ. A track's cues and grid are read from the file when the track is loaded into a deck. For tracks Serato already shows, select them in the library and use **Rescan ID3 Tags** in the **Files** panel so the library columns catch up too.
4. Before you analyse these tracks in Serato, untick **Set Beatgrid / BPM** in the analysis options, or lock the grid of each track. Otherwise Serato's analysis may replace the grid, and the cues sit on its bars. Untick **Set Key** to keep Euphoria's key.
5. Load a track and check its cues and grid against the waveform.

Mixxx reads the same cues and grid from the file when it loads a track (MP3, M4A and FLAC). VirtualDJ is reported to read them when its `getCuesFromTags` option is on. Neither was tried here.

## Timing in MP3 files

Different MP3 decoders start some files a few dozen milliseconds apart, depending on how the file was encoded. Mixxx corrects this when it reads Serato's cues from some MP3 files. Euphoria writes positions as its own decoder finds them, so in some MP3 files Euphoria's cues and grid may sit up to about 26 ms off in Serato. Check a few tracks first. **(not verified)**

## Undo

`euphoria tags undo` restores every file of the last write, Serato fields included, to the exact bytes it had. A file you changed in Serato since the write is left alone and listed, so a cue you set afterwards is never lost. [Tags](../reference/tags.md#undo) has the details.
