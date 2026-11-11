---
title: Quick start
description: Analyze your first tracks in a minute.
---

# Quick start

Point Euphoria at a folder of music:

```
euphoria analyze ~/Music/DJ
```

It searches the folder and its subfolders, analyzes every track, and prints one line per track:

```
Key             BPM  Energy  Time  File
8A   A minor  124.0       6  6:48  /Users/you/Music/DJ/Low Tide.flac
9A   E minor  122.0       4  7:12  /Users/you/Music/DJ/Saffron.m4a
```

- **Key** is the Camelot code and the musical key. Tracks whose codes are equal or next to each other mix well; see [keys and the Camelot wheel](../concepts/keys-and-camelot.md).
- **BPM** is the tempo in beats per minute, always reported between 90 and 180; see [how the tempo is chosen](../concepts/accuracy.md#how-the-tempo-is-chosen).
- **Energy** is how intense the track sounds, from 1 (calm) to 10 (relentless); see [energy rating](../concepts/energy.md).
- **Time** is the length of the track.

A large library takes a while: roughly a few seconds of processing per track, spread over several tracks at once. A progress bar shows how far along it is.

## Next steps

- [Analyze your library](../guides/analyze-your-library.md) covers large libraries, failed files and exporting results.
- The [command reference](../reference/cli.md) lists every option.
