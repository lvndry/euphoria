---
title: Desktop app
description: Run the Euphoria desktop app from source and see what it does today.
---

# Desktop app

The desktop app shows your library as a table: add folders, watch each track get its key and tempo, then sort, search and find tracks that mix together. It uses the same analysis as the command line tool, so both give the same answers.

The app is an early version. It runs from a source checkout on macOS; there is no download yet.

## Run it from source

Install the requirements in [building from source](../development/building.md). On macOS you also need Xcode, which provides the Metal shader compiler the interface is built with. Then:

```
cargo run --release -p euphoria-app
```

Use `--release`: analysis is many times slower in a debug build.

## Add music

On first launch the window shows a drop zone. Drop a folder, or several folders and files, from Finder onto the window, or click **Choose folder**. Later, use **Add folder** in the toolbar, **File > Add Folder** or Command O.

Folders are searched all the way down for MP3, M4A, FLAC, WAV and AIFF files. A track Euphoria has analyzed before is filled in at once. Every other track appears as a pending row and fills in when its analysis finishes. The toolbar shows the progress, for example "Analyzing 12 of 340".

The app analyzes as many tracks at once as your computer has cores, up to 4, the same limit the command line tool uses by default. See [large libraries and memory](analyze-your-library.md#large-libraries-and-memory).

Your files are only read. The app does not write tags or change files.

## The track table

| Column | Shows |
| --- | --- |
| Title | The file name |
| Key | The Camelot code, coloured by its number |
| Musical key | The key by name, such as A minor |
| BPM | The tempo, to one decimal |
| Time | The length |
| Format | The file type |
| Folder | The folder holding the file |

Click a column header to sort by it, and click again to reverse the order. Sorting by key follows the Camelot wheel: by number, then the minor key (A) before the major key (B). Tracks with no value in the sorted column come last.

Type in the search field, or press Command F first, to filter as you type. Each word must match part of the file name, the whole key code (`8a`), or the start of the tempo (`124`).

Click a row to select it. The arrow keys, Page Up, Page Down, Home and End move the selection.

## The sidebar

- **All tracks** shows the whole library.
- **Needs review** shows the tracks that could not be analyzed.
- **Missing files** is a placeholder and always shows 0.
- **Folders** lists the folders you added. Click one to show only its tracks.

## The inspector

The inspector shows the selected track: its name, format and length, its key, tempo and time in three boxes, and its waveform. The waveform is read from the file when you select the track.

**Mixes well with** lists up to eight other tracks in your library whose key mixes with the selected one and whose tempo is within 6 percent: the same key, the keys one step either way on the Camelot wheel, and the relative major or minor. Each row shows the key, the name, how much faster or slower the track is, and how the keys relate. Click a row to go to that track. [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) explains these moves.

## When a file fails

A file that cannot be analyzed stays in the table with **Failed** in its key column and is counted under **Needs review**. Select it to read the reason in the inspector. The other files carry on.

## Themes

Choose a theme from the menu at the right of the toolbar, or from **View > Theme**.

| Theme | Looks like |
| --- | --- |
| Slate | Dark grey with a blue accent. The default. |
| Paper | Light with a blue accent. |
| Deck | Slate colours, with the track detail in a strip under the table and a wide waveform. |
| Graphite | Warm dark grey with an amber accent, compact rows, and keys as coloured text. |
| Key first | Slate colours with solid key chips, a Camelot wheel in the inspector that highlights the keys that mix with the selected track, and the table sorted by key. |

## What the app remembers

The folders you added and the theme are kept in `settings.json`:

| System | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/euphoria` |
| Linux | `$XDG_DATA_HOME/euphoria`, or `~/.local/share/euphoria` |
| Windows | `%APPDATA%\euphoria` |

Set the `EUPHORIA_DATA_DIR` environment variable to keep it somewhere else.

Files dropped on their own, outside a folder, are not remembered.

## Remembered results

The app keeps each finished analysis in the same folder, in the cache the command line tool also uses. A track analyzed once, by the app or by `euphoria analyze`, is not analyzed again: when the app starts, the remembered folders are searched and their known tracks are filled in within a moment. Only new or changed files are analyzed. A file that was moved or renamed is recognised by its content when its turn comes.

The count above the table says how many tracks were filled in this way, for example "48 tracks, 44 from cache".

If the cache cannot be opened, the app says why in a notice along the bottom of the window and analyzes without it, so results are not remembered until the problem is fixed. `euphoria cache clear` resets a damaged cache. The [command reference](../reference/cli.md#the-analysis-cache) explains how files are recognised and when a track is analyzed again.

## Quitting

You can quit while tracks are being analyzed. The app closes at once: tracks already finished are remembered, and the ones that were waiting or in progress are analyzed the next time the app starts.

## Build Euphoria.app

A binary started with `cargo run` has no Dock icon of its own. To get an application with the Euphoria icon that you can keep in the Dock, build a bundle on macOS:

```
./scripts/bundle-macos.sh
open target/release/bundle/Euphoria.app
```

The script builds the app in release mode, makes the icon from `assets/logo` and writes `target/release/bundle/Euphoria.app`. The bundle is meant for the computer that built it: it uses the libkeyfinder installed there and is not signed.

## Try it without music

To see how the interface behaves with a large library, start the app with made up tracks. Their keys and tempos are invented and nothing is analyzed:

```
EUPHORIA_DEMO_TRACKS=10000 cargo run -p euphoria-app
```

## Not there yet

- Playback, cue points and energy ratings.
- Writing tags and exporting to DJ software.
- Artist and title from file tags: the table shows file names.
- Filters for key and tempo ranges.
- Linux and Windows builds: the app is only built and tested on macOS so far.
