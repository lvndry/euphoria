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
Key                  BPM       Energy              Quality   Time  File
 8A  A minor  ●●●  124.0  ●●○       6  ━━━━━━────  lossless  6:48  Low Tide.flac
 9A  E minor  ●●○  122.0  ●●●       4  ━━━━──────  good      7:12  Saffron.m4a
```

- **Key** is the Camelot code, in the colour of its place on the wheel, and the musical key. Tracks whose codes are equal or next to each other mix well; see [keys and the Camelot wheel](../concepts/keys-and-camelot.md).
- The **dots** after the key and the tempo say how sure Euphoria is: `●●●` sure, `●●○` fairly sure, `●○○` worth checking by ear.
- **BPM** is the tempo in beats per minute, always reported between 90 and 180; see [how the tempo is chosen](../concepts/accuracy.md#how-the-tempo-is-chosen).
- **Energy** is how intense the track sounds, from 1 (calm) to 10 (relentless); see [energy rating](../concepts/energy.md).
- **Quality** is how good the file sounds on a big system; see [audio quality](../concepts/audio-quality.md).
- **Time** is the length of the track.

A large library takes a while: roughly a few seconds of processing per track, spread over several tracks at once. A line per track shows what Euphoria is doing with it, and a bar counts the tracks done.

To see everything Euphoria found about one track, name the file:

```
euphoria analyze ~/Music/DJ/Saffron.m4a
```

## Next steps

- [Analyze your library](../guides/analyze-your-library.md) covers large libraries, failed files and exporting results.
- The [command reference](../reference/cli.md) lists every option.
