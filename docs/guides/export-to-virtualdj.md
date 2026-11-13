---
title: Export to VirtualDJ
description: Write the keys, tempos, beat grids, cue points and energy ratings Euphoria found into the songs of your VirtualDJ database.
---

# Export to VirtualDJ

One command analyzes your music and writes the keys, tempos and energy ratings into the songs VirtualDJ already knows, in its library file `database.xml`:

```
euphoria export virtualdj ~/Music/DJ
```

Euphoria saves a copy of `database.xml` first, changes only the songs of the tracks you export, and keeps everything else in the file as it was. Your audio files are never modified.

**Tested in VirtualDJ: no.** VirtualDJ is not installed on the machine Euphoria is developed on. The format follows VirtualDJ's own [description of its database](https://www.virtualdj.com/wiki/VDJ_database.html), a database VirtualDJ wrote, published on the [Mixxx wiki](https://github.com/mixxxdj/mixxx/wiki/Virtual-Dj-Cue-Storage-Format), and the reader and writer of [vdj-data](https://github.com/epistemex/vdj-data) (`src/vdj.poi.js` and `src/vdj.song.js`). Euphoria's tests merge into databases, read them back and compare, and check that every byte outside the changed attributes is kept. That is not the same as opening the result in VirtualDJ. If you try it, please report what you see.

## Before you start

- **Close VirtualDJ.** VirtualDJ keeps its database in memory and saves it when it closes, which would undo the export. Euphoria refuses to write while VirtualDJ is running.
- **Let VirtualDJ see your tracks once.** Euphoria updates the songs the database already holds; it does not add new ones. Browse the folders in VirtualDJ, or analyse them, then close it.
- VirtualDJ's own page on the database says that other programs may read it and that writing to it is not recommended, and that databases damaged by other programs are not supported. Euphoria keeps a copy so you can always go back.

## Export your library

```
euphoria export virtualdj ~/Music/DJ --dry-run
euphoria export virtualdj ~/Music/DJ --beat-grid --cues
```

The first command checks the database and says how many songs would change, writing nothing. The second writes. Then start VirtualDJ.

| Option | What it does |
| --- | --- |
| `--database <FILE>` | The `database.xml` to update. Found by itself in your home folder when not given. |
| `--label <WHERE>` | Where the key and energy label, such as `8A - Energy 7`, goes: `comment-start` (the default), `comment-end`, `grouping` or `none`. |
| `--key-notation <NOTATION>` | How the key is spelled in the label: `camelot` (the default), `musical` or `open-key`. |
| `--beat-grid` | Write each track's beat grid, except grids rated low confidence. |
| `--cues` | Write cues where each track's sections start, as hot cues 1 to 8. Needs `--beat-grid`. |
| `--dry-run` | Show what would change and write nothing. |

## Where the database is

| System | File |
| --- | --- |
| macOS | `~/Library/Application Support/VirtualDJ/database.xml` |
| Windows | `Documents\VirtualDJ\database.xml` in your user folder |

Tracks on an external drive are not in that file: VirtualDJ keeps them in `VirtualDJ/database.xml` at the root of the drive. Give that file with `--database` to update them.

## What Euphoria writes

Each song is found by its `FilePath`. In a song that matches one of your tracks:

| Information | Where it goes |
| --- | --- |
| Key | `Key` of `<Tags>` and `<Scan>`, as a musical name with sharps such as `Am` or `F#m`. VirtualDJ shows it in the notation of its `keyDisplay` option. |
| Tempo | `Bpm` of `<Tags>` and `<Scan>`. VirtualDJ stores the time between two beats in seconds, so 128 BPM is `0.468750`. |
| Beat grid | `Phase` of `<Scan>`, and one `<Poi Type="beatgrid">` at the first downbeat in place of the song's grid points. VirtualDJ 2026 reads the anchor from `Phase` ([beatgrid_fix](https://github.com/DeedjayNo1/beatgrid_fix)); older versions read the point. |
| Cues | `<Poi Name="Drop" Pos="30.000000" Num="2" />`, one per hot cue, numbered from 1. |
| Energy | The label, at the start or end of `<Comment>`, or at the start of `Grouping` in `<Tags>`. |

Before and after, for a track in A minor at 128 BPM with energy 7 and a hot cue on pad 2:

```xml
<Song FilePath="/music/a.mp3" FileSize="1">
 <Tags Author="A" Title="T" Flag="1" />
 <Infos SongLength="300.0" PlayCount="7" />
 <Comment>my note</Comment>
 <Scan Version="801" Bpm="0.500000" Key="C" Flag="32768" />
 <Poi Pos="0.100000" Type="beatgrid" />
 <Poi Pos="10.000000" Type="automix" Point="fadeStart" />
</Song>
```

```xml
<Song FilePath="/music/a.mp3" FileSize="1">
 <Tags Author="A" Title="T" Flag="1" Bpm="0.468750" Key="Am" />
 <Infos SongLength="300.0" PlayCount="7" />
 <Comment>8A - Energy 7 - my note</Comment>
 <Scan Version="801" Bpm="0.468750" Key="Am" Flag="32768" Phase="0.250000" />
 <Poi Pos="0.250000" Type="beatgrid" />
 <Poi Pos="10.000000" Type="automix" Point="fadeStart" />
 <Poi Name="Drop" Pos="30.000000" Num="2" />
</Song>
```

## What is kept

- Every other song, element and attribute, including ones Euphoria does not know, and the file's layout and line endings.
- Play counts, colours, automix points and anything else in the songs it changes.
- Your own cues: a song that has a cue point in VirtualDJ gets no cues from Euphoria.
- The text of your comment or grouping. A label an earlier export wrote is replaced, so exporting twice leaves one label.

## Undo

Close VirtualDJ and put the backup Euphoria made, `database.xml.euphoria-backup-<date>-<time>` next to `database.xml`, in place of `database.xml`.

## Limits

- VirtualDJ has no memory cues, so cues on no hot cue pad are left out.
- A song VirtualDJ has not analysed has no `<Scan>`, so it gets key and tempo in `<Tags>` and the grid point only. VirtualDJ may replace them when it analyses the song.
- VirtualDJ may replace the tempo and grid when it analyses a track again.
- Songs not in the database are reported and left out.

## Sources

- VirtualDJ, [VDJ database](https://www.virtualdj.com/wiki/VDJ_database.html): the elements and attributes, and `Bpm` as seconds per beat.
- Mixxx wiki, [Virtual DJ cue storage format](https://github.com/mixxxdj/mixxx/wiki/Virtual-Dj-Cue-Storage-Format): a database VirtualDJ wrote, with its beat grid and cue points.
- [vdj-data](https://github.com/epistemex/vdj-data): a reader and writer of `database.xml`, `src/vdj.poi.js` and `src/vdj.song.js`.
- [beatgrid_fix](https://github.com/DeedjayNo1/beatgrid_fix): the `Phase` attribute of VirtualDJ 2026.
