<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark.svg" alt="" width="72" height="72">
</picture>

# Euphoria

Find the key, BPM and energy of every track in your music collection, so you can mix in key. Free, open source, and fully offline.

Euphoria reads your music, finds the key and tempo of every track, rates its energy, and exports the results to your DJ software. It is an open alternative to paid harmonic mixing tools such as Mixed In Key: no account, no activation, and nothing leaves your computer.

![The Euphoria desktop app in its dark theme. A sidebar holds a folder tree of house and techno subfolders with track counts. The table lists eleven house tracks with their title, artist, Camelot key chip, musical key, tempo, an energy rating with a bar, length, format and subfolder. The selected track, Lady Paris by Francesco Bonora and SeHou, is shown on the right with its key 8A, 124.9 BPM, energy 5, its coloured waveform with the energy curve under it, and four tracks that mix well with it.](docs/images/desktop-app/main-window.png)

## Features

Available now:

- Key detection, shown in Camelot (8A) and musical (A minor) notation
- Tempo detection in BPM
- Energy rating from 1 to 10, on Euphoria's own [documented scale](docs/concepts/energy.md)
- [Audio quality](docs/concepts/audio-quality.md) for DJ use: codec, real bit rate, where the content stops, and files that were likely re-encoded from a lower quality source
- `euphoria analyze`, a command line tool with table and JSON output
- `euphoria export rekordbox`, which writes keys, tempos, energy ratings and a playlist per folder as a rekordbox XML file
- `euphoria export traktor`, which writes a Traktor NML collection with keys, tempos and playlists, plus beat grids and cues with `--beat-grid` and `--cues`; importing it in Traktor has not yet been tested
- `euphoria export engine`, which writes keys, tempos, beat grids and hot cues into the tracks of your Engine DJ library after saving a copy; not yet tested in Engine DJ
- `euphoria tags write --serato-markers`, which writes cue points and beat grids into the tags Serato DJ reads, keeping the cues already there; not yet tested in Serato
- `euphoria tags write`, which writes key, tempo and energy into the tags of your files after showing every change, `euphoria tags set`, which edits title, artist, genre and other fields across many files with find and replace, and `euphoria tags undo`, which puts the old tags back byte for byte
- MP3, M4A and MP4 (AAC or ALAC), FLAC, Ogg Vorbis, WAV and AIFF
- A desktop app for macOS, downloadable below: see the [desktop app guide](docs/guides/desktop-app.md)
- Playlists built in the desktop app, with each set's key path, tempo range and energy flow at a glance, exported to rekordbox or as M3U8 for any other program

In development: an export for Serato DJ.

## Install

On a Mac with Apple Silicon and macOS 15 or later:

1. [Download Euphoria](https://github.com/lvndry/euphoria/releases/latest/download/Euphoria.dmg), the newest release.
2. Open the disk image and drag Euphoria onto Applications.
3. The first time, right click Euphoria in Applications and choose **Open**, because the app is not notarized by Apple yet. If macOS offers no **Open** button, choose **Done**, then **System Settings > Privacy & Security > Open Anyway**. After that it opens normally.

On Linux (x86_64, glibc 2.35 or later, such as Ubuntu 22.04 and newer):

```
curl -LO https://github.com/lvndry/euphoria/releases/latest/download/Euphoria-x86_64.AppImage
chmod +x Euphoria-x86_64.AppImage
./Euphoria-x86_64.AppImage
```

Every release, with its notes and the command line tool for both systems, is on the [releases page](https://github.com/lvndry/euphoria/releases). To build from source, or for what the AppImage needs from your desktop, see [installation](docs/getting-started/installation.md).

## Quick start

```
$ euphoria analyze ~/Music/DJ
Key                  BPM       Energy              Quality   Time  File
 8A  A minor  ●●●  124.0  ●●○       6  ━━━━━━────  lossless  6:48  Low Tide.flac
 9A  E minor  ●●○  122.0  ●●●       4  ━━━━──────  good      7:12  Saffron.m4a
```

One file is shown in detail, with the runner up key, the half or double tempo and the reason for its sound quality: `euphoria analyze ~/Music/DJ/Saffron.m4a`.

Bring the results into rekordbox:

```
$ euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml
```

## Documentation

The full documentation is in [docs](docs/index.md):

- [Installation](docs/getting-started/installation.md) and [quick start](docs/getting-started/quick-start.md)
- [Analyze your library](docs/guides/analyze-your-library.md)
- [Build a set along an energy curve](docs/guides/build-a-set.md)
- [Export to rekordbox](docs/guides/export-to-rekordbox.md), [to Traktor](docs/guides/export-to-traktor.md) and [to Engine DJ](docs/guides/export-to-engine-dj.md)
- [Cue points and beat grids for Serato](docs/guides/serato-cues.md)
- [DJ program support](docs/reference/dj-program-support.md)
- [Use Euphoria with your DJ program](docs/guides/dj-programs.md)
- [Find and export mashups in the desktop app](docs/guides/mashups.md)
- [Keys and the Camelot wheel](docs/concepts/keys-and-camelot.md)
- [Accuracy](docs/concepts/accuracy.md)
- [Audio quality](docs/concepts/audio-quality.md)
- [Command line reference](docs/reference/cli.md)
- [Development](docs/development/index.md) and [contributing](CONTRIBUTING.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
