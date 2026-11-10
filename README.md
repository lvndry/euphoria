# Euphoria

Key, tempo and energy analysis for DJ libraries. Free, open source, and fully offline.

Euphoria reads your music, finds the key and tempo of every track, and will write the results where your DJ software can see them. It is an open alternative to paid harmonic mixing tools such as Mixed In Key: no account, no activation, and nothing leaves your computer.

## Features

Available now:

- Key detection, shown in Camelot (8A) and musical (A minor) notation
- Tempo detection in BPM
- `euphoria analyze`, a command line tool with table and JSON output
- MP3, M4A (AAC), FLAC, WAV and AIFF

In development: energy ratings, tag writing for rekordbox, Serato, Traktor and Engine DJ, harmonic matching, and a desktop app.

## Install

Homebrew packages are coming. Until then, [build from source](docs/building.md).

## Quick start

```
$ euphoria analyze ~/Music/DJ
Key             BPM  Time  File
8A   A minor  124.0  6:48  /Users/you/Music/DJ/Low Tide.flac
9A   E minor  122.0  7:12  /Users/you/Music/DJ/Saffron.m4a
```

## Documentation

- [Usage](docs/usage.md): every command and option
- [Accuracy](docs/accuracy.md): how detection is measured, and the results
- [Building from source](docs/building.md)
- [Architecture](docs/architecture.md): how the code is organized
- [Contributing](CONTRIBUTING.md)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
