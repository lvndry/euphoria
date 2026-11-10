---
title: Export to rekordbox
description: Bring the keys and tempos Euphoria found into your rekordbox collection with a rekordbox XML file.
---

# Export to rekordbox

One command analyzes your music and writes the keys and tempos as a rekordbox XML file:

```
euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
```

rekordbox reads that file as a second library sitting next to your own, and you choose which tracks and playlists to bring into your collection. Your audio files are never modified.

This is the way to get Euphoria's results into rekordbox for every format. rekordbox does not read the key tag of FLAC and WAV files, and never reads the tempo from a file's tags, but it takes both from a rekordbox XML file.

The import steps below were written from Pioneer DJ's published description of the XML format and from rekordbox 6 and 7 documentation. They have not been run against a rekordbox installation. Steps marked **(not verified)** are the ones most likely to differ on your version.

## Export your library

```
$ euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml
```

Name one or more folders or files, as with `euphoria analyze`. Tracks that were analyzed before come from the [cache](../reference/cli.md#the-analysis-cache), so exporting a library that is already analyzed takes seconds. New tracks are analyzed first, with a progress bar.

- The export refuses to replace a file that exists. Pass `--force` to replace it, for example when you export again after adding music.
- Keys are written as Camelot codes. Pass `--key-notation musical` for names such as `Am`.
- Pass `--no-playlists` to export the tracks without playlists.

A file that cannot be analyzed is left out and listed with the reason, the rest is exported, and the command exits with status 1. The [command reference](../reference/cli.md#euphoria-export-rekordbox) has every option.

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

Each track's title and artist are read from the file's own tags: ID3 tags in MP3 and AIFF files, Vorbis comments in FLAC, MP4 metadata in M4A, and the `INFO` list in WAV files. A WAV file tagged only with an ID3 chunk is read as having no tags. A track with no title in its tags is given its file name without the extension, so no track arrives nameless.

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

A track with no key or no tempo has no key or tempo in the file. Euphoria never writes an empty key or a tempo of zero.

The file also holds the playlists and folders described above. Playlist folders can be nested up to 32 levels deep.

Not in the file: beat grids, cue points, album, genre, rating, comments, colours, play counts, loops and waveforms.

The Euphoria library, in `euphoria_core::export::rekordbox`, can also write a beat grid with a constant tempo, memory cues and hot cues on pads A to P for programs built on it. The command does not write them, since Euphoria does not detect them yet. rekordbox's export mode uses only pads A to H, so hot cues on pads I to P may not show on CDJ and XDJ players that read a drive prepared there. **(not verified)**

### Key notation

rekordbox shows the key text exactly as the file gives it, so the notation is chosen when the file is exported, with `--key-notation`:

| Notation | Option | A minor | F sharp minor | D flat major |
| --- | --- | --- | --- | --- |
| Camelot (default) | `--key-notation camelot` | `8A` | `11A` | `3B` |
| Musical | `--key-notation musical` | `Am` | `F#m` | `Db` |

See [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) for how the two relate.

### Characters that are removed

The XML format cannot hold a small set of invisible control characters, which sometimes end up in tags written by faulty software. Euphoria removes them from titles, artist names, cue names, playlist names and folder names, and writes the rest of the text unchanged. Tabs and line breaks are kept. File locations are not affected: every character of a path is kept.

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

1. Open rekordbox and go to **Preferences > View > Layout**. Under the tree view options, tick **rekordbox xml**. **(not verified)**
2. Go to **Preferences > Advanced > Database**. Under **rekordbox xml**, next to **Imported Library**, click **Browse** and choose the file Euphoria exported. **(not verified)**
3. Close the preferences. In the tree on the left of the browser, a **rekordbox xml** entry appears. Click the refresh arrows next to it so rekordbox reads the file. Do this again each time you export a new file. **(not verified)**
4. Open the **rekordbox xml** entry. **All Tracks** lists every track in the file, and **Playlists** holds the playlists and folders.
5. Select the tracks you want, right click and choose **Import To Collection**. To bring in a whole playlist with its tracks, right click the playlist and choose **Import Playlist**. **(not verified)**
6. Check the result in your collection: the **Key** and **BPM** columns should show Euphoria's values.

## What happens to tracks you already have

When a track from the file is already in your collection, rekordbox asks whether to replace its information with what the file says. **(not verified)**

Replacing can affect more than key and tempo. A beat grid or cue points you set by hand in rekordbox may be replaced by the ones in the file, or removed when the file has none. Try the import on a few tracks first and look at their grids and cues before importing a whole library. **(not verified)**

rekordbox may also analyze newly imported tracks and write its own key and tempo over the imported ones. If the values change right after the import, look in **Preferences > Analysis** for the options that analyze tracks on import, and turn off key and BPM analysis before importing. **(not verified)**

## If something looks wrong

| What you see | Likely cause |
| --- | --- |
| The **rekordbox xml** entry is empty | The file was not chosen in the preferences, or the list was not refreshed. |
| Tracks show as missing | The audio files moved after the export. Export again from their current location, with `--force`. |
| The key column shows `8A` and you wanted `Am`, or the reverse | The notation is fixed when the file is exported. Export again with the other `--key-notation`. |
| A track is named after its file | The file has no title in its tags, or it is a WAV file tagged only with an ID3 chunk. |
