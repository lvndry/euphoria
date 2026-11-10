---
title: Architecture
description: How the Euphoria code base is organized.
---

# Architecture

Euphoria is a Cargo workspace. All product logic lives in one library crate, and every front end is a thin crate on top of it.

```
crates/
  euphoria-core/    the library: decoding, detection, library scanning, the analysis cache
  euphoria-cli/     the `euphoria` command line tool
  euphoria-bench/   accuracy benchmark, not shipped to users
bench/              dataset download script, Essentia reference script, results
```

A desktop app (`euphoria-app`) will join the workspace as another crate.

## Rules between crates

- `euphoria-core` depends on no front end. It contains no command line parsing, no user interface code, and prints nothing.
- Front ends depend on `euphoria-core` and never on each other. They translate input into core calls and core results into output.
- A feature is built in the core first, then exposed by each front end, so the CLI and the app always give the same answer.

## euphoria-core

| Module | Responsibility |
| --- | --- |
| `library` | Expands files and folders into the audio files to analyze, skipping hidden files. |
| `analysis` | Analyzes one file, or a batch in parallel with progress callbacks and one result per file. |
| `cache` | Remembers analyses in a SQLite database keyed by a hash of each file's content, and runs batches that analyze only the files it does not know. |
| `audio` | Decodes MP3, M4A, FLAC, WAV and AIFF with Symphonia and mixes them to mono. |
| `key` | The `Key` type, Camelot and Open Key codes, and parsing of key names and codes. |
| `harmony` | How two keys relate (`Relation`) and the ranking of the tracks that mix into another (`mixes_into`). |
| `detect` | The `KeyDetector` and `TempoDetector` traits and their implementations. |
| `export` | Writes analysis results for other DJ software, starting with rekordbox XML. |
| `error` | `EuphoriaError`, the single error type returned by the library. |

### Detectors

Key and tempo detection sit behind traits so implementations can be compared and replaced without touching callers.

- `KeyFinderDetector` calls libkeyfinder, a C++ library, through a small C wrapper in `src/detect/keyfinder_wrapper.cpp`. `build.rs` compiles the wrapper and finds libkeyfinder with pkg-config.
- `StratumDetector` uses the pure Rust `stratum-dsp` crate for both key and tempo.

`analysis` uses libkeyfinder for key when the `libkeyfinder` feature is on, and `stratum-dsp` for tempo.

## euphoria-cli

Parses arguments with clap, runs `cache::analyze_files_cached` (or `analysis::analyze_files` with `--no-cache`) on a Rayon pool sized by `--jobs`, shows a progress bar on a terminal, and formats the results as a table or JSON. It contains no analysis logic.

## Detector choice

Which detector ships is decided by measurement. `euphoria-bench` scores every detector on the GiantSteps key and tempo datasets, and `bench/essentia_reference.py` scores Essentia on the same tracks as a reference point.

## Licensing

libkeyfinder is GPL-3.0-or-later, so Euphoria is too. `deny.toml` lists the dependency licenses that are compatible, and CI rejects anything else.
