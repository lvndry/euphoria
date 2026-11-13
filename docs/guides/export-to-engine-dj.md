---
title: Export to Engine DJ
description: Write the keys, tempos, beat grids and hot cues Euphoria found into the tracks of your Engine DJ library, or bring them in through a rekordbox XML file.
---

# Export to Engine DJ

One command analyzes your music and writes the keys, tempos, beat grids, hot cues and energy ratings into the tracks Engine DJ already has in its library:

```
euphoria export engine ~/Music/DJ --beat-grid --cues
```

Euphoria saves a copy of the library database first, changes only the tracks you export, and refuses any library it was not checked against. Your audio files are never modified.

**Tested in Engine DJ: no.** Engine DJ is not installed on the machine Euphoria is developed on. The format follows [libdjinterop](https://github.com/xsco/libdjinterop), the library Mixxx exports to Engine DJ with, and Euphoria's tests update libraries built from the exact schemas Engine DJ 2.0, 4.0.1, 4.3 and 5.0 create, read them back and compare. That is not the same as opening the result in Engine DJ. If you try it, please report what you see.

## Is writing the library safe

Engine DJ does not support programs that change its library. Its [article on third-party database tools](https://support.denondj.com/en/support/solutions/articles/69000834165) says:

> 3rd Party Applications which make changes to the Engine DJ folder should not be open simultaneously as Engine DJ. This can lead to database corruption.

> All .db files in the Engine Library/Database2 should not have their schema edited. Doing so will cause Engine DJ to refuse to load the database.

Euphoria keeps to both rules, and adds its own:

- It refuses to write while Engine DJ is running.
- It refuses when a journal file next to the database holds a write that did not finish.
- It refuses a schema version it was not checked against, and a library whose `Information` table does not hold exactly one row with id 1.
- It saves a copy of `m.db` before it writes.
- It only updates rows that exist, in one transaction. It never adds or removes tables, columns, indexes or rows.
- It checks the database with SQLite's quick check afterwards.

Even so, a program Engine DJ's makers did not test is writing their database. Keep the backup until you have checked your library in Engine DJ, and use `--dry-run` first.

### Schema versions

| Schema | Engine DJ |
| --- | --- |
| 2.18.0 | 2.0 to 2.1 |
| 2.20.1 | 2.2 to 2.3 |
| 2.20.2 | 2.4 |
| 2.20.3 | 3.0 |
| 2.21.0 | 3.1 to 3.4 |
| 2.21.1 | 4.0.0 |
| 2.21.2 | 4.0.1 |
| 3.0.0 | 4.1 to 4.2 |
| 3.0.1 | 4.3 |
| 3.0.2 | 4.5 and 5.0 |

From libdjinterop's `include/djinterop/engine/engine_schema.hpp`. Whether Engine DJ 5.1 and later keep schema 3.0.2 is not known. A newer schema is refused until Euphoria is checked against it.

## Export your library

1. Close Engine DJ.
2. Check what would change, writing nothing:

   ```
   euphoria export engine ~/Music/DJ --dry-run
   ```

3. Write:

   ```
   euphoria export engine ~/Music/DJ --beat-grid --cues
   ```

4. Start Engine DJ, check a few tracks, then sync your drives as usual.

| Option | What it does |
| --- | --- |
| `--library <FOLDER>` | The `Engine Library` folder. `Music/Engine Library` in your home folder when not given. |
| `--label <WHERE>` | Where the key and energy label, such as `8A - Energy 7`, goes in the comment: `comment-start` (the default), `comment-end` or `none`. |
| `--key-notation <NOTATION>` | How the key is spelled in the label: `camelot` (the default), `musical` or `open-key`. |
| `--beat-grid` | Write each track's beat grid, except grids rated low confidence. |
| `--cues` | Write cues where each track's sections start, as hot cues 1 to 8. Needs `--beat-grid`. |
| `--dry-run` | Check the library and say what would change, writing nothing. |

## Where the library is

Engine DJ keeps its library in `Music/Engine Library` in your home folder, on macOS and Windows; the database Euphoria updates is `Engine Library/Database2/m.db`. Each drive prepared for a player has its own `Engine Library` with its own database. Give it with `--library` to update the tracks on that drive.

## What Euphoria writes

A track is found by the path Engine DJ stores for it, relative to the `Engine Library` folder.

| Information | Where it goes |
| --- | --- |
| Key | `Track.key`, 0 to 23 along the circle of fifths from C major (0 C, 1 A minor, 2 G, and so on), and the key in the `trackData` blob. Engine DJ shows it in the notation set in its preferences. |
| Tempo | `bpmAnalyzed` exactly and `bpm` rounded. |
| Beat grid | `beatData`: a constant grid from four beats before the first downbeat to past the end of the track, in samples at the sample rate Engine DJ found, as libdjinterop writes it. |
| Cues | Hot cues 1 to 8 in `quickCues`, in Engine DJ's pad colours. Pad 7's colour is Euphoria's own: no library with pad 7 set was found to copy it from. The main cue is kept. |
| Energy | The label at the start or end of the comment, keeping your text. A label an earlier export wrote is replaced. |

## What is kept

- Every other track, and everything else in the tracks it changes: playlists, history, loops, waveforms, play counts, ratings.
- Your own hot cues: a track with a hot cue in Engine DJ gets no cues from Euphoria.

## Undo

Close Engine DJ and copy `Database2/m.db` from the folder `Engine Library backup by Euphoria <date> <time>`, next to your `Engine Library`, over `Engine Library/Database2/m.db`.

## Limits

- A track Engine DJ has not analysed has no sample rate in the library, so it gets key, tempo and label only. Analyse it in Engine DJ and export again.
- Re-analysing a track replaces its key, tempo and grid. Engine DJ's user guide: "Re-analyzing a track will always use the latest Engine DJ BPM and key detection".
- Memory cues and cues on pads above 8 are not written: Engine DJ has eight hot cues.
- Positions are counted from the start of the decoded audio. How Engine DJ's decoder handles the encoder delay at the start of MP3 and AAC files is not known, so grids and cues of those files may be off by a few milliseconds.
- Tracks not in the library are reported and left out.

## The other route: a rekordbox XML file

Engine DJ's Import Assistant reads a rekordbox XML file, so `euphoria export rekordbox --beat-grid --cues` reaches Engine DJ without writing its database. Turn on **Preferences > Library > Rekordbox Library**, then use **Import > Rekordbox** and choose the file. See [export to rekordbox](export-to-rekordbox.md).

By Engine DJ's user guide it imports "hot cues, memory loops and beat grids" and playlists. The guide does not say whether it takes the key (`Tonality`), tempo or comment from the file, and a track already in Engine DJ is not updated by a second import: it must be removed and imported again.

## Sources

- [libdjinterop](https://github.com/xsco/libdjinterop), commit `d61727c5`: the schemas in `src/djinterop/engine/schema/`, the blobs in `src/djinterop/engine/v2/*_blob.cpp`, the key numbers in `include/djinterop/musical_key.hpp`, and the schemas Engine DJ creates in `testdata/ref/engine/desktop/`, which Euphoria's tests use.
- Mixxx's Engine export, `src/library/export/engineprimeexportjob.cpp` in [Mixxx](https://github.com/mixxxdj/mixxx).
- [Open Engine Lib](https://github.com/benedikt-weyer/Open-Engine-Lib): what a real Engine DJ 5 library holds, including the pad colours.
- Engine DJ, [support for third-party database tools](https://support.denondj.com/en/support/solutions/articles/69000834165).
- [Engine DJ User Guide 5.1](https://cdn.inmusicbrands.com/engine/51/Engine%20DJ%20-%20User%20Guide%20-%20v5.1.0.pdf): the rekordbox import and re-analysis.
