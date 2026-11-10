<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark.svg" alt="" width="72" height="72">
</picture>

# Euphoria

Find the key and BPM of every track in your music collection, so you can mix in key. Free, open source, and fully offline.

Euphoria reads your music, finds the key and tempo of every track, and will write the results where your DJ software can see them. It is an open alternative to paid harmonic mixing tools such as Mixed In Key: no account, no activation, and nothing leaves your computer.

## Features

Available now:

- Key detection, shown in Camelot (8A) and musical (A minor) notation
- Tempo detection in BPM
- `euphoria analyze`, a command line tool with table and JSON output
- MP3, M4A (AAC), FLAC, WAV and AIFF

In development: energy ratings, tag writing for rekordbox, Serato, Traktor and Engine DJ, harmonic matching, and a desktop app.

## Install

Homebrew packages are coming. Until then, see [installation](docs/getting-started/installation.md).

## Quick start

```
$ euphoria analyze ~/Music/DJ
Key             BPM  Time  File
8A   A minor  124.0  6:48  /Users/you/Music/DJ/Low Tide.flac
9A   E minor  122.0  7:12  /Users/you/Music/DJ/Saffron.m4a
```

## Documentation

The full documentation is in [docs](docs/index.md):

- [Installation](docs/getting-started/installation.md) and [quick start](docs/getting-started/quick-start.md)
- [Analyze your library](docs/guides/analyze-your-library.md)
- [Keys and the Camelot wheel](docs/concepts/keys-and-camelot.md)
- [Accuracy](docs/concepts/accuracy.md)
- [Command line reference](docs/reference/cli.md)
- [Development](docs/development/index.md) and [contributing](CONTRIBUTING.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
