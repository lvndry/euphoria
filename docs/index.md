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
- Orders a folder or playlist into a set that follows an energy curve you choose, with every mix in key and small tempo steps, with `euphoria set build`.
- Exports keys, tempos, beat grids, cues and playlists to a Traktor NML collection, with `euphoria export traktor`. Importing it in Traktor has not yet been tested.
- Writes keys, tempos, beat grids and hot cues into the tracks of your Engine DJ library, with `euphoria export engine`. Not yet tested in Engine DJ.
- Writes key, tempo and energy into the tags of your files, with `euphoria tags write`. Every change is shown first, nothing else in the file changes, and `euphoria tags undo` puts the old tags back.
- Reads MP3, M4A and MP4 (AAC or ALAC), FLAC, Ogg Vorbis, WAV and AIFF.
- Shows your library in a desktop app with a sortable track table, waveforms and suggestions for tracks that mix together. The app is [downloadable for macOS](getting-started/installation.md#download-for-macos).

Exports for Serato and Engine DJ are in development.

## Where to start

- New here: [install Euphoria](getting-started/installation.md), then follow the [quick start](getting-started/quick-start.md).
- Preparing a set: [analyze your library](guides/analyze-your-library.md), then [build a set along an energy curve](guides/build-a-set.md).
- Using rekordbox: [export to rekordbox](guides/export-to-rekordbox.md).
- Using Traktor: [export to Traktor](guides/export-to-traktor.md).
- Using Engine DJ: [export to Engine DJ](guides/export-to-engine-dj.md).
- Using Serato: [cue points and beat grids for Serato](guides/serato-cues.md).
- Which DJ program gets what: [DJ program support](reference/dj-program-support.md).
- Want the key and energy inside the files, for any DJ software: [write tags into your files](guides/write-tags.md), then [set up your DJ program](guides/dj-programs.md).
- Prefer a window to a terminal: [desktop app](guides/desktop-app.md).
- Layering two tracks: [mashups](guides/mashups.md).
- Curious how keys are named and mixed: [keys and the Camelot wheel](concepts/keys-and-camelot.md).
- Curious what the energy number means: [energy rating](concepts/energy.md).
- Wondering whether your files are good enough to play loud: [audio quality](concepts/audio-quality.md).
- Wondering how much to trust the results: [accuracy](concepts/accuracy.md).
- Want to know what gets written to your files: [tags](reference/tags.md).
