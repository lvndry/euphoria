---
title: Export to rekordbox
description: Bring the keys and tempos Euphoria found into your rekordbox collection with a rekordbox XML file.
---

# Export to rekordbox

One command analyzes your music and writes the keys, tempos and energy ratings as a rekordbox XML file:

```
euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
```

rekordbox reads that file as a second library sitting next to your own, and you choose which tracks and playlists to bring into your collection. Your audio files are never modified.

This is the route Pioneer documents for handing rekordbox a key, a tempo with two decimals and a comment, whatever the format of the file. It is also the only way to give rekordbox Euphoria's tempo: tested in rekordbox 7, it reads the key and comment from a file's tags when the track is added, but not the tempo, and its own analysis sets a tempo of its own. [Use Euphoria with your DJ program](dj-programs.md#rekordbox) has the details and a test kit to check your version.

The import steps below were written from Pioneer DJ's published description of the XML format and from rekordbox 6 and 7 documentation, and checked in rekordbox 7 with a test file. Steps marked **(not verified)** were not part of that check.

## Export your library

```
$ euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml
```

Name one or more folders or files, as with `euphoria analyze`. Tracks that were analyzed before come from the [cache](../reference/cli.md#the-analysis-cache), so exporting a library that is already analyzed takes seconds. New tracks are analyzed first, with a progress bar.

- The export refuses to replace a file that exists. Pass `--force` to replace it, for example when you export again after adding music.
- Keys are written as Camelot codes. Pass `--key-notation musical` for names such as `Am`.
- Pass `--no-playlists` to export the tracks without playlists.
- Pass `--beat-grid` to give rekordbox Euphoria's [beat grid](#beat-grids) as well.
- Pass `--cues --collection <FILE>` as well to add [cue points](#cue-points) at the sections of each track.

A file that cannot be analyzed is the only kind left out. It is listed with the reason, the rest is exported, and the command exits with status 1. The [command reference](../reference/cli.md#euphoria-export-rekordbox) has every option.

### Playlists follow your folders

The playlists in the file mirror the folders you exported, so a library sorted into genre folders arrives in rekordbox as the same tree:

```
~/Music/DJ/                    DJ (folder)
  House/                         House (playlist)
  Techno/                        Techno (folder)
    Peak Time.flac                 Techno (playlist, holds Peak Time.flac)
    Hypnotic/                      Hypnotic (playlist)
  Artwork/                       left out, it holds no tracks
```

- A folder that holds only tracks becomes a playlist.
- A folder that holds other folders with tracks becomes a playlist folder. Tracks sitting directly in it go into a playlist of the same name, first inside it, because a rekordbox folder cannot hold tracks itself.
- A folder with no tracks anywhere inside is left out.
- Files you name directly go into one playlist named after the output file: `euphoria.xml` gives a playlist called `euphoria`.
- Each folder you name becomes one entry at the top level, in the order you named them. If two entries side by side would have the same name, the later one gets ` (2)` after its name.
- Tracks are listed in path order.

### Titles and artists

Each track's title and artist are read from the file's own tags: ID3 tags in MP3 and AIFF files, Vorbis comments in FLAC and Ogg Vorbis, MP4 metadata in M4A and MP4, and the `INFO` list in WAV files. A WAV file tagged only with an ID3 chunk is read as having no tags. A track with no title in its tags is given its file name without the extension, so no track arrives nameless.

A track whose tags cannot be read at all is still exported with its key and tempo, under its file name and without an artist. The command lists those tracks with the reason, and still exits with status 0.

## What the file contains

For each track:

| Information | Written when | Notes |
| --- | --- | --- |
| File location | Always | The full path of the audio file. rekordbox matches tracks by this path. |
| Duration | Always | In whole seconds. |
| Title | Always | From the file's tags, or the file name without its extension. |
| Artist | When the file's tags have one | Left out otherwise. |
| Key | When Euphoria found one | As a Camelot code such as `8A`, or a musical name such as `Am`. |
| Tempo | When Euphoria found one | The average BPM with two decimals, such as `128.00`. |
| Energy | When Euphoria rated the track | In the comments, as `Energy 7`. See [energy](../concepts/energy.md). |
| Beat grid | With `--beat-grid`, when the grid is rated medium or high | The first downbeat and the tempo of the grid, which is then the tempo written. See [beat grids](#beat-grids). |
| Cue points | With `--cues`, on tracks with a beat grid in the file and no cues in your collection | Memory cues and hot cues A to H named Start, Main, Breakdown, Drop and Outro. See [cue points](#cue-points). |

A track with no key or no tempo has no key or tempo in the file. Euphoria never writes an empty key or a tempo of zero.

The file also holds the playlists and folders described above. Playlist folders can be nested up to 32 levels deep.

The comment of a track holds only the energy rating. If you keep your own notes in the comments of tracks that are already in your collection, know that **Import To Collection** replaces the comment of a track that is already there with the one in the file (tested in rekordbox 7), along with its key and tempo. Import only the tracks whose comments you do not mind losing, or keep your notes in another column.

Not in the file: loops, album, genre, rating, the comments in your files' tags, colours, play counts, loops and waveforms.

The Euphoria library, in `euphoria_core::export::rekordbox`, can write hot cues on pads A to P for programs built on it. The command uses pads A to H only. rekordbox's export mode uses only pads A to H, so hot cues on pads I to P may not show on CDJ and XDJ players that read a drive prepared there. **(not verified)**

### Key notation

rekordbox recognises both notations and shows keys in the format chosen in **Preferences > View** (tested in rekordbox 7). With **Display key information on the database** it shows the text exactly as the file gives it, so the notation still matters there. Choose it when the file is exported, with `--key-notation`:

| Notation | Option | A minor | F sharp minor | D flat major |
| --- | --- | --- | --- | --- |
| Camelot (default) | `--key-notation camelot` | `8A` | `11A` | `3B` |
| Musical | `--key-notation musical` | `Am` | `F#m` | `Db` |

See [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) for how the two relate.

### Characters that are removed

The XML format cannot hold a small set of invisible control characters, which sometimes end up in tags written by faulty software. Euphoria removes them from titles, artist names, cue names, playlist names and folder names, and writes the rest of the text unchanged. Tabs and line breaks are kept. File locations are not affected: every character of a path is kept.

### Beat grids

With `--beat-grid`, each track whose [beat grid](../concepts/accuracy.md#beat-grid) is rated medium or high gets it: one constant tempo with two decimals and the time of the first downbeat, in 4/4. rekordbox places every beat and bar from those two, so it needs no grid analysis of its own for that track. On the benchmark, with the tempo in the right octave, 97% or more of the grids rated medium or high had their beats right.

A grid rated low is left out, and so is the tempo of the grid; the track gets Euphoria's tempo, and rekordbox draws a grid when it analyzes it. That covers tracks with no steady beat and tracks whose tempo drifts, such as live recordings and vinyl transfers, which one constant tempo cannot follow. The command says how many tracks got a grid:

```
Beat grids for 312 of 340 tracks; the others have no clear beat or a grid rated low confidence, so rekordbox analyzes them.
```

The beats of a grid are more reliable than its bars. On the benchmark the first downbeat was right for about half of the excerpts, and otherwise the bars start one, two or three beats off while every beat stays in place. Check where the bars start on tracks you mix by phrase.

To keep the beat grid rekordbox draws itself, export without `--beat-grid`.

### Cue points

With `--cues`, Euphoria marks where each track's sections start, as described on the [accuracy](../concepts/accuracy.md#cue-points) page: Start, Main, Breakdown, Drop and Outro, at most 8, each on the first beat of a bar at a phrase boundary. Each is written twice, as a memory cue and as a hot cue, on pads from A in order. Only tracks whose beat grid is in the file get cues, since the cues sit on its bars.

Euphoria never changes cues you set. `--cues` needs `--collection`, your collection exported from rekordbox with **File > Export Collection in xml format**, and a track that has any cue, hot cue or loop there is left out of the file entirely, so no import of the file can touch its cues:

```
euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml --beat-grid --cues --collection ~/Music/rekordbox-collection.xml
```

Export your collection again before each export, so it lists the cues you set since. Export it on the computer you export from: a track with cues whose file Euphoria cannot find stops the export, since its cues could not be protected. The [command reference](../reference/cli.md#cue-points) explains this and `--allow-unmatched-cued-tracks`. The command says how many tracks it left out. To update the key and tempo of those tracks, export again without `--cues`; that file has no cues, but see [what happens to tracks you already have](#what-happens-to-tracks-you-already-have) before importing it.

The cues are right only where the bars of the grid are. When the grid's first downbeat is off, which happens on about half of the benchmark excerpts, every cue is off by the same one to three beats. Look at a few tracks before you rely on them.

### What the export refuses

Euphoria stops before writing anything, and names the track or folder, when:

- folders are nested more than 32 levels deep. Export the deeper folders on their own, or pass `--no-playlists`,
- a tempo is not a number or would be written as `0.00`,
- a path is not a full path on a local drive.

The file is written under a temporary name and moved into place once it is complete, so a failed export never leaves a half written file, and never damages the file from an earlier export.

## Before you import

- Back up your rekordbox library first: **File > Library > Backup Library**. **(not verified)**
- Keep the audio files where they were when the file was exported. The file refers to each track by its full path, so a track that has moved, or a drive mounted under another name, will show as missing. A folder reached through a symbolic link is written with the path it resolves to.
- Files must be on a local or external drive. Network locations written as `\\server\share` are refused by the export, because the format only describes files on the computer itself.

## Import into rekordbox

1. Open rekordbox and go to **Preferences > View > Layout**. Under the tree view options, tick **rekordbox xml**.
2. Go to **Preferences > Advanced > Database**. Under **rekordbox xml**, next to **Imported Library**, click **Browse** and choose the file Euphoria exported.
3. Close the preferences. In the tree on the left of the browser, a **rekordbox xml** entry appears. Click the refresh arrows next to it so rekordbox reads the file. Do this again each time you export a new file.
4. Open the **rekordbox xml** entry. **All Tracks** lists every track in the file, and **Playlists** holds the playlists and folders.
5. Select the tracks you want, right click and choose **Import To Collection**. To bring in a whole playlist with its tracks, right click the playlist and choose **Import Playlist**.

   For tracks that are already in your collection, select the tracks themselves, under **All Tracks** or inside the playlist, and choose **Import To Collection**. Importing only the playlist does not update tracks that already exist (tested in rekordbox 7).
6. Check the result in your collection: the **Key** and **BPM** columns should show Euphoria's values.

## What happens to tracks you already have

**Import To Collection** on a track that is already in your collection replaces its key, tempo and comment with what the file says (tested in rekordbox 7). Importing only a playlist leaves them as they were.

Replacing may affect more than that. Expect a beat grid or cue points you set by hand in rekordbox to be replaced by the ones in the file, or removed when the file has none. A file exported with `--beat-grid` holds a grid for most tracks. With `--cues --collection`, tracks that have cues in your collection are not in the file at all, so importing it cannot change them. The test did not check grids or cues, so try the import on a few tracks first and look at them before importing a whole library. **(not verified)**

rekordbox's own analysis replaces imported values (tested in rekordbox 7: analyzing a track with **KEY** ticked replaced both its key and its tempo). Choose before importing, in **Preferences > Analysis > Track Analysis**:

- **Keep Euphoria's key and tempo:** turn **Auto Analysis** off. rekordbox then draws no beat grid of its own until you analyze a track.
- **Keep Euphoria's key, and let rekordbox draw the beat grid:** leave Auto Analysis on and untick **KEY** in **Track Analysis Setting**, keeping **BPM / Grid** ticked. rekordbox's analysis then sets the tempo to its own value. This is the setup Mixed In Key recommends.

## If something looks wrong

| What you see | Likely cause |
| --- | --- |
| The **rekordbox xml** entry is empty | The file was not chosen in the preferences, or the list was not refreshed. |
| Tracks show as missing | The audio files moved after the export. Export again from their current location, with `--force`. |
| The key column shows `8A` and you wanted `Am`, or the reverse | rekordbox shows keys in the format chosen in **Preferences > View**; change it there. With **Display key information on the database** it shows the exported text, so export again with the other `--key-notation`. |
| A track is named after its file | The file has no title in its tags, or it is a WAV file tagged only with an ID3 chunk. |
