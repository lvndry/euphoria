<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark.svg" alt="" width="72" height="72">
</picture>

# Euphoria

Find the key, BPM and energy of every track in your music collection, so you can mix in key. Free, open source, and fully offline.

Euphoria reads your music, finds the key and tempo of every track, rates its energy, and exports the results to your DJ software. It is an open alternative to paid harmonic mixing tools such as Mixed In Key: no account, no activation, and nothing leaves your computer.

## Features

Available now:

- Key detection, shown in Camelot (8A) and musical (A minor) notation
- Tempo detection in BPM
- Energy rating from 1 to 10, on Euphoria's own [documented scale](docs/concepts/energy.md)
- `euphoria analyze`, a command line tool with table and JSON output
- `euphoria export rekordbox`, which writes keys, tempos, energy ratings and a playlist per folder as a rekordbox XML file
- `euphoria tags write`, which writes key, tempo and energy into the tags of your files after showing every change, and `euphoria tags undo`, which puts the old tags back byte for byte
- MP3, M4A (AAC), FLAC, WAV and AIFF
- An early desktop app, run from source on macOS: see the [desktop app guide](docs/guides/desktop-app.md)

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
- [Keys and the Camelot wheel](docs/concepts/keys-and-camelot.md)
- [Accuracy](docs/concepts/accuracy.md)
- [Command line reference](docs/reference/cli.md)
- [Development](docs/development/index.md) and [contributing](CONTRIBUTING.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
