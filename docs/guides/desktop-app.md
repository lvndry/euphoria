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

Folders are searched all the way down for MP3, M4A, FLAC, WAV and AIFF files. A track Euphoria has analyzed before is filled in at once. Every other track appears as a row marked "Queued", changes to "Analyzing" when its turn comes, and fills in when its analysis finishes. The toolbar shows how many are done, for example "12 of 340 done". A file that could not be analyzed counts as done.

The app analyzes as many tracks at once as your computer has cores, up to 4. The command line tool goes up to 8 by default and lets you choose. See [large libraries and memory](analyze-your-library.md#large-libraries-and-memory).

Your files are only read. The app does not write tags or change files.

## The track table

| Column | Shows |
| --- | --- |
| Title | The file name |
| Key | The Camelot code, coloured by its number |
| Musical key | The key by name, such as A minor |
| BPM | The tempo, to one decimal |
| Energy | The [energy rating](../concepts/energy.md) from 1 to 10, with a bar |
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

To give the table more room, fold the sidebar away. Any of these folds it, and the same one brings it back:

- the sidebar button at the left of the toolbar, beside the window buttons, which stays there while the sidebar is folded
- Control Command S (Control Alt S on Linux and Windows)
- **View > Hide Sidebar**, which reads **Show Sidebar** while it is folded

## The inspector

The inspector shows the selected track: its name, format and length, its key, tempo and energy in three boxes, and its waveform. The waveform is read from the file when you select the track.

**Mixes well with** lists up to eight other tracks in your library whose key mixes with the selected one and whose tempo is within 6 percent: the same key, the keys one step either way on the Camelot wheel, and the relative major or minor. The smoothest key moves come first, then the closest tempos, then the closest energy. Each row shows the key, the name, how much faster or slower the track is, and how the keys relate. Click a row to go to that track. [Keys and the Camelot wheel](../concepts/keys-and-camelot.md) explains these moves.

### Resize the inspector

Drag the left edge of the inspector to make it wider or narrower. The pointer turns into a resize arrow over the edge. The waveform, the three boxes and the list of mixes follow the new width. Double click the edge to return to the usual width.

| | Width |
| --- | --- |
| Usual | 320 |
| Narrowest | 288 |
| Widest | 640, or less when the window is small: the table always keeps at least 400 |

In the Deck theme the track detail is a strip under the table, and you drag its top edge to change its height in the same way. The waveform grows with the strip, and a taller strip lists more mixes. The strip is 180 high to begin with, and goes from 124 to 420, always leaving at least 200 for the table.

A size you chose in a large window is kept when the window gets smaller. It is only drawn smaller while the window has no room for it.

## Listen to a track

Select a track and press Space, or click the round play button, to hear it. Press Space again to pause. Playing starts at once, however long the track is, and carries on while other tracks are being analyzed. Space types a space as usual while you are in the search field.

- Selecting another track while one is playing switches to it. While paused or stopped, selecting a track only gets it ready.
- The position and the length are shown as "2:23 / 6:48".
- When a track reaches its end it stops there. Play starts it again from the beginning.

In most themes the player is a bar along the bottom of the window, with the play button, the track, the position, a thin progress line and the length. In the Deck theme the play button and the position sit in the strip under the table, beside the wide waveform.

If a track cannot be played, the player says why in a few words, for example "This file is no longer there" or "No audio output to play through", and everything else keeps working.

## The waveform

The waveform shows the whole track from left to right, and its colours show what is sounding:

| Colour | Band | Typical sounds |
| --- | --- | --- |
| Blue | Bass, below 200 Hz | Kick drums and bass lines |
| Amber | Middle, 200 Hz to 2.5 kHz | Vocals, chords and snares |
| White, or near black in the Paper theme | Treble, above 2.5 kHz | Hi hats and cymbals |

The three are drawn on top of one another, bass at the back, each as tall as that band is loud. A stretch with a kick is tall and blue, a breakdown without drums loses its blue and shrinks to amber, and hi hats show as light tips.

The waveform is also where you move through the track. A line marks where playing is, and the part already played is fainter. Click anywhere on the waveform to jump there, or hold the button and drag. The progress line in the player bar works the same way. Before you press play, a click sets where playing will start.

## When a file fails

A file that cannot be analyzed stays in the table with **Failed** in its key column and is counted under **Needs review**. Select it to read the reason in the inspector. The other files carry on.

## Themes

Choose a theme from the menu at the right of the toolbar, or from **View > Theme**.

| Theme | Looks like |
| --- | --- |
| Slate | Dark grey with a blue accent. The default. |
| Paper | Light with a blue accent. |
| Deck | Slate colours, with the track detail and the player in a strip under the table and a wide waveform. |
| Graphite | Warm dark grey with an amber accent, compact rows, and keys as coloured text. |
| Key first | Slate colours with solid key chips, a Camelot wheel in the inspector that highlights the keys that mix with the selected track, and the table sorted by key. |

## What the app remembers

The folders you added, the theme, whether the sidebar is folded, and the sizes you gave the inspector and the Deck strip are kept in `settings.json`:

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

- Cue points, and the reference tone for checking a key by ear.
- Writing tags and exporting to DJ software.
- Artist and title from file tags: the table shows file names.
- Filters for key and tempo ranges.
- Linux and Windows builds: the app is only built and tested on macOS so far.
