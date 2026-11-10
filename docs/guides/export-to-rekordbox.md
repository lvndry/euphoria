---
title: Export to rekordbox
description: Bring the keys and tempos Euphoria found into your rekordbox collection with a rekordbox XML file.
---

# Export to rekordbox

Euphoria can write its results as a rekordbox XML file. rekordbox reads that file as a second library sitting next to your own, and you choose which tracks and playlists to bring into your collection. Your audio files are never modified.

The export lives in the Euphoria library, in `euphoria_core::export::rekordbox`. The command line tool does not offer it yet, so for now the file comes from a program built on the library.

The steps below were written from Pioneer DJ's published description of the XML format and from rekordbox 6 and 7 documentation. They have not been run against a rekordbox installation. Steps marked **(not verified)** are the ones most likely to differ on your version.

## What the file contains

For each track:

| Information | Written when | Notes |
| --- | --- | --- |
| File location | Always | The full path of the audio file. rekordbox matches tracks by this path. |
| Duration | Always | In whole seconds. |
| Title and artist | When known | Left out otherwise. |
| Key | When Euphoria found one | As a Camelot code such as `8A`, or a musical name such as `Am`. |
| Tempo | When Euphoria found one | The average BPM with two decimals, such as `128.00`. |
| Beat grid | When both the tempo and the first beat are known | One anchor at the first beat with a constant tempo, counted in 4/4. |
| Cue points | When there are any | Memory cues and hot cues, with their names. |

A track with no key or no tempo has no key or tempo in the file. Euphoria never writes an empty key or a tempo of zero.

The file also holds playlists and folders. Each playlist lists its tracks in order.

Not in the file: album, genre, rating, comments, colours, play counts, loops and waveforms.

### Key notation

rekordbox shows the key text exactly as the file gives it, so the notation is chosen when the file is exported:

| Notation | A minor | F sharp minor | D flat major |
| --- | --- | --- | --- |
| Camelot (default) | `8A` | `11A` | `3B` |
| Musical | `Am` | `F#m` | `Db` |

See [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) for how the two relate.

## Before you import

- Back up your rekordbox library first: **File > Library > Backup Library**. **(not verified)**
- Keep the audio files where they were when the file was exported. The file refers to each track by its full path, so a track that has moved, or a drive mounted under another name, will show as missing.
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
| Tracks show as missing | The audio files moved after the export. Export again from their current location. |
| The key column shows `8A` and you wanted `Am`, or the reverse | The notation is fixed when the file is exported. Export again with the other notation. |
| The beat grid is off by a beat in the bar | Euphoria marks the first beat it found as beat 1 of a bar. Shift the grid in rekordbox's grid edit panel. |
