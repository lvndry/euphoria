---
title: Architecture
description: How the Euphoria code base is organized.
---

# Architecture

Euphoria is a Cargo workspace. All product logic lives in one library crate, and every front end is a thin crate on top of it.

```
crates/
  euphoria-core/    the library: decoding, detection, energy, library scanning, the analysis cache
  euphoria-cli/     the `euphoria` command line tool
  euphoria-app/     the desktop app
  euphoria-bench/   accuracy benchmark, not shipped to users
bench/              dataset download script, Essentia reference script, results
```

## Rules between crates

- `euphoria-core` depends on no front end. It contains no command line parsing, no user interface code, and prints nothing.
- Front ends depend on `euphoria-core` and never on each other. They translate input into core calls and core results into output.
- A feature is built in the core first, then exposed by each front end, so the CLI and the app always give the same answer.

## euphoria-core

| Module | Responsibility |
| --- | --- |
| `library` | Expands files and folders into the audio files to analyze, skipping hidden files. |
| `analysis` | Analyzes one file, or a batch in parallel with progress callbacks and one result per file. |
| `cache` | Remembers analyses in a SQLite database keyed by a hash of each file's content, and analyzes only the files it does not know, as a batch or one file at a time. |
| `audio` | Decodes MP3, M4A, FLAC, WAV and AIFF with Symphonia and mixes them to mono. |
| `key` | The `Key` type, Camelot and Open Key codes, and parsing of key names and codes. |
| `harmony` | How two keys relate (`Relation`) and the ranking of the tracks that mix into another (`mixes_into`). |
| `detect` | The `KeyDetector` and `TempoDetector` traits and their implementations, the tuning measurement and the key over time. |
| `energy` | The energy rating from 1 to 10, the features behind it and the energy curve. |
| `track_name` | Reads the title and artist stored in an audio file. |
| `export` | Builds a library from analysis results, mirrors folders as a playlist tree, and writes it for other DJ software, starting with rekordbox XML. |
| `error` | `EuphoriaError`, the single error type returned by the library. |

### Detectors

Key and tempo detection sit behind traits so implementations can be compared and replaced without touching callers.

- `KeyFinderDetector` calls libkeyfinder, a C++ library, through a small C wrapper in `src/detect/keyfinder_wrapper.cpp`. `build.rs` compiles the wrapper and finds libkeyfinder with pkg-config. The wrapper returns the chromagram libkeyfinder computes, one row of note strengths per analysis frame, and scores chroma vectors against the 24 keys. The key of a track is the best score of the mean of all rows, which is what libkeyfinder itself reports. `read_key` also scores windows of about 7 seconds and follows the key through them for the key over time.
- `KeyFinderVariant` holds the ways of deciding the key that were measured and are not used: tuning correction and a vote between sections. They stay in the code so the benchmark keeps comparing them with what ships.
- `measure_tuning` is pure Rust and measures how far a track is tuned from A = 440 Hz. It runs in every build.
- `StratumDetector` uses the pure Rust `stratum-dsp` crate for both key and tempo. Its tempo detection runs only the tempogram estimate, on at most the middle ten minutes of a track, reports the result between 90 and 180 BPM, and costs a fraction of its key detection.

`analysis` uses libkeyfinder for key when the `libkeyfinder` feature is on, and the tempo detection of `stratum-dsp`, so the `stratum-dsp` key analysis runs only in builds without the feature. Those builds report the tuning and no key over time.

## euphoria-cli

Parses arguments with clap, runs `cache::analyze_files_cached` (or `analysis::analyze_files` with `--no-cache`) on a Rayon pool sized by `--jobs`, shows a progress bar on a terminal, and formats the results as a table or JSON. `analyze` and `export` share that batch code in `batch.rs`. `export rekordbox` passes the results to `export::assemble_library` and `export::rekordbox::write_xml_file`. The crate contains no analysis or export logic.

## euphoria-app

The desktop app, built with GPUI and the gpui-component library. It asks the core for the audio files under each added folder, fills in the ones `cache::CachedAnalyzer` already knows, analyzes the rest through it on a Rayon pool of at most 4 workers so that each row fills in as its file finishes, and asks `harmony` which tracks mix with the selected one. State that needs no window (the library, sorting and searching, progress, settings, themes) is in plain modules with unit tests; the views only draw it. It contains no analysis logic and no harmonic rules.

## Detector choice

Which detector ships is decided by measurement. `euphoria-bench` scores every detector on the GiantSteps key and tempo datasets, and `bench/essentia_reference.py` scores Essentia on the same tracks as a reference point.

## Licensing

libkeyfinder is GPL-3.0-or-later, so Euphoria is too. `deny.toml` lists the dependency licenses that are compatible, and CI rejects anything else.
