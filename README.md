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
- `euphoria tags write`, which writes key, tempo and energy into the tags of your files after showing every change, and `euphoria tags undo`, which puts the old tags back byte for byte
- MP3, M4A (AAC), FLAC, WAV and AIFF
- An early desktop app, run from source on macOS: see the [desktop app guide](docs/guides/desktop-app.md)
- Playlists built in the desktop app, with each set's key path, tempo range and energy flow at a glance, exported to rekordbox or as M3U8 for any other program

In development: exports for Serato, Traktor and Engine DJ.

## Install

Homebrew packages are coming. Until then, see [installation](docs/getting-started/installation.md).

## Quick start

```
$ euphoria analyze ~/Music/DJ
Key             BPM  Energy  Time  File
8A   A minor  124.0       6  6:48  /Users/you/Music/DJ/Low Tide.flac
9A   E minor  122.0       4  7:12  /Users/you/Music/DJ/Saffron.m4a
```

Bring the results into rekordbox:

```
$ euphoria export rekordbox ~/Music/DJ --output ~/Music/euphoria.xml
Exported 340 tracks in 12 playlists to /Users/you/Music/euphoria.xml
```

## Documentation

The full documentation is in [docs](docs/index.md):

- [Installation](docs/getting-started/installation.md) and [quick start](docs/getting-started/quick-start.md)
- [Analyze your library](docs/guides/analyze-your-library.md)
- [Export to rekordbox](docs/guides/export-to-rekordbox.md)
- [Use Euphoria with your DJ program](docs/guides/dj-programs.md)
- [Keys and the Camelot wheel](docs/concepts/keys-and-camelot.md)
- [Accuracy](docs/concepts/accuracy.md)
- [Audio quality](docs/concepts/audio-quality.md)
- [Command line reference](docs/reference/cli.md)
- [Development](docs/development/index.md) and [contributing](CONTRIBUTING.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
