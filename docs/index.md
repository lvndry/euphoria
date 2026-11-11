---
title: Euphoria
description: Find the key, BPM and energy of every track in your music collection, so you can mix in key.
---

# Euphoria

Euphoria finds the key and tempo of every track in your music library and rates its energy, so you can mix harmonically, match tempos and shape a set without guessing. It is free, open source and works fully offline: no account, no activation, and your music never leaves your computer.

It is an open alternative to paid harmonic mixing tools such as Mixed In Key.

## What it does today

- Detects the key of a track, shown in Camelot notation (8A) and as a musical key (A minor).
- Detects the tempo in beats per minute.
- Rates the energy of a track from 1 to 10.
- Measures how far a track is tuned from A = 440 Hz and follows its key over time, both in the JSON output.
- Analyzes whole folders from the command line, as a table or as JSON.
- Exports keys, tempos, energy ratings and a playlist per folder to rekordbox, with `euphoria export rekordbox`.
- Writes key, tempo and energy into the tags of your files, with `euphoria tags write`. Every change is shown first, nothing else in the file changes, and `euphoria tags undo` puts the old tags back.
- Reads MP3, M4A (AAC), FLAC, WAV and AIFF.
- Shows your library in a desktop app with a sortable track table, waveforms and suggestions for tracks that mix together. The app is an early version that runs from source on macOS.

Exports for other DJ software are in development.

## Where to start

- New here: [install Euphoria](getting-started/installation.md), then follow the [quick start](getting-started/quick-start.md).
- Preparing a set: [analyze your library](guides/analyze-your-library.md).
- Using rekordbox: [export to rekordbox](guides/export-to-rekordbox.md).
- Want the key and energy inside the files, for any DJ software: [write tags into your files](guides/write-tags.md).
- Prefer a window to a terminal: [desktop app](guides/desktop-app.md).
- Curious how keys are named and mixed: [keys and the Camelot wheel](concepts/keys-and-camelot.md).
- Curious what the energy number means: [energy rating](concepts/energy.md).
- Wondering how much to trust the results: [accuracy](concepts/accuracy.md).
- Want to know what gets written to your files: [tags](reference/tags.md).
