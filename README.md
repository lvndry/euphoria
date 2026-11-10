# Euphoria

Key, tempo and energy analysis for DJ libraries. Free, open source, and fully offline.

Euphoria reads your music folder, works out the key, the BPM and an energy rating for every track, and writes the results where your DJ software can see them. It is a native desktop app and a command line tool built on the same core, so both always give the same answer.

It is an open alternative to paid harmonic mixing tools such as Mixed In Key. There is no account, no activation and no network access. Nothing leaves your computer.

## What it does

- **Key detection** in Camelot notation (8A), musical notation (A minor) and Open Key (1m), with a confidence score and the second most likely key.
- **Tempo detection** in BPM with beat positions, and correction of half and double tempo mistakes.
- **Energy rating** from 1 to 10, so you can shape a set and not only match keys.
- **Harmonic matching**: pick a track and see what mixes with it, labelled by how (same key, adjacent, relative, energy boost), within a BPM range you choose.
- **Tag writing** for rekordbox, Serato, Traktor and Engine DJ, with a preview of every change before anything is written and an undo afterwards.
- **A review list** of tracks where the key is uncertain, with the reason (key change, sparse harmony, off pitch tuning) and a reference tone to check by ear.
- **rekordbox export** and optional cue points that snap to the beat grid.

Supported formats: MP3, M4A (AAC), FLAC, WAV and AIFF.

## Principles

- **Your files are yours.** Euphoria never changes a file until you ask it to, shows you exactly what will change, and keeps the original tags so a write can be undone.
- **Offline, always.** No sign in, no license server, no telemetry.
- **Measured, not claimed.** Accuracy is benchmarked on public datasets and the numbers are published in this repository, including the misses.
- **One core, two front ends.** The desktop app and the CLI call the same code.

## Install

Packaged releases are not out yet. When they are, installation will be:

```
brew install euphoria
```

Until then you can build from source, see below.

## Usage

The command line tool analyzes a folder and prints a table, or JSON for scripting:

```
euphoria analyze ~/Music/DJ
euphoria analyze ~/Music/DJ --json
euphoria analyze ~/Music/DJ --write-tags --preset rekordbox
```

The desktop app does the same with a library view: drop a folder in, sort and filter by key, BPM and energy, and select a track to see its waveform, its key confidence and the tracks that mix with it.

The command line tool and the desktop app are in development, so these commands are not available in a release yet.

## Accuracy

Detectors are scored on the [GiantSteps key](https://github.com/GiantSteps/giantsteps-key-dataset) and [GiantSteps tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset) datasets, which are made of electronic dance music. Key results use the MIREX weighted score, which gives partial credit for near misses such as the relative key. Tempo results count an estimate as correct within 4 percent.

Results are published under `bench/results` as each detector is measured. You can reproduce them on your own machine with the steps below.

## Build from source

You need a Rust toolchain, CMake and libkeyfinder. On macOS:

```
brew install rust cmake libkeyfinder
git clone https://github.com/lvndry/euphoria.git
cd euphoria
cargo test
```

To run the accuracy benchmark, download the datasets (about 1.7 GB) and run the benchmark crate:

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
```

An optional reference run scores Essentia on the same tracks. It needs [uv](https://docs.astral.sh/uv/):

```
uv run bench/essentia_reference.py bench/data
```

## How it is organized

```
crates/
  euphoria-core/    decoding, key and tempo detection, harmony. No user interface.
  euphoria-bench/   accuracy benchmark runner
bench/              dataset download and reference scripts, results
```

The CLI and the desktop app will each be their own crate on top of `euphoria-core`.

## Contributing

Issues and pull requests are welcome. The most useful reports right now are tracks where the detected key or tempo is wrong: say what you expected, what Euphoria reported, and where the track can be found.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
